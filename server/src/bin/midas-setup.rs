use std::time::Duration;

use midas_server::{auth::setup, database::Database, transport::TransportError};

#[tokio::main]
async fn main() -> Result<(), TransportError> {
    let origin = std::env::var("MIDAS_AUTH_ORIGIN").map_err(|_| "MIDAS_AUTH_ORIGIN is required")?;
    let minutes: u64 = std::env::var("MIDAS_SETUP_MINUTES")
        .ok()
        .map_or(Ok(30), |value| value.parse())
        .map_err(|_| "MIDAS_SETUP_MINUTES must be a whole number")?;
    let database = Database::from_env()
        .await?
        .ok_or("MIDAS_DATABASE_URL is required")?;
    let token = setup::issue_token(&database, Duration::from_secs(minutes.saturating_mul(60)))
        .await
        .map_err(|_| "setup authorization refused: an administrator may already exist")?;
    println!("{}/#setup={}", origin.trim_end_matches('/'), *token);
    database.close().await;
    Ok(())
}
