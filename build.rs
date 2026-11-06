//! Links the Windows icon and version info (packaging/windows) into Windows x86_64 and arm64 builds.

fn main() {
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
