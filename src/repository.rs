use std::{
    collections::{BTreeSet, HashMap},
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    process::Command,
};

use gpui_kit::SharedString;
use gpui_kit::component::diff::{DiffFile, DiffFileStatus};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeGroup {
    Staged,
    Changes,
}
impl ChangeGroup {
    pub fn index(self) -> usize {
        if self == Self::Staged { 0 } else { 1 }
    }
    pub fn other(self) -> Self {
        if self == Self::Staged {
            Self::Changes
        } else {
            Self::Staged
        }
    }
    pub fn label(self) -> &'static str {
        if self == Self::Staged {
            "Staged"
        } else {
            "Changes"
        }
    }
}

pub struct GroupSnapshot {
    pub patch: String,
    pub files: Vec<DiffFile>,
    pub revisions: HashMap<SharedString, u64>,
}
impl GroupSnapshot {
    pub fn parse(patch: String) -> Result<Self, String> {
        let mut files =
            DiffFile::parse(&patch).map_err(|e| format!("Couldn't read this diff: {e}"))?;
        let starts = patch
            .match_indices("diff --git ")
            .filter(|(ix, _)| *ix == 0 || patch.as_bytes()[ix - 1] == b'\n')
            .map(|(ix, _)| ix)
            .collect::<Vec<_>>();
        let revisions = files
            .iter()
            .enumerate()
            .map(|(ix, file)| {
                let block = starts.get(ix).map_or(patch.as_str(), |start| {
                    &patch[*start..starts.get(ix + 1).copied().unwrap_or(patch.len())]
                });
                let mut hash = std::collections::hash_map::DefaultHasher::new();
                block.hash(&mut hash);
                (file.path().clone(), hash.finish())
            })
            .collect();
        files.sort_by(|a, b| a.path().cmp(b.path()));
        Ok(Self {
            patch,
            files,
            revisions,
        })
    }
}

pub struct Snapshot {
    pub root: PathBuf,
    pub branch: String,
    pub git_dirs: Vec<PathBuf>,
    pub staged: GroupSnapshot,
    pub changes: GroupSnapshot,
}

fn git_command() -> Result<Command, String> {
    let mut paths: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect())
        .unwrap_or_default();
    // Finder/LaunchServices need not inherit a shell's PATH. Keep user entries
    // first, then add standard installation paths for Git and its helpers.
    #[cfg(target_os = "macos")]
    paths.extend(
        [
            "/opt/homebrew/bin",
            "/usr/local/bin",
            "/usr/bin",
            "/bin",
            "/usr/sbin",
            "/sbin",
        ]
        .map(PathBuf::from),
    );
    #[cfg(all(unix, not(target_os = "macos")))]
    paths.extend(["/usr/local/bin", "/usr/bin", "/bin"].map(PathBuf::from));
    let name = if cfg!(windows) { "git.exe" } else { "git" };
    let executable = paths
        .iter()
        .map(|p| p.join(name))
        .find(|p| p.is_file())
        .ok_or("Git wasn't found. Install Git, then reopen Diffy.")?;
    let mut command = Command::new(executable);
    if let Ok(path) = std::env::join_paths(paths) {
        command.env("PATH", path);
    }
    Ok(command)
}

fn git(path: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let output = git_command()?
        .args([
            "--no-pager",
            "--no-optional-locks",
            "--literal-pathspecs",
            "-C",
        ])
        .arg(path)
        .args(args)
        .output()
        .map_err(|error| format!("Couldn't run Git: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    Ok(output.stdout)
}

fn text(bytes: Vec<u8>) -> Result<String, String> {
    String::from_utf8(bytes).map_err(|_| {
        "This repository contains non-UTF-8 paths or source that this POC cannot display.".into()
    })
}

pub fn load(path: &Path) -> Result<Snapshot, String> {
    let root = text(git(path, &["rev-parse", "--show-toplevel"])?)?;
    let root = PathBuf::from(root.trim_end_matches('\n'));
    let branch = text(
        git(&root, &["symbolic-ref", "--quiet", "--short", "HEAD"])
            .or_else(|_| git(&root, &["rev-parse", "--short", "HEAD"]))?,
    )?;
    let diff_args = [
        "diff",
        "--no-color",
        "--no-ext-diff",
        "--no-textconv",
        "--find-renames",
        "--src-prefix=a/",
        "--dst-prefix=b/",
        "--unified=1000000",
    ];
    let mut cached_args = diff_args.to_vec();
    cached_args.extend(["--cached", "--"]);
    let staged = GroupSnapshot::parse(text(git(&root, &cached_args)?)?)?;
    let mut working_args = diff_args.to_vec();
    working_args.push("--");
    let mut patch = text(git(&root, &working_args)?)?;
    let args = &["ls-files", "-z", "--others", "--exclude-standard"];
    let names = text(git(&root, args)?)?;
    for name in names
        .split('\0')
        .filter(|name| !name.is_empty())
        .collect::<BTreeSet<_>>()
    {
        let path = root.join(name);
        // A staged deletion on an unborn branch may no longer exist on disk.
        let metadata = match path.symlink_metadata() {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(format!("Couldn't inspect {name}: {error}")),
        };
        let is_symlink = metadata.file_type().is_symlink();
        let content = if is_symlink {
            path.read_link()
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .as_bytes()
                .to_vec()
        } else {
            std::fs::read(&path).map_err(|e| format!("Couldn't read {name}: {e}"))?
        };
        patch.push_str(&addition_patch(name, &content, is_symlink));
    }
    let mut git_dirs = Vec::new();
    for arg in ["--git-dir", "--git-common-dir"] {
        let dir = PathBuf::from(text(git(&root, &["rev-parse", arg])?)?.trim_end_matches('\n'));
        let dir = if dir.is_absolute() {
            dir
        } else {
            root.join(dir)
        };
        let dir = std::fs::canonicalize(&dir).unwrap_or(dir);
        if !git_dirs.contains(&dir) {
            git_dirs.push(dir);
        }
    }
    Ok(Snapshot {
        root,
        branch: branch.trim().into(),
        git_dirs,
        staged,
        changes: GroupSnapshot::parse(patch)?,
    })
}

/// Change only the selected file's index entry; never touch working-tree bytes.
pub fn set_staged(root: &Path, file: &DiffFile, staged: bool) -> Result<(), String> {
    set_files_staged(root, std::slice::from_ref(file), staged)
}

/// Mutates all supplied paths in one Git invocation, including both sides of renames.
pub fn set_files_staged(root: &Path, files: &[DiffFile], staged: bool) -> Result<(), String> {
    if files.is_empty() {
        return Ok(());
    }
    let mut paths = Vec::new();
    for file in files {
        paths.push(file.path().as_str());
        if file.status() == DiffFileStatus::Renamed {
            paths.extend(file.original_path().map(|p| p.as_str()));
            paths.extend(file.modified_path().map(|p| p.as_str()));
        }
    }
    paths.sort_unstable();
    paths.dedup();
    let mut args = if staged {
        vec!["add", "--all", "--"]
    } else if git(root, &["rev-parse", "--verify", "HEAD"]).is_ok() {
        vec!["restore", "--staged", "--"]
    } else {
        vec!["rm", "--cached", "-f", "--"]
    };
    args.extend(paths);
    git(root, &args).map(|_| ())
}

// Git's C-style path quoting preserves whitespace, quotes, and Unicode bytes.
fn quote_path(path: &str) -> String {
    let mut result = String::from("\"");
    for byte in path.bytes() {
        match byte {
            b'"' => result.push_str("\\\""),
            b'\\' => result.push_str("\\\\"),
            32..=126 => result.push(byte as char),
            _ => result.push_str(&format!("\\{byte:03o}")),
        }
    }
    result.push('"');
    result
}

fn addition_patch(name: &str, content: &[u8], symlink: bool) -> String {
    let old = quote_path(&format!("a/{name}"));
    let new = quote_path(&format!("b/{name}"));
    let mut patch = format!(
        "diff --git {old} {new}\nnew file mode {}\n",
        if symlink { "120000" } else { "100644" }
    );
    let Ok(content) = std::str::from_utf8(content) else {
        patch.push_str(&format!("Binary files /dev/null and {new} differ\n"));
        return patch;
    };
    if content.contains('\0') {
        patch.push_str(&format!("Binary files /dev/null and {new} differ\n"));
        return patch;
    }
    patch.push_str(&format!("--- /dev/null\n+++ {new}\n"));
    if !content.is_empty() {
        patch.push_str(&format!(
            "@@ -0,0 +1,{} @@\n",
            content.split_inclusive('\n').count()
        ));
        for line in content.split_inclusive('\n') {
            patch.push('+');
            patch.push_str(line);
            if !line.ends_with('\n') {
                patch.push_str("\n\\ No newline at end of file\n");
            }
        }
    }
    patch
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn untracked_paths_and_missing_newlines_round_trip() {
        let name = "src/hello\t\"é.rs";
        let patch = addition_patch(name, b"first\nlast", false);
        let files = DiffFile::parse(&patch).unwrap();
        assert_eq!(files[0].path().as_str(), name);
        assert_eq!(files[0].additions(), 2);
        assert!(patch.contains("\\ No newline at end of file"));
    }

    #[test]
    fn empty_and_binary_additions_are_visible() {
        for content in [b"".as_slice(), b"\0binary", b"\xff"] {
            let files = DiffFile::parse(&addition_patch("new.dat", content, false)).unwrap();
            assert_eq!(files.len(), 1);
            assert_eq!(files[0].path().as_str(), "new.dat");
            assert_eq!(files[0].is_binary(), !content.is_empty());
        }
    }

    #[test]
    fn demo_patches_parse() {
        for demo in crate::demo::projects() {
            assert!(!DiffFile::parse(&demo.changes).unwrap().is_empty());
        }
    }

    struct TestRepository(PathBuf);

    impl TestRepository {
        fn new() -> Self {
            let suffix = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path =
                std::env::temp_dir().join(format!("differ-test-{}-{suffix}", std::process::id()));
            std::fs::create_dir(&path).unwrap();
            git(&path, &["init", "--initial-branch=main"]).unwrap();
            Self(path)
        }

        fn write(&self, name: &str, content: &str) {
            let path = self.0.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, content).unwrap();
        }

        fn commit(&self) {
            git(&self.0, &["add", "."]).unwrap();
            git(
                &self.0,
                &[
                    "-c",
                    "user.name=Diffy Test",
                    "-c",
                    "user.email=test@example.invalid",
                    "-c",
                    "commit.gpgsign=false",
                    "-c",
                    "core.hooksPath=/dev/null",
                    "commit",
                    "-m",
                    "fixture",
                ],
            )
            .unwrap();
        }
    }

    impl Drop for TestRepository {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn scopes_separate_index_and_disk_changes_without_writing_index() {
        let repo = TestRepository::new();
        repo.write("modified.rs", "first\nsecond\n");
        repo.write("deleted.rs", "deleted\n");
        repo.write("old.rs", "unchanged content for rename detection\n");
        repo.write(".gitignore", "ignored.txt\n");
        repo.commit();
        repo.write("modified.rs", "staged\nsecond\n");
        git(&repo.0, &["add", "modified.rs"]).unwrap();
        repo.write("modified.rs", "staged\nunstaged\n");
        std::fs::remove_file(repo.0.join("deleted.rs")).unwrap();
        std::fs::rename(repo.0.join("old.rs"), repo.0.join("renamed.rs")).unwrap();
        git(&repo.0, &["add", "old.rs", "renamed.rs"]).unwrap();
        repo.write("src/é space.rs", "untracked\n");
        repo.write("ignored.txt", "ignored\n");
        let index = std::fs::read(repo.0.join(".git/index")).unwrap();
        let before = git(&repo.0, &["status", "--porcelain=v1", "-z"]).unwrap();
        let snapshot = load(&repo.0).unwrap();
        let paths = snapshot
            .changes
            .files
            .iter()
            .map(|file| file.path().as_str())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            paths,
            BTreeSet::from(["modified.rs", "deleted.rs", "src/é space.rs"])
        );
        assert!(snapshot.changes.patch.contains("+unstaged"));
        assert!(snapshot.staged.patch.contains("+staged"));
        assert_eq!(snapshot.staged.files.len(), 2);
        assert_eq!(
            before,
            git(&repo.0, &["status", "--porcelain=v1", "-z"]).unwrap()
        );
        assert_eq!(index, std::fs::read(repo.0.join(".git/index")).unwrap());
    }

    #[test]
    fn unborn_head_includes_staged_and_untracked_files_once() {
        let repo = TestRepository::new();
        repo.write("staged.rs", "staged\n");
        git(&repo.0, &["add", "staged.rs"]).unwrap();
        repo.write("staged.rs", "current contents\n");
        repo.write("untracked.rs", "untracked\n");
        let snapshot = load(&repo.0).unwrap();
        assert_eq!(snapshot.changes.files.len(), 2);
        assert!(snapshot.changes.patch.contains("+current contents"));
        assert!(snapshot.staged.patch.contains("+staged\n"));
    }
    #[test]
    fn stage_and_unstage_only_selected_file_and_preserve_disk() {
        let repo = TestRepository::new();
        repo.write("one.rs", "base\n");
        repo.write("other.rs", "base\n");
        repo.commit();
        repo.write("one.rs", "staged\n");
        repo.write("other.rs", "unrelated staged\n");
        git(&repo.0, &["add", "one.rs", "other.rs"]).unwrap();
        repo.write("one.rs", "latest working tree\n");
        let snapshot = load(&repo.0).unwrap();
        assert_eq!(snapshot.changes.files.len(), 1);
        set_staged(&repo.0, &snapshot.changes.files[0], true).unwrap();
        let snapshot = load(&repo.0).unwrap();
        assert!(snapshot.changes.files.is_empty());
        assert!(snapshot.staged.patch.contains("+latest working tree"));
        let file = snapshot
            .staged
            .files
            .iter()
            .find(|f| f.path().as_str() == "one.rs")
            .unwrap();
        set_staged(&repo.0, file, false).unwrap();
        let snapshot = load(&repo.0).unwrap();
        assert_eq!(snapshot.staged.files[0].path().as_str(), "other.rs");
        assert_eq!(snapshot.changes.files[0].path().as_str(), "one.rs");
        assert_eq!(
            std::fs::read_to_string(repo.0.join("one.rs")).unwrap(),
            "latest working tree\n"
        );
    }

    #[test]
    fn literal_pathspecs_and_unborn_unstage() {
        let repo = TestRepository::new();
        let name = ":(glob)* [é]\t\".rs";
        repo.write(name, "literal\n");
        repo.write("other.rs", "other\n");
        let snapshot = load(&repo.0).unwrap();
        let file = snapshot
            .changes
            .files
            .iter()
            .find(|f| f.path().as_str() == name)
            .unwrap();
        set_staged(&repo.0, file, true).unwrap();
        let snapshot = load(&repo.0).unwrap();
        assert_eq!(snapshot.staged.files.len(), 1);
        assert_eq!(snapshot.changes.files[0].path().as_str(), "other.rs");
        set_staged(&repo.0, &snapshot.staged.files[0], false).unwrap();
        assert!(load(&repo.0).unwrap().staged.files.is_empty());
        assert_eq!(
            std::fs::read_to_string(repo.0.join(name)).unwrap(),
            "literal\n"
        );
    }

    #[test]
    fn stage_all_preserves_worktree_and_handles_literals_deletions_and_partial_staging() {
        let repo = TestRepository::new();
        repo.write("one.rs", "base\n");
        repo.write("delete.rs", "gone\n");
        repo.commit();
        repo.write("one.rs", "index\n");
        git(&repo.0, &["add", "one.rs"]).unwrap();
        repo.write("one.rs", "latest\n");
        repo.write(":(glob)*.rs", "literal\n");
        repo.write("ignored.rs", "ignored\n");
        repo.write(".git/info/exclude", "ignored.rs\n");
        std::fs::remove_file(repo.0.join("delete.rs")).unwrap();
        let before = load(&repo.0).unwrap();
        set_files_staged(&repo.0, &before.changes.files, true).unwrap();
        let after = load(&repo.0).unwrap();
        assert!(after.changes.files.is_empty());
        assert_eq!(after.staged.files.len(), 3);
        assert!(after.staged.patch.contains("+latest"));
        assert_eq!(
            std::fs::read_to_string(repo.0.join("one.rs")).unwrap(),
            "latest\n"
        );
        assert_eq!(
            std::fs::read_to_string(repo.0.join("ignored.rs")).unwrap(),
            "ignored\n"
        );
        assert!(!repo.0.join("delete.rs").exists());
        set_files_staged(&repo.0, &after.staged.files, false).unwrap();
        assert!(load(&repo.0).unwrap().staged.files.is_empty());
        assert!(!repo.0.join("delete.rs").exists());
    }

    #[test]
    fn renamed_and_deleted_files_unstage_without_restoring_disk() {
        let repo = TestRepository::new();
        repo.write("old.rs", "unchanged content for rename detection\n");
        repo.write("deleted.rs", "deleted\n");
        repo.commit();
        std::fs::rename(repo.0.join("old.rs"), repo.0.join("new.rs")).unwrap();
        std::fs::remove_file(repo.0.join("deleted.rs")).unwrap();
        let snapshot = load(&repo.0).unwrap();
        let deletion = snapshot
            .changes
            .files
            .iter()
            .find(|f| f.path().as_str() == "deleted.rs")
            .unwrap();
        set_staged(&repo.0, deletion, true).unwrap();
        git(&repo.0, &["add", "--all", "--", "old.rs", "new.rs"]).unwrap();
        let snapshot = load(&repo.0).unwrap();
        let rename = snapshot
            .staged
            .files
            .iter()
            .find(|f| f.status() == DiffFileStatus::Renamed)
            .unwrap();
        set_staged(&repo.0, rename, false).unwrap();
        let snapshot = load(&repo.0).unwrap();
        assert_eq!(snapshot.staged.files.len(), 1);
        set_staged(&repo.0, &snapshot.staged.files[0], false).unwrap();
        assert!(!repo.0.join("old.rs").exists());
        assert!(!repo.0.join("deleted.rs").exists());
        assert!(repo.0.join("new.rs").exists());
        assert!(load(&repo.0).unwrap().staged.files.is_empty());
    }

    #[test]
    fn index_lock_error_preserves_index_and_is_recoverable() {
        let repo = TestRepository::new();
        repo.write("one.rs", "base\n");
        repo.commit();
        repo.write("one.rs", "changed\n");
        let snapshot = load(&repo.0).unwrap();
        let before = std::fs::read(repo.0.join(".git/index")).unwrap();
        std::fs::write(repo.0.join(".git/index.lock"), b"test lock").unwrap();
        assert!(
            set_staged(&repo.0, &snapshot.changes.files[0], true)
                .unwrap_err()
                .contains("index.lock")
        );
        assert_eq!(before, std::fs::read(repo.0.join(".git/index")).unwrap());
        assert_eq!(load(&repo.0).unwrap().changes.files.len(), 1);
        std::fs::remove_file(repo.0.join(".git/index.lock")).unwrap();
        set_staged(&repo.0, &snapshot.changes.files[0], true).unwrap();
        assert!(load(&repo.0).unwrap().changes.files.is_empty());
    }
    #[test]
    fn copy_action_does_not_stage_its_modified_source() {
        let repo = TestRepository::new();
        repo.write("source.rs", "base\n");
        repo.commit();
        repo.write("source.rs", "leave unstaged\n");
        repo.write("copy.rs", "base\n");
        let file = DiffFile::parse("diff --git a/source.rs b/copy.rs\nsimilarity index 100%\ncopy from source.rs\ncopy to copy.rs\n").unwrap().remove(0);
        assert_eq!(file.status(), DiffFileStatus::Copied);
        set_staged(&repo.0, &file, true).unwrap();
        let snapshot = load(&repo.0).unwrap();
        assert_eq!(snapshot.staged.files.len(), 1);
        assert_eq!(snapshot.staged.files[0].path().as_str(), "copy.rs");
        assert_eq!(snapshot.changes.files[0].path().as_str(), "source.rs");
    }
}
