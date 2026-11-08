use axum_server::{Handle, tls_rustls::RustlsConfig};
use clap::Parser;
use futures_util::StreamExt;
use midas_server::{
    agent::AgentProtocol,
    agent_router_with,
    config::Arguments,
    enrollment::{Authority, Enrollment},
    router,
    transport::{PeerCertificates, TransportError, pem_resolver, server_config},
    web_router_with,
};
use std::{sync::Arc, time::Duration};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), TransportError> {
    let args = Arguments::parse();
    args.validate()?;
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .json()
        .init();
    let database = midas_server::database::Database::from_env().await?;
    if database.is_some() {
        tracing::info!("runtime database connected");
    }
    let enrollment = match (&database, &args.agent_ca_key) {
        (Some(database), Some(key)) => {
            let certificate =
                std::fs::read_to_string(args.agent_ca.as_ref().ok_or("missing agent CA")?)?;
            let key = midas_server::config::private_pem(key)?;
            Some(Arc::new(Enrollment::new(
                database,
                Authority::from_pem(&certificate, &key)?,
            )))
        }
        (None, Some(_)) => return Err("device enrollment requires a database".into()),
        _ => None,
    };
    if args.agent_bind.is_some() && enrollment.is_none() {
        return Err("the agent listener requires a database and --agent-ca-key".into());
    }
    if let Some(enrollment) = enrollment.clone() {
        let mailer = match &args.smtp_host {
            Some(host) => {
                let credentials = match (&args.smtp_user, &args.smtp_password_file) {
                    (Some(user), Some(file)) => Some((
                        user.clone(),
                        zeroize::Zeroizing::new(
                            std::fs::read_to_string(file)?.trim_end().to_owned(),
                        ),
                    )),
                    _ => None,
                };
                let settings = midas_server::alerts::SmtpSettings {
                    host: host.clone(),
                    port: args.smtp_port,
                    security: match args.smtp_security.as_str() {
                        "tls" => midas_server::alerts::SmtpSecurity::Tls,
                        "none" => midas_server::alerts::SmtpSecurity::None,
                        _ => midas_server::alerts::SmtpSecurity::Starttls,
                    },
                    credentials,
                    from: args.smtp_from.clone().unwrap_or_default(),
                };
                midas_server::alerts::Mailer::smtp(&settings)?
            }
            None => {
                tracing::warn!("no SMTP relay configured; alert e-mail stays queued");
                midas_server::alerts::Mailer::Disabled
            }
        };
        let delivering = enrollment.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(30));
            loop {
                tick.tick().await;
                if let Err(error) = delivering.deliver(&mailer).await {
                    tracing::error!(%error, "alert delivery failed");
                }
            }
        });
        tokio::spawn(async move {
            let keepalive = AgentProtocol::default().keepalive;
            let mut tick = tokio::time::interval(keepalive);
            let mut maintained: Option<std::time::Instant> = None;
            loop {
                tick.tick().await;
                if let Err(error) = enrollment.sweep(keepalive.saturating_mul(3)).await {
                    tracing::error!(%error, "offline sweep failed");
                }
                if let Err(error) = enrollment.alert_sweep().await {
                    tracing::error!(%error, "alert sweep failed");
                }
                if maintained.is_none_or(|at| at.elapsed() >= Duration::from_secs(3600)) {
                    enrollment.maintain().await;
                    maintained = Some(std::time::Instant::now());
                }
            }
        });
    }
    let mut web = web_router_with(enrollment.clone());
    if let Some(origin) = &args.auth_origin {
        use midas_server::auth::{AuthConfig, http::WebAuth, sessions::SessionConfig};
        let database = database
            .as_ref()
            .ok_or("browser authentication requires a database")?;
        let key = midas_server::config::authentication_key(
            args.auth_key_file
                .as_deref()
                .ok_or("missing authentication key file")?,
        )?;
        let mut auth = WebAuth::new(
            database,
            AuthConfig::new(origin.clone(), key),
            SessionConfig {
                absolute_lifetime: Duration::from_secs(args.session_seconds),
                idle_lifetime: Duration::from_secs(args.session_idle_seconds),
            },
            args.session_socket_capacity,
        )
        .await?;
        if let Some(enrollment) = &enrollment {
            auth = auth.with_enrollment(enrollment.clone());
        }
        web = web.merge(midas_server::auth::http::router(Arc::new(auth)));
        tracing::info!("browser authentication enabled");
    }
    if args.tls_cert.is_none() && args.acme_domain.is_empty() {
        let listener = tokio::net::TcpListener::bind(args.bind).await?;
        tracing::info!(address = %listener.local_addr()?, "development server listening");
        axum::serve(listener, router())
            .with_graceful_shutdown(shutdown_signal())
            .await?;
        return Ok(());
    }
    let handle = Handle::new();
    let web_listener = std::net::TcpListener::bind(args.bind)?;
    web_listener.set_nonblocking(true)?;
    let agent_listener = args
        .agent_bind
        .map(std::net::TcpListener::bind)
        .transpose()?;
    if let Some(listener) = &agent_listener {
        listener.set_nonblocking(true)?;
    }
    let ca = args.agent_ca.as_ref().map(std::fs::read).transpose()?;
    let crl = args.agent_crl.as_ref().map(std::fs::read).transpose()?;
    let mut acme = if let Some(cache) = &args.acme_cache {
        if !cache.is_dir() {
            return Err("ACME cache must be an existing private directory".into());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if std::fs::metadata(cache)?.permissions().mode() & 0o077 != 0 {
                return Err("ACME cache permissions must exclude group and other users".into());
            }
        }
        Some(
            rustls_acme::AcmeConfig::new(args.acme_domain.clone())
                .contact([format!(
                    "mailto:{}",
                    args.acme_contact.as_deref().unwrap_or_default()
                )])
                .cache(rustls_acme::caches::DirCache::new(cache.clone()))
                .directory_lets_encrypt(args.acme_production)
                .state(),
        )
    } else {
        None
    };
    let resolver: Arc<dyn rustls::server::ResolvesServerCert> = if let Some(state) = &acme {
        state.resolver()
    } else {
        pem_resolver(
            &std::fs::read(args.tls_cert.as_ref().ok_or("missing TLS certificate")?)?,
            &std::fs::read(args.tls_key.as_ref().ok_or("missing TLS key")?)?,
        )?
    };
    let public_config = server_config(resolver.clone(), None, None)?;
    let operational_config = ca
        .as_deref()
        .map(|ca| server_config(resolver, Some(ca), crl.as_deref()))
        .transpose()?;
    let acme_acceptor = acme
        .as_ref()
        .map(|state| state.axum_acceptor(public_config.clone()));
    let worker = acme.take().map(|mut state| {
        tokio::spawn(async move {
            while let Some(event) = state.next().await {
                match event {
                    Ok(event) => tracing::info!(?event, "ACME state updated"),
                    Err(_) => {
                        tracing::warn!("ACME operation failed; automatic retry remains active")
                    }
                }
            }
        })
    });
    let public_handle = handle.clone();
    let public = async move {
        tracing::info!(address = %web_listener.local_addr()?, "HTTPS listener started");
        if let Some(acceptor) = acme_acceptor {
            axum_server::from_tcp(web_listener)?
                .acceptor(acceptor)
                .handle(public_handle)
                .serve(web.into_make_service_with_connect_info::<std::net::SocketAddr>())
                .await
        } else {
            axum_server::from_tcp_rustls(web_listener, RustlsConfig::from_config(public_config))?
                .handle(public_handle)
                .serve(web.into_make_service_with_connect_info::<std::net::SocketAddr>())
                .await
        }
    };
    let agent_handle = handle.clone();
    let operational = async move {
        if let (Some(listener), Some(config)) = (agent_listener, operational_config) {
            tracing::info!(address = %listener.local_addr()?, "mTLS listener started");
            let protocol = AgentProtocol {
                identities: enrollment,
                ..AgentProtocol::default()
            };
            axum_server::from_tcp(listener)?
                .acceptor(PeerCertificates::new(config))
                .handle(agent_handle)
                .serve(agent_router_with(protocol).into_make_service())
                .await
        } else {
            std::future::pending::<std::io::Result<()>>().await
        }
    };
    tokio::pin!(public, operational);
    let result = tokio::select! {
        result = &mut public => result,
        result = &mut operational => result,
        () = shutdown_signal() => {
            handle.graceful_shutdown(Some(Duration::from_secs(10)));
            if args.agent_bind.is_some() { tokio::try_join!(&mut public, &mut operational).map(|_| ()) }
            else { public.await }
        }
    };
    handle.shutdown();
    if let Some(worker) = worker {
        worker.abort();
    }
    result?;
    tracing::info!("server stopped");
    Ok(())
}

async fn shutdown_signal() {
    let interrupt = async {
        if let Err(error) = tokio::signal::ctrl_c().await {
            tracing::error!(%error, "failed to install interrupt handler");
        }
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(error) => tracing::error!(%error, "failed to install termination handler"),
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = interrupt => {},
        () = terminate => {},
    }
}
