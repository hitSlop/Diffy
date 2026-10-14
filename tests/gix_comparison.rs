#[path = "../benches/support/fixture.rs"]
mod fixture;
#[path = "../benches/support/gix.rs"]
mod native;
use diffy::repository::{self, ChangeGroup, MetadataBackend, MetadataSnapshot};
use std::collections::BTreeSet;
use std::{fs, path::Path, process::Command};

/// Compare every field that reaches the app, including cross-backend preview
/// cache keys, watch locations and Git's branch label. Each read preserves index
/// bytes and mtime, and creates no lock file.
fn parity(path: &Path, expected_backend: MetadataBackend) -> MetadataSnapshot {
    let git = repository::load_metadata_with_backend(path, None, MetadataBackend::Git).unwrap();
    let index = git.git_dirs[0].join("index");
    let before = fs::read(&index).ok();
    let modified = fs::metadata(&index).ok().and_then(|m| m.modified().ok());
    let native =
        repository::load_metadata_with_backend(path, Some(&git.context), MetadataBackend::Gix)
            .unwrap();
    assert_eq!(
        native.backend, expected_backend,
        "fallback: {:?}",
        native.fallback_reason
    );
    assert_eq!(native.root, git.root);
    assert_eq!(native.branch, git.branch);
    assert_eq!(native.git_dirs, git.git_dirs);
    assert_eq!(
        native.context.git_directories(),
        git.context.git_directories()
    );
    assert_eq!(native.groups, git.groups);
    assert_eq!(fs::read(&index).ok(), before);
    assert_eq!(
        fs::metadata(&index).ok().and_then(|m| m.modified().ok()),
        modified
    );
    assert!(!index.with_extension("lock").exists());
    native
}

fn commit(f: &fixture::Fixture) {
    f.git(&["add", "."]);
    f.git(&["commit", "-m", "fixture"]);
}

#[test]
fn production_adapter_matches_complete_metadata_for_both_workloads() {
    for mixed in [false, true] {
        parity(&fixture::Fixture::workload(mixed).0, MetadataBackend::Gix);
    }
}

#[test]
fn unborn_staging_partial_edits_and_cache_revisions() {
    let f = fixture::Fixture::new("adapter-unborn");
    f.write("one.txt", "first\n");
    let untracked = parity(&f.0, MetadataBackend::Gix);
    f.git(&["add", "."]);
    let staged = parity(&f.0, MetadataBackend::Gix);
    f.write("one.txt", "second longer\n");
    let partial = parity(&f.0, MetadataBackend::Gix);
    assert_eq!(staged.groups[0], partial.groups[0]);
    assert!(!partial.groups[1][0].untracked);
    f.write("one.txt", "third considerably longer\n");
    let edited = parity(&f.0, MetadataBackend::Gix);
    assert_ne!(partial.groups[1][0].revision, edited.groups[1][0].revision);
    f.git(&["add", "."]);
    let restaged = parity(&f.0, MetadataBackend::Gix);
    assert_ne!(
        partial.groups[0][0].revision,
        restaged.groups[0][0].revision
    );
    assert!(restaged.groups[1].is_empty());
    assert!(untracked.groups[1][0].untracked);
}

#[test]
fn exact_rename_literal_paths_preview_and_writes_work_across_backends() {
    let f = fixture::Fixture::new("adapter-rename");
    f.write("old.rs", "unique unchanged rename content\n");
    f.write("tracked.txt", "base\n");
    commit(&f);
    f.git(&["mv", "old.rs", "new.rs"]);
    f.write("new.rs", "working change after rename\n");
    f.write(
        if cfg!(windows) {
            "[é] 雪 space.rs"
        } else {
            ":(glob)*\n雪.rs"
        },
        "literal file\n",
    );
    let listing = parity(&f.0, MetadataBackend::Gix);
    assert_eq!(listing.groups[0][0].original.as_deref(), Some("old.rs"));
    for group in [ChangeGroup::Staged, ChangeGroup::Changes] {
        for entry in &listing.groups[group.index()] {
            assert_eq!(
                repository::load_preview(&listing.root, group, entry)
                    .unwrap()
                    .file
                    .path(),
                &entry.path
            );
        }
    }
    let bytes = fs::read(f.0.join("new.rs")).unwrap();
    repository::set_entries_staged(&f.0, &listing.groups[1], true).unwrap();
    let after = parity(&f.0, MetadataBackend::Git);
    assert!(after.groups[1].is_empty());
    repository::set_entries_staged(&f.0, &after.groups[0], false).unwrap();
    parity(&f.0, MetadataBackend::Gix);
    assert_eq!(fs::read(f.0.join("new.rs")).unwrap(), bytes);
}

#[test]
fn linked_worktrees_detached_labels_and_nested_discovery() {
    let f = fixture::Fixture::new("adapter-worktree");
    f.write("tracked.txt", "base\n");
    commit(&f);
    let linked = f.0.join("linked");
    f.write(".git/info/exclude", "linked/\n");
    f.git(&[
        "worktree",
        "add",
        "--detach",
        linked.to_str().unwrap(),
        "HEAD",
    ]);
    f.git(&["config", "core.abbrev", "12"]);
    fs::write(linked.join("tracked.txt"), "worktree change\n").unwrap();
    let listing = parity(&linked, MetadataBackend::Gix);
    assert_eq!(listing.branch.len(), 12);
    assert_eq!(listing.git_dirs.len(), 2);
    fs::create_dir(linked.join("nested")).unwrap();
    parity(&linked.join("nested"), MetadataBackend::Gix);
    // Ambiguous short reference labels must retain Git's rules.
    f.git(&["tag", "main"]);
    assert_eq!(parity(&f.0, MetadataBackend::Gix).branch, "heads/main");
}

#[test]
fn ignore_rules_conditional_includes_and_status_overrides() {
    let f = fixture::Fixture::new("adapter-config");
    f.write("tracked.ignored", "base\n");
    f.write("deleted.txt", "base\n");
    commit(&f);
    f.write(".gitignore", "*.ignored\nignored-dir/\n");
    f.write(".git/info/exclude", "info-only.txt\n");
    let excludes = f.0.join(".git/extra-excludes");
    fs::write(&excludes, "config-only.txt\n").unwrap();
    let included = f.0.join(".git/included-config");
    fs::write(&included, format!("[core]\nexcludesFile = {}\n[status]\nshowUntrackedFiles = no\nrenames = false\n[diff]\nrenames = copies\n", excludes.display())).unwrap();
    f.git(&[
        "config",
        &format!(
            "includeIf.gitdir:{}/.git.path",
            fs::canonicalize(&f.0).unwrap().display()
        ),
        included.to_str().unwrap(),
    ]);
    f.write("tracked.ignored", "tracked files still visible\n");
    fs::remove_file(f.0.join("deleted.txt")).unwrap();
    for name in [
        "new.ignored",
        "ignored-dir/file",
        "info-only.txt",
        "config-only.txt",
        "visible.txt",
        "dir/visible.txt",
    ] {
        f.write(name, "new\n");
    }
    fs::create_dir(f.0.join("empty-dir")).unwrap();
    let listing = parity(&f.0, MetadataBackend::Gix);
    assert!(
        listing.groups[1]
            .iter()
            .any(|f| f.path.as_str() == "visible.txt")
    );
    assert!(
        !listing.groups[1]
            .iter()
            .any(|f| f.path.as_str() == "config-only.txt")
    );
}

#[cfg(unix)]
#[test]
fn symlinks_type_changes_and_executable_modes() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let f = fixture::Fixture::new("adapter-modes");
    f.git(&["config", "core.filemode", "true"]);
    f.write("exec.sh", "#!/bin/sh\ntrue\n");
    f.write("regular.txt", "base\n");
    symlink("regular.txt", f.0.join("link")).unwrap();
    commit(&f);
    fs::set_permissions(f.0.join("exec.sh"), fs::Permissions::from_mode(0o755)).unwrap();
    fs::remove_file(f.0.join("regular.txt")).unwrap();
    symlink("exec.sh", f.0.join("regular.txt")).unwrap();
    fs::remove_file(f.0.join("link")).unwrap();
    f.write("link", "now a regular file\n");
    symlink("link", f.0.join("untracked-link")).unwrap();
    parity(&f.0, MetadataBackend::Gix);
    f.git(&["add", "."]);
    parity(&f.0, MetadataBackend::Gix);
}

#[test]
fn assume_unchanged_skip_worktree_and_crlf_attributes() {
    let f = fixture::Fixture::new("adapter-index-flags");
    f.write("assumed.txt", "base\n");
    f.write("skipped.txt", "base\n");
    f.write(".gitattributes", "crlf.txt text eol=crlf\n");
    f.write("crlf.txt", "one\ntwo\n");
    commit(&f);
    f.git(&["update-index", "--assume-unchanged", "assumed.txt"]);
    f.git(&["update-index", "--skip-worktree", "skipped.txt"]);
    f.write("assumed.txt", "hidden change\n");
    f.write("skipped.txt", "hidden change\n");
    assert!(parity(&f.0, MetadataBackend::Gix).groups[1].is_empty());
    f.write("crlf.txt", "one\r\ntwo\r\n");
    let normalized = parity(&f.0, MetadataBackend::Git);
    assert!(normalized.groups[1].is_empty(), "{:?}", normalized.groups);
    f.write("crlf.txt", "one\r\nchanged\r\n");
    assert_eq!(
        parity(&f.0, MetadataBackend::Git).fallback_reason,
        Some("worktree content conversion")
    );
}

#[test]
fn rename_settings_empty_renames_and_missing_index_match_git() {
    let f = fixture::Fixture::new("adapter-rename-config");
    f.write("old.txt", "unique content\n");
    f.write("empty.txt", "");
    commit(&f);
    f.git(&["config", "status.renames", "false"]);
    f.git(&["config", "diff.renames", "copies"]);
    f.git(&["config", "diff.renameLimit", "1"]);
    f.git(&["mv", "old.txt", "new.txt"]);
    parity(&f.0, MetadataBackend::Gix);
    f.git(&["reset", "--hard", "HEAD"]);
    f.git(&["mv", "empty.txt", "empty-new.txt"]);
    parity(&f.0, MetadataBackend::Gix);
    f.git(&["reset", "--hard", "HEAD"]);
    fs::remove_file(f.0.join(".git/index")).unwrap();
    parity(&f.0, MetadataBackend::Gix);
}

#[test]
fn harmless_attributes_stay_native_and_clean_filters_fall_back() {
    let f = fixture::Fixture::new("adapter-attributes");
    f.write(".gitattributes", "one.txt whitespace=-blank-at-eol\n");
    f.write("one.txt", "base\n");
    commit(&f);
    f.write("one.txt", "actual change\n");
    parity(&f.0, MetadataBackend::Gix);
    f.write(".gitattributes", "one.txt filter=cleaner\n");
    f.git(&["config", "filter.cleaner.clean", "cat"]);
    assert_eq!(
        parity(&f.0, MetadataBackend::Git).fallback_reason,
        Some("worktree content conversion")
    );
    f.git(&["config", "core.autocrlf", "input"]);
    parity(&f.0, MetadataBackend::Git);
}

#[test]
fn embedded_repositories_and_native_config_use_git() {
    let f = fixture::Fixture::new("adapter-embedded");
    f.write("one.txt", "base\n");
    commit(&f);
    fs::create_dir(f.0.join("nested")).unwrap();
    f.git(&["-C", "nested", "init", "--initial-branch=main"]);
    f.write("nested/file.txt", "inner\n");
    assert_eq!(
        parity(&f.0, MetadataBackend::Git).fallback_reason,
        Some("embedded repository")
    );
    f.git(&["config", "gitoxide.core.indexFile", "nonexistent-index"]);
    assert_eq!(
        parity(&f.0, MetadataBackend::Git).fallback_reason,
        Some("gitoxide configuration")
    );
}

#[test]
fn intent_to_add_and_sparse_checkout_fall_back_without_mutation() {
    let f = fixture::Fixture::new("adapter-sparse");
    f.write("keep/file.txt", "base\n");
    f.write("other/file.txt", "base\n");
    commit(&f);
    f.write("intent.txt", "not staged yet\n");
    f.git(&["add", "--intent-to-add", "intent.txt"]);
    assert_eq!(
        parity(&f.0, MetadataBackend::Git).fallback_reason,
        Some("intent-to-add index")
    );
    f.git(&["reset", "--", "intent.txt"]);
    f.git(&["sparse-checkout", "init", "--cone", "--sparse-index"]);
    f.git(&["sparse-checkout", "set", "keep"]);
    f.write("keep/file.txt", "changed\n");
    assert_eq!(
        parity(&f.0, MetadataBackend::Git).fallback_reason,
        Some("sparse checkout")
    );
}

#[test]
fn conflicts_fall_back_and_remain_view_only() {
    let f = fixture::Fixture::new("adapter-conflict");
    f.write("conflict.txt", "base\n");
    f.write("other.txt", "base\n");
    commit(&f);
    f.git(&["checkout", "-b", "side"]);
    f.write("conflict.txt", "side\n");
    commit(&f);
    f.git(&["checkout", "main"]);
    f.write("conflict.txt", "main\n");
    commit(&f);
    let output = Command::new("git")
        .arg("-C")
        .arg(&f.0)
        .args([
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "merge",
            "side",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    f.write("other.txt", "ordinary change\n");
    let listing = parity(&f.0, MetadataBackend::Git);
    assert_eq!(listing.fallback_reason, Some("unmerged index"));
    let conflict = listing.groups[1].iter().find(|f| !f.stageable()).unwrap();
    repository::load_preview(&listing.root, ChangeGroup::Changes, conflict).unwrap();
    repository::set_entries_staged(&listing.root, &listing.groups[1], true).unwrap();
    let after = parity(&f.0, MetadataBackend::Git);
    assert_eq!(after.groups[0][0].path.as_str(), "other.txt");
    assert!(!after.groups[1][0].stageable());
}

#[test]
fn similarity_and_ambiguous_renames_use_git_decisions() {
    let f = fixture::Fixture::new("adapter-fuzzy");
    let base = (0..100)
        .map(|n| format!("distinct line number {n}\n"))
        .collect::<String>();
    f.write("old.txt", &base);
    commit(&f);
    f.git(&["mv", "old.txt", "new.txt"]);
    f.write("new.txt", &base.replacen("number 50", "changed 50", 1));
    f.git(&["add", "."]);
    let listing = parity(&f.0, MetadataBackend::Git);
    assert_eq!(listing.groups[0][0].original.as_deref(), Some("old.txt"));
    f.git(&["reset", "--hard", "HEAD"]);
    f.write("another.txt", &base);
    commit(&f);
    f.git(&["mv", "old.txt", "new-a.txt"]);
    f.git(&["mv", "another.txt", "new-b.txt"]);
    assert_eq!(
        parity(&f.0, MetadataBackend::Git).fallback_reason,
        Some("ambiguous rename candidates")
    );
}

#[test]
fn submodule_changes_and_ignore_configuration_use_git() {
    let child = fixture::Fixture::new("adapter-child");
    child.write("file.txt", "base\n");
    commit(&child);
    let f = fixture::Fixture::new("adapter-submodule");
    f.git(&[
        "-c",
        "protocol.file.allow=always",
        "submodule",
        "add",
        child.0.to_str().unwrap(),
        "module",
    ]);
    commit(&f);
    f.write("module/file.txt", "dirty submodule\n");
    assert_eq!(
        parity(&f.0, MetadataBackend::Git).fallback_reason,
        Some("submodules")
    );
    f.git(&["config", "diff.ignoreSubmodules", "all"]);
    assert!(parity(&f.0, MetadataBackend::Git).groups[1].is_empty());
}

#[test]
fn missing_git_descriptor_re_discovers_parent() {
    let f = fixture::Fixture::new("adapter-rediscover");
    let nested = f.0.join("nested");
    fs::create_dir(&nested).unwrap();
    f.git(&[
        "-C",
        nested.to_str().unwrap(),
        "init",
        "--initial-branch=main",
    ]);
    fs::write(nested.join("file.txt"), "inner\n").unwrap();
    let initial = parity(&nested, MetadataBackend::Gix);
    fs::rename(nested.join(".git"), f.0.join("git-backup")).unwrap();
    let fresh = repository::load_metadata_with_backend(
        &initial.root,
        Some(&initial.context),
        MetadataBackend::Gix,
    )
    .unwrap();
    assert_eq!(fresh.root, fs::canonicalize(&f.0).unwrap());
    parity(&nested, MetadataBackend::Gix);
}

#[cfg(unix)]
#[test]
fn invalid_utf8_is_an_error_in_both_backends() {
    use std::{io::Write, process::Stdio};
    let f = fixture::Fixture::new("adapter-utf8");
    f.write("valid.txt", "base\n");
    commit(&f);
    let id = Command::new("git")
        .arg("-C")
        .arg(&f.0)
        .args(["rev-parse", "HEAD:valid.txt"])
        .output()
        .unwrap();
    // APFS rejects non-UTF-8 disk names, but Git's index can still contain them.
    let mut child = Command::new("git")
        .arg("-C")
        .arg(&f.0)
        .args(["update-index", "-z", "--index-info"])
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    input
        .write_all(format!("100644 {}\t", String::from_utf8(id.stdout).unwrap().trim()).as_bytes())
        .unwrap();
    input.write_all(b"invalid-\xff.txt\0").unwrap();
    drop(input);
    assert!(child.wait().unwrap().success());
    for backend in [MetadataBackend::Git, MetadataBackend::Gix] {
        assert!(repository::load_metadata_with_backend(&f.0, None, backend).is_err());
    }
}

#[test]
fn environment_probe() {
    let Some(path) = std::env::var_os("DIFFY_TEST_GIX_ENV_ROOT") else {
        return;
    };
    let snapshot = parity(Path::new(&path), MetadataBackend::Git);
    assert_eq!(snapshot.fallback_reason, Some("Git environment overrides"));
    assert_eq!(snapshot.groups[0][0].path.as_str(), "one.txt");
    assert!(snapshot.groups[1].is_empty());
    // This process is isolated: verify the app's configured switch too.
    let configured = repository::load_metadata(Path::new(&path)).unwrap();
    assert_eq!(configured.fallback_reason, snapshot.fallback_reason);
    assert_eq!(configured.groups, snapshot.groups);
}

#[test]
fn alternate_index_and_global_config_are_preserved_via_fallback() {
    let f = fixture::Fixture::new("adapter-env");
    f.write("one.txt", "base\n");
    commit(&f);
    let alternate = f.0.join(".git/alternate-index");
    fs::copy(f.0.join(".git/index"), &alternate).unwrap();
    f.write("one.txt", "changed\n");
    assert!(
        Command::new("git")
            .arg("-C")
            .arg(&f.0)
            .args(["add", "one.txt"])
            .env("GIT_INDEX_FILE", &alternate)
            .status()
            .unwrap()
            .success()
    );
    let global = f.0.join(".git/global-config");
    let excludes = f.0.join(".git/global-excludes");
    fs::write(&excludes, "global-only.txt\n").unwrap();
    fs::write(
        &global,
        format!("[core]\nexcludesFile = {}\n", excludes.display()),
    )
    .unwrap();
    f.write("global-only.txt", "ignored\n");
    let original = fs::read(f.0.join(".git/index")).unwrap();
    let alt_before = fs::read(&alternate).unwrap();
    assert!(
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "environment_probe", "--nocapture"])
            .env("DIFFY_TEST_GIX_ENV_ROOT", &f.0)
            .env("DIFFY_GIT_BACKEND", "gix")
            .env("GIT_INDEX_FILE", &alternate)
            .env("GIT_CONFIG_GLOBAL", &global)
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(fs::read(f.0.join(".git/index")).unwrap(), original);
    assert_eq!(fs::read(alternate).unwrap(), alt_before);
}

#[test]
fn gix_scope_paths_match_git_without_writing_index() {
    for mixed in [false, true] {
        let fixture = fixture::Fixture::workload(mixed);
        let index = fixture.0.join(".git/index");
        let before = std::fs::read(&index).ok();
        let metadata = diffy::repository::load_metadata(&fixture.0).unwrap();
        let expected: [BTreeSet<String>; 2] = metadata
            .groups
            .map(|files| files.into_iter().map(|f| f.path.to_string()).collect());
        assert_eq!(native::scope_paths(&fixture.0).unwrap(), expected);
        assert_eq!(std::fs::read(index).ok(), before);
    }
}

#[test]
fn gix_handles_literal_names_renames_and_linked_worktrees() {
    let fixture = fixture::Fixture::new("gix-compatibility");
    fixture.write("old.rs", "unchanged content for rename detection\n");
    fixture.write("tracked.rs", "base\n");
    fixture.git(&["add", "."]);
    fixture.git(&["commit", "-m", "base"]);
    fixture.git(&["mv", "old.rs", "new.rs"]);
    fixture.write("new.rs", "unstaged content\n");
    fixture.write(
        if cfg!(windows) {
            "[é] 雪 space.rs"
        } else {
            ":(glob)*\n雪.rs"
        },
        "literal new file\n",
    );
    let metadata = diffy::repository::load_metadata(&fixture.0).unwrap();
    let expected = metadata.groups.map(|files| {
        files
            .into_iter()
            .map(|f| f.path.to_string())
            .collect::<BTreeSet<_>>()
    });
    assert_eq!(native::scope_paths(&fixture.0).unwrap(), expected);
    let linked = fixture.0.join("linked");
    fixture.git(&[
        "worktree",
        "add",
        "--detach",
        linked.to_str().unwrap(),
        "HEAD",
    ]);
    std::fs::write(linked.join("tracked.rs"), "worktree change\n").unwrap();
    let metadata = diffy::repository::load_metadata(&linked).unwrap();
    let refreshed =
        diffy::repository::load_metadata_with_context(&metadata.root, Some(&metadata.context))
            .unwrap();
    assert_eq!(metadata.groups, refreshed.groups);
    let expected = metadata.groups.map(|files| {
        files
            .into_iter()
            .map(|f| f.path.to_string())
            .collect::<BTreeSet<_>>()
    });
    assert_eq!(native::scope_paths(&linked).unwrap(), expected);
}
