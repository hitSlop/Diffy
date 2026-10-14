//! Native CI uses an ephemeral signing key and disposable installations.
//! The shipping binary always trusts packaging/updater.pub.
#[cfg(any(target_os = "linux", windows))]
#[test]
#[ignore = "requires a CI-only signed native package and an isolated destination"]
fn apply_signed_native_package() {
    use cargo_packager_updater::{Config, Update, UpdateFormat, WindowsConfig};
    use std::{fs, path::PathBuf, time::Duration};
    assert!(std::env::var_os("CI").is_some());
    let package = PathBuf::from(std::env::var_os("DIFFY_CI_UPDATE_PACKAGE").unwrap());
    let signature =
        fs::read_to_string(std::env::var_os("DIFFY_CI_UPDATE_SIGNATURE").unwrap()).unwrap();
    let public = fs::read_to_string(std::env::var_os("DIFFY_CI_UPDATE_PUBLIC").unwrap()).unwrap();
    let destination = PathBuf::from(std::env::var_os("DIFFY_CI_UPDATE_DESTINATION").unwrap());
    let update = Update {
        config: Config {
            pubkey: public,
            windows: Some(WindowsConfig {
                installer_args: Some(vec!["/R".into(), format!("/D={}", destination.display())]),
                ..Default::default()
            }),
            ..Default::default()
        },
        body: None,
        current_version: "0.0.0".into(),
        version: diffy::updates::VERSION.into(),
        date: None,
        target: diffy::updates::Target::current().unwrap().id.into(),
        extract_path: destination,
        download_url: "https://example.com/unused-ci-fixture".parse().unwrap(),
        signature,
        timeout: Some(Duration::from_secs(15)),
        headers: Default::default(),
        format: if cfg!(windows) {
            UpdateFormat::Nsis
        } else {
            UpdateFormat::AppImage
        },
    };
    let bytes = fs::read(package).unwrap();
    let mut corrupted = bytes.clone();
    corrupted[0] ^= 1;
    assert!(diffy::updates::Package::install(&update, corrupted).is_err());
    diffy::updates::Package::install(&update, bytes).expect("install verified CI package");
}
