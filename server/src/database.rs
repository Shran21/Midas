use serde::Serialize;
use sqlx::{
    ConnectOptions, Connection, PgConnection, PgPool, Postgres, Transaction,
    postgres::{PgConnectOptions, PgPoolOptions, PgSslMode},
};
use std::{net::IpAddr, str::FromStr, time::Duration};
use uuid::Uuid;

use crate::transport::TransportError;

pub static MIGRATIONS: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

pub fn connection_options(
    url: &str,
    local_plaintext: bool,
) -> Result<PgConnectOptions, TransportError> {
    let options = PgConnectOptions::from_str(url).map_err(|_| "invalid database connection URL")?;
    let mode = if local_plaintext {
        if !options
            .get_host()
            .parse::<IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
        {
            return Err("plaintext database connections require a loopback IP address".into());
        }
        PgSslMode::Disable
    } else {
        PgSslMode::VerifyFull
    };
    Ok(options
        .ssl_mode(mode)
        .disable_statement_logging()
        .application_name("midas"))
}

pub async fn migrate(url: &str, local_plaintext: bool) -> Result<(), TransportError> {
    let mut connection = tokio::time::timeout(
        Duration::from_secs(10),
        PgConnection::connect_with(&connection_options(url, local_plaintext)?),
    )
    .await??;
    let version: i32 =
        sqlx::query_scalar("SELECT current_setting('server_version_num')::integer / 10000")
            .fetch_one(&mut connection)
            .await?;
    if version != 18 {
        return Err("PostgreSQL 18 is required".into());
    }
    sqlx::raw_sql("SET ROLE midas_owner; SET search_path TO midas, pg_catalog;")
        .execute(&mut connection)
        .await?;
    MIGRATIONS.run(&mut connection).await?;
    connection.close().await?;
    Ok(())
}

#[derive(Clone)]
pub struct Database {
    pool: PgPool,
}

impl Database {
    pub(crate) fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub async fn connect(
        url: &str,
        local_plaintext: bool,
        connections: u32,
    ) -> Result<Self, TransportError> {
        if connections == 0 {
            return Err("database pool size must be positive".into());
        }
        let options = connection_options(url, local_plaintext)?;
        let mut probe = tokio::time::timeout(
            Duration::from_secs(10),
            PgConnection::connect_with(&options),
        )
        .await??;
        validate_runtime_role(&mut probe).await?;
        probe.close().await?;
        let pool = PgPoolOptions::new()
            .max_connections(connections)
            .min_connections(0)
            .acquire_timeout(Duration::from_secs(10))
            .idle_timeout(Some(Duration::from_secs(300)))
            .after_connect(|connection, _| {
                Box::pin(async move {
                    validate_runtime_role(connection).await?;
                    reset_context(connection).await
                })
            })
            .before_acquire(|connection, _| {
                Box::pin(async move {
                    reset_context(connection).await?;
                    Ok(true)
                })
            })
            .connect_with(options)
            .await?;
        Ok(Self { pool })
    }

    pub async fn from_env() -> Result<Option<Self>, TransportError> {
        let Some(url) = std::env::var_os("MIDAS_DATABASE_URL") else {
            return Ok(None);
        };
        let url = url
            .into_string()
            .map_err(|_| "invalid database URL encoding")?;
        let connections = std::env::var("MIDAS_DATABASE_POOL_SIZE")
            .unwrap_or_else(|_| "8".into())
            .parse()
            .map_err(|_| "invalid database pool size")?;
        Self::connect(&url, local_plaintext_from_env()?, connections)
            .await
            .map(Some)
    }

    pub async fn begin(&self, tenant: Uuid) -> Result<TenantTransaction<'_>, sqlx::Error> {
        if tenant.is_nil() {
            return Err(sqlx::Error::Protocol("empty tenant identifier".into()));
        }
        let mut transaction = self.pool.begin().await?;
        sqlx::query("SELECT set_config('midas.tenant_id', $1, true)")
            .bind(tenant.to_string())
            .execute(&mut *transaction)
            .await?;
        Ok(TenantTransaction {
            tenant,
            transaction,
        })
    }

    pub async fn ready(&self) -> bool {
        sqlx::query_scalar::<_, i32>("SELECT 1")
            .fetch_one(&self.pool)
            .await
            .is_ok()
    }

    pub async fn close(&self) {
        self.pool.close().await;
    }
}

pub fn local_plaintext_from_env() -> Result<bool, TransportError> {
    match std::env::var("MIDAS_DATABASE_LOCAL_PLAINTEXT").as_deref() {
        Err(std::env::VarError::NotPresent) | Ok("0") => Ok(false),
        Ok("1") => Ok(true),
        _ => Err("MIDAS_DATABASE_LOCAL_PLAINTEXT must be 0 or 1".into()),
    }
}

async fn reset_context(connection: &mut PgConnection) -> Result<(), sqlx::Error> {
    sqlx::query(
        "SELECT set_config('midas.tenant_id', '', false), set_config('row_security', 'on', false)",
    )
    .execute(connection)
    .await?;
    Ok(())
}

async fn validate_runtime_role(connection: &mut PgConnection) -> Result<(), sqlx::Error> {
    let unsafe_role: bool = sqlx::query_scalar(include_str!("../database/validate-role.sql"))
        .fetch_one(connection)
        .await?;
    if unsafe_role {
        return Err(sqlx::Error::Protocol(
            "unsafe or uninitialized runtime database role".into(),
        ));
    }
    Ok(())
}

pub struct TenantTransaction<'a> {
    tenant: Uuid,
    transaction: Transaction<'a, Postgres>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Site {
    pub id: Uuid,
    pub name: String,
}

impl TenantTransaction<'_> {
    pub async fn create_tenant(&mut self, name: &str) -> Result<(), sqlx::Error> {
        sqlx::query("INSERT INTO midas.tenants (id, name) VALUES ($1, $2)")
            .bind(self.tenant)
            .bind(name)
            .execute(&mut *self.transaction)
            .await?;
        Ok(())
    }

    pub async fn create_site(&mut self, name: &str) -> Result<Site, sqlx::Error> {
        sqlx::query_as(
            "INSERT INTO midas.sites (tenant_id, id, name) VALUES ($1, $2, $3) RETURNING id, name",
        )
        .bind(self.tenant)
        .bind(Uuid::new_v4())
        .bind(name)
        .fetch_one(&mut *self.transaction)
        .await
    }

    pub async fn sites(&mut self) -> Result<Vec<Site>, sqlx::Error> {
        sqlx::query_as(
            "SELECT id, name FROM midas.sites WHERE tenant_id = $1 ORDER BY name, id LIMIT 100",
        )
        .bind(self.tenant)
        .fetch_all(&mut *self.transaction)
        .await
    }

    pub async fn commit(self) -> Result<(), sqlx::Error> {
        self.transaction.commit().await
    }

    pub async fn rollback(self) -> Result<(), sqlx::Error> {
        self.transaction.rollback().await
    }
}
