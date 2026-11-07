//! Links the Windows icon and version info (packaging/windows) into Windows x86_64 and arm64 builds.

fn main() {
    // `usb_lasers`: K40 and galvo boards over USB, where the `nusb` library has a backend
    println!("cargo:rustc-check-cfg=cfg(usb_lasers)");
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if matches!(target_os.as_str(), "linux" | "android" | "windows" | "macos") {
        println!("cargo:rustc-cfg=usb_lasers");
    }
    println!("cargo:rerun-if-changed=packaging/windows/burncraft-res-x86_64.o");
    println!("cargo:rerun-if-changed=packaging/windows/burncraft-res-aarch64.o");
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    if os == "windows" && (arch == "x86_64" || arch == "aarch64") {
        let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let res = std::path::Path::new(&dir).join(format!("packaging/windows/burncraft-res-{arch}.o"));
        if res.exists() {
            println!("cargo:rustc-link-arg-bins={}", res.display());
        }
    }
}
