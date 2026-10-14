//! Read-only rollout adapter. Keep repository handles on the refresh worker and
//! never persist status's suggested index updates.
use super::*;
use gix::{
    diff::index::Change as TreeChange,
    status::{
        Item,
        index_worktree::Item as WorktreeItem,
        plumbing::index_as_worktree::{Change, EntryStatus},
    },
};

fn utf8(path: &[u8]) -> Result<&str, &'static str> {
    std::str::from_utf8(path).map_err(|_| "non-UTF-8 paths")
}

pub(super) fn load(path: &Path) -> Result<MetadataSnapshot, &'static str> {
    let _span = tracing::info_span!("repository_metadata_gix").entered();
    // Git's inherited environment can redirect the repository, index, object
    // store or config. Delegate these uncommon contexts instead of guessing.
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
        return Err("Git environment overrides");
    }
    let options = gix::open::Options::default()
        .strict_config(true)
        .bail_if_untrusted(true)
        // `status.showUntrackedFiles=no` disables the dirwalk before the
        // platform's Files override can act. Match ls-files unconditionally.
        .config_overrides(["status.showUntrackedFiles=all"]);
    let repo = gix::ThreadSafeRepository::discover_opts(
        path,
        Default::default(),
        gix::sec::trust::Mapping {
            full: options.clone(),
            reduced: options,
        },
    )
    .map_err(|_| "native repository discovery")?
    .to_thread_local();
    let root = std::fs::canonicalize(repo.workdir().ok_or("bare repository")?)
        .map_err(|_| "worktree location")?;
    // Unsupported index states go to Git before starting the expensive scan.
    let index = repo.index_or_empty().map_err(|_| "native index read")?;
    let config = repo.config_snapshot();
    // Windows canonical paths can use extended/long names while Git's
    // conditional include matcher sees DOS aliases. The pinned gix release
    // resolves these differently; retain Git's ignore/config decisions.
    if cfg!(windows)
        && config
            .plumbing()
            .sections()
            .any(|section| section.header().name().eq_ignore_ascii_case(b"includeIf"))
    {
        return Err("conditional Git configuration on Windows");
    }
    if config.string("core.autocrlf").is_some() && config.boolean("core.autocrlf") != Some(false) {
        return Err("worktree content conversion");
    }
    if index.is_sparse() || config.boolean("core.sparseCheckout") == Some(true) {
        return Err("sparse checkout");
    }
    if index
        .entries()
        .iter()
        .any(|entry| entry.mode.is_submodule())
    {
        return Err("submodules");
    }
    if index
        .entries()
        .iter()
        .any(|entry| entry.flags.stage_raw() != 0)
    {
        return Err("unmerged index");
    }
    if index.entries().iter().any(|entry| {
        entry
            .flags
            .contains(gix::index::entry::Flags::INTENT_TO_ADD)
    }) {
        return Err("intent-to-add index");
    }
    // Experimental gitoxide-only configuration isn't interpreted by Git.
    if config.plumbing().sections().any(|section| {
        section.meta().source != gix::config::Source::EnvOverride
            && section.header().name().to_ascii_lowercase() == b"gitoxide"
    }) {
        return Err("gitoxide configuration");
    }
    let rewrites = gix::diff::Rewrites {
        // `git diff --find-renames` overrides status.renames and diff.renames,
        // including the `copies` value. Don't inherit status-only configuration.
        copies: None,
        limit: config
            .try_integer("diff.renameLimit")
            .map_err(|_| "rename configuration")?
            .map(|n| usize::try_from(n).map_err(|_| "rename configuration"))
            .transpose()?
            .unwrap_or(1000),
        track_empty: true, // Git diff's default is --rename-empty.
        ..Default::default()
    };
    let mut groups: [Vec<ChangedFile>; 2] = Default::default();
    let null = gix::hash::ObjectId::null(repo.object_hash()).to_string();
    let mut attributes = repo
        .attributes_only(
            &index,
            gix::worktree::stack::state::attributes::Source::WorktreeThenIdMapping,
        )
        .map_err(|_| "native attributes read")?;
    let mut matches = gix::attrs::search::Outcome::default();
    // Worktree rewrite tracking intentionally stays off. `git diff` only
    // considers tracked entries here; moved untracked files remain additions.
    let mut iter = repo
        .status(gix::progress::Discard)
        .map_err(|_| "native status setup")?
        .index(gix::worktree::IndexPersistedOrInMemory::Persisted(index))
        .tree_index_track_renames(gix::status::tree_index::TrackRenames::Given(rewrites))
        .untracked_files(gix::status::UntrackedFiles::Files)
        .dirwalk_options(|opts| {
            opts.empty_patterns_match_prefix(false)
                .emit_empty_directories(false)
        })
        .index_worktree_options_mut(|opts| opts.thread_limit = Some(2))
        .into_iter(Vec::<gix::bstr::BString>::new())
        .map_err(|_| "native status read")?;
    let mut rename_count = 0;
    for item in iter.by_ref() {
        let item = item.map_err(|_| "native status read")?;
        match item {
            Item::TreeIndex(change) => {
                let path = utf8(change.location())?;
                let (old_mode, new_mode, old_id, new_id, code, original) = match &change {
                    TreeChange::Addition { entry_mode, id, .. } => (
                        0,
                        entry_mode.bits(),
                        null.clone(),
                        id.to_hex().to_string(),
                        b'A',
                        None,
                    ),
                    TreeChange::Deletion { entry_mode, id, .. } => (
                        entry_mode.bits(),
                        0,
                        id.to_hex().to_string(),
                        null.clone(),
                        b'D',
                        None,
                    ),
                    TreeChange::Modification {
                        previous_entry_mode,
                        entry_mode,
                        previous_id,
                        id,
                        ..
                    } => {
                        let code = if previous_entry_mode.bits() & 0o170000
                            != entry_mode.bits() & 0o170000
                        {
                            b'T'
                        } else {
                            b'M'
                        };
                        (
                            previous_entry_mode.bits(),
                            entry_mode.bits(),
                            previous_id.to_hex().to_string(),
                            id.to_hex().to_string(),
                            code,
                            None,
                        )
                    }
                    TreeChange::Rewrite {
                        source_location,
                        source_entry_mode,
                        entry_mode,
                        source_id,
                        id,
                        copy,
                        ..
                    } => {
                        // Git and gix use different fuzzy-match/tie-breaking
                        // algorithms. Keep such rename decisions authoritative.
                        if *copy || source_id != id {
                            return Err("similarity or copy detection");
                        }
                        rename_count += 1;
                        (
                            source_entry_mode.bits(),
                            entry_mode.bits(),
                            source_id.to_hex().to_string(),
                            id.to_hex().to_string(),
                            b'R',
                            Some(utf8(source_location)?),
                        )
                    }
                };
                groups[0].push(tracked_file(
                    &root,
                    path,
                    original,
                    TrackedState {
                        old_mode,
                        new_mode,
                        old_id,
                        new_id,
                        code,
                    },
                    true,
                ));
            }
            Item::IndexWorktree(WorktreeItem::Modification {
                entry,
                rela_path,
                status,
                ..
            }) => {
                let path = utf8(&rela_path)?;
                let (new_mode, code) = match status {
                    EntryStatus::Change(Change::Removed) => (0, b'D'),
                    EntryStatus::Change(Change::Type { worktree_mode }) => {
                        if !matches!(worktree_mode.bits() & 0o170000, 0o100000 | 0o120000) {
                            return Err("special worktree file type");
                        }
                        (worktree_mode.bits(), b'T')
                    }
                    EntryStatus::Change(Change::Modification {
                        executable_bit_changed,
                        ..
                    }) => {
                        // The status iterator's FastEq compares raw disk sizes
                        // before applying clean/EOL filters. Such a comparison
                        // can falsely mark normalized content as changed.
                        attributes
                            .at_path(path, Some(entry.mode))
                            .map_err(|_| "native attributes read")?
                            .matching_attributes(&mut matches);
                        if matches.iter().any(|m| {
                            matches!(
                                m.assignment.name.as_str(),
                                "text"
                                    | "eol"
                                    | "crlf"
                                    | "ident"
                                    | "filter"
                                    | "working-tree-encoding"
                            )
                        }) {
                            return Err("worktree content conversion");
                        }
                        (
                            if executable_bit_changed {
                                entry.mode.bits() ^ 0o111
                            } else {
                                entry.mode.bits()
                            },
                            b'M',
                        )
                    }
                    EntryStatus::NeedsUpdate(_) => continue,
                    _ => return Err("unsupported worktree status"),
                };
                groups[1].push(tracked_file(
                    &root,
                    path,
                    None,
                    TrackedState {
                        old_mode: entry.mode.bits(),
                        new_mode,
                        old_id: entry.id.to_hex().to_string(),
                        new_id: null.clone(),
                        code,
                    },
                    false,
                ));
            }
            Item::IndexWorktree(WorktreeItem::DirectoryContents { entry, .. }) => {
                if entry.status == gix::dir::entry::Status::Untracked {
                    let path = utf8(&entry.rela_path)?;
                    // `ls-files --others` expands embedded repositories; the
                    // status dirwalk represents them as one directory instead.
                    if root.join(path).is_dir() {
                        return Err("embedded repository");
                    }
                    groups[1].push(untracked_file(&root, path));
                }
            }
            Item::IndexWorktree(WorktreeItem::Rewrite { .. }) => return Err("worktree rewrite"),
        }
    }
    // Residual add/delete pairs may be fuzzy renames missed by gix. A rewrite
    // alongside other candidates can also have a different Git tie-break.
    let added = groups[0].iter().any(|f| f.status == DiffFileStatus::Added);
    let deleted = groups[0]
        .iter()
        .any(|f| f.status == DiffFileStatus::Deleted);
    if (added && deleted) || rename_count > 1 || (rename_count > 0 && (added || deleted)) {
        return Err("ambiguous rename candidates");
    }
    for files in &mut groups {
        files.sort_by(|a, b| a.path.cmp(&b.path));
    }
    let mut git_dirs = Vec::new();
    for dir in [repo.git_dir(), repo.common_dir()] {
        let dir = std::fs::canonicalize(dir).map_err(|_| "Git directory location")?;
        if !git_dirs.contains(&dir) {
            git_dirs.push(dir);
        }
    }
    // Retain Git's exact label rules for ambiguous refs and configured detached
    // abbreviations. This one small read avoids subtly different UI labels.
    let branch = branch_name(&root).map_err(|_| "branch label")?;
    let context = MetadataContext {
        root: root.clone(),
        git_dirs: git_dirs.clone(),
        link_stamp: git_link_stamp(&root),
    };
    // Never call iter.outcome_mut().write_changes(). Read-only status may
    // suggest stat refreshes; they are deliberately discarded.
    Ok(MetadataSnapshot {
        root,
        branch,
        git_dirs,
        groups,
        context,
        backend: MetadataBackend::Gix,
        fallback_reason: None,
    })
}
