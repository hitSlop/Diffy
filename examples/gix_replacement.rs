//! Isolated full-replacement study. Never used by the production application.
//! Native mutation is deliberately a narrow probe, not an implementation of git add.
#[path = "../benches/support/fixture.rs"]
mod fixture;
use diffy::repository::{self, MetadataBackend};
use std::{path::Path, process::Command, time::Instant};

fn git(root: &Path, args: &[&str]) -> Vec<u8> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

fn native_stage(root: &Path, path: &str) -> Result<String, Box<dyn std::error::Error>> {
    let repo = gix::discover(root)?;
    let id = repo.write_blob(std::fs::read(root.join(path))?)?.detach();
    let mut index = gix::index::File::at(
        repo.index_path(),
        repo.object_hash(),
        false,
        Default::default(),
    )?;
    let entry = index
        .entry_mut_by_path_and_stage(
            path.as_bytes().into(),
            gix::index::entry::Stage::Unconflicted,
        )
        .ok_or("probe requires an existing stage-zero entry")?;
    entry.id = id;
    entry.stat = Default::default();
    index.remove_tree();
    index.write(Default::default())?;
    Ok(id.to_string())
}

fn stage_probe(converted: bool) -> serde_json::Value {
    let f = fixture::Fixture::new(if converted {
        "gix-stage-crlf"
    } else {
        "gix-stage-plain"
    });
    if converted {
        f.write(".gitattributes", "*.txt text eol=crlf\n");
    }
    f.write("selected.txt", "before\n");
    f.write("unrelated.txt", "untouched\n");
    f.git(&["add", "."]);
    f.git(&["commit", "-m", "initial"]);
    let content = if converted { "after\r\n" } else { "after\n" };
    f.write("selected.txt", content);
    f.git(&["add", "--", "selected.txt"]);
    let expected = git(&f.0, &["rev-parse", ":selected.txt"]);
    f.git(&["reset", "HEAD", "--", "selected.txt"]);
    let unrelated = git(&f.0, &["ls-files", "--stage", "--", "unrelated.txt"]);
    // The mutation child cannot find Git. Git is used only by the parent oracle.
    let native = Command::new(std::env::current_exe().unwrap())
        .args(["--native-stage", f.0.to_str().unwrap(), "selected.txt"])
        .env("PATH", "/diffy-study-no-programs")
        .output()
        .unwrap();
    assert!(
        native.status.success(),
        "{}",
        String::from_utf8_lossy(&native.stderr)
    );
    assert_eq!(
        std::fs::read(f.0.join("selected.txt")).unwrap(),
        content.as_bytes()
    );
    assert_eq!(
        unrelated,
        git(&f.0, &["ls-files", "--stage", "--", "unrelated.txt"])
    );
    serde_json::json!({ "git_free_native_write": true, "working_tree_preserved": true,
        "unrelated_index_entry_preserved": true,
        "git_object_parity": expected.trim_ascii() == native.stdout.trim_ascii() })
}

fn benchmark(mixed: bool, backend: MetadataBackend) -> serde_json::Value {
    let f = fixture::Fixture::workload(mixed);
    let initial = repository::load_metadata_with_backend(&f.0, None, backend).unwrap();
    assert_eq!(initial.backend, backend);
    let index = std::fs::read(f.0.join(".git/index")).ok();
    let stamp = std::fs::metadata(f.0.join(".git/index"))
        .ok()
        .and_then(|m| m.modified().ok());
    let mut times = Vec::new();
    for _ in 0..30 {
        let start = Instant::now();
        let snapshot =
            repository::load_metadata_with_backend(&f.0, Some(&initial.context), backend).unwrap();
        assert_eq!(snapshot.backend, backend);
        times.push(start.elapsed().as_secs_f64() * 1000.);
    }
    times.sort_by(f64::total_cmp);
    assert_eq!(index, std::fs::read(f.0.join(".git/index")).ok());
    assert_eq!(
        stamp,
        std::fs::metadata(f.0.join(".git/index"))
            .ok()
            .and_then(|m| m.modified().ok())
    );
    serde_json::json!({"median_ms": times[15], "p95_ms": times[28], "iterations": 30, "index_preserved": true})
}

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.first().is_some_and(|arg| arg == "--native-stage") {
        match native_stage(Path::new(&args[1]), &args[2]) {
            Ok(id) => println!("{id}"),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return;
    }
    if args.first().is_some_and(|arg| arg == "--benchmark") {
        let backend = if args[1] == "gix" {
            MetadataBackend::Gix
        } else {
            MetadataBackend::Git
        };
        println!("{}", benchmark(args[2] == "mixed", backend));
        return;
    }
    let plain = stage_probe(false);
    let converted = stage_probe(true);
    assert_eq!(plain["git_object_parity"], true);
    assert_eq!(
        converted["git_object_parity"], false,
        "If conversion parity changes, revisit the capability report."
    );
    let report = serde_json::json!({
        "decision": "retain-production-git", "gix_version": "0.89.0",
        "plain_stage": plain, "crlf_stage": converted,
        "metadata": {
            "untracked_git": benchmark(false, MetadataBackend::Git),
            "untracked_gix": benchmark(false, MetadataBackend::Gix),
            "mixed_git": benchmark(true, MetadataBackend::Git),
            "mixed_gix": benchmark(true, MetadataBackend::Gix)
        },
        "limits": ["Native write probe does not implement filters, sparse/conflicted indexes, additions, deletions, or rename pairs",
            "Production metadata adapter still invokes Git for branch labels", "Previews and whole-file writes remain Git-backed"]
    });
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
}
