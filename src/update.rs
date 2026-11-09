//! Update checker: asks GitHub for the latest BurnCraft release, and downloads the file that matches how this
//! copy was installed (.rpm, .deb, Flatpak, AppImage, tarball, Windows installer or zip, macOS .dmg) and its
//! processor (x86_64, arm64, riscv64). Nothing is installed by itself: the downloaded file is handed to the
//! system (software installer, Windows installer, Finder) or shown in its folder.

use serde_json::Value;

pub const REPO: &str = "amruthwo/BurnCraft";
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallKind {
    Rpm,
    Deb,
    Flatpak,
    AppImage,
    LinuxTarball,
    WindowsSetup,
    WindowsMsi,
    WindowsPortable,
    MacApp,
    FreeBsdTarball,
    Browser,
}

impl InstallKind {
    pub fn label(self) -> &'static str {
        match self {
            InstallKind::Rpm => ".rpm package",
            InstallKind::Deb => ".deb package",
            InstallKind::Flatpak => "Flatpak",
            InstallKind::AppImage => "AppImage",
            InstallKind::LinuxTarball => "portable Linux .tar.gz",
            InstallKind::WindowsSetup => "Windows setup (.exe)",
            InstallKind::WindowsMsi => "Windows installer (.msi)",
            InstallKind::WindowsPortable => "portable Windows .zip",
            InstallKind::MacApp => "macOS app",
            InstallKind::FreeBsdTarball => "portable FreeBSD .tar.gz",
            InstallKind::Browser => "browser version",
        }
    }
    /// Downloads that come in a package the system installs (as opposed to unpacking yourself).
    pub fn installs(self) -> bool {
        matches!(self, InstallKind::Rpm | InstallKind::Deb | InstallKind::Flatpak | InstallKind::WindowsSetup | InstallKind::WindowsMsi | InstallKind::MacApp)
    }
}

/// What can tell us how this copy was installed (gathered by `detect`, passed in so tests can fake it).
pub struct Clues<'a> {
    pub os: &'a str,
    pub exe: &'a str,
    pub flatpak: bool,
    pub appimage: bool,
    pub dpkg_owns: bool,
    pub rpm_system: bool,
    pub uninstaller_beside: bool,
}

pub fn kind_from(c: &Clues) -> InstallKind {
    let exe = c.exe.replace('\\', "/").to_lowercase();
    match c.os {
        "linux" => {
            if c.flatpak {
                InstallKind::Flatpak
            } else if c.appimage {
                InstallKind::AppImage
            } else if exe.starts_with("/usr/") || exe.starts_with("/opt/") {
                if c.dpkg_owns {
                    InstallKind::Deb
                } else if c.rpm_system {
                    InstallKind::Rpm
                } else {
                    InstallKind::LinuxTarball
                }
            } else {
                InstallKind::LinuxTarball
            }
        }
        "windows" => {
            if c.uninstaller_beside {
                InstallKind::WindowsSetup
            } else if exe.contains("/program files") {
                InstallKind::WindowsMsi
            } else {
                InstallKind::WindowsPortable
            }
        }
        "macos" => InstallKind::MacApp,
        "freebsd" => InstallKind::FreeBsdTarball,
        _ => InstallKind::Browser,
    }
}

/// How this copy was installed, by looking at where it runs from and what's around it.
pub fn detect() -> InstallKind {
    #[cfg(target_arch = "wasm32")]
    {
        InstallKind::Browser
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let exe = std::env::current_exe().ok();
        let exe_s = exe.as_ref().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
        let beside = |name: &str| exe.as_ref().and_then(|p| p.parent()).map(|d| d.join(name).exists()).unwrap_or(false);
        let clues = Clues {
            os: std::env::consts::OS,
            exe: &exe_s,
            flatpak: std::env::var_os("FLATPAK_ID").is_some() || std::path::Path::new("/.flatpak-info").exists(),
            appimage: std::env::var_os("APPIMAGE").is_some(),
            dpkg_owns: std::path::Path::new("/var/lib/dpkg/info/burncraft.list").exists(),
            rpm_system: std::path::Path::new("/var/lib/rpm").exists() || std::path::Path::new("/usr/lib/sysimage/rpm").exists(),
            uninstaller_beside: beside("uninstall.exe") || beside("Uninstall.exe"),
        };
        kind_from(&clues)
    }
}

/// The names this processor goes by in release file names.
pub fn arch_names(arch: &str) -> &'static [&'static str] {
    match arch {
        "x86_64" => &["x86_64", "amd64", "x64"],
        "aarch64" => &["aarch64", "arm64"],
        "riscv64" => &["riscv64"],
        _ => &[],
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
    /// "sha256:…" when GitHub publishes it.
    pub digest: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub tag: String,
    pub page: String,
    pub notes: String,
    pub assets: Vec<Asset>,
}

/// "v0.1.10" / "0.1.10-beta" → (0, 1, 10).
pub fn version(s: &str) -> Option<(u64, u64, u64)> {
    let s = s.trim().trim_start_matches(['v', 'V']);
    let core = s.split(['-', '+']).next()?;
    let mut it = core.split('.').map(|p| p.parse::<u64>().ok());
    Some((it.next()??, it.next().flatten().unwrap_or(0), it.next().flatten().unwrap_or(0)))
}

pub fn is_newer(latest: &str, current: &str) -> bool {
    matches!((version(latest), version(current)), (Some(a), Some(b)) if a > b)
}

pub fn parse_release(v: &Value) -> Result<Release, String> {
    let tag = v.get("tag_name").and_then(|t| t.as_str()).ok_or("GitHub's answer had no version")?.to_string();
    let assets = v
        .get("assets")
        .and_then(|a| a.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| {
                    Some(Asset {
                        name: x.get("name")?.as_str()?.to_string(),
                        url: x.get("browser_download_url")?.as_str()?.to_string(),
                        size: x.get("size").and_then(|s| s.as_u64()).unwrap_or(0),
                        digest: x.get("digest").and_then(|d| d.as_str()).map(|d| d.to_string()),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(Release {
        tag,
        page: v.get("html_url").and_then(|t| t.as_str()).unwrap_or("https://github.com/amruthwo/BurnCraft/releases").to_string(),
        notes: v.get("body").and_then(|t| t.as_str()).unwrap_or("").to_string(),
        assets,
    })
}

/// The release file for this kind of install and processor, if the release has one.
pub fn pick_asset<'a>(rel: &'a Release, kind: InstallKind, arch: &str) -> Option<&'a Asset> {
    let archs = arch_names(arch);
    let has_arch = |n: &str| archs.iter().any(|a| n.contains(a));
    let names_any_arch = |n: &str| ["x86_64", "amd64", "x64", "aarch64", "arm64", "riscv64"].iter().any(|a| n.contains(a));
    let find = |test: &dyn Fn(&str) -> bool| rel.assets.iter().find(|a| test(&a.name.to_lowercase()));
    match kind {
        InstallKind::Rpm => find(&|n| n.ends_with(".rpm") && has_arch(n)),
        InstallKind::Deb => find(&|n| n.ends_with(".deb") && has_arch(n)),
        InstallKind::AppImage => find(&|n| n.ends_with(".appimage") && has_arch(n)),
        // a Flatpak file without a processor in its name is the x86_64 one
        InstallKind::Flatpak => find(&|n| n.ends_with(".flatpak") && (has_arch(n) || (!names_any_arch(n) && arch == "x86_64"))),
        InstallKind::LinuxTarball => find(&|n| n.contains("linux") && n.ends_with(".tar.gz") && has_arch(n)),
        InstallKind::FreeBsdTarball => find(&|n| n.contains("freebsd") && n.ends_with(".tar.gz") && has_arch(n)),
        InstallKind::WindowsPortable => find(&|n| n.contains("windows") && n.ends_with(".zip") && has_arch(n)),
        // either Windows installer will do if the usual one isn't built for this processor
        InstallKind::WindowsMsi => find(&|n| n.ends_with(".msi") && has_arch(n)).or_else(|| find(&|n| n.ends_with("-setup.exe") && has_arch(n))),
        InstallKind::WindowsSetup => find(&|n| n.ends_with("-setup.exe") && has_arch(n)).or_else(|| find(&|n| n.ends_with(".msi") && has_arch(n))),
        // macOS builds are universal (Apple silicon and Intel)
        InstallKind::MacApp => find(&|n| n.ends_with(".dmg")).or_else(|| find(&|n| n.ends_with(".app.zip"))),
        InstallKind::Browser => None,
    }
}

/// The plain file-name part of a release file's name, so a download can never land outside the Downloads
/// folder: anything before the last `/` or `\` is dropped, and empty, `.`/`..`, drive-style (`:`) or
/// control-character names are refused.
pub fn safe_file_name(name: &str) -> Result<String, String> {
    let last = name.rsplit(['/', '\\']).next().unwrap_or("").trim();
    if last.is_empty() || last == "." || last == ".." || last.contains(':') || last.chars().any(|c| c.is_control()) {
        return Err(format!("the release file name \u{201c}{}\u{201d} isn't a plain file name", name.escape_debug()));
    }
    Ok(last.to_string())
}

#[derive(Clone, Debug)]
pub enum State {
    Idle,
    Checking,
    UpToDate(String),
    Available { release: Release, asset: Option<Asset> },
    Downloading { asset: Asset, done: u64 },
    Downloaded(std::path::PathBuf),
    Failed(String),
}

#[cfg(not(target_arch = "wasm32"))]
pub mod net {
    use super::*;
    use std::io::{Read, Write};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    pub type Shared = Arc<Mutex<State>>;

    fn agent() -> ureq::Agent {
        ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(3600))).timeout_connect(Some(Duration::from_secs(15))).build().into()
    }

    /// Ask GitHub for the latest release (on a background thread).
    pub fn check(state: Shared) {
        *state.lock().unwrap() = State::Checking;
        std::thread::spawn(move || {
            let r = (|| {
                let mut resp = agent()
                    .get(format!("https://api.github.com/repos/{REPO}/releases/latest"))
                    .header("User-Agent", format!("BurnCraft/{CURRENT}"))
                    .header("Accept", "application/vnd.github+json")
                    .call()
                    .map_err(|e| format!("couldn't reach GitHub ({e})"))?;
                let v: Value = resp.body_mut().read_json().map_err(|e| format!("unexpected answer from GitHub ({e})"))?;
                parse_release(&v)
            })();
            let s = match r {
                Ok(rel) if is_newer(&rel.tag, CURRENT) => {
                    let asset = pick_asset(&rel, detect(), std::env::consts::ARCH).cloned();
                    State::Available { release: rel, asset }
                }
                Ok(rel) => State::UpToDate(rel.tag),
                Err(e) => State::Failed(e),
            };
            *state.lock().unwrap() = s;
        });
    }

    /// Where downloads go: the Downloads folder, else the temp folder.
    pub fn download_dir() -> std::path::PathBuf {
        directories::UserDirs::new().and_then(|u| u.download_dir().map(|d| d.to_path_buf())).unwrap_or_else(std::env::temp_dir)
    }

    /// Download `asset` (on a background thread), checking its size and, when GitHub gives one, its SHA-256.
    pub fn download(state: Shared, asset: Asset) {
        let name = match safe_file_name(&asset.name) {
            Ok(n) => n,
            Err(e) => {
                *state.lock().unwrap() = State::Failed(e);
                return;
            }
        };
        *state.lock().unwrap() = State::Downloading { asset: asset.clone(), done: 0 };
        std::thread::spawn(move || {
            let dir = download_dir();
            let dest = dir.join(&name);
            let part = dir.join(format!("{name}.part"));
            let r = (|| {
                let resp = agent().get(&asset.url).header("User-Agent", format!("BurnCraft/{CURRENT}")).call().map_err(|e| format!("download failed ({e})"))?;
                let mut reader = resp.into_body().into_reader();
                let mut file = std::fs::File::create(&part).map_err(|e| format!("can't write {} ({e})", part.display()))?;
                let mut hasher = <sha2::Sha256 as sha2::Digest>::new();
                let mut buf = vec![0u8; 64 * 1024];
                let mut done = 0u64;
                loop {
                    let n = reader.read(&mut buf).map_err(|e| format!("download interrupted ({e})"))?;
                    if n == 0 {
                        break;
                    }
                    file.write_all(&buf[..n]).map_err(|e| format!("can't write the file ({e})"))?;
                    sha2::Digest::update(&mut hasher, &buf[..n]);
                    done += n as u64;
                    if let Ok(mut s) = state.lock() {
                        *s = State::Downloading { asset: asset.clone(), done };
                    }
                }
                drop(file);
                if asset.size > 0 && done != asset.size {
                    return Err(format!("the download is incomplete ({done} of {} bytes)", asset.size));
                }
                if let Some(want) = asset.digest.as_deref().and_then(|d| d.strip_prefix("sha256:")) {
                    let got: String = sha2::Digest::finalize(hasher).iter().map(|b| format!("{b:02x}")).collect();
                    if !got.eq_ignore_ascii_case(want) {
                        return Err("the download doesn't match GitHub's checksum; it was deleted".into());
                    }
                }
                std::fs::rename(&part, &dest).map_err(|e| format!("can't save the file ({e})"))?;
                Ok(dest.clone())
            })();
            let s = match r {
                Ok(p) => State::Downloaded(p),
                Err(e) => {
                    let _ = std::fs::remove_file(&part);
                    State::Failed(e)
                }
            };
            *state.lock().unwrap() = s;
        });
    }

    /// Hand a file or folder to the system: its installer / file manager / browser.
    pub fn open(target: &str) -> Result<(), String> {
        // Explorer opens installers and folders with no shell in between, so characters like & ^ | % in a
        // path (e.g. C:\Users\Tom&Jerry\Downloads) stay part of the path
        #[cfg(target_os = "windows")]
        let r = std::process::Command::new("explorer.exe").arg(target).spawn();
        #[cfg(target_os = "macos")]
        let r = std::process::Command::new("open").arg(target).spawn();
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        let r = std::process::Command::new("xdg-open").arg(target).spawn();
        r.map(|_| ()).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release() -> Release {
        // the real v0.1.2 file names
        let names = [
            "burncraft-0.1.2-1.aarch64.rpm",
            "burncraft-0.1.2-1.x86_64.rpm",
            "BurnCraft-0.1.2-macos-universal.app.zip",
            "BurnCraft-0.1.2-macos-universal.dmg",
            "burncraft-freebsd-arm64.tar.gz",
            "burncraft-freebsd-x86_64.tar.gz",
            "burncraft-linux-arm64.tar.gz",
            "burncraft-linux-riscv64.tar.gz",
            "burncraft-linux-x86_64.tar.gz",
            "burncraft-web.zip",
            "burncraft-windows-arm64-portable.zip",
            "burncraft-windows-x86_64-portable.zip",
            "burncraft.flatpak",
            "burncraft_0.1.2_amd64.deb",
            "burncraft_0.1.2_arm64-setup.exe",
            "burncraft_0.1.2_arm64.deb",
            "burncraft_0.1.2_x64-setup.exe",
            "burncraft_0.1.2_x64_en-US.msi",
        ];
        Release { tag: "v0.1.2".into(), page: String::new(), notes: String::new(), assets: names.iter().map(|n| Asset { name: n.to_string(), url: format!("https://x/{n}"), size: 1, digest: None }).collect() }
    }

    #[test]
    fn picks_the_file_for_this_install_and_processor() {
        let r = release();
        let pick = |k, a| pick_asset(&r, k, a).map(|x| x.name.clone());
        assert_eq!(pick(InstallKind::Rpm, "x86_64").as_deref(), Some("burncraft-0.1.2-1.x86_64.rpm"));
        assert_eq!(pick(InstallKind::Rpm, "aarch64").as_deref(), Some("burncraft-0.1.2-1.aarch64.rpm"));
        assert_eq!(pick(InstallKind::Rpm, "riscv64"), None, "no riscv64 rpm: don't offer another processor's");
        assert_eq!(pick(InstallKind::Deb, "x86_64").as_deref(), Some("burncraft_0.1.2_amd64.deb"));
        assert_eq!(pick(InstallKind::Deb, "aarch64").as_deref(), Some("burncraft_0.1.2_arm64.deb"));
        assert_eq!(pick(InstallKind::Flatpak, "x86_64").as_deref(), Some("burncraft.flatpak"));
        assert_eq!(pick(InstallKind::Flatpak, "aarch64"), None);
        assert_eq!(pick(InstallKind::LinuxTarball, "riscv64").as_deref(), Some("burncraft-linux-riscv64.tar.gz"));
        assert_eq!(pick(InstallKind::FreeBsdTarball, "x86_64").as_deref(), Some("burncraft-freebsd-x86_64.tar.gz"));
        assert_eq!(pick(InstallKind::WindowsMsi, "x86_64").as_deref(), Some("burncraft_0.1.2_x64_en-US.msi"));
        assert_eq!(pick(InstallKind::WindowsMsi, "aarch64").as_deref(), Some("burncraft_0.1.2_arm64-setup.exe"), "no arm64 msi: the setup .exe");
        assert_eq!(pick(InstallKind::WindowsSetup, "x86_64").as_deref(), Some("burncraft_0.1.2_x64-setup.exe"));
        assert_eq!(pick(InstallKind::WindowsPortable, "aarch64").as_deref(), Some("burncraft-windows-arm64-portable.zip"));
        assert_eq!(pick(InstallKind::MacApp, "aarch64").as_deref(), Some("BurnCraft-0.1.2-macos-universal.dmg"));
        assert_eq!(pick(InstallKind::AppImage, "x86_64"), None, "no AppImage in this release");
    }

    #[test]
    fn picks_files_with_the_standard_release_names() {
        // the names .github/scripts/release-names.sh gives release files from v0.1.3 on
        let names = [
            "burncraft-0.1.3-freebsd-arm64.tar.gz",
            "burncraft-0.1.3-freebsd-x86_64.tar.gz",
            "burncraft-0.1.3-linux-arm64.deb",
            "burncraft-0.1.3-linux-arm64.rpm",
            "burncraft-0.1.3-linux-arm64.tar.gz",
            "burncraft-0.1.3-linux-riscv64.tar.gz",
            "burncraft-0.1.3-linux-x86_64.AppImage",
            "burncraft-0.1.3-linux-x86_64.deb",
            "burncraft-0.1.3-linux-x86_64.flatpak",
            "burncraft-0.1.3-linux-x86_64.rpm",
            "burncraft-0.1.3-linux-x86_64.tar.gz",
            "burncraft-0.1.3-macos-universal.app.zip",
            "burncraft-0.1.3-macos-universal.dmg",
            "burncraft-0.1.3-web.zip",
            "burncraft-0.1.3-windows-arm64-portable.zip",
            "burncraft-0.1.3-windows-arm64-setup.exe",
            "burncraft-0.1.3-windows-x86_64.msi",
            "burncraft-0.1.3-windows-x86_64-portable.zip",
            "burncraft-0.1.3-windows-x86_64-setup.exe",
        ];
        let r = Release { tag: "v0.1.3".into(), page: String::new(), notes: String::new(), assets: names.iter().map(|n| Asset { name: n.to_string(), url: format!("https://x/{n}"), size: 1, digest: None }).collect() };
        let pick = |k, a| pick_asset(&r, k, a).map(|x| x.name.clone());
        assert_eq!(pick(InstallKind::Rpm, "x86_64").as_deref(), Some("burncraft-0.1.3-linux-x86_64.rpm"));
        assert_eq!(pick(InstallKind::Rpm, "aarch64").as_deref(), Some("burncraft-0.1.3-linux-arm64.rpm"));
        assert_eq!(pick(InstallKind::Deb, "x86_64").as_deref(), Some("burncraft-0.1.3-linux-x86_64.deb"));
        assert_eq!(pick(InstallKind::Deb, "aarch64").as_deref(), Some("burncraft-0.1.3-linux-arm64.deb"));
        assert_eq!(pick(InstallKind::AppImage, "x86_64").as_deref(), Some("burncraft-0.1.3-linux-x86_64.AppImage"));
        assert_eq!(pick(InstallKind::Flatpak, "x86_64").as_deref(), Some("burncraft-0.1.3-linux-x86_64.flatpak"));
        assert_eq!(pick(InstallKind::Flatpak, "aarch64"), None);
        assert_eq!(pick(InstallKind::LinuxTarball, "x86_64").as_deref(), Some("burncraft-0.1.3-linux-x86_64.tar.gz"));
        assert_eq!(pick(InstallKind::LinuxTarball, "riscv64").as_deref(), Some("burncraft-0.1.3-linux-riscv64.tar.gz"));
        assert_eq!(pick(InstallKind::FreeBsdTarball, "x86_64").as_deref(), Some("burncraft-0.1.3-freebsd-x86_64.tar.gz"));
        assert_eq!(pick(InstallKind::FreeBsdTarball, "aarch64").as_deref(), Some("burncraft-0.1.3-freebsd-arm64.tar.gz"));
        assert_eq!(pick(InstallKind::FreeBsdTarball, "riscv64"), None, "no FreeBSD riscv64 build");
        assert_eq!(pick(InstallKind::WindowsMsi, "x86_64").as_deref(), Some("burncraft-0.1.3-windows-x86_64.msi"));
        assert_eq!(pick(InstallKind::WindowsMsi, "aarch64").as_deref(), Some("burncraft-0.1.3-windows-arm64-setup.exe"), "no arm64 msi: the setup .exe");
        assert_eq!(pick(InstallKind::WindowsSetup, "x86_64").as_deref(), Some("burncraft-0.1.3-windows-x86_64-setup.exe"));
        assert_eq!(pick(InstallKind::WindowsPortable, "x86_64").as_deref(), Some("burncraft-0.1.3-windows-x86_64-portable.zip"));
        assert_eq!(pick(InstallKind::WindowsPortable, "aarch64").as_deref(), Some("burncraft-0.1.3-windows-arm64-portable.zip"));
        assert_eq!(pick(InstallKind::MacApp, "aarch64").as_deref(), Some("burncraft-0.1.3-macos-universal.dmg"));
    }

    #[test]
    fn downloads_keep_only_a_plain_file_name() {
        assert_eq!(safe_file_name("burncraft-0.1.3-linux-x86_64.rpm").unwrap(), "burncraft-0.1.3-linux-x86_64.rpm");
        // folders in front are dropped, whichever slash they use
        assert_eq!(safe_file_name("../../evil.exe").unwrap(), "evil.exe");
        assert_eq!(safe_file_name("a/b.rpm").unwrap(), "b.rpm");
        assert_eq!(safe_file_name("..\\..\\Windows\\evil.exe").unwrap(), "evil.exe");
        assert_eq!(safe_file_name("/etc/passwd").unwrap(), "passwd");
        // names that aren't a plain file are refused
        for bad in ["", "   ", "..", ".", "a/..", "dir/", "C:evil.exe", "bad\nname.rpm", "x\0.deb"] {
            assert!(safe_file_name(bad).is_err(), "{bad:?} should be refused");
        }
    }

    #[test]
    fn versions_and_install_detection() {
        assert!(is_newer("v0.1.10", "0.1.9"));
        assert!(is_newer("v1.0.0", "0.9.9"));
        assert!(!is_newer("v0.1.2", "0.1.2"));
        assert!(!is_newer("v0.1.1", "0.1.2"));
        assert_eq!(version("0.2"), Some((0, 2, 0)));
        assert_eq!(version("v0.3.1-beta"), Some((0, 3, 1)));
        assert_eq!(version("nonsense"), None);
        let base = Clues { os: "linux", exe: "/usr/bin/burncraft", flatpak: false, appimage: false, dpkg_owns: false, rpm_system: true, uninstaller_beside: false };
        assert_eq!(kind_from(&base), InstallKind::Rpm);
        assert_eq!(kind_from(&Clues { dpkg_owns: true, ..base }), InstallKind::Deb);
        assert_eq!(kind_from(&Clues { flatpak: true, exe: "/app/bin/burncraft", ..base }), InstallKind::Flatpak);
        assert_eq!(kind_from(&Clues { exe: "/home/me/apps/burncraft/burncraft", ..base }), InstallKind::LinuxTarball);
        assert_eq!(kind_from(&Clues { os: "windows", exe: "C:\\Program Files\\BurnCraft\\burncraft.exe", ..base }), InstallKind::WindowsMsi);
        assert_eq!(kind_from(&Clues { os: "windows", exe: "C:\\Program Files\\BurnCraft\\burncraft.exe", uninstaller_beside: true, ..base }), InstallKind::WindowsSetup);
        assert_eq!(kind_from(&Clues { os: "windows", exe: "D:\\Tools\\BurnCraft\\BurnCraft.exe", ..base }), InstallKind::WindowsPortable);
        assert_eq!(kind_from(&Clues { os: "freebsd", ..base }), InstallKind::FreeBsdTarball);
        let v: Value = serde_json::json!({"tag_name": "v9.0.0", "html_url": "https://h", "body": "notes", "assets": [{"name": "a.rpm", "browser_download_url": "https://d/a.rpm", "size": 5, "digest": "sha256:ab"}]});
        let r = parse_release(&v).unwrap();
        assert_eq!(r.assets[0].digest.as_deref(), Some("sha256:ab"));
        assert!(is_newer(&r.tag, CURRENT));
    }
}
