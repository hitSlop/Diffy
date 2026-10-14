//! Blocking update work runs on GPUI's background executor. Only the UI may request installation.
use cargo_packager_updater::{Config, RemoteRelease, Update, UpdateFormat};
use std::{
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const ENDPOINT: &str =
    "https://github.com/hitSlop/diffy/releases/latest/download/update-macos-aarch64.json";
pub const PUBLIC_KEY: &str = include_str!("../packaging/updater.pub");

#[derive(Clone, Copy, Debug)]
pub struct Target {
    pub id: &'static str,
    pub endpoint: &'static str,
    pub format: UpdateFormat,
}
impl Target {
    pub fn for_platform(os: &str, arch: &str) -> Option<Self> {
        match (os, arch) {
            ("macos", "aarch64") => Some(Self {
                id: "macos-aarch64",
                endpoint: ENDPOINT,
                format: UpdateFormat::App,
            }),
            ("windows", "x86_64") => Some(Self {
                id: "windows-x86_64",
                endpoint: "https://github.com/hitSlop/diffy/releases/latest/download/update-windows-x86_64.json",
                format: UpdateFormat::Nsis,
            }),
            ("linux", "x86_64") => Some(Self {
                id: "linux-x86_64",
                endpoint: "https://github.com/hitSlop/diffy/releases/latest/download/update-linux-x86_64.json",
                format: UpdateFormat::AppImage,
            }),
            _ => None,
        }
    }
    pub fn current() -> Option<Self> {
        Self::for_platform(std::env::consts::OS, std::env::consts::ARCH)
    }
}

pub fn launch_executable() -> Result<PathBuf, String> {
    #[cfg(target_os = "linux")]
    if let Some(path) = std::env::var_os("APPIMAGE") {
        return Ok(PathBuf::from(path));
    }
    std::env::current_exe().map_err(|e| e.to_string())
}
fn installation_path() -> Result<PathBuf, String> {
    let executable = launch_executable()?;
    #[cfg(target_os = "macos")]
    return bundle_path(&executable)
        .ok_or("Install Diffy.app in Applications to use automatic updates.".into());
    #[cfg(target_os = "windows")]
    {
        if env!("DIFFY_PACKAGE_KIND") != "nsis"
            || !executable
                .parent()
                .is_some_and(|parent| parent.join("diffy-cli.exe").is_file())
        {
            return Err(
                "Install Diffy using its Windows installer to use automatic updates.".into(),
            );
        }
    }
    #[cfg(target_os = "linux")]
    if std::env::var_os("APPIMAGE").is_none() {
        return Err("Use the packaged Diffy AppImage to install updates.".into());
    }
    #[cfg(not(target_os = "macos"))]
    Ok(executable)
}

pub trait Package: Send + Sync {
    fn download(&self, progress: &dyn Fn(usize, Option<u64>)) -> Result<Vec<u8>, String>;
    fn install(&self, bytes: Vec<u8>) -> Result<(), String>;
}
#[derive(Clone)]
pub struct Release {
    pub version: String,
    pub package: Arc<dyn Package>,
}
pub trait Backend: Send + Sync {
    fn check(&self) -> Result<Option<Release>, String>;
}

#[derive(Debug, Clone, PartialEq)]
pub enum Phase {
    Idle,
    Checking,
    Current,
    Available(String),
    Downloading { received: u64, total: Option<u64> },
    WaitingForGit,
    Installing,
    Error(String),
}
pub struct Controller {
    pub phase: Phase,
    pub show_check: bool,
    pub release: Option<Release>,
    pub backend: Arc<dyn Backend>,
}
impl Default for Controller {
    fn default() -> Self {
        Self {
            phase: Phase::Idle,
            show_check: false,
            release: None,
            backend: Arc::new(GithubBackend),
        }
    }
}
impl Controller {
    pub fn busy(&self) -> bool {
        matches!(
            self.phase,
            Phase::Checking | Phase::Downloading { .. } | Phase::WaitingForGit | Phase::Installing
        )
    }
    pub fn blocks_mutations(&self) -> bool {
        matches!(
            self.phase,
            Phase::Downloading { .. } | Phase::WaitingForGit | Phase::Installing
        )
    }
    pub fn begin_check(&mut self) -> bool {
        if self.busy() {
            return false;
        }
        self.phase = Phase::Checking;
        true
    }
    pub fn checked(&mut self, result: Result<Option<Release>, String>, manual: bool) {
        match result {
            Ok(Some(release)) => {
                self.phase = Phase::Available(release.version.clone());
                self.release = Some(release);
            }
            Ok(None) => {
                self.release = None;
                self.phase = if manual { Phase::Current } else { Phase::Idle };
            }
            Err(error) => {
                self.phase = if manual {
                    Phase::Error(error)
                } else {
                    Phase::Idle
                };
            }
        }
    }
    pub fn dismiss(&mut self) {
        if !self.busy() {
            self.phase = Phase::Idle;
        }
    }
}

pub fn bundle_path(executable: &Path) -> Option<PathBuf> {
    let macos = executable.parent()?;
    let contents = macos.parent()?;
    let bundle = contents.parent()?;
    (macos.file_name()? == "MacOS"
        && contents.file_name()? == "Contents"
        && bundle.file_name()? == "Diffy.app")
        .then(|| bundle.to_path_buf())
}
pub fn installation_problem() -> Option<String> {
    if Target::current().is_none() {
        return Some("Updates aren't available for this platform or architecture.".into());
    }
    let bundle = match installation_path() {
        Ok(path) => path,
        Err(error) => return Some(error),
    };
    if cfg!(target_os = "macos")
        && (bundle.to_string_lossy().contains("AppTranslocation") || bundle.starts_with("/Volumes"))
    {
        return Some("Move Diffy.app to Applications and reopen it before updating.".into());
    }
    let Some(parent) = bundle.parent() else {
        return Some("Couldn't locate the installed app.".into());
    };
    // Installation replaces a directory; checking only the executable's writable bit is insufficient.
    let probe = parent.join(format!(".diffy-write-check-{}", std::process::id()));
    match std::fs::OpenOptions::new().write(true).create_new(true).open(&probe) {
        Ok(_) => { let _ = std::fs::remove_file(probe); None }
        Err(_) => Some("Diffy can't update this read-only location. Move it to a writable installation location.".into()),
    }
}

struct GithubBackend;
impl Backend for GithubBackend {
    fn check(&self) -> Result<Option<Release>, String> {
        let bundle = installation_path()?;
        let target = Target::current().ok_or("Unsupported update target")?;
        let _ = rustls::crypto::ring::default_provider().install_default();
        let response = reqwest::blocking::Client::builder()
            .https_only(true)
            .redirect(reqwest::redirect::Policy::limited(5))
            .timeout(Duration::from_secs(15))
            .build()
            .map_err(|e| e.to_string())?
            .get(target.endpoint)
            .header("Accept", "application/json")
            .header("User-Agent", concat!("Diffy/", env!("CARGO_PKG_VERSION")))
            .send()
            .map_err(|e| format!("Couldn't check for updates: {e}"))?;
        match response.status().as_u16() {
            404 | 204 => return Ok(None),
            _ => {}
        }
        let response = response
            .error_for_status()
            .map_err(|e| format!("Couldn't check for updates: {e}"))?;
        let mut bytes = Vec::new();
        response
            .take(1_048_577)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > 1_048_576 {
            return Err("Update manifest is too large.".into());
        }
        release_for_target(&bytes, bundle, target)
    }
}
fn release_for_target(
    bytes: &[u8],
    bundle: PathBuf,
    target: Target,
) -> Result<Option<Release>, String> {
    let release: RemoteRelease =
        serde_json::from_slice(bytes).map_err(|e| format!("Invalid update manifest: {e}"))?;
    if release.version
        <= VERSION
            .parse()
            .map_err(|e| format!("Invalid application version: {e}"))?
    {
        return Ok(None);
    }
    let download_url = release
        .download_url(target.id)
        .map_err(|e| e.to_string())?
        .clone();
    if download_url.scheme() != "https" {
        return Err("Update downloads must use HTTPS.".into());
    }
    let format = release.format(target.id).map_err(|e| e.to_string())?;
    if std::mem::discriminant(&format) != std::mem::discriminant(&target.format) {
        return Err(format!("The update package doesn't match {}.", target.id));
    }
    let signature = release
        .signature(target.id)
        .map_err(|e| e.to_string())?
        .clone();
    let version = release.version.to_string();
    let update = Update {
        config: Config {
            endpoints: vec![
                target
                    .endpoint
                    .parse()
                    .map_err(|e| format!("Invalid update endpoint: {e}"))?,
            ],
            pubkey: PUBLIC_KEY.trim().into(),
            windows: Some(cargo_packager_updater::WindowsConfig {
                installer_args: Some(vec!["/R".into()]),
                ..Default::default()
            }),
            ..Default::default()
        },
        body: release.notes,
        current_version: VERSION.into(),
        version: version.clone(),
        date: release.pub_date,
        target: target.id.into(),
        extract_path: bundle,
        download_url,
        signature,
        timeout: Some(Duration::from_secs(180)),
        headers: Default::default(),
        format,
    };
    Ok(Some(Release {
        version,
        package: Arc::new(update),
    }))
}
/// Verify again at the installation boundary, including offline release exercises.
pub fn verify_archive(bytes: &[u8], public_key: &str, signature: &str) -> Result<(), String> {
    use base64::Engine as _;
    let decode = |value: &str| -> Result<String, String> {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(value.trim())
            .map_err(|e| e.to_string())?;
        String::from_utf8(bytes).map_err(|e| e.to_string())
    };
    let key =
        minisign_verify::PublicKey::decode(&decode(public_key)?).map_err(|e| e.to_string())?;
    let signature =
        minisign_verify::Signature::decode(&decode(signature)?).map_err(|e| e.to_string())?;
    key.verify(bytes, &signature, true)
        .map_err(|e| format!("Update signature verification failed: {e}"))
}
impl Package for Update {
    fn download(&self, progress: &dyn Fn(usize, Option<u64>)) -> Result<Vec<u8>, String> {
        self.download_extended(progress, || {})
            .map_err(|e| format!("Couldn't verify and download the update: {e}"))
    }
    fn install(&self, bytes: Vec<u8>) -> Result<(), String> {
        verify_archive(&bytes, &self.config.pubkey, &self.signature)?;
        #[cfg(target_os = "macos")]
        {
            let parent = self
                .extract_path
                .parent()
                .ok_or("Couldn't locate the app for backup.")?;
            let suffix = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_nanos();
            let backup = parent.join(format!(".Diffy-backup-{}-{suffix}.app", std::process::id()));
            let copy = std::process::Command::new("/usr/bin/ditto")
                .arg(&self.extract_path)
                .arg(&backup)
                .status()
                .map_err(|e| e.to_string())?;
            if !copy.success() {
                let _ = std::fs::remove_dir_all(&backup);
                return Err(
                    "Couldn't back up Diffy before updating. The installed app is unchanged."
                        .into(),
                );
            }
            match Update::install(self, bytes) {
                Ok(()) => {
                    let _ = std::fs::remove_dir_all(backup);
                    Ok(())
                }
                Err(error) => {
                    let restored = (|| -> std::io::Result<()> {
                        if self.extract_path.exists() {
                            std::fs::remove_dir_all(&self.extract_path)?;
                        }
                        std::fs::rename(&backup, &self.extract_path)
                    })();
                    match restored {
                        Ok(()) => Err(format!(
                            "Couldn't install the update: {error}. The previous app was restored."
                        )),
                        Err(_) => Err(format!(
                            "Couldn't install the update: {error}. Your previous app is backed up at {}.",
                            backup.display()
                        )),
                    }
                }
            }
        }
        #[cfg(not(target_os = "macos"))]
        Update::install(self, bytes).map_err(|e| format!("Couldn't install the update: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn release_from_manifest(bytes: &[u8], bundle: PathBuf) -> Result<Option<Release>, String> {
        release_for_target(
            bytes,
            bundle,
            Target::for_platform("macos", "aarch64").unwrap(),
        )
    }
    #[test]
    fn platform_packages_are_selected_and_mismatches_rejected() {
        for (os, arch, format) in [
            ("macos", "aarch64", "app"),
            ("windows", "x86_64", "nsis"),
            ("linux", "x86_64", "appimage"),
        ] {
            let target = Target::for_platform(os, arch).unwrap();
            let manifest = serde_json::to_vec(&serde_json::json!({"version":"99.0.0", "platforms":{target.id:{"url":"https://example.com/update", "format":format, "signature":"fixture"}}})).unwrap();
            assert!(
                release_for_target(&manifest, PathBuf::from("fixture"), target)
                    .unwrap()
                    .is_some()
            );
            let wrong =
                Target::for_platform(if os == "linux" { "windows" } else { "linux" }, "x86_64")
                    .unwrap();
            assert!(release_for_target(&manifest, PathBuf::from("fixture"), wrong).is_err());
        }
        assert!(Target::for_platform("windows", "aarch64").is_none());
    }
    struct FakePackage;
    impl Package for FakePackage {
        fn download(&self, progress: &dyn Fn(usize, Option<u64>)) -> Result<Vec<u8>, String> {
            progress(2, Some(2));
            Ok(vec![1, 2])
        }
        fn install(&self, _: Vec<u8>) -> Result<(), String> {
            Err("read only".into())
        }
    }
    struct FakeBackend;
    impl Backend for FakeBackend {
        fn check(&self) -> Result<Option<Release>, String> {
            Ok(Some(Release {
                version: "0.2.0".into(),
                package: Arc::new(FakePackage),
            }))
        }
    }
    #[test]
    fn manifest_validation_rejects_insecure_urls_bad_formats_and_old_versions() {
        let manifest = |url: &str, format: &str, version: &str| {
            serde_json::to_vec(&serde_json::json!({"version":version,"url":url,"format":format,"signature":"fixture"})).unwrap()
        };
        let destination = PathBuf::from("/Applications/Diffy.app");
        assert!(
            release_from_manifest(
                &manifest("http://example.com/update", "app", "9.0.0"),
                destination.clone()
            )
            .is_err()
        );
        assert!(
            release_from_manifest(
                &manifest("https://example.com/update", "nsis", "9.0.0"),
                destination.clone()
            )
            .is_err()
        );
        assert!(
            release_from_manifest(
                &manifest("https://example.com/update", "app", "0.0.0"),
                destination.clone()
            )
            .unwrap()
            .is_none()
        );
        assert!(release_from_manifest(b"broken json", destination.clone()).is_err());
        assert_eq!(
            release_from_manifest(
                &manifest("https://example.com/update", "app", "9.0.0"),
                destination
            )
            .unwrap()
            .unwrap()
            .version,
            "9.0.0"
        );
    }
    #[test]
    fn update_states_preserve_retry_and_gate_git_mutations() {
        let mut controller = Controller {
            backend: Arc::new(FakeBackend),
            ..Default::default()
        };
        assert!(controller.begin_check());
        assert!(!controller.begin_check());
        assert!(!controller.blocks_mutations());
        controller.checked(controller.backend.check(), true);
        assert_eq!(controller.phase, Phase::Available("0.2.0".into()));
        let release = controller.release.clone().unwrap();
        controller.phase = Phase::Downloading {
            received: 0,
            total: None,
        };
        assert!(controller.blocks_mutations());
        let bytes = release
            .package
            .download(&|count, total| {
                assert_eq!(count, 2);
                assert_eq!(total, Some(2));
            })
            .unwrap();
        controller.phase = Phase::WaitingForGit;
        assert!(controller.blocks_mutations());
        controller.phase = Phase::Installing;
        assert!(controller.blocks_mutations());
        controller.phase = Phase::Error(release.package.install(bytes).unwrap_err());
        assert!(controller.release.is_some());
        assert!(!controller.blocks_mutations());
        controller.dismiss();
        assert_eq!(controller.phase, Phase::Idle);
    }
    #[test]
    fn background_failures_and_missing_releases_are_quiet() {
        let mut c = Controller::default();
        c.checked(Err("offline".into()), false);
        assert_eq!(c.phase, Phase::Idle);
        c.checked(Err("offline".into()), true);
        assert_eq!(c.phase, Phase::Error("offline".into()));
        c.checked(Ok(None), true);
        assert_eq!(c.phase, Phase::Current);
        c.checked(Ok(None), false);
        assert_eq!(c.phase, Phase::Idle);
    }
    #[test]
    fn development_binaries_are_never_update_targets() {
        assert_eq!(
            bundle_path(Path::new("/Applications/Diffy.app/Contents/MacOS/diffy")),
            Some(PathBuf::from("/Applications/Diffy.app"))
        );
        assert!(bundle_path(Path::new("target/debug/diffy")).is_none());
        assert!(bundle_path(Path::new("/Applications/Other.app/Contents/MacOS/diffy")).is_none());
    }
}
