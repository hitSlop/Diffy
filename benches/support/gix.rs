use std::{collections::BTreeSet, path::Path};

/// Compare scope membership without persisting gix's optional index updates.
pub fn scope_paths(root: &Path) -> Result<[BTreeSet<String>; 2], String> {
    let repo = gix::discover(root).map_err(|e| e.to_string())?;
    let _branch = repo.head_name().map_err(|e| e.to_string())?;
    let mut paths: [BTreeSet<String>; 2] = Default::default();
    let mut iter = repo
        .status(gix::progress::Discard)
        .map_err(|e| e.to_string())?
        .untracked_files(gix::status::UntrackedFiles::Files)
        .into_iter(Vec::<gix::bstr::BString>::new())
        .map_err(|e| e.to_string())?;
    for item in iter.by_ref() {
        let item = item.map_err(|e| e.to_string())?;
        let group = match &item {
            gix::status::Item::TreeIndex(_) => 0,
            gix::status::Item::IndexWorktree(_) => 1,
        };
        let bytes: &[u8] = item.location().as_ref();
        paths[group].insert(String::from_utf8_lossy(bytes).into_owned());
    }
    // Deliberately do not call Outcome::write_changes().
    Ok(paths)
}
