fn main() {
    // Loading user32, or gdi32, shell32 and ole32 which initialize it, connects the process to its desktop,
    // and a remote participant may run on a desktop its token cannot use. These and the capture
    // libraries load only on first use, in every executable: test programs also start as
    // participants in the isolated trial.
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        for library in [
            "user32.dll",
            "gdi32.dll",
            "shell32.dll",
            "ole32.dll",
            "d3d11.dll",
            "dxgi.dll",
            "sas.dll",
        ] {
            println!("cargo:rustc-link-arg=/DELAYLOAD:{library}");
        }
        // The consent window is a task dialog, which exists only in Common Controls 6. The manifest
        // that selects it is a resource the window activates for itself, not the process manifest:
        // a process manifest would make every participant initialize its desktop connection.
        let manifest = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap())
            .join("windows.manifest");
        println!("cargo:rerun-if-changed=build.rs");
        println!("cargo:rerun-if-changed=windows.manifest");
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED,ID=2");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
        println!("cargo:rustc-link-arg=/DELAYLOAD:comctl32.dll");
        // Media Foundation is an optional Windows component; it loads only for software encoding.
        println!("cargo:rustc-link-arg=/DELAYLOAD:mfplat.dll");
        println!("cargo:rustc-link-arg=delayimp.lib");
    }
}
