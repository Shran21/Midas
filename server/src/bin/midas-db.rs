use midas_server::{database, transport::TransportError};

#[tokio::main]
async fn main() -> Result<(), TransportError> {
    let url = std::env::var("MIDAS_MIGRATION_DATABASE_URL")
        .map_err(|_| "MIDAS_MIGRATION_DATABASE_URL is required")?;
    database::migrate(&url, database::local_plaintext_from_env()?).await?;
    println!("Database migrations applied");
    Ok(())
}
