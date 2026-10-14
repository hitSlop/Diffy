//! Offline verification and installation exercises never modify the user's app.
#![cfg(debug_assertions)]
use cargo_packager_updater::{Config, Update, UpdateFormat};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

fn update(public_key: &str, signature: &str, destination: PathBuf) -> Update {
    Update {
        config: Config {
            pubkey: public_key.trim().into(),
            ..Default::default()
        },
        body: None,
        current_version: "0.0.0".into(),
        version: diffy::updates::VERSION.into(),
        date: None,
        target: "macos-aarch64".into(),
        extract_path: destination,
        download_url: "https://example.com/unused-fixture".parse().unwrap(),
        signature: signature.trim().into(),
        timeout: Some(Duration::from_secs(15)),
        headers: Default::default(),
        format: UpdateFormat::App,
    }
}

#[test]
fn real_signatures_accept_exact_bytes_and_reject_corruption() {
    let payload = include_bytes!("fixtures/update/payload.txt").to_vec();
    let public = include_str!("fixtures/update/public.key");
    let signature = include_str!("fixtures/update/payload.txt.sig");
    assert!(diffy::updates::verify_archive(&payload, public, signature).is_ok());
    let mut corrupted = payload.clone();
    corrupted[0] ^= 1;
    assert!(diffy::updates::verify_archive(&corrupted, public, signature).is_err());
    assert!(diffy::updates::verify_archive(&payload, public, "invalid signature").is_err());
    let invalid = update(public, signature, PathBuf::from("/unused"));
    assert!(diffy::updates::Package::install(&invalid, corrupted).is_err());
}

#[test]
#[cfg(target_os = "macos")]
#[ignore = "requires a signed release archive; run explicitly during release verification"]
fn signed_app_archive_installs_and_relaunches_an_isolated_bundle() {
    let archive = std::env::var_os("DIFFY_TEST_UPDATE_ARCHIVE")
        .map(PathBuf::from)
        .expect("DIFFY_TEST_UPDATE_ARCHIVE is required for release verification");
    let signature = fs::read_to_string(
        std::env::var_os("DIFFY_TEST_UPDATE_SIGNATURE").expect("signature path required"),
    )
    .unwrap();
    let expected_version = std::env::var("DIFFY_TEST_UPDATE_VERSION")
        .unwrap_or_else(|_| diffy::updates::VERSION.to_owned());
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "diffy-signed-update-{}-{suffix}",
        std::process::id()
    ));
    assert!(
        !root.exists(),
        "Use an isolated, fresh update test directory"
    );
    fs::create_dir_all(&root).unwrap();
    let current = root.join("Diffy.app");
    let source = archive.parent().unwrap().join("Diffy.app");
    run("/usr/bin/ditto", &[source.as_os_str(), current.as_os_str()]);
    let plist = current.join("Contents/Info.plist");
    run(
        "/usr/bin/plutil",
        &[
            "-replace".as_ref(),
            "CFBundleShortVersionString".as_ref(),
            "-string".as_ref(),
            "0.0.0".as_ref(),
            plist.as_os_str(),
        ],
    );
    run(
        "/usr/bin/codesign",
        &[
            "--force".as_ref(),
            "--options".as_ref(),
            "runtime".as_ref(),
            "--timestamp".as_ref(),
            "--keychain".as_ref(),
            Path::new(&format!(
                "{}/Library/Keychains/login.keychain-db",
                std::env::var("HOME").unwrap()
            ))
            .as_os_str(),
            "--sign".as_ref(),
            "Developer ID Application: Mushroom DAO Holdings Corp. (78UAXU8QG8)".as_ref(),
            current.as_os_str(),
        ],
    );
    run(
        "/usr/bin/codesign",
        &[
            "--verify".as_ref(),
            "--deep".as_ref(),
            "--strict".as_ref(),
            current.as_os_str(),
        ],
    );
    let preferences = root.join("preferences/state.json");
    fs::create_dir_all(preferences.parent().unwrap()).unwrap();
    fs::write(&preferences, b"preserve these preferences").unwrap();
    let mut package = update(diffy::updates::PUBLIC_KEY, &signature, current.clone());
    package.version = expected_version.clone();
    let bytes = fs::read(&archive).unwrap();
    diffy::updates::verify_archive(&bytes, diffy::updates::PUBLIC_KEY, &signature)
        .expect("Verify the real release archive");
    diffy::updates::Package::install(&package, bytes)
        .expect("Install into the isolated fixture bundle");
    run(
        "/usr/bin/codesign",
        &[
            "--verify".as_ref(),
            "--deep".as_ref(),
            "--strict".as_ref(),
            current.as_os_str(),
        ],
    );
    run(
        "/usr/bin/xcrun",
        &["stapler".as_ref(), "validate".as_ref(), current.as_os_str()],
    );
    let launch = Command::new(current.join("Contents/MacOS/diffy"))
        .arg("--version")
        .env("PATH", "")
        .output()
        .unwrap();
    assert!(launch.status.success());
    assert_eq!(
        String::from_utf8(launch.stdout).unwrap().trim(),
        format!("Diffy {expected_version}")
    );
    assert_eq!(
        fs::read(&preferences).unwrap(),
        b"preserve these preferences"
    );
    fs::remove_dir_all(root).unwrap();
}

#[cfg(target_os = "macos")]
fn run(tool: &str, args: &[&std::ffi::OsStr]) {
    let result = Command::new(tool)
        .args(args)
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .status()
        .unwrap();
    assert!(result.success(), "{tool} failed");
}
