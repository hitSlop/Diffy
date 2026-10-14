use std::{fs, path::PathBuf, process::Command};
pub struct Fixture(pub PathBuf);
impl Fixture {
    pub fn new(label: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "diffy-perf-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let fixture = Self(root);
        fixture.git(&["init", "--initial-branch=main"]);
        fixture
    }
    pub fn git(&self, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(&self.0)
            .args([
                "-c",
                "user.name=Diffy Bench",
                "-c",
                "user.email=bench@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "core.hooksPath=/dev/null",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    pub fn write(&self, name: &str, text: &str) {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    #[allow(dead_code)]
    pub fn workload(mixed: bool) -> Self {
        let fixture = Self::new(if mixed { "mixed" } else { "untracked" });
        let source = (0..50_000)
            .map(|n| format!("source line {n} with representative text and context\n"))
            .collect::<String>();
        fixture.write("large.txt", &source);
        for n in 0..1000 {
            fixture.write(&format!("files/{n:04}.txt"), "small file\n");
        }
        if mixed {
            fixture.git(&["add", "."]);
            fixture.git(&["commit", "-m", "fixture"]);
            fixture.write(
                "large.txt",
                &source.replacen("source line 25000", "edited line 25000", 1),
            );
            for n in 0..100 {
                fixture.write(&format!("files/{n:04}.txt"), "staged change\n");
            }
            fixture.git(&["add", "files"]);
            for n in 0..50 {
                fixture.write(&format!("files/{n:04}.txt"), "working change\n");
            }
            for n in 0..100 {
                fixture.write(&format!("untracked/{n}.txt"), "new file\n");
            }
        }
        fixture
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
