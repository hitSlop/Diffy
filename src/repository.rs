use std::{
    collections::{BTreeSet, HashMap, HashSet},
    ffi::OsString,
    fs::File,
    hash::{Hash, Hasher},
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::OnceLock,
};

use gpui_kit::SharedString;
use gpui_kit::component::diff::{DiffFile, DiffFileStatus};
mod metadata;
pub use metadata::*;
mod worktrees;
pub use worktrees::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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
    /// Hash of `patch`, so the viewer can skip an unchanged refresh without keeping a second copy.
    pub fingerprint: u64,
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
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        patch.hash(&mut hasher);
        Ok(Self {
            patch,
            files,
            revisions,
            fingerprint: hasher.finish(),
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

struct GitLaunch {
    program: PathBuf,
    path: Option<OsString>,
}

fn git_launch() -> Result<&'static GitLaunch, String> {
    static CACHED: OnceLock<GitLaunch> = OnceLock::new();
    if let Some(launch) = CACHED.get() {
        return Ok(launch);
    }
    // Finder and LaunchServices need not inherit a shell PATH. Keep user
    // entries first, then add the usual install locations. The successful
    // lookup is reused: a refresh runs Git several times.
    let mut paths: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect())
        .unwrap_or_default();
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
    #[cfg(windows)]
    {
        paths.extend(
            ["ProgramFiles", "ProgramFiles(x86)"]
                .into_iter()
                .filter_map(std::env::var_os)
                .map(|root| PathBuf::from(root).join("Git/cmd")),
        );
        if let Some(dirs) = directories::BaseDirs::new() {
            paths.push(dirs.data_local_dir().join("Programs/Git/cmd"));
        }
    }
    let name = if cfg!(windows) { "git.exe" } else { "git" };
    let program = paths
        .iter()
        .map(|p| p.join(name))
        .find(|p| p.is_file())
        .ok_or("Git wasn't found. Install Git, then reopen Diffy.")?;
    #[cfg(target_os = "macos")]
    let program = if program == Path::new("/usr/bin/git") {
        // Apple's /usr/bin/git dispatches through the selected developer tools.
        // Resolve that tool once instead of paying for the dispatcher on every read.
        Command::new("/usr/bin/xcrun")
            .args(["--find", "git"])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .map(|path| PathBuf::from(path.trim_end_matches('\n')))
            .filter(|path| path.is_file())
            .unwrap_or(program)
    } else {
        program
    };
    let launch = GitLaunch {
        program,
        path: std::env::join_paths(paths).ok(),
    };
    Ok(CACHED.get_or_init(|| launch))
}

fn git_command() -> Result<Command, String> {
    let launch = git_launch()?;
    let mut command = Command::new(&launch.program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW: background Git reads must not open consoles.
    }
    if let Some(path) = &launch.path {
        command.env("PATH", path);
    }
    Ok(command)
}

pub(crate) fn git(path: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let _span = tracing::info_span!("git_read").entered();
    let output = git_command()?
        .args([
            "--no-pager",
            "--no-optional-locks",
            "--literal-pathspecs",
            // `git diff` can refresh a stat-only index even with optional
            // locks disabled (notably after EOL normalization). Reads must
            // preserve both index contents and its timestamp.
            "-c",
            "diff.autoRefreshIndex=false",
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

/// Git is authoritative for excludesFile overrides, conditional includes and tracked paths.
/// Feed names through stdin: check-ignore deliberately does not accept --literal-pathspecs.
pub(crate) fn ignored_paths(root: &Path, paths: &[PathBuf]) -> Result<HashSet<PathBuf>, String> {
    use std::io::Write as _;
    if paths.is_empty() {
        return Ok(HashSet::new());
    }
    let mut child = git_command()?
        .args(["--no-pager", "--no-optional-locks", "-C"])
        .arg(root)
        .args(["check-ignore", "-z", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let mut input = child.stdin.take().ok_or("Git stdin unavailable")?;
    let (output, write) = std::thread::scope(|scope| {
        let writer = scope.spawn(move || {
            for path in paths {
                input.write_all(path.as_os_str().as_encoded_bytes())?;
                input.write_all(&[0])?;
            }
            Ok::<_, std::io::Error>(())
        });
        // Drain stdout while writing stdin; a large ignored batch exceeds pipe capacity.
        let output = child.wait_with_output();
        let write = writer.join().expect("Git input writer panicked");
        (output, write)
    });
    let output = output.map_err(|e| e.to_string())?;
    write.map_err(|e| e.to_string())?;
    if !matches!(output.status.code(), Some(0 | 1)) {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    Ok(text(output.stdout)?
        .split('\0')
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .collect())
}

pub(crate) struct RefreshSlot(async_channel::Sender<()>);
impl Drop for RefreshSlot {
    fn drop(&mut self) {
        let _ = self.0.try_send(());
    }
}
/// Bound aggregate refresh fan-out when restoring or watching many projects.
pub(crate) async fn refresh_slot() -> RefreshSlot {
    static POOL: OnceLock<(async_channel::Sender<()>, async_channel::Receiver<()>)> =
        OnceLock::new();
    let (tx, rx) = POOL.get_or_init(|| {
        let (tx, rx) = async_channel::bounded(2);
        tx.try_send(()).unwrap();
        tx.try_send(()).unwrap();
        (tx, rx)
    });
    rx.recv().await.expect("refresh pool stays open");
    RefreshSlot(tx.clone())
}

pub fn resolve_root(path: &Path) -> Result<PathBuf, String> {
    let root = text(git(path, &["rev-parse", "--show-toplevel"])?)?;
    let root = PathBuf::from(root.trim_end_matches('\n'));
    Ok(std::fs::canonicalize(&root).unwrap_or(root))
}

fn text(bytes: Vec<u8>) -> Result<String, String> {
    String::from_utf8(bytes).map_err(|_| {
        "This repository contains non-UTF-8 paths or source that this POC cannot display.".into()
    })
}

fn branch_name(root: &Path) -> Result<String, String> {
    let bytes = git(root, &["symbolic-ref", "--quiet", "--short", "HEAD"])
        .or_else(|_| git(root, &["rev-parse", "--short", "HEAD"]))?;
    Ok(text(bytes)?.trim().into())
}

fn git_directories(root: &Path) -> Result<Vec<PathBuf>, String> {
    let output = text(git(root, &["rev-parse", "--git-dir", "--git-common-dir"])?)?;
    let mut git_dirs = Vec::new();
    for line in output.lines() {
        if line.is_empty() {
            continue;
        }
        let dir = PathBuf::from(line);
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
    Ok(git_dirs)
}

/// Tracked paths that also match an ignore rule. `git check-ignore` hides these,
/// but editing one still changes the diff.
pub(crate) fn tracked_ignored_paths(root: &Path) -> Result<HashSet<PathBuf>, String> {
    let bytes = git(
        root,
        &["ls-files", "-z", "--cached", "-i", "--exclude-standard"],
    )?;
    Ok(text(bytes)?
        .split('\0')
        .filter(|name| !name.is_empty())
        .map(PathBuf::from)
        .collect())
}

/// Reads a worktree file, but stops once the bytes cannot be shown as text.
/// A large untracked binary otherwise has to be loaded in full before the
/// patch throws that content away.
fn read_untracked(path: &Path) -> std::io::Result<Vec<u8>> {
    let mut file = File::open(path)?;
    let mut content = Vec::new();
    let mut buf = [0u8; 8192];
    let mut checked = 0;
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        if buf[..n].contains(&0) {
            return Ok(vec![0]);
        }
        content.extend_from_slice(&buf[..n]);
        match std::str::from_utf8(&content[checked..]) {
            Ok(_) => checked = content.len(),
            Err(error) if error.error_len().is_none() => checked += error.valid_up_to(),
            Err(_) => return Ok(vec![0]),
        }
    }
    if std::str::from_utf8(&content[checked..]).is_err() {
        return Ok(vec![0]);
    }
    Ok(content)
}

pub(crate) struct RefreshSnapshot {
    pub root: PathBuf,
    pub branch: String,
    pub git_dirs: Vec<PathBuf>,
    pub staged: Option<GroupSnapshot>,
    pub changes: Option<GroupSnapshot>,
}

pub fn load(path: &Path) -> Result<Snapshot, String> {
    let snapshot = load_refresh(path, [None, None])?;
    Ok(Snapshot {
        root: snapshot.root,
        branch: snapshot.branch,
        git_dirs: snapshot.git_dirs,
        staged: snapshot.staged.expect("initial staged snapshot"),
        changes: snapshot.changes.expect("initial changes snapshot"),
    })
}

pub(crate) fn load_refresh(
    path: &Path,
    previous: [Option<u64>; 2],
) -> Result<RefreshSnapshot, String> {
    let root = resolve_root(path)?;
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
    let mut working_args = diff_args.to_vec();
    working_args.push("--");
    let untracked_args = ["ls-files", "-z", "--others", "--exclude-standard"];
    // These reads do not depend on each other. `--no-optional-locks` keeps them
    // from taking the index lock, so they can run together.
    let (branch, staged_patch, patch, names, git_dirs) = std::thread::scope(|scope| {
        let branch = scope.spawn(|| branch_name(&root));
        let staged = scope.spawn(|| git(&root, &cached_args).and_then(text));
        let working = scope.spawn(|| git(&root, &working_args).and_then(text));
        let names = scope.spawn(|| git(&root, &untracked_args).and_then(text));
        let dirs = scope.spawn(|| git_directories(&root));
        (
            branch.join().expect("branch read panicked"),
            staged.join().expect("staged diff panicked"),
            working.join().expect("working diff panicked"),
            names.join().expect("untracked list panicked"),
            dirs.join().expect("git dir read panicked"),
        )
    });
    let branch = branch?;
    let staged_patch = staged_patch?;
    let mut patch = patch?;
    let names = names?;
    let git_dirs = git_dirs?;
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
            read_untracked(&path).map_err(|e| format!("Couldn't read {name}: {e}"))?
        };
        patch.push_str(&addition_patch(name, &content, is_symlink));
    }
    let changed = |patch: String, previous: Option<u64>| {
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        patch.hash(&mut hash);
        if previous == Some(hash.finish()) {
            Ok(None)
        } else {
            GroupSnapshot::parse(patch).map(Some)
        }
    };
    Ok(RefreshSnapshot {
        root,
        branch,
        git_dirs,
        staged: changed(staged_patch, previous[0])?,
        changes: changed(patch, previous[1])?,
    })
}

/// Change only the selected file's index entry; never touch working-tree bytes.
pub fn set_staged(root: &Path, file: &DiffFile, staged: bool) -> Result<(), String> {
    set_files_staged(root, std::slice::from_ref(file), staged)
}

/// Mutates all supplied paths in one Git invocation, including both sides of renames.
pub fn set_files_staged(root: &Path, files: &[DiffFile], staged: bool) -> Result<(), String> {
    let entries = files
        .iter()
        .map(|f| ChangedFile::from_diff(f, 0))
        .collect::<Vec<_>>();
    set_entries_staged(root, &entries, staged)
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
            let path = tempfile::Builder::new()
                .prefix("diffy-repository-test-")
                .tempdir()
                .unwrap()
                .keep();
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
        #[cfg(not(windows))]
        let name = ":(glob)* [é]\t\".rs";
        #[cfg(windows)]
        let name = "[é] 日本語 space.rs";
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
        repo.write(
            if cfg!(windows) {
                "[é] space.rs"
            } else {
                ":(glob)*.rs"
            },
            "literal\n",
        );
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

    #[test]
    fn long_untracked_text_keeps_characters_split_across_reads() {
        let repo = TestRepository::new();
        let mut body = vec![b'a'; 8191];
        body.extend("é\n".as_bytes());
        std::fs::write(repo.0.join("long.rs"), &body).unwrap();
        let snapshot = load(&repo.0).unwrap();
        let patch = &snapshot.changes.patch;
        assert!(patch.contains("é"));
        assert!(patch.contains(&"a".repeat(8191)));
        assert!(!snapshot.changes.files[0].is_binary());
    }

    #[test]
    fn untracked_binary_stops_being_text_at_the_first_nul() {
        let repo = TestRepository::new();
        let mut body = vec![b'a'; 9000];
        body[10] = 0;
        std::fs::write(repo.0.join("blob.bin"), &body).unwrap();
        let snapshot = load(&repo.0).unwrap();
        assert!(snapshot.changes.files[0].is_binary());
        assert!(!snapshot.changes.patch.contains(&"a".repeat(20)));
    }
    #[test]
    fn unchanged_refresh_returns_metadata_without_reparsing_groups() {
        let repo = TestRepository::new();
        let root = &repo.0;
        std::fs::write(root.join("new.txt"), "first\n").unwrap();
        let initial = load(root).unwrap();
        let hashes = [
            Some(initial.staged.fingerprint),
            Some(initial.changes.fingerprint),
        ];
        let unchanged = load_refresh(root, hashes).unwrap();
        assert!(unchanged.staged.is_none() && unchanged.changes.is_none());
        std::fs::write(root.join("new.txt"), "second\n").unwrap();
        let changed = load_refresh(root, hashes).unwrap();
        assert!(changed.staged.is_none() && changed.changes.is_some());
    }
    #[test]
    fn ignored_batches_larger_than_pipe_capacity_complete() {
        let repo = TestRepository::new();
        repo.write(".gitignore", "ignored/\n");
        let paths = (0..4096)
            .map(|ix| PathBuf::from(format!("ignored/{}/{ix}.txt", "long".repeat(30))))
            .collect::<Vec<_>>();
        assert_eq!(ignored_paths(&repo.0, &paths).unwrap().len(), paths.len());
    }

    #[test]
    #[ignore = "manual refresh performance comparison"]
    fn refresh_performance_comparison() {
        let repo = TestRepository::new();
        let large = (0..50_000)
            .map(|n| format!("source line {n} with representative text and context\n"))
            .collect::<String>();
        repo.write("large.txt", &large);
        for n in 0..1000 {
            repo.write(&format!("files/{n:04}.txt"), "small new file\n");
        }
        let initial = load(&repo.0).unwrap();
        let hashes = [
            Some(initial.staged.fingerprint),
            Some(initial.changes.fingerprint),
        ];
        let mut full = Vec::new();
        let mut unchanged = Vec::new();
        for _ in 0..9 {
            let start = std::time::Instant::now();
            drop(load(&repo.0).unwrap());
            full.push(start.elapsed().as_secs_f64() * 1000.);
            let start = std::time::Instant::now();
            drop(load_refresh(&repo.0, hashes).unwrap());
            unchanged.push(start.elapsed().as_secs_f64() * 1000.);
        }
        full.sort_by(f64::total_cmp);
        unchanged.sort_by(f64::total_cmp);
        println!(
            "50k source lines + 1000 files: full parse p50={:.1}ms p90={:.1}ms; unchanged refresh p50={:.1}ms p90={:.1}ms",
            full[4], full[8], unchanged[4], unchanged[8]
        );
    }

    #[test]
    fn metadata_and_lazy_previews_preserve_scopes_and_index() {
        let repo = TestRepository::new();
        repo.write("one.rs", "base\n");
        repo.write("old.rs", "rename unchanged content\n");
        repo.commit();
        repo.write("one.rs", "staged\n");
        git(&repo.0, &["add", "one.rs"]).unwrap();
        repo.write("one.rs", "working\n");
        git(&repo.0, &["mv", "old.rs", "new.rs"]).unwrap();
        repo.write(
            if cfg!(windows) {
                "[é] unusual name.rs"
            } else {
                ":(glob)* unusual\nname.rs"
            },
            "new\n",
        );
        std::fs::write(repo.0.join("binary.dat"), b"\0binary").unwrap();
        let before = std::fs::read(repo.0.join(".git/index")).unwrap();
        let listing = load_metadata(&repo.0).unwrap();
        let eager = load(&repo.0).unwrap();
        for (group, eager) in [
            (ChangeGroup::Staged, eager.staged),
            (ChangeGroup::Changes, eager.changes),
        ] {
            let entries = &listing.groups[group.index()];
            assert_eq!(
                entries
                    .iter()
                    .map(|f| (&f.path, f.status))
                    .collect::<Vec<_>>(),
                eager
                    .files
                    .iter()
                    .map(|f| (f.path(), f.status()))
                    .collect::<Vec<_>>()
            );
            for entry in entries {
                let preview = load_preview(&listing.root, group, entry).unwrap();
                let expected = eager
                    .files
                    .iter()
                    .find(|f| f.path() == entry.path())
                    .unwrap();
                assert_eq!(
                    (
                        preview.file.additions(),
                        preview.file.deletions(),
                        preview.file.is_binary()
                    ),
                    (
                        expected.additions(),
                        expected.deletions(),
                        expected.is_binary()
                    )
                );
            }
        }
        assert_eq!(before, std::fs::read(repo.0.join(".git/index")).unwrap());
        set_entries_staged(&repo.0, &listing.groups[1], true).unwrap();
        assert!(load_metadata(&repo.0).unwrap().groups[1].is_empty());
    }

    #[test]
    fn conflicts_are_readable_and_never_staged_with_other_files() {
        let repo = TestRepository::new();
        repo.write("conflict.txt", "base\n");
        repo.write("other.txt", "base\n");
        repo.commit();
        git(&repo.0, &["checkout", "-b", "side"]).unwrap();
        repo.write("conflict.txt", "side\n");
        repo.commit();
        git(&repo.0, &["checkout", "main"]).unwrap();
        repo.write("conflict.txt", "main\n");
        repo.commit();
        assert!(
            git(
                &repo.0,
                &[
                    "-c",
                    "user.name=Test",
                    "-c",
                    "user.email=test@example.invalid",
                    "merge",
                    "side"
                ]
            )
            .is_err()
        );
        repo.write("other.txt", "ordinary change\n");
        let before = git(&repo.0, &["ls-files", "--unmerged", "-z"]).unwrap();
        let snapshot = load_metadata(&repo.0).unwrap();
        assert!(snapshot.groups[0].is_empty());
        let conflict = snapshot.groups[1].iter().find(|f| !f.stageable()).unwrap();
        assert_eq!(conflict.path.as_str(), "conflict.txt");
        assert_eq!(
            load_preview(&repo.0, ChangeGroup::Changes, conflict)
                .unwrap()
                .file
                .path(),
            conflict.path()
        );
        set_entries_staged(&repo.0, &snapshot.groups[1], true).unwrap();
        assert_eq!(
            git(&repo.0, &["ls-files", "--unmerged", "-z"]).unwrap(),
            before
        );
        assert_eq!(
            load_metadata(&repo.0).unwrap().groups[0][0].path.as_str(),
            "other.txt"
        );
    }

    #[test]
    fn metadata_revisions_follow_disk_edits_and_unborn_entries() {
        let repo = TestRepository::new();
        repo.write("a.txt", "first\n");
        let first = load_metadata(&repo.0).unwrap();
        repo.write("a.txt", "second longer\n");
        let second = load_metadata(&repo.0).unwrap();
        assert_ne!(first.groups[1][0].revision, second.groups[1][0].revision);
        set_entries_staged(&repo.0, &second.groups[1], true).unwrap();
        let staged = load_metadata(&repo.0).unwrap();
        set_entries_staged(&repo.0, &staged.groups[0], false).unwrap();
        assert_eq!(
            std::fs::read_to_string(repo.0.join("a.txt")).unwrap(),
            "second longer\n"
        );
        assert!(load_metadata(&repo.0).unwrap().groups[0].is_empty());
    }

    #[test]
    fn cached_location_is_revalidated_when_git_directory_disappears() {
        let parent = TestRepository::new();
        parent.write("outer.txt", "base\n");
        parent.commit();
        let nested = parent.0.join("nested");
        std::fs::create_dir(&nested).unwrap();
        git(&nested, &["init", "--initial-branch=main"]).unwrap();
        std::fs::write(nested.join("inner.txt"), "inner\n").unwrap();
        let initial = load_metadata(&nested).unwrap();
        std::fs::rename(nested.join(".git"), parent.0.join("git-backup")).unwrap();
        let refreshed = load_metadata_with_context(&initial.root, Some(&initial.context)).unwrap();
        assert_eq!(refreshed.root, std::fs::canonicalize(&parent.0).unwrap());
    }
}
