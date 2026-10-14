use crate::repository;
use ignore::WalkBuilder;
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::borrow::Cow;
use std::collections::{BTreeSet, HashSet};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone, Default)]
pub struct RefreshHint {
    pub full: bool,
    pub paths: BTreeSet<PathBuf>,
}
impl RefreshHint {
    pub fn merge(&mut self, other: Self) {
        self.full |= other.full;
        if self.full {
            self.paths.clear();
        } else {
            self.paths.extend(other.paths);
            if self.paths.len() > 4096 {
                self.full = true;
                self.paths.clear();
            }
        }
    }
}

/// Topology has its own signal so checkout creation/removal doesn't force a
/// status read for every event in the common object database.
pub fn start_topology(
    common: &Path,
) -> Result<(RecommendedWatcher, async_channel::Receiver<()>), String> {
    let (tx, rx) = async_channel::bounded(1);
    let poll_tx = tx.clone();
    let root = common.to_owned();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        let relevant = event.map_or(true, |event| {
            !matches!(event.kind, EventKind::Access(_))
                && (event.need_rescan()
                    || event.paths.is_empty()
                    || event.paths.iter().any(|p| topology_event(&root, p)))
        });
        if relevant {
            let _ = tx.try_send(());
        }
    })
    .map_err(|e| e.to_string())?;
    watcher
        .watch(common, RecursiveMode::Recursive)
        .map_err(|e| e.to_string())?;
    // FSEvents can coalesce or omit .git directory registration changes. This
    // backstop only stamps the small registry, never the object database or
    // source files, and stops when the repository project drops its receiver.
    let registry = common.join("worktrees");
    let mut previous = topology_stamp(&registry);
    std::thread::Builder::new()
        .name("diffy-worktrees".into())
        .spawn(move || {
            while !poll_tx.is_closed() {
                std::thread::sleep(Duration::from_secs(1));
                let current = topology_stamp(&registry);
                if current != previous {
                    previous = current;
                    let _ = poll_tx.try_send(());
                }
            }
        })
        .map_err(|e| e.to_string())?;
    Ok((watcher, rx))
}

fn topology_stamp(registry: &Path) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    let mut entries = std::fs::read_dir(registry)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    entries.sort();
    for entry in entries {
        entry.hash(&mut hash);
        for file in ["gitdir", "commondir", "HEAD", "locked"] {
            let metadata = std::fs::metadata(entry.join(file)).ok();
            metadata
                .as_ref()
                .map(|m| (m.len(), m.modified().ok()))
                .hash(&mut hash);
        }
    }
    hash.finish()
}

fn topology_event(common: &Path, path: &Path) -> bool {
    // FSEvents can report /var while discovery returned /private/var. Deletion
    // events need the nearest existing ancestor because the leaf no longer exists.
    let aligned = if path.starts_with(common) {
        path.to_owned()
    } else {
        path.ancestors()
            .find_map(|ancestor| {
                std::fs::canonicalize(ancestor).ok().and_then(|base| {
                    path.strip_prefix(ancestor)
                        .ok()
                        .map(|suffix| base.join(suffix))
                })
            })
            .unwrap_or_else(|| path.to_owned())
    };
    let Ok(relative) = aligned.strip_prefix(common) else {
        return false;
    };
    let parts = relative
        .components()
        .map(|p| p.as_os_str().to_string_lossy())
        .collect::<Vec<_>>();
    (parts.first().is_some_and(|p| p == "worktrees")
        && (parts.len() <= 2
            || (parts.len() == 3
                && matches!(
                    parts[2].as_ref(),
                    "HEAD" | "gitdir" | "commondir" | "locked"
                ))))
        || matches!(relative.to_str(), Some("config" | "HEAD"))
}

pub fn start(
    root: &Path,
    git_dirs: &[PathBuf],
    state_path: Option<PathBuf>,
) -> Result<(RecommendedWatcher, async_channel::Receiver<RefreshHint>), String> {
    let (tx, rx) = async_channel::bounded(1);
    let (wake, receive) = std::sync::mpsc::sync_channel(1);
    let pending = Arc::new(Mutex::new(PendingEvents::default()));
    let events = pending.clone();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if event
            .as_ref()
            .is_ok_and(|e| matches!(e.kind, EventKind::Access(_)) && !e.need_rescan())
        {
            return;
        }
        events.lock().unwrap_or_else(|e| e.into_inner()).add(event);
        let _ = wake.try_send(());
    })
    .map_err(|e| format!("Live refresh unavailable: {e}"))?;
    watcher
        .watch(root, RecursiveMode::Recursive)
        .map_err(|e| format!("Couldn't watch repository: {e}"))?;
    for dir in git_dirs {
        if !dir.starts_with(root) {
            watcher
                .watch(dir, RecursiveMode::Recursive)
                .map_err(|e| format!("Couldn't watch Git metadata: {e}"))?;
        }
    }
    let mut filter = WatchFilter::open(root, git_dirs, state_path.as_deref());
    let queued = rx.clone();
    std::thread::Builder::new()
        .name("diffy-watch".into())
        .spawn(move || {
            while receive.recv().is_ok() {
                std::thread::sleep(Duration::from_millis(150));
                while receive.try_recv().is_ok() {}
                let batch = std::mem::take(&mut *pending.lock().unwrap_or_else(|e| e.into_inner()));
                if batch.rescan {
                    filter.reload();
                }
                let paths = batch.paths.into_iter().collect::<Vec<_>>();
                if batch.rescan || filter.wants_refresh(&paths) {
                    let mut hint = RefreshHint {
                        full: batch.rescan,
                        ..Default::default()
                    };
                    for path in &paths {
                        let path = filter.align(path);
                        if filter.git_rel(&path).is_some()
                            || path.file_name().is_some_and(|n| n == ".gitignore")
                        {
                            hint.full = true;
                        } else if let Some(rel) = filter.worktree_rel(&path) {
                            hint.paths.insert(rel.to_owned());
                        } else {
                            hint.full = true;
                        }
                    }
                    if hint.full {
                        hint.paths.clear();
                    }
                    if let Err(error) = tx.try_send(hint) {
                        if tx.is_closed() {
                            break;
                        }
                        let mut hint = error.into_inner();
                        if let Ok(old) = queued.try_recv() {
                            hint.merge(old);
                        }
                        let _ = tx.try_send(hint);
                    }
                }
            }
        })
        .map_err(|e| format!("Couldn't start live refresh: {e}"))?;
    Ok((watcher, rx))
}

#[derive(Default)]
struct PendingEvents {
    paths: BTreeSet<PathBuf>,
    rescan: bool,
}
impl PendingEvents {
    fn add(&mut self, event: notify::Result<notify::Event>) {
        let Ok(event) = event else {
            self.rescan = true;
            return;
        };
        if event.need_rescan() || event.paths.is_empty() {
            self.rescan = true;
        }
        if !self.rescan {
            self.paths.extend(event.paths);
            if self.paths.len() > 4096 {
                self.rescan = true;
                self.paths.clear();
            }
        }
    }
}

enum Class {
    Noise,
    Ignored,
    Refresh,
    /// Ignore rules changed, so the matcher itself has to be rebuilt.
    Rules,
    /// The index changed, so the set of force-added ignored files may have too.
    Index,
}

struct WatchFilter {
    root: PathBuf,
    /// The repository root and its canonical path, when those differ (`/var` and `/private/var`).
    roots: Vec<PathBuf>,
    git_dirs: Vec<PathBuf>,
    state_files: Vec<PathBuf>,
    ignore: Option<ignore::IncrementalIgnore>,
    tracked_ignored: HashSet<PathBuf>,
}

impl WatchFilter {
    fn open(root: &Path, git_dirs: &[PathBuf], state_path: Option<&Path>) -> Self {
        let mut dirs = Vec::new();
        for dir in git_dirs {
            for alias in aliases(dir) {
                push_unique(&mut dirs, alias);
            }
        }
        for alias in aliases(&root.join(".git")) {
            if alias.is_dir() {
                push_unique(&mut dirs, alias);
            }
        }
        let mut filter = Self {
            root: root.to_path_buf(),
            roots: aliases(root),
            git_dirs: dirs,
            state_files: state_path.map(aliases).unwrap_or_default(),
            ignore: None,
            tracked_ignored: HashSet::new(),
        };
        filter.reload();
        filter
    }

    fn wants_refresh(&mut self, paths: &[PathBuf]) -> bool {
        if paths.is_empty() {
            return true;
        }
        let mut refresh = false;
        let mut rules = false;
        let mut tracked = false;
        let mut candidates = Vec::new();
        for path in paths {
            let path = self.align(path);
            if self.boring(path.as_ref()) {
                continue;
            }
            match self.classify(path.as_ref()) {
                Class::Noise => {}
                Class::Ignored => {
                    if let Some(rel) = self.worktree_rel(path.as_ref()) {
                        candidates.push(rel.to_path_buf());
                    } else {
                        refresh = true;
                    }
                }
                Class::Refresh => refresh = true,
                Class::Rules => {
                    refresh = true;
                    rules = true;
                }
                Class::Index => {
                    refresh = true;
                    tracked = true;
                }
            }
        }
        if rules {
            self.reload();
        } else if tracked {
            self.reload_tracked();
        }
        // A cheap matcher only proposes exclusions. Git confirms them in one batch;
        // matcher errors or stale rules must never hide a real change.
        if !refresh && !candidates.is_empty() {
            refresh = repository::ignored_paths(&self.root, &candidates).map_or(true, |ignored| {
                candidates.iter().any(|p| !ignored.contains(p))
            });
        }
        refresh
    }

    fn align<'a>(&self, path: &'a Path) -> Cow<'a, Path> {
        if self.roots.iter().any(|root| path.starts_with(root))
            || self.git_dirs.iter().any(|dir| path.starts_with(dir))
            || self.state_files.iter().any(|file| path == file)
        {
            return Cow::Borrowed(path);
        }
        std::fs::canonicalize(path)
            .map(Cow::Owned)
            .unwrap_or(Cow::Borrowed(path))
    }

    fn boring(&self, path: &Path) -> bool {
        let lock = self.git_rel(path).is_some()
            && path.file_name().is_some_and(|name| name == "index.lock");
        lock || self.state_files.iter().any(|file| {
            path == file
                || (path.parent() == file.parent()
                    && path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with(".state-") && n.ends_with(".tmp")))
        })
    }

    fn classify(&mut self, path: &Path) -> Class {
        if let Some(rel) = self.git_rel(path) {
            return classify_git(rel);
        }
        if path.file_name().is_some_and(|name| name == ".gitignore") {
            return Class::Rules;
        }
        if self.ignored_worktree(path) {
            Class::Ignored
        } else {
            Class::Refresh
        }
    }

    /// Longest matching git directory wins, so a linked worktree's `HEAD`
    /// is not mistaken for the common directory's `worktrees/` noise.
    fn git_rel<'a>(&self, path: &'a Path) -> Option<&'a Path> {
        self.git_dirs
            .iter()
            .filter_map(|dir| path.strip_prefix(dir).ok())
            .min_by_key(|rel| rel.as_os_str().len())
    }

    fn worktree_rel<'a>(&self, path: &'a Path) -> Option<&'a Path> {
        self.roots.iter().find_map(|root| {
            let rel = path.strip_prefix(root).ok()?;
            if rel.as_os_str().is_empty() {
                None
            } else {
                Some(rel)
            }
        })
    }

    fn ignored_worktree(&mut self, path: &Path) -> bool {
        let Some(rel) = self.worktree_rel(path) else {
            return false;
        };
        if self.tracked_ignored.contains(rel) {
            return false;
        }
        let is_dir = path.symlink_metadata().is_ok_and(|meta| meta.is_dir());
        self.ignore
            .as_mut()
            .is_some_and(|ignore| ignore.matched(rel, is_dir).is_ignore())
    }

    fn reload(&mut self) {
        self.ignore = ignore_matcher(&self.root);
        self.reload_tracked();
    }

    fn reload_tracked(&mut self) {
        match repository::tracked_ignored_paths(&self.root) {
            Ok(mut paths) => {
                let parents = paths
                    .iter()
                    .flat_map(|path| {
                        path.ancestors()
                            .skip(1)
                            .filter(|p| !p.as_os_str().is_empty())
                            .map(Path::to_path_buf)
                    })
                    .collect::<Vec<_>>();
                paths.extend(parents);
                self.tracked_ignored = paths;
            }
            // Without this list a force-added file looks ignored, so stop filtering.
            Err(_) => {
                self.ignore = None;
                self.tracked_ignored.clear();
            }
        }
    }
}

fn classify_git(rel: &Path) -> Class {
    let Some(name) = rel.components().find_map(|component| match component {
        Component::Normal(name) => name.to_str(),
        _ => None,
    }) else {
        return Class::Refresh;
    };
    match name {
        // High-volume metadata. A commit or fetch also updates `HEAD`, `refs`, or the index.
        "objects" | "logs" | "hooks" | "lfs" | "worktrees" | "modules" => Class::Noise,
        "FETCH_HEAD" | "ORIG_HEAD" | "COMMIT_EDITMSG" | "description" | "MERGE_HEAD"
        | "CHERRY_PICK_HEAD" | "REVERT_HEAD" | "REBASE_HEAD" | "AUTO_MERGE" | "MERGE_MSG"
        | "SQUASH_MSG" => Class::Noise,
        "index" => Class::Index,
        "config" => Class::Rules,
        "info" if rel.ends_with("exclude") => Class::Rules,
        _ => Class::Refresh,
    }
}

fn ignore_matcher(root: &Path) -> Option<ignore::IncrementalIgnore> {
    let mut builder = WalkBuilder::new(root);
    builder
        .hidden(false)
        .ignore(false)
        .parents(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .require_git(true)
        .ignore_case_insensitive(git_ignore_case(root));
    builder.build_matchers().pop()
}

fn git_ignore_case(root: &Path) -> bool {
    repository::git(
        root,
        &[
            "config",
            "--get",
            "--default",
            "false",
            "--bool",
            "core.ignorecase",
        ],
    )
    .ok()
    .is_some_and(|bytes| bytes.trim_ascii() == b"true")
}

fn aliases(path: &Path) -> Vec<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|dir| dir.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    };
    let mut found = vec![absolute.clone()];
    if let Ok(canonical) = std::fs::canonicalize(&absolute)
        && canonical != absolute
    {
        found.push(canonical);
    }
    found
}

fn push_unique(dirs: &mut Vec<PathBuf>, path: PathBuf) {
    if !dirs.contains(&path) {
        dirs.push(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Fixture(PathBuf);

    #[test]
    fn topology_only_signals_registrations_paths_locks_and_heads() {
        let root = Path::new("/repo/.git");
        for path in [
            "worktrees",
            "worktrees/new",
            "worktrees/new/gitdir",
            "worktrees/new/locked",
            "worktrees/new/HEAD",
            "HEAD",
            "config",
        ] {
            assert!(topology_event(root, &root.join(path)), "{path}");
        }
        for path in [
            "objects/ab/cd",
            "logs/HEAD",
            "worktrees/new/logs/HEAD",
            "worktrees/new/index",
            "worktrees/new/index.lock",
        ] {
            assert!(!topology_event(root, &root.join(path)), "{path}");
        }
    }

    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let n = NEXT.fetch_add(1, Ordering::Relaxed);
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir()
                .join(format!("diffy-watch-{}-{nanos}-{n}", std::process::id()));
            std::fs::create_dir(&path).unwrap();
            repository::git(&path, &["init", "--initial-branch=main"]).unwrap();
            Self(path)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn git_ignores(repo: &Path, rel: &str) -> bool {
        // `repository::git` forces `--literal-pathspecs`, which `check-ignore` rejects.
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(["check-ignore", "-q", "--", rel])
            .output()
            .unwrap();
        match output.status.code() {
            Some(0) => true,
            Some(1) => false,
            _ => panic!(
                "check-ignore {rel}: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        }
    }

    fn assert_same(filter: &mut WatchFilter, repo: &Path, rel: &str) {
        let refresh = filter.wants_refresh(&[repo.join(rel)]);
        assert_eq!(refresh, !git_ignores(repo, rel), "{rel}");
    }

    #[test]
    fn ignored_paths_match_git_and_git_metadata_does_not_refresh() {
        let repo = Fixture::new();
        std::fs::create_dir_all(repo.0.join("target")).unwrap();
        std::fs::write(repo.0.join(".gitignore"), "target/\n").unwrap();
        std::fs::write(repo.0.join(".ignore"), "secret.txt\n").unwrap();
        std::fs::write(repo.0.join("target/keep.rs"), "kept\n").unwrap();
        std::fs::write(repo.0.join("target/other.rs"), "skip\n").unwrap();
        repository::git(&repo.0, &["add", "-f", "--", "target/keep.rs"]).unwrap();
        let mut filter = WatchFilter::open(&repo.0, &[repo.0.join(".git")], None);
        assert_same(&mut filter, &repo.0, "target/keep.rs");
        assert_same(&mut filter, &repo.0, "target/other.rs");
        assert_same(&mut filter, &repo.0, "target/debug/deps/lib.o");
        assert_same(&mut filter, &repo.0, "src/main.rs");
        assert_same(&mut filter, &repo.0, ".github/workflows/ci.yml");
        assert_same(&mut filter, &repo.0, "secret.txt");
        assert!(!filter.wants_refresh(&[
            repo.0.join("target/other.rs"),
            repo.0.join(".git/objects/ab/cd"),
        ]));
        assert!(
            filter.wants_refresh(&[repo.0.join("target/other.rs"), repo.0.join("src/main.rs"),])
        );
        assert!(filter.wants_refresh(&[]));
        assert!(!filter.wants_refresh(&[repo.0.join(".git/objects/ab/cd")]));
        assert!(!filter.wants_refresh(&[repo.0.join(".git/logs/HEAD")]));
        assert!(!filter.wants_refresh(&[repo.0.join(".git/FETCH_HEAD")]));
        assert!(!filter.wants_refresh(&[repo.0.join(".git/index.lock")]));
        assert!(filter.wants_refresh(&[repo.0.join(".git/HEAD")]));
        assert!(filter.wants_refresh(&[repo.0.join(".git/refs/heads/main")]));
        assert!(filter.wants_refresh(&[repo.0.join(".git/index")]));
    }

    #[test]
    fn negation_and_rewritten_rules_follow_git() {
        let repo = Fixture::new();
        std::fs::create_dir_all(repo.0.join("target")).unwrap();
        std::fs::write(repo.0.join(".gitignore"), "target/*\n!target/keep.rs\n").unwrap();
        std::fs::write(repo.0.join("target/keep.rs"), "kept\n").unwrap();
        std::fs::write(repo.0.join("target/other.rs"), "skip\n").unwrap();
        let mut filter = WatchFilter::open(&repo.0, &[repo.0.join(".git")], None);
        assert_same(&mut filter, &repo.0, "target/keep.rs");
        assert_same(&mut filter, &repo.0, "target/other.rs");
        std::fs::write(repo.0.join(".gitignore"), "generated.c\n").unwrap();
        assert!(filter.wants_refresh(&[repo.0.join(".gitignore")]));
        assert_same(&mut filter, &repo.0, "generated.c");
        assert_same(&mut filter, &repo.0, "target/other.rs");
        std::fs::write(repo.0.join(".git/info/exclude"), "local.txt\n").unwrap();
        assert!(filter.wants_refresh(&[repo.0.join(".git/info/exclude")]));
        assert_same(&mut filter, &repo.0, "local.txt");
    }

    #[test]
    fn index_updates_the_force_added_set() {
        let repo = Fixture::new();
        std::fs::create_dir_all(repo.0.join("target")).unwrap();
        std::fs::write(repo.0.join(".gitignore"), "target/\n").unwrap();
        std::fs::write(repo.0.join("target/second.rs"), "second\n").unwrap();
        let mut filter = WatchFilter::open(&repo.0, &[repo.0.join(".git")], None);
        assert!(!filter.wants_refresh(&[repo.0.join("target/second.rs")]));
        repository::git(&repo.0, &["add", "-f", "--", "target/second.rs"]).unwrap();
        assert!(filter.wants_refresh(&[repo.0.join(".git/index")]));
        assert!(filter.wants_refresh(&[repo.0.join("target/second.rs")]));
    }

    #[test]
    fn linked_worktree_head_is_not_dropped_as_common_dir_noise() {
        let repo = Fixture::new();
        let worktree_git = repo.0.join(".git/worktrees/name");
        std::fs::create_dir_all(&worktree_git).unwrap();
        std::fs::write(worktree_git.join("HEAD"), "ref: refs/heads/main\n").unwrap();
        let mut filter =
            WatchFilter::open(&repo.0, &[repo.0.join(".git"), worktree_git.clone()], None);
        assert!(filter.wants_refresh(&[worktree_git.join("HEAD")]));
        assert!(!filter.wants_refresh(&[repo.0.join(".git/worktrees/name/objects/ab/cd")]));
    }

    #[test]
    fn preference_file_does_not_refresh() {
        let repo = Fixture::new();
        let prefs = repo.0.join("preferences/state.json");
        let mut filter = WatchFilter::open(&repo.0, &[repo.0.join(".git")], Some(&prefs));
        assert!(!filter.wants_refresh(&[prefs]));
        assert!(filter.wants_refresh(&[repo.0.join("src/main.rs")]));
    }
    #[test]
    fn ignored_directory_with_tracked_children_still_refreshes() {
        let repo = Fixture::new();
        std::fs::create_dir(repo.0.join("generated")).unwrap();
        std::fs::write(repo.0.join(".gitignore"), "generated/\n").unwrap();
        std::fs::write(repo.0.join("generated/keep.rs"), "tracked\n").unwrap();
        repository::git(&repo.0, &["add", "-f", "generated/keep.rs"]).unwrap();
        let mut filter = WatchFilter::open(&repo.0, &[repo.0.join(".git")], None);
        assert!(filter.wants_refresh(&[repo.0.join("generated")]));
        assert!(!filter.wants_refresh(&[repo.0.join("generated/untracked.rs")]));
    }

    #[test]
    fn stale_excludes_matcher_cannot_override_git() {
        let repo = Fixture::new();
        let stale = repo.0.join(".git/stale-global-ignore");
        let effective = repo.0.join(".git/effective-ignore");
        std::fs::write(&stale, "generated.rs\n").unwrap();
        std::fs::write(&effective, "").unwrap();
        repository::git(
            &repo.0,
            &["config", "core.excludesFile", effective.to_str().unwrap()],
        )
        .unwrap();
        let mut filter = WatchFilter::open(&repo.0, &[repo.0.join(".git")], None);
        // Simulate the library loading a global rule despite a repository override,
        // without mutating process-wide environment in parallel tests.
        let mut builder = WalkBuilder::new(&repo.0);
        assert!(builder.add_ignore(&stale).is_none());
        filter.ignore = builder.build_matchers().pop();
        assert!(filter.ignored_worktree(&repo.0.join("generated.rs")));
        assert_same(&mut filter, &repo.0, "generated.rs");
    }

    #[test]
    fn root_preferences_only_exclude_our_own_writes() {
        let repo = Fixture::new();
        let prefs = repo.0.join("state.json");
        let mut filter = WatchFilter::open(&repo.0, &[repo.0.join(".git")], Some(&prefs));
        assert!(!filter.wants_refresh(&[prefs]));
        assert!(!filter.wants_refresh(&[repo.0.join(".state-123-456.tmp")]));
        assert!(filter.wants_refresh(&[repo.0.join("src/main.rs")]));
        assert!(filter.wants_refresh(&[repo.0.join("src/index.lock")]));
    }

    #[test]
    fn rescans_and_overflow_keep_a_refresh_pending() {
        let mut pending = PendingEvents::default();
        let mut event = notify::Event::new(EventKind::Other);
        event.attrs.set_flag(notify::event::Flag::Rescan);
        event.paths.push(PathBuf::from("ignored"));
        pending.add(Ok(event));
        assert!(pending.rescan);
        let mut pending = PendingEvents::default();
        let mut event = notify::Event::new(EventKind::Other);
        event.paths = (0..5000)
            .map(|ix| PathBuf::from(format!("ignored/{ix}")))
            .collect();
        pending.add(Ok(event));
        assert!(pending.rescan);
        assert!(pending.paths.is_empty());
    }
}
