//! Windows agent: service host, protected status pipe and enrollment state.

pub use midas_agent_core::host::{config, runner};

pub mod ipc;
#[cfg(windows)]
pub mod win;

#[cfg(windows)]
pub async fn run_agent(
    paths: config::Paths,
    shutdown: tokio::sync::watch::Receiver<bool>,
    rule: ipc::SessionRule,
    pipe: String,
) {
    let (state, observed) = tokio::sync::watch::channel(runner::State::Starting);
    let status = match std::env::current_exe() {
        Ok(image) => {
            let policy = ipc::Policy {
                session: rule,
                image,
            };
            Some(tokio::spawn(ipc::serve(
                pipe,
                policy,
                observed,
                shutdown.clone(),
            )))
        }
        Err(error) => {
            tracing::error!(%error, "status pipe disabled: program path unknown");
            None
        }
    };
    runner::serve(
        paths,
        midas_protocol::enrollment::Platform::Windows,
        runner::Probes {
            inventory: Some(std::sync::Arc::new(win::inventory::collect)),
            health: Some(std::sync::Arc::new(win::health::check)),
            checks: win::health::CHECKS.to_vec(),
        },
        state,
        shutdown,
    )
    .await;
    if let Some(status) = status
        && let Ok(Err(error)) = status.await
    {
        tracing::error!(%error, "status pipe stopped");
    }
}
