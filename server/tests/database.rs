use midas_server::database::{Database, connection_options, migrate};
use sqlx::{
    ConnectOptions, Connection, PgConnection, PgPool,
    postgres::{PgPoolOptions, PgSslMode},
};
use uuid::Uuid;

#[path = "support/authentication.rs"]
mod authentication;
#[path = "support/sessions.rs"]
mod sessions;

#[test]
fn database_transport_requires_verified_tls_or_explicit_loopback() {
    let secure =
        connection_options("postgres://user@example.com/midas?sslmode=disable", false).unwrap();
    assert!(matches!(secure.get_ssl_mode(), PgSslMode::VerifyFull));
    assert!(connection_options("postgres://user@example.com/midas", true).is_err());
    assert!(connection_options("postgres://user@192.168.1.1/midas", true).is_err());
    let local = connection_options("postgres://user@127.0.0.1/midas", true).unwrap();
    assert!(matches!(local.get_ssl_mode(), PgSslMode::Disable));
    let error = connection_options("private-credential", false).unwrap_err();
    assert!(!error.to_string().contains("private-credential"));
}

async fn context(connection: &mut PgConnection, tenant: Uuid) {
    sqlx::query("SELECT set_config('midas.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(connection)
        .await
        .unwrap();
}

async fn expect_denied(
    pool: &PgPool,
    tenant: Uuid,
    statement: &'static str,
    parameters: &[Uuid],
    code: &str,
) {
    let mut tx = pool.begin().await.unwrap();
    context(&mut tx, tenant).await;
    let mut query = sqlx::query(statement);
    for parameter in parameters {
        query = query.bind(*parameter);
    }
    let error = query.execute(&mut *tx).await.unwrap_err();
    assert_eq!(
        error.as_database_error().and_then(|e| e.code()).as_deref(),
        Some(code)
    );
    tx.rollback().await.unwrap();
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL 18 database; run just test-database"]
async fn postgres_isolates_tenants_and_reused_connections() {
    let url = std::env::var("MIDAS_TEST_DATABASE_ADMIN_URL")
        .expect("disposable test database URL required");
    let options = connection_options(&url, true).unwrap();
    assert!(
        options
            .get_database()
            .unwrap_or_default()
            .starts_with("midas_test_")
    );
    let mut admin = PgConnection::connect_with(&options).await.unwrap();
    let version: String = sqlx::query_scalar("SHOW server_version_num")
        .fetch_one(&mut admin)
        .await
        .unwrap();
    assert_eq!(version.parse::<u32>().unwrap() / 10000, 18);
    let existing: i64 = sqlx::query_scalar("SELECT count(*) FROM pg_roles WHERE rolname IN ('midas_owner', 'midas_runtime', 'midas_test_application', 'midas_test_migrator')")
        .fetch_one(&mut admin).await.unwrap();
    assert_eq!(existing, 0, "test requires an unused cluster");
    sqlx::raw_sql(include_str!("../database/bootstrap.sql"))
        .execute(&mut admin)
        .await
        .unwrap();
    let password = Uuid::new_v4().simple().to_string();
    sqlx::query("SELECT set_config('midas.test_password', $1, false)")
        .bind(&password)
        .execute(&mut admin)
        .await
        .unwrap();
    sqlx::raw_sql("DO $$ BEGIN EXECUTE format('CREATE ROLE midas_test_application LOGIN PASSWORD %L', current_setting('midas.test_password')); EXECUTE format('CREATE ROLE midas_test_migrator LOGIN PASSWORD %L', current_setting('midas.test_password')); END $$; GRANT midas_runtime TO midas_test_application; GRANT midas_owner TO midas_test_migrator;")
        .execute(&mut admin).await.unwrap();
    let runtime = options
        .clone()
        .username("midas_test_application")
        .password(&password);
    let runtime_url = runtime.to_url_lossy().to_string();
    let migrator_url = options
        .clone()
        .username("midas_test_migrator")
        .password(&password)
        .to_url_lossy()
        .to_string();
    migrate(&migrator_url, true).await.unwrap();
    migrate(&migrator_url, true).await.unwrap();
    let applied: i64 =
        sqlx::query_scalar("SELECT count(*) FROM midas._sqlx_migrations WHERE success")
            .fetch_one(&mut admin)
            .await
            .unwrap();
    assert_eq!(
        applied,
        midas_server::database::MIGRATIONS.iter().count() as i64
    );
    assert!(Database::connect(&url, true, 1).await.is_err());
    assert!(Database::connect(&migrator_url, true, 1).await.is_err());
    assert!(migrate(&runtime_url, true).await.is_err());
    let database = Database::connect(&runtime_url, true, 1).await.unwrap();
    assert!(database.ready().await);
    assert!(database.begin(Uuid::nil()).await.is_err());
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    let mut sites = Vec::new();
    for (tenant, name) in [(a, "A"), (b, "B")] {
        let mut tx = database.begin(tenant).await.unwrap();
        tx.create_tenant(name).await.unwrap();
        sites.push(tx.create_site(name).await.unwrap().id);
        tx.commit().await.unwrap();
    }
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(runtime)
        .await
        .unwrap();
    for (index, tenant) in [a, b].into_iter().enumerate() {
        let mut tx = pool.begin().await.unwrap();
        context(&mut tx, tenant).await;
        sqlx::query(
            "INSERT INTO midas.device_groups (tenant_id, id, name) VALUES ($1, $2, 'group')",
        )
        .bind(tenant)
        .bind(tenant)
        .execute(&mut *tx)
        .await
        .unwrap();
        sqlx::query("INSERT INTO midas.devices (tenant_id, id, site_id, name, kind, platform) VALUES ($1, $2, $3, 'device', 'server', 'windows')")
            .bind(tenant).bind(tenant).bind(sites[index]).execute(&mut *tx).await.unwrap();
        sqlx::query("INSERT INTO midas.device_group_members (tenant_id, group_id, device_id) VALUES ($1, $1, $1)")
            .bind(tenant).execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
    }
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&pool)
        .await
        .unwrap();
    for tenant in [a, b, a] {
        let mut tx = pool.begin().await.unwrap();
        context(&mut tx, tenant).await;
        let current_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        assert_eq!(pid, current_pid);
        for statement in [
            "SELECT tenant_id FROM midas.sites",
            "SELECT tenant_id FROM midas.device_groups",
            "SELECT tenant_id FROM midas.devices",
            "SELECT tenant_id FROM midas.device_group_members",
        ] {
            let ids: Vec<Uuid> = sqlx::query_scalar(statement)
                .fetch_all(&mut *tx)
                .await
                .unwrap();
            assert_eq!(ids, vec![tenant]);
        }
        let ids: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM midas.tenants")
            .fetch_all(&mut *tx)
            .await
            .unwrap();
        assert_eq!(ids, vec![tenant]);
        tx.commit().await.unwrap();
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM midas.sites")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 0);
        let mut safe = database.begin(tenant).await.unwrap();
        let visible = safe.sites().await.unwrap();
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].name, if tenant == a { "A" } else { "B" });
        safe.rollback().await.unwrap();
    }
    for commit in [false, true] {
        let mut tx = pool.begin().await.unwrap();
        context(&mut tx, a).await;
        if commit {
            tx.rollback().await.unwrap();
        } else {
            drop(tx);
        }
        let ids: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM midas.tenants")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert!(ids.is_empty());
    }
    expect_denied(
        &pool,
        a,
        "INSERT INTO midas.sites VALUES ($1, $2, 'foreign')",
        &[b, Uuid::new_v4()],
        "42501",
    )
    .await;
    expect_denied(
        &pool,
        a,
        "UPDATE midas.sites SET tenant_id = $1",
        &[b],
        "42501",
    )
    .await;
    expect_denied(
        &pool,
        a,
        "UPDATE midas.devices SET site_id = $1",
        &[sites[1]],
        "23503",
    )
    .await;
    expect_denied(
        &pool,
        a,
        "INSERT INTO midas.device_group_members VALUES ($1, $1, $2)",
        &[a, b],
        "23503",
    )
    .await;
    for statement in [
        "TRUNCATE midas.sites CASCADE",
        "ALTER TABLE midas.sites DISABLE ROW LEVEL SECURITY",
        "SET ROLE midas_owner",
        "SELECT * FROM midas._sqlx_migrations",
        "CREATE TABLE midas.intrusion (id int)",
    ] {
        expect_denied(&pool, a, statement, &[], "42501").await;
    }
    let mut tx = pool.begin().await.unwrap();
    context(&mut tx, a).await;
    for statement in [
        "UPDATE midas.sites SET name = 'wrong' WHERE tenant_id = $1",
        "DELETE FROM midas.sites WHERE tenant_id = $1",
    ] {
        assert_eq!(
            sqlx::query(statement)
                .bind(b)
                .execute(&mut *tx)
                .await
                .unwrap()
                .rows_affected(),
            0
        );
    }
    tx.commit().await.unwrap();
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("SELECT set_config('midas.tenant_id', 'invalid', true)")
        .execute(&mut *tx)
        .await
        .unwrap();
    assert!(
        sqlx::query("SELECT * FROM midas.tenants")
            .execute(&mut *tx)
            .await
            .is_err()
    );
    tx.rollback().await.unwrap();
    assert!(
        sqlx::query("INSERT INTO midas.tenants (id, name) VALUES ($1, 'no context')")
            .bind(Uuid::new_v4())
            .execute(&pool)
            .await
            .is_err()
    );
    for (grant, undo) in [
        (
            "ALTER ROLE midas_test_application BYPASSRLS",
            "ALTER ROLE midas_test_application NOBYPASSRLS",
        ),
        (
            "GRANT midas_owner TO midas_test_application WITH INHERIT FALSE",
            "REVOKE midas_owner FROM midas_test_application",
        ),
        (
            "ALTER TABLE midas.sites NO FORCE ROW LEVEL SECURITY",
            "ALTER TABLE midas.sites FORCE ROW LEVEL SECURITY",
        ),
    ] {
        sqlx::raw_sql(grant).execute(&mut admin).await.unwrap();
        assert!(Database::connect(&runtime_url, true, 1).await.is_err());
        sqlx::raw_sql(undo).execute(&mut admin).await.unwrap();
    }
    let mut owner = PgConnection::connect_with(&connection_options(&migrator_url, true).unwrap())
        .await
        .unwrap();
    sqlx::query("SET ROLE midas_owner")
        .execute(&mut owner)
        .await
        .unwrap();
    let owner_rows: i64 = sqlx::query_scalar("SELECT count(*) FROM midas.tenants")
        .fetch_one(&mut owner)
        .await
        .unwrap();
    assert_eq!(owner_rows, 0);
    owner.close().await.unwrap();
    let authentication_database = Database::connect(&runtime_url, true, 4).await.unwrap();
    authentication::verify(&authentication_database, &mut admin).await;
    sessions::verify(&authentication_database, &mut admin).await;
    authentication_database.close().await;
    pool.close().await;
    database.close().await;
    admin.close().await.unwrap();
}
