use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};

pub fn start(
    root: &Path,
    git_dirs: &[PathBuf],
    state_path: Option<PathBuf>,
) -> Result<(RecommendedWatcher, async_channel::Receiver<()>), String> {
    let (tx, rx) = async_channel::bounded(1);
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if let Ok(event) = &event {
            if matches!(event.kind, EventKind::Access(_)) {
                return;
            }
            if !event.paths.is_empty()
                && event.paths.iter().all(|p| {
                    state_path
                        .as_ref()
                        .and_then(|p| p.parent())
                        .is_some_and(|dir| p.starts_with(dir))
                        || p.file_name().is_some_and(|name| name == "index.lock")
                })
            {
                return;
            }
        }
        let _ = tx.try_send(());
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
    Ok((watcher, rx))
}
