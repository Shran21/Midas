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
    routing::{delete, get, post, put},
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
    administration::{Administration, CreatedOperator, Member, RoleView},
    audit::{self, Action, Actor, Event, Outcome},
    authorization::{
        AccessError, Authorizer, Effect, Permission, Rule, Scope, TenantContext, TenantSummary,
    },
    database::{Database, Site},
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
    administration: Administration,
    enrollment: Option<Arc<crate::enrollment::Enrollment>>,
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
            administration: Administration::new(database),
            enrollment: None,
        })
    }

    /// Serves the device enrollment administration routes.
    pub fn with_enrollment(mut self, enrollment: Arc<crate::enrollment::Enrollment>) -> Self {
        self.enrollment = Some(enrollment);
        self
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
        AuthenticationMethod::Directory => "directory",
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
            AccessError::Denied => Self(StatusCode::NOT_FOUND, "not_found"),
            AccessError::InvalidInput => Self(StatusCode::BAD_REQUEST, "invalid_request"),
            AccessError::Escalation => Self(StatusCode::FORBIDDEN, "escalation_denied"),
            AccessError::FreshPasskeyRequired => {
                Self(StatusCode::FORBIDDEN, "fresh_passkey_required")
            }
            AccessError::Conflict => Self(StatusCode::CONFLICT, "conflict"),
            AccessError::Unavailable => {
                Self(StatusCode::SERVICE_UNAVAILABLE, "temporarily_unavailable")
            }
            AccessError::Offline => Self(StatusCode::CONFLICT, "device_offline"),
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
        .route("/api/roles", get(roles))
        .route("/api/admin/tenants", post(create_tenant))
        .route("/api/admin/operators", post(create_operator))
        .route("/api/tenants/{id}/members", get(members).post(add_member))
        .route("/api/tenants/{id}/members/{operator}", put(set_member))
        .route("/api/tenants/{id}/assignments", post(assign))
        .route(
            "/api/tenants/{id}/assignments/{assignment}",
            delete(unassign),
        )
        .route("/api/tenants/{id}/overrides", post(set_override))
        .route(
            "/api/tenants/{id}/overrides/{rule}",
            delete(remove_override),
        )
        .route("/api/tenants/{id}/sites", get(sites).post(create_site))
        .route(
            "/api/tenants/{id}/enrollment-tokens",
            post(create_enrollment_token),
        )
        .route("/api/tenants/{id}/devices", get(devices))
        .route("/api/tenants/{id}/devices/pending", get(pending_devices))
        .route("/api/tenants/{id}/devices/decisions", get(device_decisions))
        .route("/api/tenants/{id}/devices/{device}", get(device))
        .route(
            "/api/tenants/{id}/devices/{device}/metrics",
            get(device_metrics),
        )
        .route(
            "/api/tenants/{id}/devices/{device}/alerts",
            get(device_alerts),
        )
        .route(
            "/api/tenants/{id}/devices/{device}/inventory",
            get(device_inventory),
        )
        .route(
            "/api/tenants/{id}/devices/{device}/health",
            get(device_health),
        )
        .route(
            "/api/tenants/{id}/devices/{device}/policy",
            get(device_policy),
        )
        .route("/api/tenants/{id}/policies", get(policies))
        .route(
            "/api/tenants/{id}/policies/{level}/{scope}/{key}",
            put(set_policy).delete(clear_policy),
        )
        .route("/api/tenants/{id}/groups", get(groups).post(create_group))
        .route("/api/tenants/{id}/alerts", get(alerts))
        .route("/api/tenants/{id}/alerts/open", get(open_alerts))
        .route(
            "/api/tenants/{id}/alerts/{alert}/acknowledge",
            post(acknowledge_alert),
        )
        .route(
            "/api/tenants/{id}/alert-recipients",
            get(alert_recipients).put(set_alert_recipients),
        )
        .route(
            "/api/tenants/{id}/groups/{group}/devices/{device}",
            put(add_member_device).delete(remove_member_device),
        )
        .route(
            "/api/tenants/{id}/directory-connector",
            get(directory_connector).put(set_directory_connector),
        )
        .route(
            "/api/tenants/{id}/directory/computers",
            get(directory_computers),
        )
        .route(
            "/api/tenants/{id}/devices/{device}/{decision}",
            post(decide_device),
        )
        .route("/api/account/passkeys/start", post(enroll_start))
        .route("/api/account/passkeys/finish", post(enroll_finish))
        .route_layer(middleware::from_fn_with_state(state.clone(), authenticate));
    Router::new()
        .merge(protected)
        .route("/api/auth/passkey/start", post(passkey_start))
        .route("/api/auth/passkey/finish", post(passkey_finish))
        .route("/api/auth/password", post(password))
        .route("/api/auth/recovery", post(recovery))
        .route("/api/auth/directory", post(directory))
        .route("/api/setup/start", post(setup_start))
        .route("/api/setup/finish", post(setup_finish))
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
struct Named {
    name: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewOperator {
    login: String,
    display_name: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Enabled {
    enabled: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewAssignment {
    operator: Uuid,
    role: Uuid,
    scope: Scope,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewOverride {
    operator: Uuid,
    permission: Permission,
    effect: Effect,
    scope: Scope,
}
#[derive(Serialize)]
struct Created {
    id: Uuid,
}

async fn roles(
    State(state): State<Arc<WebAuth>>,
    Extension(context): Extension<Context>,
) -> Result<Json<Vec<RoleView>>> {
    Ok(Json(
        state
            .administration
            .roles(&state.sessions, &context.token)
            .await?,
    ))
}
async fn create_tenant(
    State(state): State<Arc<WebAuth>>,
    Extension(context): Extension<Context>,
    Json(body): Json<Named>,
) -> Result<(StatusCode, Json<TenantSummary>)> {
    let tenant = state
        .administration
        .create_tenant(&state.sessions, &context.token, &body.name)
        .await?;
    Ok((StatusCode::CREATED, Json(tenant)))
}
async fn create_operator(
    State(state): State<Arc<WebAuth>>,
    Extension(context): Extension<Context>,
    Json(body): Json<NewOperator>,
) -> Result<(StatusCode, Json<CreatedOperator>)> {
    let operator = state
        .administration
        .create_operator(
            &state.auth,
            &state.sessions,
            &context.token,
            &body.login,
            &body.display_name,
        )
        .await?;
    Ok((StatusCode::CREATED, Json(operator)))
}
async fn members(
    State(state): State<Arc<WebAuth>>,
    Extension(context): Extension<Context>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<Member>>> {
    Ok(Json(
        state
            .administration
            .members(&state.sessions, &context.token, id)
            .await?,
    ))
}
async fn add_member(
    State(state): State<Arc<WebAuth>>,
    Extension(context): Extension<Context>,
    Path(id): Path<Uuid>,
    Json(body): Json<Login>,
) -> Result<(StatusCode, Json<Created>)> {
    let operator = state
        .administration
        .add_member(&state.sessions, &context.token, id, &body.login)
        .await?;
    Ok((StatusCode::CREATED, Json(Created { id: operator })))
}
async fn set_member(
    State(state): State<Arc<WebAuth>>,
    Extension(context): Extension<Context>,
    Path((id, operator)): Path<(Uuid, Uuid)>,
    Json(body): Json<Enabled>,
) -> Result<StatusCode> {
    state
        .administration
        .set_member(&state.sessions, &context.token, id, operator, body.enabled)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
async fn assign(
    State(state): State<Arc<WebAuth>>,
    Extension(context): Extension<Context>,
    Path(id): Path<Uuid>,
    Json(body): Json<NewAssignment>,
) -> Result<(StatusCode, Json<Created>)> {
    let assignment = state
        .administration
        .assign_role(
            &state.sessions,
            &context.token,
            id,
            body.operator,
            body.role,
            body.scope,
        )
        .await?;
    Ok((StatusCode::CREATED, Json(Created { id: assignment })))
}
async fn unassign(
    State(state): State<Arc<WebAuth>>,
    Extension(context): Extension<Context>,
    Path((id, assignment)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode> {
    if !state
        .administration
        .remove_assignment(&state.sessions, &context.token, id, assignment)
        .await?
    {
        return Err(Failure(StatusCode::NOT_FOUND, "not_found"));
    }
    Ok(StatusCode::NO_CONTENT)
}
async fn set_override(
    State(state): State<Arc<WebAuth>>,
    Extension(context): Extension<Context>,
    Path(id): Path<Uuid>,
    Json(body): Json<NewOverride>,
) -> Result<(StatusCode, Json<Created>)> {
    let rule = state
        .administration
        .set_override(
            &state.sessions,
            &context.token,
            id,
            body.operator,
            body.scope,
            Rule {
                permission: body.permission,
                effect: body.effect,
            },
        )
        .await?;
    Ok((StatusCode::CREATED, Json(Created { id: rule })))
}
async fn remove_override(
    State(state): State<Arc<WebAuth>>,
    Extension(context): Extension<Context>,
    Path((id, rule)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode> {
    if !state
        .administration
        .remove_override(&state.sessions, &context.token, id, rule)
        .await?
    {
        return Err(Failure(StatusCode::NOT_FOUND, "not_found"));
    }
    Ok(StatusCode::NO_CONTENT)
}
async fn sites(
    State(state): State<Arc<WebAuth>>,
    Extension(context): Extension<Context>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<Site>>> {
    Ok(Json(
        state
            .administration
            .sites(&state.sessions, &context.token, id)
            .await?,
    ))
}
async fn create_site(
    State(state): State<Arc<WebAuth>>,
    Extension(context): Extension<Context>,
    Path(id): Path<Uuid>,
    Json(body): Json<Named>,
) -> Result<(StatusCode, Json<Site>)> {
    let site = state
        .administration
        .create_site(&state.sessions, &context.token, id, &body.name)
        .await?;
    Ok((StatusCode::CREATED, Json(site)))
}

fn enrollment(state: &WebAuth) -> Result<&crate::enrollment::Enrollment> {
    state.enrollment.as_deref().ok_or(Failure(
        StatusCode::SERVICE_UNAVAILABLE,
        "temporarily_unavailable",
    ))
}

async fn create_enrollment_token(
    State(state): State<Arc<WebAuth>>,
    Extension(context): Extension<Context>,
    Path(id): Path<Uuid>,
    Json(body): Json<crate::enrollment::TokenSpec>,
) -> Result<(StatusCode, Json<crate::enrollment::CreatedToken>)> {
    let created = enrollment(&state)?
        .create_token(&state.sessions, &context.token, id, &body)
        .await?;
    Ok((StatusCode::CREATED, Json(created)))
}

async fn pending_devices(
    State(state): State<Arc<WebAuth>>,
    Extension(context): Extension<Context>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<crate::enrollment::PendingDevice>>> {
    Ok(Json(
        enrollment(&state)?
            .pending(&state.sessions, &context.token, id)
            .await?,
    ))
}

async fn devices(
    State(state): State<Arc<WebAuth>>,
    Extension(context): Extension<Context>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<crate::devices::DeviceSummary>>> {
    Ok(Json(
        enrollment(&state)?
            .devices(&state.sessions, &context.token, id)
            .await?,
    ))
}

async fn device(
    State(state): State<Arc<WebAuth>>,
    Extension(context): Extension<Context>,
    Path((id, device)): Path<(Uuid, Uuid)>,
) -> Result<Json<crate::devices::DeviceSummary>> {
    Ok(Json(
        enrollment(&state)?
            .device(&state.sessions, &context.token, id, device)
            .await?,
    ))
}

async fn device_metrics(
    State(state): State<Arc<WebAuth>>,
    Extension(context): Extension<Context>,
    Path((id, device)): Path<(Uuid, Uuid)>,
) -> Result<Json<crate::devices::MetricsView>> {
    Ok(Json(
        enrollment(&state)?
            .device_metrics(&state.sessions, &context.token, id, device)
            .await?,
    ))
}

async fn device_alerts(
    State(state): State<Arc<WebAuth>>,
    Extension(context): Extension<Context>,
    Path((id, device)): Path<(Uuid, Uuid)>,
) -> Result<Json<Vec<crate::alerts::AlertView>>> {
    Ok(Json(
        enrollment(&state)?
            .device_alerts(&state.sessions, &context.token, id, device)
            .await?,
    ))
}

async fn device_decisions(
    State(state): State<Arc<WebAuth>>,
    Extension(context): Extension<Context>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<crate::enrollment::Decision>>> {
    Ok(Json(
        enrollment(&state)?
            .decisions(&state.sessions, &context.token, id)
            .await?,
    ))
}

async fn device_inventory(
    State(state): State<Arc<WebAuth>>,
    Extension(context): Extension<Context>,
    Path((id, device)): Path<(Uuid, Uuid)>,
) -> Result<Json<crate::inventory::InventoryView>> {
    Ok(Json(
        enrollment(&state)?
            .inventory(&state.sessions, &context.token, id, device)
            .await?,
    ))
}

async fn device_health(
    State(state): State<Arc<WebAuth>>,
    Extension(context): Extension<Context>,
    Path((id, device)): Path<(Uuid, Uuid)>,
) -> Result<Json<crate::health::HealthView>> {
    Ok(Json(
        enrollment(&state)?
            .health(&state.sessions, &context.token, id, device)
            .await?,
    ))
}

async fn device_policy(
    State(state): State<Arc<WebAuth>>,
    Extension(context): Extension<Context>,
    Path((id, device)): Path<(Uuid, Uuid)>,
) -> Result<Json<Vec<crate::policy::EffectiveSetting>>> {
    Ok(Json(
        enrollment(&state)?
            .device_policy(&state.sessions, &context.token, id, device)
            .await?,
    ))
}

async fn policies(
    State(state): State<Arc<WebAuth>>,
    Extension(context): Extension<Context>,
    Path(id): Path<Uuid>,
) -> Result<Json<crate::policy::PolicyView>> {
    Ok(Json(
        enrollment(&state)?
            .policies(&state.sessions, &context.token, id)
            .await?,
    ))
}

async fn set_policy(
    State(state): State<Arc<WebAuth>>,
    Extension(context): Extension<Context>,
    Path((id, level, scope, key)): Path<(Uuid, crate::policy::Level, Uuid, String)>,
    Json(body): Json<crate::policy::ValueSpec>,
) -> Result<StatusCode> {
    enrollment(&state)?
        .set_policy(
            &state.sessions,
            &context.token,
            id,
            (level, scope, &key),
            body.value,
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn clear_policy(
    State(state): State<Arc<WebAuth>>,
    Extension(context): Extension<Context>,
    Path((id, level, scope, key)): Path<(Uuid, crate::policy::Level, Uuid, String)>,
) -> Result<StatusCode> {
    if !enrollment(&state)?
        .clear_policy(&state.sessions, &context.token, id, (level, scope, &key))
        .await?
    {
        return Err(Failure(StatusCode::NOT_FOUND, "not_found"));
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn groups(
    State(state): State<Arc<WebAuth>>,
    Extension(context): Extension<Context>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<crate::policy::Group>>> {
    Ok(Json(
        enrollment(&state)?
            .groups(&state.sessions, &context.token, id)
            .await?,
    ))
}

async fn create_group(
    State(state): State<Arc<WebAuth>>,
    Extension(context): Extension<Context>,
    Path(id): Path<Uuid>,
    Json(body): Json<crate::policy::GroupSpec>,
) -> Result<(StatusCode, Json<crate::policy::Group>)> {
    let group = enrollment(&state)?
        .create_group(&state.sessions, &context.token, id, &body)
        .await?;
    Ok((StatusCode::CREATED, Json(group)))
}

async fn add_member_device(
    State(state): State<Arc<WebAuth>>,
    Extension(context): Extension<Context>,
    Path((id, group, device)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<StatusCode> {
    enrollment(&state)?
        .set_membership(&state.sessions, &context.token, id, (group, device), true)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn remove_member_device(
    State(state): State<Arc<WebAuth>>,
    Extension(context): Extension<Context>,
    Path((id, group, device)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<StatusCode> {
    if !enrollment(&state)?
        .set_membership(&state.sessions, &context.token, id, (group, device), false)
        .await?
    {
        return Err(Failure(StatusCode::NOT_FOUND, "not_found"));
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn alerts(
    State(state): State<Arc<WebAuth>>,
    Extension(context): Extension<Context>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<crate::alerts::AlertView>>> {
    Ok(Json(
        enrollment(&state)?
            .alerts(&state.sessions, &context.token, id, false)
            .await?,
    ))
}

async fn open_alerts(
    State(state): State<Arc<WebAuth>>,
    Extension(context): Extension<Context>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<crate::alerts::AlertView>>> {
    Ok(Json(
        enrollment(&state)?
            .alerts(&state.sessions, &context.token, id, true)
            .await?,
    ))
}

async fn acknowledge_alert(
    State(state): State<Arc<WebAuth>>,
    Extension(context): Extension<Context>,
    Path((id, alert)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode> {
    enrollment(&state)?
        .acknowledge(&state.sessions, &context.token, id, alert)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn alert_recipients(
    State(state): State<Arc<WebAuth>>,
    Extension(context): Extension<Context>,
    Path(id): Path<Uuid>,
) -> Result<Json<crate::alerts::Recipients>> {
    Ok(Json(
        enrollment(&state)?
            .alert_recipients(&state.sessions, &context.token, id)
            .await?,
    ))
}

async fn set_alert_recipients(
    State(state): State<Arc<WebAuth>>,
    Extension(context): Extension<Context>,
    Path(id): Path<Uuid>,
    Json(body): Json<crate::alerts::Recipients>,
) -> Result<StatusCode> {
    enrollment(&state)?
        .set_alert_recipients(&state.sessions, &context.token, id, &body)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn directory_computers(
    State(state): State<Arc<WebAuth>>,
    Extension(context): Extension<Context>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<crate::discovery::ComputerView>>> {
    Ok(Json(
        enrollment(&state)?
            .directory_computers(&state.sessions, &context.token, id)
            .await?,
    ))
}

async fn directory_connector(
    State(state): State<Arc<WebAuth>>,
    Extension(context): Extension<Context>,
    Path(id): Path<Uuid>,
) -> Result<Json<crate::connector::ConnectorView>> {
    Ok(Json(
        enrollment(&state)?
            .connector(&state.sessions, &context.token, id)
            .await?,
    ))
}

async fn set_directory_connector(
    State(state): State<Arc<WebAuth>>,
    Extension(context): Extension<Context>,
    Path(id): Path<Uuid>,
    Json(body): Json<crate::connector::ConnectorSpec>,
) -> Result<Json<serde_json::Value>> {
    let revision = enrollment(&state)?
        .set_connector(&state.sessions, &context.token, id, &body)
        .await?;
    Ok(Json(serde_json::json!({ "revision": revision })))
}

async fn decide_device(
    State(state): State<Arc<WebAuth>>,
    Extension(context): Extension<Context>,
    Path((id, device, decision)): Path<(Uuid, Uuid, String)>,
) -> Result<StatusCode> {
    let enrollment = enrollment(&state)?;
    let (sessions, token) = (&state.sessions, &context.token);
    match decision.as_str() {
        "approve" => enrollment.approve(sessions, token, id, device).await?,
        "reject" => enrollment.reject(sessions, token, id, device).await?,
        "revoke" => enrollment.revoke(sessions, token, id, device).await?,
        "refresh" => enrollment.refresh(sessions, token, id, device).await?,
        _ => return Err(Failure(StatusCode::NOT_FOUND, "not_found")),
    }
    Ok(StatusCode::NO_CONTENT)
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
struct DirectoryLogin {
    login: String,
    password: String,
}

async fn directory(
    State(state): State<Arc<WebAuth>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<DirectoryLogin>,
) -> Result<Response> {
    let result = state
        .auth
        .login_directory(&body.login, Zeroizing::new(body.password), peer.ip())
        .await;
    sign_in(
        &state,
        AuthenticationMethod::Directory,
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

fn binding_from(headers: &HeaderMap) -> Result<BrowserBinding> {
    let encoded = cookie(headers, BINDING_COOKIE)?.ok_or(AuthError::InvalidCredentials)?;
    let mut bytes = Zeroizing::new([0; 32]);
    if encoded.len() != 64 {
        return Err(AuthError::InvalidCredentials.into());
    }
    hex::decode_to_slice(encoded, bytes.as_mut()).map_err(|_| AuthError::InvalidCredentials)?;
    Ok(BrowserBinding::from_bytes(bytes)?)
}

fn with_binding<T: Serialize>(body: T, binding: &BrowserBinding) -> Result<Response> {
    let mut response = Json(body).into_response();
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
struct SetupStart {
    token: String,
    login: String,
    display_name: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Registration {
    id: String,
    response: webauthn_rs::prelude::RegisterPublicKeyCredential,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetupFinish {
    token: String,
    id: String,
    response: webauthn_rs::prelude::RegisterPublicKeyCredential,
}

async fn setup_start(
    State(state): State<Arc<WebAuth>>,
    Json(body): Json<SetupStart>,
) -> Result<Response> {
    let binding = BrowserBinding::random()?;
    let issued = state
        .auth
        .begin_setup(
            &Zeroizing::new(body.token),
            &body.login,
            &body.display_name,
            &binding,
        )
        .await?;
    with_binding(issued, &binding)
}

async fn setup_finish(
    State(state): State<Arc<WebAuth>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<SetupFinish>,
) -> Result<Response> {
    let binding = binding_from(&headers)?;
    let proof = state
        .auth
        .finish_setup(
            &Zeroizing::new(body.token),
            &body.id,
            &binding,
            &body.response,
        )
        .await?;
    signed_in(&state, proof, &headers, peer.ip()).await
}

// Adding a credential to an account needs a recent sign-in, so an old stolen cookie cannot persist access.
async fn enroll_start(
    State(state): State<Arc<WebAuth>>,
    Extension(context): Extension<Context>,
) -> Result<Response> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| AuthError::Unavailable)?
        .as_secs() as i64;
    if now - context.info.authenticated_at > 300 {
        return Err(Failure(StatusCode::FORBIDDEN, "recent_sign_in_required"));
    }
    let binding = BrowserBinding::random()?;
    let issued = state
        .auth
        .begin_registration(context.info.operator_id, &binding)
        .await?;
    with_binding(issued, &binding)
}

async fn enroll_finish(
    State(state): State<Arc<WebAuth>>,
    Extension(context): Extension<Context>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<Registration>,
) -> Result<Response> {
    let binding = binding_from(&headers)?;
    let proof = state
        .auth
        .finish_registration(&body.id, &binding, &body.response)
        .await?;
    if proof.operator_id() != context.info.operator_id {
        return Err(AuthError::InvalidCredentials.into());
    }
    let mut event = Event::new(
        Action::PasskeyRegister,
        Outcome::Success,
        Actor::session(&context.info),
    );
    event.subject = Some(context.info.operator_id);
    event.source = Some(peer.ip());
    state.record_best_effort(&event).await;
    signed_in(&state, proof, &headers, peer.ip()).await
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
    /// Device changes in this tenant are announced without details; the panel refetches what it may see.
    Watch {
        tenant: Uuid,
    },
}

/// Bursts of changes become one notice.
const CHANGE_DELAY: Duration = Duration::from_millis(500);

async fn next_change(
    changes: &mut Option<tokio::sync::broadcast::Receiver<Uuid>>,
) -> std::result::Result<Uuid, tokio::sync::broadcast::error::RecvError> {
    match changes {
        Some(changes) => changes.recv().await,
        None => std::future::pending().await,
    }
}

async fn until(due: Option<tokio::time::Instant>) {
    match due {
        Some(at) => tokio::time::sleep_until(at).await,
        None => std::future::pending().await,
    }
}

async fn member(state: &WebAuth, token: &SessionToken, tenant: Uuid) -> bool {
    state
        .authorizer
        .tenant_context(&state.sessions, token, tenant)
        .await
        .is_ok()
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
        let mut watched: Option<Uuid> = None;
        let mut changes = None;
        let mut due = None;
        loop {
            tokio::select! {
                biased;
                event = revoked.recv() => { if event.is_err() || event.is_ok_and(|revoked_id| revoked_id == id) { break; } }
                _ = interval.tick() => {
                    if state.sessions.validate(&token, false).await.is_err() { break; }
                    if let Some(tenant) = watched && !member(&state, &token, tenant).await {
                        watched = None;
                        changes = None;
                        due = None;
                    }
                }
                change = next_change(&mut changes) => match change {
                    Ok(tenant) if Some(tenant) != watched => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => changes = None,
                    _ => { due.get_or_insert_with(|| tokio::time::Instant::now() + CHANGE_DELAY); }
                },
                () = until(due) => {
                    due = None;
                    let Some(tenant) = watched else { continue; };
                    let notice = serde_json::json!({ "type": "devices_changed", "tenant": tenant }).to_string();
                    if !send(&mut socket, Message::Text(notice.into())).await { break; }
                }
                message = socket.recv() => {
                    if window.elapsed() >= Duration::from_secs(10) { window = tokio::time::Instant::now(); messages = 0; }
                    messages += 1;
                    if messages > 30 { break; }
                    let Ok(info) = state.sessions.validate(&token, false).await else { break; };
                    match message {
                        Some(Ok(Message::Text(text))) => {
                            let reply = match serde_json::from_str::<Command>(&text) {
                                Ok(Command::Session) => serde_json::to_string(&info).ok(),
                                Ok(Command::Watch { tenant }) => {
                                    let allowed = member(&state, &token, tenant).await;
                                    watched = allowed.then_some(tenant);
                                    due = None;
                                    if !allowed {
                                        changes = None;
                                    } else if changes.is_none() {
                                        changes = state.enrollment.as_ref().map(|enrollment| enrollment.subscribe());
                                    }
                                    Some(serde_json::json!({ "type": if allowed { "watching" } else { "watch_refused" }, "tenant": tenant }).to_string())
                                }
                                Err(_) => break,
                            };
                            let Some(text) = reply else { break; };
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
