use super::Diffy;
use crate::repository::GroupSnapshot;
use gpui_kit::AppContext as _;
use gpui_kit::Context;
use gpui_kit::FocusHandle;
use gpui_kit::ScrollStrategy;
use gpui_kit::SharedString;
use gpui_kit::component::VirtualListScrollHandle;
use gpui_kit::component::diff::DiffFile;
use gpui_kit::component::diff::DiffInlineUnit;
use gpui_kit::component::diff::DiffMode;
use gpui_kit::component::diff::DiffState;
use gpui_kit::{Pixels, Size, px, size};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

/// Labels built once when a snapshot arrives, so scrolling does not format them again.
pub(super) struct FileRow {
    pub(super) gap: f32,
    pub(super) directory: Option<SharedString>,
    pub(super) filename: SharedString,
    pub(super) group: SharedString,
    pub(super) stage_label: SharedString,
    pub(super) unstage_label: SharedString,
}

impl FileRow {
    fn from_path(
        path: &SharedString,
        previous: Option<&SharedString>,
        project_id: u64,
        index: usize,
    ) -> Self {
        let (directory, filename) = match path.rsplit_once('/') {
            None => (None, path.clone()),
            Some((dir, name)) => (
                Some(SharedString::from(format!("{dir}/"))),
                SharedString::from(name),
            ),
        };
        Self {
            gap: if previous.is_some()
                && path.rsplit_once('/').map(|(dir, _)| dir)
                    != previous.and_then(|path| path.rsplit_once('/').map(|(dir, _)| dir))
            {
                4.
            } else {
                0.
            },
            directory,
            filename,
            group: SharedString::from(format!("file-row-{project_id}-{index}")),
            stage_label: SharedString::from(format!("Stage {path}")),
            unstage_label: SharedString::from(format!("Unstage {path}")),
        }
    }
}

pub(super) struct ReviewView {
    /// Hash of the raw patch. The parsed files already hold the text, so the
    /// patch string itself is not kept around for the next equality check.
    pub(super) fingerprint: Option<u64>,
    pub(super) indices: HashMap<SharedString, usize>,
    pub(super) files: Arc<Vec<DiffFile>>,
    pub(super) rows: Arc<Vec<FileRow>>,
    pub(super) revisions: HashMap<SharedString, u64>,
    pub(super) selected: Option<SharedString>,
    pub(super) diff: gpui_kit::Entity<DiffState>,
    pub(super) scroll: VirtualListScrollHandle,
    pub(super) row_sizes: Rc<Vec<Size<Pixels>>>,
    pub(super) focus: FocusHandle,
}

impl ReviewView {
    pub(super) fn new(cx: &mut Context<Diffy>) -> Self {
        Self {
            fingerprint: None,
            indices: HashMap::new(),
            files: Arc::default(),
            rows: Arc::default(),
            revisions: HashMap::new(),
            selected: None,
            diff: cx.new(|cx| {
                DiffState::new([], cx)
                    .with_mode(DiffMode::Unified)
                    .with_context_lines(Some(3))
                    .with_inline_unit(Some(DiffInlineUnit::Word))
            }),
            scroll: VirtualListScrollHandle::new(),
            row_sizes: Rc::default(),
            focus: cx.focus_handle(),
        }
    }

    pub(super) fn loaded(&self) -> bool {
        self.fingerprint.is_some()
    }

    pub(super) fn selected_file(&self) -> Option<&DiffFile> {
        self.selected
            .as_ref()
            .and_then(|path| self.indices.get(path))
            .and_then(|ix| self.files.get(*ix))
    }

    pub(super) fn select(&mut self, path: SharedString, cx: &mut Context<Diffy>) {
        if self.selected.as_ref() == Some(&path) {
            return;
        }
        self.selected = Some(path);
        self.show_selected(cx);
    }

    fn show_selected(&self, cx: &mut Context<Diffy>) {
        let file = self.selected_file().cloned();
        self.diff.update(cx, |diff, cx| diff.set_files(file, cx));
        if let Some(ix) = self
            .selected
            .as_ref()
            .and_then(|path| self.indices.get(path))
            .copied()
        {
            self.scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
        }
    }

    pub(super) fn install(
        &mut self,
        snapshot: GroupSnapshot,
        project_id: u64,
        cx: &mut Context<Diffy>,
    ) {
        if self.fingerprint == Some(snapshot.fingerprint) {
            if self.selected.is_some() && self.selected_file().is_none() {
                self.selected = None;
                self.show_selected(cx);
            }
            return;
        }
        let previous_selected = self.selected.clone();
        let previous_revision = self
            .selected
            .as_ref()
            .and_then(|path| self.revisions.get(path))
            .copied();
        let ordinal = self
            .selected
            .as_ref()
            .and_then(|path| self.indices.get(path))
            .copied()
            .unwrap_or(0);
        self.fingerprint = Some(snapshot.fingerprint);
        self.revisions = snapshot.revisions;
        self.rows = Arc::new(
            snapshot
                .files
                .iter()
                .enumerate()
                .map(|(ix, file)| {
                    FileRow::from_path(
                        file.path(),
                        ix.checked_sub(1)
                            .and_then(|previous| snapshot.files.get(previous))
                            .map(|file| file.path()),
                        project_id,
                        ix,
                    )
                })
                .collect(),
        );
        self.row_sizes = Rc::new(
            self.rows
                .iter()
                .map(|row| size(px(0.), px(30. + row.gap)))
                .collect(),
        );
        self.indices = snapshot
            .files
            .iter()
            .enumerate()
            .map(|(ix, file)| (file.path().clone(), ix))
            .collect();
        self.files = Arc::new(snapshot.files);
        self.selected = self
            .selected
            .take()
            .filter(|p| self.indices.contains_key(p))
            .or_else(|| {
                self.files
                    .get(ordinal.min(self.files.len().saturating_sub(1)))
                    .map(|f| f.path().clone())
            });
        let revision = self
            .selected
            .as_ref()
            .and_then(|path| self.revisions.get(path))
            .copied();
        if self.selected != previous_selected || revision != previous_revision {
            if self.selected == previous_selected {
                let file = self.selected_file().cloned();
                self.diff
                    .update(cx, |diff, cx| diff.set_files_preserving_position(file, cx));
            } else {
                self.show_selected(cx);
            }
        }
    }
}
