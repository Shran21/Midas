use axum_server::{Handle, tls_rustls::RustlsConfig};
use clap::Parser;
use futures_util::StreamExt;
use midas_server::{
    agent_router,
    config::Arguments,
    router,
    transport::{TransportError, pem_resolver, server_config},
    web_router,
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
                .serve(web_router().into_make_service())
                .await
        } else {
            axum_server::from_tcp_rustls(web_listener, RustlsConfig::from_config(public_config))?
                .handle(public_handle)
                .serve(web_router().into_make_service())
                .await
        }
    };
    let agent_handle = handle.clone();
    let operational = async move {
        if let (Some(listener), Some(config)) = (agent_listener, operational_config) {
            tracing::info!(address = %listener.local_addr()?, "mTLS listener started");
            axum_server::from_tcp_rustls(listener, RustlsConfig::from_config(config))?
                .handle(agent_handle)
                .serve(agent_router().into_make_service())
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
