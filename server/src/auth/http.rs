use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::Duration,
};

use axum::{
    Extension, Json, Router,
    extract::{
        ConnectInfo, DefaultBodyLimit, Path, Request, State, WebSocketUpgrade,
        ws::{CloseFrame, Message, WebSocket},
    },
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use uuid::Uuid;
use webauthn_rs::prelude::PublicKeyCredential;
use zeroize::Zeroizing;

use super::{
    AuthConfig, AuthError, AuthService, Authenticated, AuthenticationMethod, BrowserBinding,
    Profile,
    sessions::{SessionConfig, SessionInfo, SessionToken, Sessions},
};
use crate::{
    audit::{self, Action, Actor, Event, Outcome},
    authorization::{AccessError, Authorizer, TenantContext, TenantSummary},
    database::Database,
};

const SESSION_COOKIE: &str = "__Host-midas-session";
const BINDING_COOKIE: &str = "__Host-midas-login";

pub struct WebAuth {
    auth: AuthService,
    sessions: Sessions,
    origin: String,
    socket_slots: Arc<Semaphore>,
    pool: PgPool,
    authorizer: Authorizer,
}

impl WebAuth {
    pub async fn new(
        database: &Database,
        config: AuthConfig,
        sessions: SessionConfig,
        socket_capacity: usize,
    ) -> super::Result<Self> {
        if socket_capacity == 0 || socket_capacity > Semaphore::MAX_PERMITS {
            return Err(AuthError::InvalidInput);
        }
        let origin = config.origin.origin().ascii_serialization();
        Ok(Self {
            auth: AuthService::new(database, config).await?,
            sessions: Sessions::new(database, sessions)?,
            origin,
            socket_slots: Arc::new(Semaphore::new(socket_capacity)),
            pool: database.pool().clone(),
            authorizer: Authorizer::new(database),
        })
    }

    pub fn authentication(&self) -> &AuthService {
        &self.auth
    }
    pub fn sessions(&self) -> &Sessions {
        &self.sessions
    }

    async fn record(&self, event: &Event) -> std::result::Result<(), sqlx::Error> {
        let mut connection = self.pool.acquire().await?;
        audit::append_platform(&mut connection, event).await
    }

    async fn record_best_effort(&self, event: &Event) {
        if self.record(event).await.is_err() {
            tracing::error!(action = event.action.as_str(), "audit write failed");
        }
    }
}

fn method_name(method: AuthenticationMethod) -> &'static str {
    match method {
        AuthenticationMethod::Passkey => "passkey",
        AuthenticationMethod::PasswordTotp => "password_totp",
        AuthenticationMethod::RecoveryCode => "recovery_code",
    }
}

#[derive(Clone)]
struct Context {
    token: SessionToken,
    info: SessionInfo,
}

#[derive(Debug)]
struct Failure(StatusCode, &'static str);
type Result<T> = std::result::Result<T, Failure>;

impl From<AuthError> for Failure {
    fn from(error: AuthError) -> Self {
        match error {
            AuthError::InvalidInput => Self(StatusCode::BAD_REQUEST, "invalid_request"),
            AuthError::InvalidCredentials | AuthError::Conflict => {
                Self(StatusCode::UNAUTHORIZED, "authentication_failed")
            }
            AuthError::RateLimited => Self(StatusCode::TOO_MANY_REQUESTS, "try_later"),
            AuthError::Busy | AuthError::Unavailable => {
                Self(StatusCode::SERVICE_UNAVAILABLE, "temporarily_unavailable")
            }
        }
    }
}
impl From<AccessError> for Failure {
    fn from(error: AccessError) -> Self {
        match error {
            AccessError::Unauthenticated => Self(StatusCode::UNAUTHORIZED, "authentication_failed"),
            AccessError::Denied | AccessError::InvalidInput => {
                Self(StatusCode::NOT_FOUND, "tenant_not_found")
            }
            AccessError::FreshPasskeyRequired => {
                Self(StatusCode::FORBIDDEN, "fresh_passkey_required")
            }
            AccessError::Conflict | AccessError::Unavailable => {
                Self(StatusCode::SERVICE_UNAVAILABLE, "temporarily_unavailable")
            }
        }
    }
}
impl IntoResponse for Failure {
    fn into_response(self) -> Response {
        (self.0, Json(serde_json::json!({ "error": self.1 }))).into_response()
    }
}

pub fn router(state: Arc<WebAuth>) -> Router {
    let protected = Router::new()
        .route("/api/session", get(current))
        .route("/api/sessions", get(list))
        .route("/api/session/logout", post(logout))
        .route("/api/sessions/{id}", delete(revoke))
        .route("/api/tenants", get(tenants))
        .route("/api/tenants/{id}", get(tenant))
        .route_layer(middleware::from_fn_with_state(state.clone(), authenticate));
    Router::new()
        .merge(protected)
        .route("/api/auth/passkey/start", post(passkey_start))
        .route("/api/auth/passkey/finish", post(passkey_finish))
        .route("/api/auth/password", post(password))
        .route("/api/auth/recovery", post(recovery))
        .route("/api/session/socket", get(upgrade))
        .layer(DefaultBodyLimit::max(65_536))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            browser_boundary,
        ))
        .layer(middleware::from_fn(crate::observe))
        .with_state(state)
}

fn one_header<'a>(headers: &'a HeaderMap, name: &str) -> Result<Option<&'a str>> {
    let mut values = headers.get_all(name).iter();
    let value = values
        .next()
        .map(|v| {
            v.to_str()
                .map_err(|_| Failure(StatusCode::BAD_REQUEST, "invalid_request"))
        })
        .transpose()?;
    if values.next().is_some() {
        return Err(Failure(StatusCode::BAD_REQUEST, "invalid_request"));
    }
    Ok(value)
}

fn check_origin(headers: &HeaderMap, expected: &str, required: bool) -> Result<()> {
    let origin = one_header(headers, "origin")?;
    if (required && origin.is_none()) || origin.is_some_and(|value| value != expected) {
        return Err(Failure(StatusCode::FORBIDDEN, "origin_rejected"));
    }
    if one_header(headers, "sec-fetch-site")?.is_some_and(|v| v != "same-origin" && v != "none") {
        return Err(Failure(StatusCode::FORBIDDEN, "origin_rejected"));
    }
    Ok(())
}

async fn browser_boundary(
    State(state): State<Arc<WebAuth>>,
    request: Request,
    next: Next,
) -> Response {
    let result = if request.uri().query().is_some() {
        Err(Failure(StatusCode::BAD_REQUEST, "query_not_supported"))
    } else {
        check_origin(
            request.headers(),
            &state.origin,
            !matches!(
                *request.method(),
                Method::GET | Method::HEAD | Method::OPTIONS
            ),
        )
    };
    let mut response = match result {
        Ok(()) => next.run(request).await,
        Err(error) => error.into_response(),
    };
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response.headers_mut().insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    response
}

fn cookie<'a>(headers: &'a HeaderMap, name: &str) -> Result<Option<&'a str>> {
    let mut found = None;
    for value in headers.get_all(header::COOKIE) {
        for item in value
            .to_str()
            .map_err(|_| Failure(StatusCode::BAD_REQUEST, "invalid_cookie"))?
            .split(';')
        {
            if let Some((key, value)) = item.trim().split_once('=')
                && key == name
            {
                if found.is_some() {
                    return Err(Failure(StatusCode::BAD_REQUEST, "ambiguous_cookie"));
                }
                found = Some(value);
            }
        }
    }
    Ok(found)
}

fn session_token(headers: &HeaderMap) -> Result<SessionToken> {
    Ok(SessionToken::parse(
        cookie(headers, SESSION_COOKIE)?.ok_or(AuthError::InvalidCredentials)?,
    )?)
}

fn set_cookie(response: &mut Response, name: &str, value: &str, seconds: u64) -> Result<()> {
    let mut value = HeaderValue::from_str(&format!(
        "{name}={value}; Path=/; Secure; HttpOnly; SameSite=Strict; Max-Age={seconds}"
    ))
    .map_err(|_| Failure(StatusCode::INTERNAL_SERVER_ERROR, "temporarily_unavailable"))?;
    value.set_sensitive(true);
    response.headers_mut().append(header::SET_COOKIE, value);
    Ok(())
}

async fn authenticate(
    State(state): State<Arc<WebAuth>>,
    mut request: Request,
    next: Next,
) -> Result<Response> {
    let token = session_token(request.headers())?;
    let info = state.sessions.validate(&token, true).await?;
    if !matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    ) {
        let csrf = one_header(request.headers(), "x-csrf-token")?
            .ok_or(Failure(StatusCode::FORBIDDEN, "csrf_rejected"))?;
        token
            .verify_csrf(csrf)
            .map_err(|_| Failure(StatusCode::FORBIDDEN, "csrf_rejected"))?;
    }
    request.extensions_mut().insert(Context { token, info });
    Ok(next.run(request).await)
}

#[derive(Serialize)]
struct SessionResponse {
    session: SessionInfo,
    csrf_token: String,
    operator: Profile,
}

async fn current(
    State(state): State<Arc<WebAuth>>,
    Extension(context): Extension<Context>,
) -> Result<Json<SessionResponse>> {
    let operator = state
        .auth
        .profile(context.info.operator_id)
        .await?
        .ok_or(AuthError::InvalidCredentials)?;
    Ok(Json(SessionResponse {
        session: context.info,
        csrf_token: context.token.csrf()?.to_string(),
        operator,
    }))
}
async fn tenants(
    State(state): State<Arc<WebAuth>>,
    Extension(context): Extension<Context>,
) -> Result<Json<Vec<TenantSummary>>> {
    Ok(Json(
        state
            .authorizer
            .tenants(&state.sessions, &context.token)
            .await?,
    ))
}
async fn tenant(
    State(state): State<Arc<WebAuth>>,
    Extension(context): Extension<Context>,
    Path(id): Path<Uuid>,
) -> Result<Json<TenantContext>> {
    Ok(Json(
        state
            .authorizer
            .tenant_context(&state.sessions, &context.token, id)
            .await?,
    ))
}
async fn list(
    State(state): State<Arc<WebAuth>>,
    Extension(context): Extension<Context>,
) -> Result<Json<Vec<SessionInfo>>> {
    Ok(Json(state.sessions.list(&context.token).await?))
}
async fn logout(
    State(state): State<Arc<WebAuth>>,
    Extension(context): Extension<Context>,
) -> Result<Response> {
    state
        .sessions
        .revoke(&context.token, context.info.id)
        .await?;
    let mut event = Event::new(
        Action::SignOut,
        Outcome::Success,
        Actor::session(&context.info),
    );
    event.subject = Some(context.info.operator_id);
    state.record_best_effort(&event).await;
    let mut response = StatusCode::NO_CONTENT.into_response();
    set_cookie(&mut response, SESSION_COOKIE, "", 0)?;
    set_cookie(&mut response, BINDING_COOKIE, "", 0)?;
    Ok(response)
}
async fn revoke(
    State(state): State<Arc<WebAuth>>,
    Extension(context): Extension<Context>,
    Path(id): Path<Uuid>,
) -> Result<Response> {
    if !state.sessions.revoke(&context.token, id).await? {
        return Err(Failure(StatusCode::NOT_FOUND, "session_not_found"));
    }
    let mut event = Event::new(
        Action::SessionRevoke,
        Outcome::Success,
        Actor::session(&context.info),
    )
    .detail("session_id", id.to_string());
    event.subject = Some(context.info.operator_id);
    state.record_best_effort(&event).await;
    let mut response = StatusCode::NO_CONTENT.into_response();
    if id == context.info.id {
        set_cookie(&mut response, SESSION_COOKIE, "", 0)?;
    }
    Ok(response)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Login {
    login: String,
}

async fn passkey_start(
    State(state): State<Arc<WebAuth>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(body): Json<Login>,
) -> Result<Response> {
    let binding = BrowserBinding::random()?;
    let issued = state
        .auth
        .begin_passkey_login(&body.login, peer.ip(), &binding)
        .await?;
    let mut response = Json(issued).into_response();
    set_cookie(
        &mut response,
        BINDING_COOKIE,
        &hex::encode(binding.as_bytes()),
        300,
    )?;
    Ok(response)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Assertion {
    id: String,
    response: PublicKeyCredential,
}

async fn passkey_finish(
    State(state): State<Arc<WebAuth>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<Assertion>,
) -> Result<Response> {
    let encoded = cookie(&headers, BINDING_COOKIE)?.ok_or(AuthError::InvalidCredentials)?;
    let mut bytes = Zeroizing::new([0; 32]);
    if encoded.len() != 64 {
        return Err(AuthError::InvalidCredentials.into());
    }
    hex::decode_to_slice(encoded, bytes.as_mut()).map_err(|_| AuthError::InvalidCredentials)?;
    let binding = BrowserBinding::from_bytes(bytes)?;
    let result = state
        .auth
        .finish_passkey_login(&body.id, peer.ip(), &binding, &body.response)
        .await;
    sign_in(
        &state,
        AuthenticationMethod::Passkey,
        None,
        peer.ip(),
        result,
        &headers,
    )
    .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Password {
    login: String,
    password: String,
    code: String,
}

async fn password(
    State(state): State<Arc<WebAuth>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<Password>,
) -> Result<Response> {
    let result = state
        .auth
        .login_password_totp(
            &body.login,
            Zeroizing::new(body.password),
            &Zeroizing::new(body.code),
            peer.ip(),
        )
        .await;
    sign_in(
        &state,
        AuthenticationMethod::PasswordTotp,
        Some(&body.login),
        peer.ip(),
        result,
        &headers,
    )
    .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Recovery {
    login: String,
    code: String,
}

async fn recovery(
    State(state): State<Arc<WebAuth>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<Recovery>,
) -> Result<Response> {
    let result = state
        .auth
        .login_recovery(&body.login, Zeroizing::new(body.code), peer.ip())
        .await;
    sign_in(
        &state,
        AuthenticationMethod::RecoveryCode,
        Some(&body.login),
        peer.ip(),
        result,
        &headers,
    )
    .await
}

// Malformed and rate-limited attempts are not recorded individually so that audit growth stays bounded.
async fn sign_in(
    state: &WebAuth,
    method: AuthenticationMethod,
    login: Option<&str>,
    source: IpAddr,
    result: super::Result<Authenticated>,
    headers: &HeaderMap,
) -> Result<Response> {
    match result {
        Ok(proof) => signed_in(state, proof, headers, source).await,
        Err(AuthError::InvalidCredentials) => {
            let mut event = Event::new(Action::SignIn, Outcome::Failure, Actor::SYSTEM)
                .detail("method", method_name(method));
            event.source = Some(source);
            if let Some(login) = login {
                event.subject = state.auth.operator_for_login(login).await.unwrap_or(None);
            }
            state.record_best_effort(&event).await;
            Err(AuthError::InvalidCredentials.into())
        }
        Err(error) => Err(error.into()),
    }
}

async fn signed_in(
    state: &WebAuth,
    proof: Authenticated,
    headers: &HeaderMap,
    source: IpAddr,
) -> Result<Response> {
    let previous = cookie(headers, SESSION_COOKIE)?.and_then(|s| SessionToken::parse(s).ok());
    let issued = state.sessions.issue(proof, previous.as_ref()).await?;
    let mut event = Event::new(
        Action::SignIn,
        Outcome::Success,
        Actor::session(&issued.info),
    )
    .detail("method", method_name(issued.info.authentication_method));
    event.subject = Some(issued.info.operator_id);
    event.source = Some(source);
    let operator = match state.record(&event).await {
        Ok(()) => state.auth.profile(issued.info.operator_id).await,
        Err(_) => Err(AuthError::Unavailable),
    };
    let Ok(Some(operator)) = operator else {
        let _ = state.sessions.revoke(&issued.token, issued.info.id).await;
        return Err(AuthError::Unavailable.into());
    };
    let mut response = Json(SessionResponse {
        session: issued.info,
        csrf_token: issued.token.csrf()?.to_string(),
        operator,
    })
    .into_response();
    set_cookie(
        &mut response,
        SESSION_COOKIE,
        &issued.token.encode(),
        state.sessions.lifetime_seconds(),
    )?;
    set_cookie(&mut response, BINDING_COOKIE, "", 0)?;
    Ok(response)
}

async fn upgrade(
    State(state): State<Arc<WebAuth>>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response> {
    check_origin(&headers, &state.origin, true)?;
    let permit = state
        .socket_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| AuthError::Busy)?;
    let token = session_token(&headers)?;
    let revoked = state.sessions.subscribe();
    let info = state.sessions.validate(&token, false).await?;
    Ok(ws
        .max_message_size(4096)
        .max_frame_size(4096)
        .on_upgrade(move |socket| channel(socket, state, token, info.id, revoked, permit)))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Handshake {
    csrf_token: String,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum Command {
    Session,
}

async fn send(socket: &mut WebSocket, message: Message) -> bool {
    matches!(
        tokio::time::timeout(Duration::from_secs(5), socket.send(message)).await,
        Ok(Ok(()))
    )
}

async fn channel(
    mut socket: WebSocket,
    state: Arc<WebAuth>,
    token: SessionToken,
    id: Uuid,
    mut revoked: tokio::sync::broadcast::Receiver<Uuid>,
    _permit: OwnedSemaphorePermit,
) {
    let handshake = tokio::time::timeout(Duration::from_secs(5), socket.recv()).await;
    let valid = match handshake {
        Ok(Some(Ok(Message::Text(text)))) => serde_json::from_str::<Handshake>(&text)
            .ok()
            .is_some_and(|body| token.verify_csrf(&body.csrf_token).is_ok()),
        _ => false,
    };
    if valid
        && let Ok(info) = state.sessions.validate(&token, false).await
        && let Ok(text) = serde_json::to_string(&info)
        && send(&mut socket, Message::Text(text.into())).await
    {
        let mut interval = tokio::time::interval(Duration::from_secs(15));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut window = tokio::time::Instant::now();
        let mut messages = 0;
        loop {
            tokio::select! {
                biased;
                event = revoked.recv() => { if event.is_err() || event.is_ok_and(|revoked_id| revoked_id == id) { break; } }
                _ = interval.tick() => { if state.sessions.validate(&token, false).await.is_err() { break; } }
                message = socket.recv() => {
                    if window.elapsed() >= Duration::from_secs(10) { window = tokio::time::Instant::now(); messages = 0; }
                    messages += 1;
                    if messages > 30 { break; }
                    let Ok(info) = state.sessions.validate(&token, false).await else { break; };
                    match message {
                        Some(Ok(Message::Text(text))) => {
                            if serde_json::from_str::<Command>(&text).is_err() { break; }
                            let Ok(text) = serde_json::to_string(&info) else { break; };
                            if !send(&mut socket, Message::Text(text.into())).await { break; }
                        }
                        Some(Ok(Message::Ping(_))) | Some(Ok(Message::Pong(_))) => {},
                        _ => break,
                    }
                }
            }
        }
    }
    send(
        &mut socket,
        Message::Close(Some(CloseFrame {
            code: 1008,
            reason: "Session ended".into(),
        })),
    )
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins_and_ambiguous_cookies_fail_closed() {
        let mut headers = HeaderMap::new();
        assert!(check_origin(&headers, "https://midas.example", true).is_err());
        for origin in [
            "null",
            "https://evil.example",
            "https://midas.example:444",
            "https://sub.midas.example",
        ] {
            headers.insert(header::ORIGIN, origin.parse().unwrap());
            assert!(check_origin(&headers, "https://midas.example", true).is_err());
        }
        headers.insert(
            header::ORIGIN,
            HeaderValue::from_static("https://midas.example"),
        );
        check_origin(&headers, "https://midas.example", true).unwrap();
        headers.append(
            header::ORIGIN,
            HeaderValue::from_static("https://midas.example"),
        );
        assert!(check_origin(&headers, "https://midas.example", true).is_err());
        headers.insert(
            header::COOKIE,
            HeaderValue::from_static("__Host-midas-session=a; __Host-midas-session=b"),
        );
        assert!(cookie(&headers, SESSION_COOKIE).is_err());
    }
}
