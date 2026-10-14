//! Read-only topology, independent of the status backend rollout.
use super::{MetadataBackend, git, text};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorktreeInfo {
    pub path: PathBuf,
    pub branch: String,
    pub main: bool,
    pub available: bool,
    pub locked: bool,
}

#[derive(Clone, Debug)]
pub struct WorktreeListing {
    pub common_dir: PathBuf,
    pub name: String,
    pub worktrees: Vec<WorktreeInfo>,
    pub backend: MetadataBackend,
}

fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_owned())
}

pub fn common_directory(path: &Path) -> Result<PathBuf, String> {
    let bytes = git(
        path,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?;
    Ok(canonical(Path::new(text(bytes)?.trim_end_matches('\n'))))
}

pub fn list_worktrees(path: &Path) -> Result<WorktreeListing, String> {
    list_worktrees_with_backend(path, MetadataBackend::Gix)
}

pub fn list_worktrees_with_backend(
    path: &Path,
    backend: MetadataBackend,
) -> Result<WorktreeListing, String> {
    if backend == MetadataBackend::Gix
        && let Ok(listing) = native(path)
    {
        return Ok(listing);
    }
    let common_dir = common_directory(path)?;
    let records = git(path, &["worktree", "list", "--porcelain", "-z"])?;
    let worktrees = parse_porcelain(&records)?;
    Ok(listing(common_dir, worktrees, MetadataBackend::Git))
}

fn listing(
    common_dir: PathBuf,
    worktrees: Vec<WorktreeInfo>,
    backend: MetadataBackend,
) -> WorktreeListing {
    let name = worktrees
        .iter()
        .find(|w| w.main)
        .map(|w| w.path.as_path())
        .unwrap_or(&common_dir)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    WorktreeListing {
        common_dir,
        name,
        worktrees,
        backend,
    }
}

fn native(path: &Path) -> Result<WorktreeListing, String> {
    // Match the existing adapter's conservative handling of inherited overrides.
    if std::env::vars_os().any(|(key, _)| {
        let key = key.to_string_lossy();
        key.starts_with("GIT_CONFIG")
            || matches!(
                key.as_ref(),
                "GIT_DIR"
                    | "GIT_COMMON_DIR"
                    | "GIT_WORK_TREE"
                    | "GIT_INDEX_FILE"
                    | "GIT_OBJECT_DIRECTORY"
                    | "GIT_ALTERNATE_OBJECT_DIRECTORIES"
                    | "GIT_NAMESPACE"
                    | "GIT_CEILING_DIRECTORIES"
                    | "GIT_DISCOVERY_ACROSS_FILESYSTEM"
                    | "GIT_NO_REPLACE_OBJECTS"
                    | "GIT_REPLACE_REF_BASE"
                    | "GIT_ATTR_NOSYSTEM"
                    | "GIT_ATTR_SOURCE"
            )
    }) {
        return Err("Git environment overrides".into());
    }
    let options = gix::open::Options::default()
        .strict_config(true)
        .bail_if_untrusted(true);
    let repo = gix::ThreadSafeRepository::discover_opts(
        path,
        gix::discover::upwards::Options::default(),
        gix::sec::trust::Mapping {
            full: options.clone(),
            reduced: options,
        },
    )
    .map_err(|e| e.to_string())?
    .to_thread_local();
    let common_dir = canonical(repo.common_dir());
    let mut worktrees = Vec::new();
    let main = repo.main_repo().map_err(|e| e.to_string())?;
    if let Some(path) = main.workdir() {
        worktrees.push(info(&main, path, true, false));
    }
    for proxy in repo.worktrees().map_err(|e| e.to_string())? {
        let path = proxy.base().map_err(|e| e.to_string())?;
        let locked = proxy.is_locked();
        let linked = proxy
            .into_repo_with_possibly_inaccessible_worktree()
            .map_err(|e| e.to_string())?;
        worktrees.push(info(&linked, &path, false, locked));
    }
    Ok(listing(common_dir, worktrees, MetadataBackend::Gix))
}

fn info(repo: &gix::Repository, path: &Path, main: bool, locked: bool) -> WorktreeInfo {
    let branch = repo
        .head_name()
        .ok()
        .flatten()
        .map(|name| name.shorten().to_string())
        .or_else(|| {
            repo.head_id()
                .ok()
                .map(|id| format!("{} (detached)", &id.to_string()[..7]))
        })
        .unwrap_or_else(|| "Unavailable".into());
    WorktreeInfo {
        path: canonical(path),
        branch,
        main,
        available: path.is_dir() && path.join(".git").exists(),
        locked,
    }
}

fn parse_porcelain(bytes: &[u8]) -> Result<Vec<WorktreeInfo>, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "Worktree paths must be UTF-8")?;
    let mut result = Vec::new();
    let mut current: Option<WorktreeInfo> = None;
    let mut bare = false;
    let mut ordinal = 0;
    for field in text.split('\0') {
        if field.is_empty() {
            if let Some(info) = current.take()
                && !bare
            {
                result.push(info);
            }
            bare = false;
        } else if let Some(path) = field.strip_prefix("worktree ") {
            current = Some(WorktreeInfo {
                path: canonical(Path::new(path)),
                branch: String::new(),
                main: ordinal == 0,
                available: Path::new(path).is_dir() && Path::new(path).join(".git").exists(),
                locked: false,
            });
            ordinal += 1;
        } else if let Some(info) = &mut current {
            if let Some(branch) = field.strip_prefix("branch refs/heads/") {
                info.branch = branch.into();
            } else if let Some(head) = field.strip_prefix("HEAD ") {
                info.branch = format!("{} (detached)", &head[..head.len().min(7)]);
            } else if field == "bare" {
                bare = true;
            } else if field == "locked" || field.starts_with("locked ") {
                info.locked = true;
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
#[path = "../../benches/support/fixture.rs"]
mod fixture;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn main_linked_locked_missing_and_literal_paths_match_git() {
        let f = fixture::Fixture::new("worktrees");
        f.git(&["commit", "--allow-empty", "-m", "initial"]);
        let linked = f.0.join(if cfg!(windows) {
            "checkout with quote'ü"
        } else {
            "checkout with\nquote'ü"
        });
        f.git(&["worktree", "add", "-b", "topic", linked.to_str().unwrap()]);
        f.git(&[
            "worktree",
            "lock",
            "--reason",
            "external drive",
            linked.to_str().unwrap(),
        ]);
        let git = list_worktrees_with_backend(&linked, MetadataBackend::Git).unwrap();
        let native = list_worktrees_with_backend(&linked, MetadataBackend::Gix).unwrap();
        assert_eq!(
            native.backend,
            MetadataBackend::Gix,
            "{:?}",
            super::native(&linked)
        );
        assert_eq!(git.common_dir, native.common_dir);
        assert_eq!(git.worktrees, native.worktrees);
        assert_eq!(native.worktrees.len(), 2);
        assert!(native.worktrees[1].locked && native.worktrees[1].available);
        let index = std::fs::read(f.0.join(".git/index")).ok();
        std::fs::remove_dir_all(&linked).unwrap();
        let git = list_worktrees_with_backend(&f.0, MetadataBackend::Git).unwrap();
        let native = list_worktrees(&f.0).unwrap();
        assert_eq!(git.worktrees, native.worktrees);
        assert!(!native.worktrees[1].available);
        assert_eq!(index, std::fs::read(f.0.join(".git/index")).ok());
    }

    #[test]
    fn bare_owner_is_not_a_review_tab() {
        let f = fixture::Fixture::new("bare-worktree");
        f.git(&["commit", "--allow-empty", "-m", "initial"]);
        f.git(&["config", "core.bare", "true"]);
        let linked = f.0.join("linked");
        f.git(&["worktree", "add", "-b", "linked", linked.to_str().unwrap()]);
        let listing = list_worktrees(&linked).unwrap();
        assert_eq!(listing.worktrees.len(), 1);
        assert!(!listing.worktrees[0].main);
    }
}
