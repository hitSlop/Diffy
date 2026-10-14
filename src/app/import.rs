use super::Diffy;
use crate::discovery::{self, Snapshot, Source};
use gpui_kit::StatefulInteractiveElement as _;
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Sizable as _, StyledExt as _, WindowExt as _, h_flex,
    v_flex, v_virtual_list,
};
use gpui_kit::component::{Icon, IconName};
use gpui_kit::component::{VirtualListScrollHandle, scroll::ScrollableElement as _};
use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    group_box::GroupBox,
    tooltip::Tooltip,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, AppContext as _, Context, Entity, FocusHandle, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, Styled as _, WeakEntity, Window, div, px, rgb, size,
};
use std::rc::Rc;
use std::{
    collections::BTreeSet,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

pub(super) struct Runtime {
    pub preferences: discovery::Preferences,
    pub snapshot: Arc<Snapshot>,
    pub picker: Option<Entity<Picker>>,
    pub requested: bool,
    pub loading: bool,
    pub cancelled: Arc<AtomicBool>,
    pub task: Option<gpui_kit::Task<()>>,
}
impl Default for Runtime {
    fn default() -> Self {
        Self {
            preferences: Default::default(),
            snapshot: Arc::new(Snapshot::default()),
            picker: None,
            requested: false,
            loading: false,
            cancelled: Arc::new(AtomicBool::new(false)),
            task: None,
        }
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

#[derive(Clone)]
enum Row {
    Header(Source),
    Project(usize, Source),
    Empty(Source),
}
pub(super) struct Picker {
    owner: WeakEntity<Diffy>,
    focus: FocusHandle,
    snapshot: Arc<Snapshot>,
    existing: BTreeSet<PathBuf>,
    selected: BTreeSet<PathBuf>,
    seen: BTreeSet<PathBuf>,
    automatic: [bool; 3],
    loading: bool,
    scroll: VirtualListScrollHandle,
    rows: Arc<Vec<Row>>,
    sizes: Rc<Vec<gpui_kit::Size<gpui_kit::Pixels>>>,
}
impl Picker {
    fn install(&mut self, snapshot: Arc<Snapshot>, existing: BTreeSet<PathBuf>, loading: bool) {
        self.existing = existing;
        self.selected.retain(|path| {
            snapshot
                .candidates
                .iter()
                .any(|c| c.path == *path && c.error.is_none())
                && !self.existing.contains(path)
        });
        for candidate in &snapshot.candidates {
            if self.seen.insert(candidate.path.clone())
                && candidate.error.is_none()
                && !self.existing.contains(&candidate.path)
            {
                self.selected.insert(candidate.path.clone());
            }
        }
        self.snapshot = snapshot;
        self.loading = loading;
        let mut rows = Vec::new();
        let mut sizes = Vec::new();
        for source in Source::ALL {
            rows.push(Row::Header(source));
            sizes.push(size(
                px(600.),
                px(if self.snapshot.errors[source.index()].is_some() {
                    112.
                } else {
                    84.
                }),
            ));
            let mut count = 0;
            let mut candidates: Vec<_> = self
                .snapshot
                .candidates
                .iter()
                .enumerate()
                .filter(|(_, candidate)| candidate.sources.contains(&source))
                .collect();
            candidates.sort_by_key(|(_, candidate)| {
                (
                    std::cmp::Reverse(candidate.created_at),
                    candidate.name().to_lowercase(),
                    &candidate.path,
                )
            });
            for (ix, _) in candidates {
                rows.push(Row::Project(ix, source));
                sizes.push(size(px(600.), px(60.)));
                count += 1;
            }
            if count == 0 {
                rows.push(Row::Empty(source));
                sizes.push(size(px(600.), px(36.)));
            }
        }
        self.rows = Arc::new(rows);
        self.sizes = Rc::new(sizes);
    }
    fn finish(&mut self, apply: bool, window: &mut Window, cx: &mut Context<Self>) {
        let selected = self.selected.clone();
        let automatic = self.automatic;
        let snapshot = self.snapshot.clone();
        let _ = self.owner.update(cx, |this, cx| {
            this.finish_import(apply, selected, automatic, &snapshot, window, cx)
        });
    }
    fn render_project(&self, ix: usize, source: Source, cx: &mut Context<Self>) -> AnyElement {
        let candidate = &self.snapshot.candidates[ix];
        let row_id = if candidate.section() == source {
            ix.to_string()
        } else {
            format!("{ix}-{}", source.index())
        };
        let path = candidate.path.clone();
        let tooltip = path.display().to_string();
        let row_path = path.clone();
        let existing = self.existing.contains(&path);
        let disabled = existing || candidate.error.is_some();
        h_flex()
            .id(format!("import-project-{row_id}"))
            .test_support()
            .h(px(60.))
            .w_full()
            .gap_3()
            .px_2()
            .rounded_md()
            .hover(|s| s.bg(rgb(0x141414)))
            .on_click(cx.listener(move |this, _, _, cx| {
                if !disabled {
                    if !this.selected.remove(&row_path) {
                        this.selected.insert(row_path.clone());
                    }
                    cx.notify();
                }
            }))
            .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
            .child(if disabled {
                Icon::new(if existing {
                    IconName::Check
                } else {
                    IconName::CircleAlert
                })
                .size_4()
                .text_color(cx.theme().muted_foreground)
                .into_any_element()
            } else {
                div()
                    .id(format!("import-check-{row_id}"))
                    .test_support()
                    .child(
                        Checkbox::new(format!("choose-project-{row_id}"))
                            .accessibility_label(format!("Import {}", candidate.path.display()))
                            .checked(self.selected.contains(&path))
                            .on_change(cx.listener(move |this, checked: &bool, _, cx| {
                                cx.stop_propagation();
                                if *checked {
                                    this.selected.insert(path.clone());
                                } else {
                                    this.selected.remove(&path);
                                }
                                cx.notify();
                            })),
                    )
                    .into_any_element()
            })
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .child(
                        h_flex()
                            .gap_2()
                            .child(div().font_medium().child(candidate.name()))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(
                                        candidate
                                            .sources
                                            .iter()
                                            .map(|s| s.label())
                                            .collect::<Vec<_>>()
                                            .join(" · "),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(candidate.path.display().to_string()),
                    )
                    .when(candidate.error.is_some(), |d| {
                        d.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().danger)
                                .child(candidate.error.clone().unwrap()),
                        )
                    }),
            )
            .when(existing, |row| {
                row.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child("Already added"),
                )
            })
            .into_any_element()
    }
}
impl Render for Picker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = self.rows.clone();
        v_flex()
            .id("import-picker")
            .test_support()
            .track_focus(&self.focus)
            .gap_3()
            .w_full()
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(cx.theme().muted_foreground)
                    .child("Choose local Git projects to add to Diffy."),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("import-select-all")
                            .ghost()
                            .small()
                            .label("Select all")
                            .disabled(self.loading)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.selected = this
                                    .snapshot
                                    .candidates
                                    .iter()
                                    .filter(|c| {
                                        c.error.is_none() && !this.existing.contains(&c.path)
                                    })
                                    .map(|c| c.path.clone())
                                    .collect();
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("import-clear")
                            .ghost()
                            .small()
                            .label("Clear")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.selected.clear();
                                cx.notify();
                            })),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("import-refresh")
                            .ghost()
                            .small()
                            .label("Refresh")
                            .disabled(self.loading)
                            .on_click(cx.listener(|this, _, _, cx| {
                                let _ = this.owner.update(cx, |owner, cx| {
                                    owner.request_import_scan(cx);
                                });
                            })),
                    ),
            )
            .child(
                div().id("import-scroll").test_support().relative().flex_none().min_h_0()
                    .h(px((f32::from(window.viewport_size().height) * 0.52).min(460.)))
                    .child(v_virtual_list(
                    cx.entity(),
                    "import-results",
                    self.sizes.clone(),
                    move |this, range, _, cx| {
                        range
                            .map(|ix| match rows[ix] {
                                Row::Header(source) => {
                                    let error = this.snapshot.errors[source.index()].clone();
                                    GroupBox::new()
                                        .title(source.label())
                                        .w_full()
                                        .mt_3()
                                        .child(
                                            div().id(format!("import-auto-{}",source.index())).test_support().child(Checkbox::new(("auto-import", source.index()))
                                                .label("Automatically add new projects")
                                                .accessibility_label(format!("Automatically add new {} projects",source.label()))
                                                .checked(this.automatic[source.index()])
                                                .on_change(cx.listener(
                                                    move |this, checked: &bool, _, cx| {
                                                        this.automatic[source.index()] = *checked;
                                                        cx.notify();
                                                    },
                                                ))),
                                        )
                                        .children(error.map(|e| {
                                            div().text_xs().text_color(cx.theme().muted_foreground).child(e)
                                        }))
                                        .into_any_element()
                                }
                                Row::Project(candidate, source) => this.render_project(candidate, source, cx),
                                Row::Empty(source) => div()
                                    .pl_2()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(if this.loading {
                                        "Looking for projects…"
                                    } else if this.snapshot.errors[source.index()].is_some() {
                                        "No readable projects found."
                                    } else {
                                        "No projects found."
                                    })
                                    .into_any_element(),
                            })
                            .collect::<Vec<_>>()
                    },
                )
                .track_scroll(&self.scroll)
                .size_full()).vertical_scrollbar(&self.scroll),
            )
            .child(
                h_flex()
                    .justify_between()
                    .gap_2()
                    .child(Button::new("import-skip").ghost().label("Cancel").on_click(
                        cx.listener(|this, _, window, cx| this.finish(false, window, cx)),
                    ))
                    .child(
                        Button::new("import-apply")
                            .ghost()
                            .disabled(self.loading)
                            .label(if self.selected.is_empty() {
                                "Done".into()
                            } else {
                                format!("Import {} projects", self.selected.len())
                            })
                            .on_click(
                                cx.listener(|this, _, window, cx| this.finish(true, window, cx)),
                            ),
                    ),
            )
    }
}

impl Diffy {
    pub(super) fn project_paths(&self) -> BTreeSet<PathBuf> {
        self.projects
            .iter()
            .filter_map(|p| p.path.clone())
            .collect()
    }
    pub(super) fn open_import(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.import.picker.is_some() {
            return;
        }
        self.stop_file_repeat();
        if self.settings.is_some() {
            self.close_settings(window, cx);
        }
        if self.palette_open {
            self.close_palette(true, window, cx);
        }
        let existing = self.project_paths();
        let owner = cx.entity().downgrade();
        let snapshot = self.import.snapshot.clone();
        let automatic = self.import.preferences.automatic;
        let excluded = self.import.preferences.excluded.clone();
        let picker = cx.new(|cx| {
            let mut picker = Picker {
                owner,
                focus: cx.focus_handle(),
                snapshot: Arc::new(Snapshot::default()),
                existing: BTreeSet::new(),
                selected: BTreeSet::new(),
                seen: excluded,
                automatic,
                loading: true,
                scroll: VirtualListScrollHandle::new(),
                rows: Arc::new(Vec::new()),
                sizes: Rc::new(Vec::new()),
            };
            picker.install(snapshot, existing, true);
            picker
        });
        self.import.picker = Some(picker.clone());
        let owner = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, _| {
            let picker = picker.clone();
            let owner = owner.clone();
            dialog
                .title("Import projects")
                .entrance(gpui_kit::component::dialog::DialogEntrance::Fade)
                .width(px(680.))
                .bg(rgb(0x090909))
                .border_0()
                .content(move |content, _, _| content.child(picker.clone()))
                .on_close(move |_, window, cx| {
                    let _ = owner.update(cx, |this, cx| {
                        this.import.picker = None;
                        this.import.preferences.onboarding_completed = true;
                        this.persist(cx);
                        this.focus.focus(window, cx);
                        cx.notify();
                    });
                })
        });
        self.request_import_scan(cx);
        if self.import.task.is_none() {
            self.start_discovery(window, cx);
        }
    }
    pub(super) fn request_import_scan(&mut self, cx: &mut Context<Self>) {
        self.import.requested = true;
        self.import.loading = true;
        if let Some(picker) = &self.import.picker {
            picker.update(cx, |p, cx| {
                p.loading = true;
                cx.notify();
            });
        }
        cx.notify();
    }
    pub(super) fn finish_import(
        &mut self,
        apply: bool,
        selected: BTreeSet<PathBuf>,
        automatic: [bool; 3],
        snapshot: &Snapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if apply {
            self.import.preferences.automatic = automatic;
            for candidate in &snapshot.candidates {
                if selected.contains(&candidate.path) {
                    self.import.preferences.excluded.remove(&candidate.path);
                } else if candidate.error.is_none()
                    && !self.project_paths().contains(&candidate.path)
                {
                    self.import
                        .preferences
                        .excluded
                        .insert(candidate.path.clone());
                }
            }
            self.import_projects(selected, cx);
        }
        self.import.preferences.onboarding_completed = true;
        self.import.picker = None;
        self.persist(cx);
        window.close_dialog(cx);
        self.focus.focus(window, cx);
        cx.notify();
    }
    fn import_projects(&mut self, paths: BTreeSet<PathBuf>, cx: &mut Context<Self>) {
        let active = self.active_project_id();
        let was_restoring = self.restoring;
        self.restoring = true;
        for path in paths {
            if !self.project_paths().contains(&path) {
                self.add_path(path, cx);
            }
        }
        if let Some(id) = active
            && let Some(ix) = self.projects.iter().position(|p| p.id == id)
        {
            self.active = ix;
        } else if active.is_none() {
            self.active = 0;
        }
        self.restoring = was_restoring;
        self.persist(cx);
    }
    pub(super) fn receive_discovery(&mut self, snapshot: Snapshot, cx: &mut Context<Self>) {
        let snapshot = Arc::new(snapshot);
        self.import.loading = false;
        self.import.snapshot = snapshot.clone();
        // While choosing, defer auto-add so existing checkbox rows do not change underneath the user.
        if self.import.picker.is_none() {
            let paths = snapshot
                .candidates
                .iter()
                .filter(|c| self.import.preferences.should_add(c))
                .map(|c| c.path.clone())
                .collect();
            self.import_projects(paths, cx);
        }
        if let Some(picker) = &self.import.picker {
            let existing = self.project_paths();
            picker.update(cx, |p, cx| {
                p.install(snapshot, existing, false);
                cx.notify();
            });
        }
        cx.notify();
    }
    pub(super) fn start_discovery(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.import.task.is_some() {
            return;
        }
        let cancelled = self.import.cancelled.clone();
        let locations = discovery::Locations::from_env();
        self.import.task = Some(cx.spawn_in(window, async move |owner, cx| {
            let mut scanner = discovery::Scanner::default();
            let mut watch = discovery::MetadataWatch::default();
            let mut next = Instant::now();
            loop {
                let Ok((enabled, requested, picker)) = owner.update(cx, |this, _| {
                    let requested = std::mem::take(&mut this.import.requested);
                    (
                        this.import.preferences.automatic,
                        requested,
                        this.import.picker.is_some(),
                    )
                }) else {
                    break;
                };
                let dirty = watch.dirty.swap(false, Ordering::Relaxed);
                if requested || (enabled.iter().any(|v| *v) && (dirty || Instant::now() >= next)) {
                    let scan_sources = if picker { [true; 3] } else { enabled };
                    let locations = locations.clone();
                    let cancelled = cancelled.clone();
                    let result = cx
                        .background_executor()
                        .spawn(async move {
                            watch.configure(&locations, enabled);
                            let snapshot = scanner.scan(&locations, scan_sources, &cancelled);
                            (scanner, watch, snapshot)
                        })
                        .await;
                    scanner = result.0;
                    watch = result.1;
                    let _ = owner.update(cx, |this, cx| this.receive_discovery(result.2, cx));
                    let herdr_live = owner
                        .read_with(cx, |this, _| this.import.snapshot.herdr_live)
                        .unwrap_or(false);
                    next = Instant::now()
                        + Duration::from_secs(if enabled[0] && herdr_live { 5 } else { 30 });
                } else {
                    // Drop disabled watchers promptly, even when there is no scan to run.
                    watch.configure(&locations, enabled);
                }
                cx.background_executor()
                    .timer(Duration::from_millis(500))
                    .await;
            }
        }));
    }
}

#[cfg(feature = "ui-tests")]
impl Diffy {
    pub fn open_import_fixture(
        &mut self,
        snapshot: Snapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_import(window, cx);
        self.import.task = None;
        self.import.requested = false;
        self.receive_discovery(snapshot, cx);
    }
    pub fn import_selected_count(&self, cx: &gpui_kit::App) -> usize {
        self.import
            .picker
            .as_ref()
            .map_or(0, |picker| picker.read(cx).selected.len())
    }
    pub fn import_automatic(&self, cx: &gpui_kit::App) -> [bool; 3] {
        self.import
            .picker
            .as_ref()
            .map_or(self.import.preferences.automatic, |p| p.read(cx).automatic)
    }
    pub fn import_is_open(&self) -> bool {
        self.import.picker.is_some()
    }
    pub fn discovery_preferences(&self) -> &discovery::Preferences {
        &self.import.preferences
    }
}
