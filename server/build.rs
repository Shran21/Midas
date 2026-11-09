use std::{env, fmt::Write, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=migrations");
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let migrations = sqlx_core::migrate::resolve_blocking(&root.join("migrations"))
        .expect("resolve embedded migrations");
    let mut source = String::from(
        "pub static MIGRATIONS: sqlx::migrate::Migrator = sqlx::migrate::Migrator { migrations: std::borrow::Cow::Borrowed(&[",
    );
    for (migration, path) in migrations {
        writeln!(
            source,
            "sqlx::migrate::Migration {{ version: {}, description: std::borrow::Cow::Borrowed({:?}), migration_type: sqlx::migrate::MigrationType::{:?}, sql: sqlx::SqlStr::from_static(include_str!({:?})), checksum: std::borrow::Cow::Borrowed(&{:?}), no_tx: {} }},",
            migration.version,
            migration.description,
            migration.migration_type,
            path.to_str().expect("UTF-8 migration path"),
            migration.checksum,
            migration.no_tx,
        )
        .unwrap();
    }
    source.push_str("]), ..sqlx::migrate::Migrator::DEFAULT };\n");
    let output = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("migrations.rs");
    fs::write(output, source).expect("write embedded migrations");
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        // Vendored OpenSSL does not install its native PDB alongside the static libraries.
        println!("cargo:rustc-link-arg=/IGNORE:4099");
    }
}
