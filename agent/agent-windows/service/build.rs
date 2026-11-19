fn main() {
    // shell32 and ole32 initialize user32 when the process loads, and a remote participant may run on
    // a desktop its token cannot use. These and the capture libraries load only on first use.
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        for library in ["shell32.dll", "ole32.dll", "d3d11.dll", "dxgi.dll"] {
            println!("cargo:rustc-link-arg-bin=midas-agent=/DELAYLOAD:{library}");
        }
        println!("cargo:rustc-link-arg-bin=midas-agent=delayimp.lib");
    }
}
