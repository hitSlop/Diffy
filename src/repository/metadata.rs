//! Lightweight listings and demand-loaded previews. Listing never retains source contents.
use super::*;
use lru::LruCache;
use std::{
    num::NonZeroUsize,
    sync::{Arc, Mutex},
};
mod native;

/// Metadata rollout only: previews and writes always use Git.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MetadataBackend {
    #[default]
    Git,
    Gix,
}
impl MetadataBackend {
    fn from_setting(value: Option<&str>) -> Self {
        match value {
            Some("gix") => Self::Gix,
            _ => Self::Git,
        }
    }
    pub fn configured() -> Self {
        Self::from_setting(std::env::var("DIFFY_GIT_BACKEND").ok().as_deref())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangedFile {
    pub path: SharedString,
    pub original: Option<SharedString>,
    pub status: DiffFileStatus,
    pub revision: u64,
    pub untracked: bool,
}
impl ChangedFile {
    pub fn path(&self) -> &SharedString {
        &self.path
    }
    pub fn status(&self) -> DiffFileStatus {
        self.status
    }
    pub fn stageable(&self) -> bool {
        self.status != DiffFileStatus::Conflicted
    }
    pub fn from_diff(file: &DiffFile, revision: u64) -> Self {
        Self {
            path: file.path().clone(),
            original: file.original_path().cloned(),
            status: file.status(),
            revision,
            untracked: false,
        }
    }
}

pub struct MetadataSnapshot {
    pub root: PathBuf,
    pub branch: String,
    pub git_dirs: Vec<PathBuf>,
    pub groups: [Vec<ChangedFile>; 2],
    pub context: MetadataContext,
    /// Backend that actually supplied the listing (including fallback).
    pub backend: MetadataBackend,
    pub fallback_reason: Option<&'static str>,
}

#[derive(Clone, PartialEq, Eq)]
pub struct MetadataContext {
    root: PathBuf,
    git_dirs: Vec<PathBuf>,
    link_stamp: Option<(u64, Option<std::time::SystemTime>)>,
}
impl MetadataContext {
    pub fn git_directories(&self) -> &[PathBuf] {
        &self.git_dirs
    }
}
fn git_link_stamp(root: &Path) -> Option<(u64, Option<std::time::SystemTime>)> {
    let metadata = root.join(".git").symlink_metadata().ok()?;
    (metadata.is_file() || metadata.file_type().is_symlink())
        .then(|| (metadata.len(), metadata.modified().ok()))
}

fn hash(value: impl Hash) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

fn disk_stamp(root: &Path, name: &str) -> (u64, Option<std::time::SystemTime>) {
    root.join(name)
        .symlink_metadata()
        .map(|m| (m.len(), m.modified().ok()))
        .unwrap_or_default()
}

/// Both adapters hash the same semantic descriptor. Similarity scores and API
/// formatting aren't cache identities; object ids, modes and disk stamps are.
#[derive(Hash)]
struct TrackedState {
    old_mode: u32,
    new_mode: u32,
    old_id: String,
    new_id: String,
    code: u8,
}
fn tracked_file(
    root: &Path,
    path: &str,
    original: Option<&str>,
    state: TrackedState,
    staged: bool,
) -> ChangedFile {
    let status = match state.code {
        b'A' => DiffFileStatus::Added,
        b'D' => DiffFileStatus::Deleted,
        b'R' => DiffFileStatus::Renamed,
        b'C' => DiffFileStatus::Copied,
        b'U' => DiffFileStatus::Conflicted,
        _ => DiffFileStatus::Modified,
    };
    ChangedFile {
        path: path.to_owned().into(),
        original: original.map(|s| s.to_owned().into()),
        status,
        revision: hash((
            state,
            original,
            if staged {
                (0, None)
            } else {
                disk_stamp(root, path)
            },
        )),
        untracked: false,
    }
}
fn untracked_file(root: &Path, path: &str) -> ChangedFile {
    ChangedFile {
        path: path.to_owned().into(),
        original: None,
        status: DiffFileStatus::Added,
        revision: hash(disk_stamp(root, path)),
        untracked: true,
    }
}

fn raw_files(root: &Path, bytes: Vec<u8>, staged: bool) -> Result<Vec<ChangedFile>, String> {
    let text = text(bytes)?;
    let mut records = text.split('\0').filter(|s| !s.is_empty()).peekable();
    let mut files = std::collections::BTreeMap::new();
    while records.peek().is_some_and(|record| record.starts_with(':')) {
        let header = records.next().unwrap();
        let status = header
            .split_whitespace()
            .last()
            .ok_or("Missing Git status")?;
        let first = records.next().ok_or("Missing Git path")?;
        let (path, original) = if status.starts_with(['R', 'C']) {
            (
                records.next().ok_or("Missing Git destination")?,
                Some(first),
            )
        } else {
            (first, None)
        };
        // Working diff emits both U and M for an unmerged path. Keep the conflict.
        if files
            .get(path)
            .is_some_and(|f: &ChangedFile| !f.stageable())
        {
            continue;
        }
        let fields: Vec<_> = header.trim_start_matches(':').split_whitespace().collect();
        if fields.len() != 5 {
            return Err("Invalid Git raw descriptor".into());
        }
        let state = TrackedState {
            old_mode: u32::from_str_radix(fields[0], 8).map_err(|e| e.to_string())?,
            new_mode: u32::from_str_radix(fields[1], 8).map_err(|e| e.to_string())?,
            old_id: fields[2].into(),
            new_id: fields[3].into(),
            code: status.as_bytes()[0],
        };
        files.insert(
            path.to_owned(),
            tracked_file(root, path, original, state, staged),
        );
    }
    if !staged {
        // Raw diff reports stat-only changes when autoRefreshIndex is disabled.
        // Numstat verifies normalized content without writing the index or
        // retaining patches. A mode-only change still produces a 0/0 row.
        let mut changed = HashSet::new();
        while let Some(record) = records.next() {
            let name = record
                .splitn(3, '\t')
                .nth(2)
                .ok_or("Invalid Git numstat record")?;
            let name = if name.is_empty() {
                records.next().ok_or("Missing Git numstat source")?;
                records.next().ok_or("Missing Git numstat destination")?
            } else {
                name
            };
            changed.insert(name);
        }
        files.retain(|path, file| !file.stageable() || changed.contains(path.as_str()));
    }
    Ok(files.into_values().collect())
}

pub fn load_metadata(path: &Path) -> Result<MetadataSnapshot, String> {
    load_metadata_with_context(path, None)
}
/// Reuse repository locations while its worktree descriptor is unchanged.
pub fn load_metadata_with_context(
    path: &Path,
    known: Option<&MetadataContext>,
) -> Result<MetadataSnapshot, String> {
    load_metadata_with_backend(path, known, MetadataBackend::configured())
}

/// Request a backend explicitly, with conservative Git fallback for unsupported
/// repositories or native read errors. No native index updates are persisted.
pub fn load_metadata_with_backend(
    path: &Path,
    known: Option<&MetadataContext>,
    backend: MetadataBackend,
) -> Result<MetadataSnapshot, String> {
    if backend == MetadataBackend::Gix {
        match native::load(path) {
            Ok(snapshot) => return Ok(snapshot),
            Err(reason) => {
                // Only a fixed category is logged, never paths or source contents.
                tracing::info!(category = reason, "gix metadata falling back to Git");
                let mut snapshot = load_git_metadata(path, known)?;
                snapshot.fallback_reason = Some(reason);
                return Ok(snapshot);
            }
        }
    }
    load_git_metadata(path, known)
}

fn load_git_metadata(
    path: &Path,
    known: Option<&MetadataContext>,
) -> Result<MetadataSnapshot, String> {
    let _span = tracing::info_span!("repository_metadata").entered();
    let known = known.filter(|context| {
        context.root == path
            && context.link_stamp == git_link_stamp(path)
            && (context.link_stamp.is_some() || path.join(".git").is_dir())
    });
    let root = known.map_or_else(|| resolve_root(path), |context| Ok(context.root.clone()))?;
    let args = [
        "diff",
        "--raw",
        "-z",
        "--abbrev=40",
        "--no-ext-diff",
        "--no-textconv",
        "--find-renames",
    ];
    let (staged, changes, names, branch, dirs) = std::thread::scope(|scope| {
        let staged = scope.spawn(|| {
            let mut args = args.to_vec();
            args.extend(["--cached", "--"]);
            git(&root, &args)
        });
        let changes = scope.spawn(|| {
            let mut args = args.to_vec();
            args.extend(["--numstat", "--"]);
            git(&root, &args)
        });
        let names =
            scope.spawn(|| git(&root, &["ls-files", "-z", "--others", "--exclude-standard"]));
        let branch = scope.spawn(|| branch_name(&root));
        let dirs = scope.spawn(|| {
            known.map_or_else(
                || git_directories(&root),
                |context| Ok(context.git_dirs.clone()),
            )
        });
        (
            staged.join().unwrap(),
            changes.join().unwrap(),
            names.join().unwrap(),
            branch.join().unwrap(),
            dirs.join().unwrap(),
        )
    });
    let mut staged = raw_files(&root, staged?, true)?;
    let mut changes = raw_files(&root, changes?, false)?;
    // Unmerged index entries belong to Changes, not a commit-ready staged scope.
    staged.retain(ChangedFile::stageable);
    for name in text(names?)?.split('\0').filter(|s| !s.is_empty()) {
        changes.push(untracked_file(&root, name));
    }
    changes.sort_by(|a, b| a.path.cmp(&b.path));
    changes.dedup_by(|a, b| a.path == b.path);
    let git_dirs = dirs?;
    let context = MetadataContext {
        root: root.clone(),
        git_dirs: git_dirs.clone(),
        link_stamp: git_link_stamp(&root),
    };
    Ok(MetadataSnapshot {
        root,
        branch: branch?,
        git_dirs,
        groups: [staged, changes],
        context,
        backend: MetadataBackend::Git,
        fallback_reason: None,
    })
}

#[derive(Clone)]
pub struct Preview {
    pub file: DiffFile,
    pub fingerprint: u64,
    pub bytes: usize,
}

pub fn load_preview(
    root: &Path,
    group: ChangeGroup,
    entry: &ChangedFile,
) -> Result<Preview, String> {
    let _span = tracing::info_span!("repository_preview").entered();
    let patch = if entry.untracked {
        let path = root.join(entry.path.as_str());
        let symlink = path
            .symlink_metadata()
            .map_err(|e| e.to_string())?
            .file_type()
            .is_symlink();
        let content = if symlink {
            path.read_link()
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .as_bytes()
                .to_vec()
        } else {
            read_untracked(&path).map_err(|e| e.to_string())?
        };
        addition_patch(entry.path.as_str(), &content, symlink)
    } else if entry.status == DiffFileStatus::Conflicted {
        let content = read_untracked(&root.join(entry.path.as_str())).map_err(|e| e.to_string())?;
        let source = std::str::from_utf8(&content)
            .map_err(|_| "This conflicted file cannot be displayed as text")?;
        if source.contains('\0') {
            return Err("This conflicted file cannot be displayed as text".into());
        }
        let file = DiffFile::unchanged(entry.path.clone(), source);
        return Ok(Preview {
            bytes: file.estimated_size_bytes(),
            fingerprint: hash(source),
            file,
        });
    } else {
        let mut args = vec![
            "diff",
            "--no-color",
            "--no-ext-diff",
            "--no-textconv",
            "--find-renames",
            "--src-prefix=a/",
            "--dst-prefix=b/",
            "--unified=1000000",
        ];
        if group == ChangeGroup::Staged {
            args.push("--cached");
        }
        args.push("--");
        if let Some(old) = &entry.original {
            args.push(old.as_str());
        }
        args.push(entry.path.as_str());
        text(git(root, &args)?)?
    };
    let fingerprint = hash(&patch);
    let _span = tracing::info_span!("parse_preview", bytes = patch.len()).entered();
    let file = DiffFile::parse(&patch)
        .map_err(|e| format!("Couldn't read this diff: {e}"))?
        .into_iter()
        .find(|f| f.path() == &entry.path)
        .ok_or("This file changed while its preview was loading. Refresh to retry.")?;
    Ok(Preview {
        bytes: file.estimated_size_bytes(),
        file,
        fingerprint,
    })
}

#[derive(Clone, Hash, PartialEq, Eq)]
pub struct PreviewKey {
    pub root: PathBuf,
    pub group: ChangeGroup,
    pub path: SharedString,
    pub revision: u64,
}
pub struct PreviewCache {
    entries: LruCache<PreviewKey, Preview>,
    bytes: usize,
    budget: usize,
}
impl Default for PreviewCache {
    fn default() -> Self {
        Self::new(32, 32 * 1024 * 1024)
    }
}
impl PreviewCache {
    pub fn new(capacity: usize, budget: usize) -> Self {
        Self {
            entries: LruCache::new(NonZeroUsize::new(capacity.max(1)).unwrap()),
            bytes: 0,
            budget,
        }
    }
    pub fn get(&mut self, key: &PreviewKey) -> Option<Preview> {
        self.entries.get(key).cloned()
    }
    pub fn insert(&mut self, key: PreviewKey, preview: Preview) {
        if preview.bytes > self.budget {
            return;
        }
        if let Some(old) = self.entries.pop(&key) {
            self.bytes -= old.bytes;
        }
        while self.bytes + preview.bytes > self.budget
            || self.entries.len() == self.entries.cap().get()
        {
            if let Some((_, old)) = self.entries.pop_lru() {
                self.bytes -= old.bytes;
            } else {
                break;
            }
        }
        self.bytes += preview.bytes;
        self.entries.put(key, preview);
    }
    pub fn invalidate(&mut self, root: &Path, paths: Option<&[PathBuf]>) {
        let keys: Vec<_> = self
            .entries
            .iter()
            .filter(|(key, _)| {
                key.root == root
                    && paths.is_none_or(|paths| {
                        paths.iter().any(|p| {
                            Path::new(key.path.as_str()).starts_with(p)
                                || p.starts_with(key.path.as_str())
                        })
                    })
            })
            .map(|(key, _)| key.clone())
            .collect();
        for key in keys {
            if let Some(old) = self.entries.pop(&key) {
                self.bytes -= old.bytes;
            }
        }
    }
}
pub type SharedPreviewCache = Arc<Mutex<PreviewCache>>;

/// Apply only literal paths and never resolve an unmerged entry implicitly.
pub fn set_entries_staged(root: &Path, files: &[ChangedFile], staged: bool) -> Result<(), String> {
    use std::io::Write;
    let _span = tracing::info_span!("git_index_write").entered();
    let conflicts = text(git(root, &["ls-files", "--unmerged", "-z"])?)?;
    let conflicts: HashSet<_> = conflicts
        .split('\0')
        .filter_map(|entry| entry.split_once('\t').map(|(_, path)| path))
        .collect();
    let mut paths = BTreeSet::new();
    for file in files
        .iter()
        .filter(|f| f.stageable() && !conflicts.contains(f.path.as_str()))
    {
        paths.insert(file.path.as_str());
        if file.status == DiffFileStatus::Renamed
            && let Some(old) = &file.original
        {
            if conflicts.contains(old.as_str()) {
                paths.remove(file.path.as_str());
                continue;
            }
            paths.insert(old.as_str());
        }
    }
    if paths.is_empty() {
        return Ok(());
    }
    let args = if staged {
        vec!["add", "--all"]
    } else if git(root, &["rev-parse", "--verify", "HEAD"]).is_ok() {
        vec!["restore", "--staged"]
    } else {
        vec!["rm", "--cached", "-f"]
    };
    let mut child = git_command()?
        .args([
            "--no-pager",
            "--no-optional-locks",
            "--literal-pathspecs",
            "-C",
        ])
        .arg(root)
        .args(args)
        .args(["--pathspec-from-file=-", "--pathspec-file-nul"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let mut input = child.stdin.take().ok_or("Git stdin unavailable")?;
    let (output, written) = std::thread::scope(|scope| {
        let writer = scope.spawn(move || {
            for path in paths {
                input.write_all(path.as_bytes())?;
                input.write_all(&[0])?;
            }
            Ok::<_, std::io::Error>(())
        });
        (
            child.wait_with_output(),
            writer.join().expect("Git writer panicked"),
        )
    });
    let output = output.map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().into());
    }
    written.map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cache_is_bounded_and_invalidation_is_targeted() {
        let file = DiffFile::unchanged("a.txt", "source\n");
        let preview = Preview {
            bytes: 100,
            fingerprint: 1,
            file,
        };
        let key = |path: &str| PreviewKey {
            root: "/repo".into(),
            path: path.into(),
            group: ChangeGroup::Changes,
            revision: 1,
        };
        let mut cache = PreviewCache::new(2, 200);
        cache.insert(key("a.txt"), preview.clone());
        cache.insert(key("b.txt"), preview.clone());
        assert!(cache.get(&key("a.txt")).is_some());
        cache.insert(key("c.txt"), preview.clone());
        assert!(cache.get(&key("b.txt")).is_none());
        cache.invalidate(Path::new("/repo"), Some(&["a.txt".into()]));
        assert!(cache.get(&key("a.txt")).is_none());
        assert!(cache.get(&key("c.txt")).is_some());
        cache.insert(
            key("large.txt"),
            Preview {
                bytes: 201,
                ..preview
            },
        );
        assert!(cache.get(&key("large.txt")).is_none());
        assert_eq!(cache.bytes, 100);
        cache.invalidate(Path::new("/repo"), None);
        assert!(cache.entries.is_empty());
        assert_eq!(cache.bytes, 0);
    }
}
