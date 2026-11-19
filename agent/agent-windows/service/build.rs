fn main() {
    // shell32 and ole32 initialize user32 when the process loads, and a remote participant may run on
    // a desktop its token cannot use. These and the capture libraries load only on first use.
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        for library in ["shell32.dll", "ole32.dll", "d3d11.dll", "dxgi.dll"] {
            println!("cargo:rustc-link-arg-bin=midas-agent=/DELAYLOAD:{library}");
        }
        // The consent window is a task dialog, which exists only in Common Controls 6. Every
        // executable of this package declares that version, and loads it only when the window opens.
        let manifest = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap())
            .join("windows.manifest");
        println!("cargo:rerun-if-changed=build.rs");
        println!("cargo:rerun-if-changed=windows.manifest");
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
        println!("cargo:rustc-link-arg=/DELAYLOAD:comctl32.dll");
        println!("cargo:rustc-link-arg=delayimp.lib");
    }
}
