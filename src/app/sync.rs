use super::Diffy;
use crate::repository;
use crate::repository::ChangeGroup;
use crate::watch;
use gpui_kit::AppContext as _;
use gpui_kit::Context;
use gpui_kit::SharedString;
use gpui_kit::component::diff::DiffFile;
use std::path::PathBuf;

impl Diffy {
    pub(super) fn load(&mut self, mutation: Option<(Vec<DiffFile>, bool)>, cx: &mut Context<Self>) {
        if mutation.is_some() && self.updates.blocks_mutations() {
            return;
        }
        let ix = self.active;
        let Some(p) = self.projects.get_mut(ix) else {
            return;
        };
        let id = p.id;
        if p.mutating {
            return;
        }
        let Some(path) = p.path.clone() else {
            return;
        };
        // Once a snapshot is on screen, another read must not dim Stage all and
        // Refresh. Those buttons follow `loading`. A stage click still takes
        // the visible path and supersedes the quiet read.
        let visible = mutation.is_some() || !p.views.iter().any(|view| view.loaded());
        if mutation.is_none() && (p.loading || p.refreshing) {
            p.pending_refresh = true;
            return;
        }
        p.generation += 1;
        let generation = p.generation;
        let fingerprints = std::array::from_fn(|ix| p.views[ix].fingerprint);
        p.loading = visible;
        p.refreshing = !visible;
        p.mutating = mutation.is_some();
        if visible {
            p.error = None;
        }
        p.task = Some(cx.spawn(async move |this, cx| {
            let slot = repository::refresh_slot().await;
            if !this
                .update(cx, |this, _| {
                    this.projects
                        .iter()
                        .any(|p| p.id == id && p.generation == generation)
                })
                .unwrap_or(false)
            {
                return;
            }
            let result = cx
                .background_spawn(async move {
                    // Keep the slot until the read ends even if the UI task is superseded.
                    let _slot = slot;
                    let mutation_error = mutation.and_then(|(file, stage)| {
                        repository::set_files_staged(&path, &file, stage).err()
                    });
                    (
                        repository::load_refresh(&path, fingerprints),
                        mutation_error,
                    )
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                let Some(ix) = this.projects.iter().position(|p| p.id == id) else {
                    return;
                };
                if this.projects[ix].generation != generation {
                    return;
                }
                if let Ok(snapshot) = &result.0
                    && let Some(existing) = this
                        .projects
                        .iter()
                        .position(|p| p.id != id && p.path.as_ref() == Some(&snapshot.root))
                {
                    let existing_id = this.projects[existing].id;
                    let was_active = this.active == ix;
                    this.projects.remove(ix);
                    if was_active {
                        this.active = this
                            .projects
                            .iter()
                            .position(|p| p.id == existing_id)
                            .unwrap_or(0);
                    } else if this.active > ix {
                        this.active -= 1;
                    }
                    this.hovered_project = None;
                    this.persist(cx);
                    cx.notify();
                    return;
                }
                let mut watch_setup = None;
                let p = &mut this.projects[ix];
                if p.generation != generation {
                    return;
                }
                p.loading = false;
                p.refreshing = false;
                p.mutating = false;
                match result.0 {
                    Ok(snapshot) => {
                        if p.custom_name.is_none() {
                            p.name = snapshot
                                .root
                                .file_name()
                                .map(|s| s.to_string_lossy().into_owned())
                                .unwrap_or_else(|| "Project".into())
                                .into();
                        }
                        if p.watch_task.is_none() {
                            watch_setup = Some((snapshot.root.clone(), snapshot.git_dirs));
                        }
                        p.path = Some(snapshot.root);
                        p.branch = snapshot.branch.into();
                        if let Some(snapshot) = snapshot.staged {
                            p.views[0].install(snapshot, id, cx);
                        }
                        if let Some(snapshot) = snapshot.changes {
                            p.views[1].install(snapshot, id, cx);
                        }
                        p.update_count();
                        p.error = result.1;
                    }
                    Err(error) => {
                        p.error = Some(result.1.map_or(error.clone(), |e| format!("{e}\n{error}")))
                    }
                }
                let again = p.pending_refresh;
                p.pending_refresh = false;
                if let Some((root, dirs)) = watch_setup {
                    this.start_watching(id, root, dirs, cx);
                }
                this.persist(cx);
                if again {
                    this.refresh_project(id, cx);
                }
                cx.notify();
            });
        }));
        if visible {
            cx.notify();
        }
    }
    pub(super) fn refresh(&mut self, cx: &mut Context<Self>) {
        if let Some(p) = self.projects.get_mut(self.active)
            && (p.loading || p.refreshing)
        {
            p.pending_refresh = true;
            return;
        }
        self.load(None, cx);
    }
    pub(super) fn refresh_project(&mut self, id: u64, cx: &mut Context<Self>) {
        if let Some(ix) = self.projects.iter().position(|p| p.id == id) {
            let active = self.active;
            self.active = ix;
            self.refresh(cx);
            self.active = active;
        }
    }
    pub(super) fn start_watching(
        &mut self,
        id: u64,
        root: PathBuf,
        dirs: Vec<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        let Some(ix) = self.projects.iter().position(|p| p.id == id) else {
            return;
        };
        let state_path = self.state_path.clone();
        self.projects[ix].watch_task = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { watch::start(&root, &dirs, state_path) })
                .await;
            let rx = match result {
                Ok((watcher, rx)) => {
                    let installed = this
                        .update(cx, |this, cx| {
                            if let Some(p) = this.projects.iter_mut().find(|p| p.id == id) {
                                p.watcher = Some(watcher);
                                cx.notify();
                                true
                            } else {
                                false
                            }
                        })
                        .unwrap_or(false);
                    if !installed {
                        return;
                    }
                    // Reconcile edits made between the initial read and watcher attachment.
                    let _ = this.update(cx, |this, cx| this.refresh_project(id, cx));
                    rx
                }
                Err(error) => {
                    let _ = this.update(cx, |this, cx| {
                        if let Some(p) = this.projects.iter_mut().find(|p| p.id == id) {
                            p.error = Some(error);
                            p.watch_task = None;
                        }
                        cx.notify();
                    });
                    return;
                }
            };
            while rx.recv().await.is_ok() {
                while rx.try_recv().is_ok() {}
                if this
                    .update(cx, |this, cx| this.refresh_project(id, cx))
                    .is_err()
                {
                    break;
                }
            }
        }));
    }
    pub(super) fn stage_file(&mut self, cx: &mut Context<Self>) {
        if self.updates.blocks_mutations() {
            return;
        }
        let Some(p) = self.projects.get(self.active) else {
            return;
        };
        if p.loading || p.path.is_none() {
            return;
        }
        if let Some(file) = p.view().selected_file().cloned() {
            self.load(Some((vec![file], p.group == ChangeGroup::Changes)), cx);
        }
    }
    pub(super) fn stage_all(&mut self, cx: &mut Context<Self>) {
        if self.updates.blocks_mutations() {
            return;
        }
        let Some(p) = self.projects.get(self.active) else {
            return;
        };
        if p.loading || p.path.is_none() || p.view().files.is_empty() {
            return;
        }
        self.load(
            Some((
                p.view().files.as_ref().clone(),
                p.group == ChangeGroup::Changes,
            )),
            cx,
        );
    }
    pub(super) fn stage_path(
        &mut self,
        id: u64,
        group: ChangeGroup,
        path: &SharedString,
        cx: &mut Context<Self>,
    ) {
        let Some(ix) = self.projects.iter().position(|p| p.id == id) else {
            return;
        };
        let p = &self.projects[ix];
        if p.loading || p.path.is_none() {
            return;
        }
        let file = p.views[group.index()]
            .files
            .iter()
            .find(|f| f.path() == path)
            .cloned();
        if let Some(file) = file {
            let active = self.active;
            self.active = ix;
            self.load(Some((vec![file], group == ChangeGroup::Changes)), cx);
            self.active = active;
        }
    }
    pub fn live_ready(&self) -> bool {
        self.projects
            .get(self.active)
            .is_some_and(|p| p.watcher.is_some())
    }
    pub fn selected_additions(&self) -> usize {
        self.projects
            .get(self.active)
            .and_then(|p| p.view().selected_file())
            .map_or(0, |f| f.additions())
    }
}
