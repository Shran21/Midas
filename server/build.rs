fn main() {
    println!("cargo:rerun-if-changed=migrations");
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        // Vendored OpenSSL does not install its native PDB alongside the static libraries.
        println!("cargo:rustc-link-arg=/IGNORE:4099");
    }
}
