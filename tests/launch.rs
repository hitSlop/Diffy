#[cfg(unix)]
#[test]
fn launch_without_a_shell_path_can_load_a_repository() {
    use std::process::Command;
    let root = std::env::temp_dir().join(format!(
        "diffy-launch-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&root).unwrap();
    let initialized = Command::new("/usr/bin/git")
        .arg("-C")
        .arg(&root)
        .args(["init", "--initial-branch=main"])
        .output()
        .unwrap();
    assert!(initialized.status.success());
    std::fs::write(root.join("example.txt"), "new source\n").unwrap();
    for path in ["", "$PATH:/path/that/does/not/exist"] {
        let output = Command::new(env!("CARGO_BIN_EXE_diffy"))
            .env("PATH", path)
            .arg("--check")
            .arg(&root)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("0 staged, 1 changes"));
    }
    std::fs::remove_dir_all(root).unwrap();
}
