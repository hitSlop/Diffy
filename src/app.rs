use crate::{
    demo, fonts, keyboard,
    repository::{self, ChangeGroup, GroupSnapshot},
    state, updates, watch,
};
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::base::motion::{Transition, transition};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, FocusTrapElement as _, Icon, IconName,
    InteractiveElementExt as _, Placement, Selectable as _, Sizable as _, StyledExt as _, Theme,
    ThemeMode,
    button::{Button, ButtonVariants as _},
    color_picker::{ColorPicker, ColorPickerState},
    command::{Command, CommandGroup, CommandItem, CommandState},
    diff::{
        Diff, DiffFile, DiffFileStatus, DiffHunkSeparator, DiffInlineUnit, DiffMode, DiffState,
    },
    h_flex,
    input::{Input, InputState},
    list::ListItem,
    resizable::{h_resizable, resizable_panel},
    scroll::ScrollbarMode,
    status_bar::StatusBar,
    tooltip::Tooltip,
    v_flex,
};
use gpui_kit::{
    Action, AnyElement, App, AppContext as _, Context, ElementId, Entity, FocusHandle,
    Focusable as _, InteractiveElement as _, IntoElement, KeyBinding, MouseButton, ObjectFit,
    ParentElement as _, PathPromptOptions, Render, ScrollStrategy, SharedString,
    StatefulInteractiveElement as _, Styled as _, StyledImage as _, Subscription, Task,
    UniformListScrollHandle, Window, div, img, prelude::FluentBuilder as _, px, rgb, uniform_list,
};
use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Duration};

gpui_kit::actions!(
    diffy,
    [
        OpenProject,
        Refresh,
        NextProject,
        PreviousProject,
        NextFile,
        PreviousFile,
        NextChange,
        PreviousChange,
        FocusDiff,
        ToggleGroup,
        StageFile,
        StageAll,
        OpenCommandPalette,
        RemoveCurrentProject,
        UnifiedMode,
        SplitMode,
        CheckForUpdates,
        InstallUpdate,
        About,
        Hide,
        HideOthers,
        ShowAll,
        Quit
    ]
);
#[derive(Clone, PartialEq, serde::Deserialize, gpui_kit::Action)]
#[action(namespace = diffy, no_json)]
pub struct SelectProject {
    pub id: u64,
}
#[derive(Clone, PartialEq, serde::Deserialize, gpui_kit::Action)]
#[action(namespace = diffy, no_json)]
pub struct RemoveProject {
    pub id: u64,
}
#[derive(Clone, PartialEq, serde::Deserialize, gpui_kit::Action)]
#[action(namespace = diffy, no_json)]
pub struct EditProject {
    pub id: u64,
}

pub fn init(cx: &mut App) {
    fonts::init(cx);
    cx.set_menus([
        gpui_kit::Menu::new("Diffy").items([
            gpui_kit::MenuItem::action("About Diffy", About),
            gpui_kit::MenuItem::action("Check for Updates…", CheckForUpdates),
            gpui_kit::MenuItem::Separator,
            gpui_kit::MenuItem::action("Hide Diffy", Hide),
            gpui_kit::MenuItem::action("Hide Others", HideOthers),
            gpui_kit::MenuItem::action("Show All", ShowAll),
            gpui_kit::MenuItem::Separator,
            gpui_kit::MenuItem::action("Quit Diffy", Quit),
        ]),
        gpui_kit::Menu::new("File").items([
            gpui_kit::MenuItem::action("Open Repository…", OpenProject),
            gpui_kit::MenuItem::action("Refresh Repository", Refresh),
        ]),
    ]);
    cx.bind_keys([
        KeyBinding::new("secondary-h", Hide, Some("Diffy")),
        KeyBinding::new("secondary-alt-h", HideOthers, Some("Diffy")),
        KeyBinding::new("secondary-p", OpenCommandPalette, Some("Diffy")),
        KeyBinding::new("secondary-o", OpenProject, Some("Diffy")),
        KeyBinding::new("secondary-r", Refresh, Some("Diffy")),
        KeyBinding::new("secondary-shift-down", NextProject, Some("Diffy")),
        KeyBinding::new("secondary-shift-up", PreviousProject, Some("Diffy")),
        KeyBinding::new("secondary-down", NextFile, Some("Diffy")),
        KeyBinding::new("secondary-up", PreviousFile, Some("Diffy")),
        KeyBinding::new("down", NextFile, Some("DiffyFiles")),
        KeyBinding::new("up", PreviousFile, Some("DiffyFiles")),
        KeyBinding::new("alt-down", NextChange, Some("Diffy")),
        KeyBinding::new("alt-up", PreviousChange, Some("Diffy")),
        KeyBinding::new("secondary-enter", FocusDiff, Some("Diffy")),
        KeyBinding::new("secondary-shift-s", ToggleGroup, Some("Diffy")),
        KeyBinding::new("secondary-s", StageFile, Some("Diffy")),
        KeyBinding::new("secondary-q", Quit, Some("Diffy")),
    ]);
}

struct ReviewView {
    patch: String,
    files: Arc<Vec<DiffFile>>,
    revisions: HashMap<SharedString, u64>,
    selected: Option<SharedString>,
    diff: Entity<DiffState>,
    scroll: UniformListScrollHandle,
    focus: FocusHandle,
}
impl ReviewView {
    fn new(cx: &mut Context<Diffy>) -> Self {
        Self {
            patch: String::new(),
            files: Arc::default(),
            revisions: HashMap::new(),
            selected: None,
            diff: cx.new(|cx| {
                DiffState::new([], cx)
                    .with_mode(DiffMode::Unified)
                    .with_context_lines(Some(3))
                    .with_inline_unit(Some(DiffInlineUnit::Word))
            }),
            scroll: UniformListScrollHandle::new(),
            focus: cx.focus_handle(),
        }
    }
    fn selected_file(&self) -> Option<&DiffFile> {
        self.files
            .iter()
            .find(|f| Some(f.path()) == self.selected.as_ref())
    }
    fn select(&mut self, path: SharedString, cx: &mut Context<Diffy>) {
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
            .files
            .iter()
            .position(|f| Some(f.path()) == self.selected.as_ref())
        {
            self.scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
        }
    }
    fn install(&mut self, snapshot: GroupSnapshot, cx: &mut Context<Diffy>) {
        if self.patch == snapshot.patch {
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
            .files
            .iter()
            .position(|f| Some(f.path()) == self.selected.as_ref())
            .unwrap_or(0);
        self.patch = snapshot.patch;
        self.revisions = snapshot.revisions;
        self.files = Arc::new(snapshot.files);
        self.selected = self
            .selected
            .take()
            .filter(|p| self.files.iter().any(|f| f.path() == p))
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
struct Project {
    id: u64,
    watcher: Option<notify::RecommendedWatcher>,
    watch_task: Option<Task<()>>,
    pending_refresh: bool,
    subscriptions: Vec<Subscription>,
    name: SharedString,
    custom_name: Option<String>,
    icon: Option<state::ProjectIcon>,
    color: u32,
    path: Option<PathBuf>,
    branch: SharedString,
    views: [ReviewView; 2],
    group: ChangeGroup,
    loading: bool,
    mutating: bool,
    error: Option<String>,
    generation: u64,
    task: Option<Task<()>>,
}
impl Project {
    fn view(&self) -> &ReviewView {
        &self.views[self.group.index()]
    }
    fn view_mut(&mut self) -> &mut ReviewView {
        &mut self.views[self.group.index()]
    }
}
pub struct Diffy {
    projects: Vec<Project>,
    active: usize,
    focus: FocusHandle,
    next_id: u64,
    state_path: Option<PathBuf>,
    state_error: Option<String>,
    restoring: bool,
    command: Entity<CommandState>,
    palette_open: bool,
    previous_focus: Option<FocusHandle>,
    hovered_project: Option<usize>,
    settings: Option<ProjectSettings>,
    repeat_direction: Option<isize>,
    file_repeat: Option<Task<()>>,
    file_key_probe: fn(isize) -> bool,
    navigation_save: Option<Task<()>>,
    persisted_state: Option<state::SavedState>,
    updates: updates::Controller,
    update_task: Option<Task<()>>,
    _keyboard_subscription: Subscription,
}
struct ProjectSettings {
    id: u64,
    focus: FocusHandle,
    name: Entity<InputState>,
    emoji: Entity<InputState>,
    color: Entity<ColorPickerState>,
    icon: Option<state::ProjectIcon>,
    error: Option<String>,
}
impl Diffy {
    pub fn new(paths: Vec<PathBuf>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::with_options(paths, false, state::default_path(), window, cx)
    }
    pub fn demo(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::with_options(Vec::new(), true, None, window, cx)
    }
    pub fn with_options(
        paths: Vec<PathBuf>,
        demo: bool,
        state_path: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        window.set_window_title("Diffy");
        Theme::change(ThemeMode::Dark, Some(window), cx);
        Theme::update(cx, |theme| {
            let background = rgb(0x000000).into();
            let rail = rgb(0x000000).into();
            let border = rgb(0x000000).into();
            theme.background = background;
            theme.sidebar = rail;
            theme.title_bar = background;
            theme.title_bar_border = border;
            theme.status_bar = background;
            theme.status_bar_border = background;
            theme.border = border;
            theme.sidebar_border = border;
            theme.tab_bar = background;
            theme.tab = background;
            theme.tab_active = background;
            theme.foreground = rgb(0xe7e7e8).into();
            theme.muted_foreground = rgb(0x8d929d).into();
            theme.primary = rgb(0xf2d45c).into();
            theme.primary_foreground = rgb(0x151510).into();
            theme.colors.list = rail;
            theme.list_active = rgb(0x1b1d21).into();
            theme.list_active_border = rgb(0x1b1d21).into();
            theme.list_hover = rgb(0x14161a).into();
            theme.secondary = rgb(0x181a1e).into();
            theme.secondary_hover = rgb(0x24262b).into();
            theme.success = rgb(0x65d5a2).into();
            theme.danger = rgb(0xf1757d).into();
            theme.muted = background;
            theme.scrollbar_mode = ScrollbarMode::Always;
            theme.font_family = fonts::FAMILY.into();
            theme.font_size = px(13.);
            theme.mono_font_family = fonts::FAMILY.into();
            theme.mono_font_size = px(13.);
        });
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        let mut this = Self {
            projects: Vec::new(),
            active: 0,
            focus,
            next_id: 1,
            state_path: if demo { None } else { state_path },
            state_error: None,
            restoring: true,
            command: cx.new(|cx| CommandState::new(window, cx)),
            palette_open: false,
            previous_focus: None,
            hovered_project: None,
            settings: None,
            repeat_direction: None,
            file_repeat: None,
            file_key_probe: keyboard::file_shortcut_pressed,
            navigation_save: None,
            persisted_state: None,
            updates: updates::Controller::default(),
            update_task: None,
            _keyboard_subscription: {
                let owner = cx.entity().downgrade();
                let window_id = window.window_handle().window_id();
                cx.intercept_keystrokes(move |event, w, cx| {
                    if w.window_handle().window_id() != window_id {
                        return;
                    }
                    let _ = owner.update(cx, |this, cx| {
                        if this.settings.is_none() {
                            let mods = event.keystroke.modifiers;
                            if !this.palette_open && mods.secondary() && !mods.shift && !mods.alt {
                                let direction = match event.keystroke.key.as_str() {
                                    "up" => Some(-1),
                                    "down" => Some(1),
                                    _ => None,
                                };
                                if let Some(direction) = direction {
                                    cx.stop_propagation();
                                    this.start_file_repeat(direction, w, cx);
                                }
                            }
                            return;
                        }
                        let key = event.keystroke.key.as_str();
                        if key == "escape" {
                            cx.stop_propagation();
                            if let Some(draft) = &this.settings
                                && draft.color.read(cx).is_open()
                            {
                                draft.color.update(cx, |s, cx| s.set_open(false, cx));
                            } else {
                                this.close_settings(w, cx);
                            }
                        } else if event.keystroke.modifiers.secondary() {
                            if matches!(key, "s" | "enter") {
                                cx.stop_propagation();
                                this.save_settings(w, cx);
                            } else if matches!(key, "up" | "down" | "p" | "o" | "r") {
                                cx.stop_propagation();
                            }
                        }
                    });
                })
            },
        };
        if demo {
            for sample in demo::projects() {
                this.add_demo(sample, cx);
            }
        } else {
            let saved = this.state_path.as_ref().map(|p| state::load(p)).transpose();
            match saved {
                Ok(Some(saved)) => {
                    for entry in saved.projects {
                        this.add_path(entry.path, cx);
                        let p = &mut this.projects[this.active];
                        p.color = entry.color;
                        p.custom_name = entry.name;
                        if let Some(name) = &p.custom_name {
                            p.name = name.clone().into();
                        }
                        p.icon = entry.icon;
                        p.group = if entry.staged {
                            ChangeGroup::Staged
                        } else {
                            ChangeGroup::Changes
                        };
                        for (ix, selected) in entry.selected.into_iter().enumerate() {
                            p.views[ix].selected = selected.map(Into::into);
                            p.views[ix].diff.update(cx, |d, cx| {
                                d.set_mode(
                                    if entry.split {
                                        DiffMode::Split
                                    } else {
                                        DiffMode::Unified
                                    },
                                    cx,
                                )
                            });
                        }
                    }
                    this.active = saved
                        .active
                        .and_then(|path| {
                            this.projects
                                .iter()
                                .position(|p| p.path.as_ref() == Some(&path))
                        })
                        .unwrap_or(0);
                }
                Err(e) => this.state_error = Some(e),
                _ => {}
            }
            for path in paths {
                this.add_path(path, cx);
            }
        }
        this.restoring = false;
        let owner = cx.entity().downgrade();
        window.on_window_should_close(cx, move |_, cx| {
            owner
                .update(cx, |this, cx| {
                    if this.updates.blocks_mutations() {
                        return false;
                    }
                    this.persist(cx);
                    true
                })
                .unwrap_or(true)
        });
        if !demo
            && std::env::current_exe()
                .ok()
                .and_then(|p| updates::bundle_path(&p))
                .is_some()
        {
            this.update_task = Some(cx.spawn(async move |owner, cx| {
                cx.background_executor().timer(Duration::from_secs(5)).await;
                let _ = owner.update(cx, |this, cx| this.check_updates(false, cx));
            }));
        }
        this
    }
    fn check_updates(&mut self, manual: bool, cx: &mut Context<Self>) {
        if !self.updates.begin_check() {
            return;
        }
        self.updates.show_check = manual;
        let backend = self.updates.backend.clone();
        self.update_task = Some(cx.spawn(async move |owner, cx| {
            let result = cx.background_spawn(async move { backend.check() }).await;
            let _ = owner.update(cx, |this, cx| {
                this.updates.checked(result, manual);
                cx.notify();
            });
        }));
        cx.notify();
    }
    fn install_update(&mut self, cx: &mut Context<Self>) {
        if self.updates.busy() {
            return;
        }
        let Some(release) = self.updates.release.clone() else {
            return;
        };
        if let Some(error) = updates::installation_problem() {
            self.updates.phase = updates::Phase::Error(error);
            cx.notify();
            return;
        }
        self.persist(cx);
        if self.state_error.is_some() {
            self.updates.phase = updates::Phase::Error(
                "Couldn't save projects. Resolve the preference error before updating.".into(),
            );
            cx.notify();
            return;
        }
        self.stop_file_repeat();
        self.updates.phase = updates::Phase::Downloading {
            received: 0,
            total: None,
        };
        self.update_task = Some(cx.spawn(async move |owner, cx| {
            let (sender, receiver) = async_channel::bounded(1);
            let package = release.package.clone();
            let download = cx.background_spawn(async move {
                let received = std::sync::atomic::AtomicU64::new(0);
                package.download(&|chunk, total| {
                    let count = received
                        .fetch_add(chunk as u64, std::sync::atomic::Ordering::Relaxed)
                        + chunk as u64;
                    let _ = sender.try_send((count, total));
                })
            });
            while let Ok((received, total)) = receiver.recv().await {
                if owner
                    .update(cx, |this, cx| {
                        this.updates.phase = updates::Phase::Downloading { received, total };
                        cx.notify();
                    })
                    .is_err()
                {
                    return;
                }
            }
            let bytes = match download.await {
                Ok(bytes) => bytes,
                Err(error) => {
                    let _ = owner.update(cx, |this, cx| {
                        this.updates.phase = updates::Phase::Error(error);
                        cx.notify();
                    });
                    return;
                }
            };
            loop {
                let waiting = owner.update(cx, |this, cx| {
                    let waiting = this.projects.iter().any(|p| p.mutating);
                    this.updates.phase = if waiting {
                        updates::Phase::WaitingForGit
                    } else {
                        updates::Phase::Installing
                    };
                    if !waiting {
                        this.persist(cx);
                    }
                    cx.notify();
                    waiting
                });
                match waiting {
                    Ok(false) => break,
                    Ok(true) => {
                        cx.background_executor()
                            .timer(Duration::from_millis(50))
                            .await
                    }
                    Err(_) => return,
                }
            }
            let can_install = owner.update(cx, |this, cx| {
                if this.state_error.is_some() {
                    this.updates.phase = updates::Phase::Error("Couldn't save projects before restarting. Resolve the preference error and retry.".into());
                    cx.notify();
                    false
                } else { true }
            }).unwrap_or(false);
            if !can_install { return; }
            let result = cx
                .background_spawn(async move { release.package.install(bytes) })
                .await;
            let _ = owner.update(cx, |this, cx| match result {
                Ok(()) => {
                    this.persist(cx);
                    cx.restart();
                }
                Err(error) => {
                    this.updates.phase = updates::Phase::Error(error);
                    cx.notify();
                }
            });
        }));
        cx.notify();
    }
    fn render_update_notice(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        use updates::Phase;
        let (message, button) = match &self.updates.phase {
            Phase::Idle => return None,
            Phase::Checking if !self.updates.show_check => return None,
            Phase::Checking => ("Checking for updates…".into(), None),
            Phase::Current => (
                "You're up to date. No newer published release is available.".into(),
                None,
            ),
            Phase::Available(version) => (
                format!("Diffy {version} is available."),
                Some("Update and Restart"),
            ),
            Phase::Downloading { received, total } => (
                match total {
                    Some(total) if *total > 0 => format!(
                        "Downloading update… {}%",
                        (received.saturating_mul(100) / total).min(100)
                    ),
                    _ => format!("Downloading update… {} KB", received / 1024),
                },
                None,
            ),
            Phase::WaitingForGit => ("Waiting for Git before restarting…".into(), None),
            Phase::Installing => ("Installing update…".into(), None),
            Phase::Error(error) => (
                error.clone(),
                Some(if self.updates.release.is_some() {
                    "Retry Update"
                } else {
                    "Retry Check"
                }),
            ),
        };
        Some(
            h_flex()
                .px_3()
                .py_2()
                .gap_3()
                .bg(rgb(0))
                .text_size(px(12.))
                .child(div().flex_1().min_w_0().child(message))
                .children(button.map(|label| {
                    Button::new("update-action")
                        .label(label)
                        .small()
                        .ghost()
                        .on_click(cx.listener(|this, _, _, cx| {
                            if this.updates.release.is_some() {
                                this.install_update(cx);
                            } else {
                                this.check_updates(true, cx);
                            }
                        }))
                }))
                .when(!self.updates.busy(), |row| {
                    row.child(
                        Button::new("dismiss-update")
                            .label("Dismiss")
                            .small()
                            .ghost()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.updates.dismiss();
                                cx.notify();
                            })),
                    )
                })
                .into_any_element(),
        )
    }
    pub fn selected_file(&self) -> Option<&SharedString> {
        self.projects
            .get(self.active)
            .and_then(|p| p.view().selected.as_ref())
    }
    pub fn selected_group(&self) -> ChangeGroup {
        self.projects
            .get(self.active)
            .map_or(ChangeGroup::Changes, |p| p.group)
    }
    pub fn is_busy(&self) -> bool {
        self.projects.get(self.active).is_some_and(|p| p.loading)
    }
    fn push_project(&mut self, mut project: Project, cx: &mut Context<Self>) {
        project.id = self.next_id;
        self.next_id += 1;
        for view in &project.views {
            project
                .subscriptions
                .push(cx.observe(&view.diff, |_, _, cx| cx.notify()));
        }
        self.projects.push(project);
    }
    fn add_demo(&mut self, demo: demo::Demo, cx: &mut Context<Self>) {
        let mut views = [ReviewView::new(cx), ReviewView::new(cx)];
        views[0].install(GroupSnapshot::parse(demo.staged).expect("valid demo"), cx);
        views[1].install(GroupSnapshot::parse(demo.changes).expect("valid demo"), cx);
        self.push_project(
            Project {
                watcher: None,
                watch_task: None,
                pending_refresh: false,
                id: 0,
                subscriptions: Vec::new(),
                name: demo.name.into(),
                custom_name: None,
                icon: None,
                color: demo.color,
                path: None,
                branch: "Demo".into(),
                views,
                group: ChangeGroup::Changes,
                loading: false,
                mutating: false,
                error: None,
                generation: 0,
                task: None,
            },
            cx,
        );
    }
    fn add_path(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let path = std::fs::canonicalize(&path).unwrap_or(path);
        if let Some(ix) = self
            .projects
            .iter()
            .position(|p| p.path.as_ref() == Some(&path))
        {
            self.active = ix;
            self.persist(cx);
            cx.notify();
            return;
        }
        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Project".into());
        let views = [ReviewView::new(cx), ReviewView::new(cx)];
        let color = [0x74d9b5, 0xff8175, 0xb9a0e8, 0x82b6e5][self.projects.len() % 4];
        self.push_project(
            Project {
                watcher: None,
                watch_task: None,
                pending_refresh: false,
                id: 0,
                subscriptions: Vec::new(),
                name: name.into(),
                custom_name: None,
                icon: None,
                color,
                path: Some(path),
                branch: "…".into(),
                views,
                group: ChangeGroup::Changes,
                loading: false,
                mutating: false,
                error: None,
                generation: 0,
                task: None,
            },
            cx,
        );
        self.active = self.projects.len() - 1;
        self.refresh(cx);
        self.persist(cx);
    }
    fn refresh(&mut self, cx: &mut Context<Self>) {
        if let Some(p) = self.projects.get_mut(self.active)
            && p.loading
        {
            p.pending_refresh = true;
            return;
        }
        self.load(None, cx);
    }
    fn stage_file(&mut self, cx: &mut Context<Self>) {
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
    fn stage_all(&mut self, cx: &mut Context<Self>) {
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
    fn stop_file_repeat(&mut self) {
        self.repeat_direction = None;
        self.file_repeat = None;
    }
    fn start_file_repeat(&mut self, direction: isize, window: &mut Window, cx: &mut Context<Self>) {
        if self.repeat_direction == Some(direction) && (self.file_key_probe)(direction) {
            return; // The timer owns repeats; ignore native autorepeat duplicates.
        }
        self.stop_file_repeat();
        self.move_file(direction, cx); // Every new press moves immediately.
        let project = self.active_project_id();
        let group = self.selected_group();
        let require_active = window.is_window_active();
        self.repeat_direction = Some(direction);
        self.file_repeat = Some(cx.spawn_in(window, async move |this, cx| {
            let mut elapsed = 0;
            let mut next_step = 320;
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(16))
                    .await;
                elapsed += 16;
                let keep = cx
                    .update(|window, cx| {
                        this.update(cx, |this, cx| {
                            if this.repeat_direction != Some(direction)
                                || !(this.file_key_probe)(direction)
                                || this.settings.is_some()
                                || this.palette_open
                                || this.active_project_id() != project
                                || this.selected_group() != group
                                || (require_active && !window.is_window_active())
                            {
                                this.repeat_direction = None;
                                return false;
                            }
                            if elapsed >= next_step {
                                this.move_file(direction, cx);
                                next_step += 200;
                            }
                            true
                        })
                    })
                    .ok()
                    .and_then(Result::ok)
                    .unwrap_or(false);
                if !keep {
                    return;
                }
            }
        }));
    }
    #[cfg(feature = "ui-tests")]
    pub fn set_update_backend(&mut self, backend: Arc<dyn updates::Backend>) {
        self.updates.backend = backend;
    }
    #[cfg(feature = "ui-tests")]
    pub fn update_phase(&self) -> &updates::Phase {
        &self.updates.phase
    }
    #[cfg(feature = "ui-tests")]
    pub fn set_file_key_probe(&mut self, probe: fn(isize) -> bool) {
        self.file_key_probe = probe;
    }
    #[cfg(feature = "ui-tests")]
    pub fn project_color_state(&self) -> Option<Entity<ColorPickerState>> {
        self.settings.as_ref().map(|s| s.color.clone())
    }
    fn persist_navigation(&mut self, cx: &mut Context<Self>) {
        self.navigation_save = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(250))
                .await;
            let _ = this.update(cx, |this, cx| this.persist(cx));
        }));
    }
    fn open_settings(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        if !self.projects.iter().any(|p| p.id == id) {
            return;
        }
        self.stop_file_repeat();
        let p = self.projects.iter().find(|p| p.id == id).unwrap();
        let color = p.color;
        let name = p.name.clone();
        let icon = p.icon.clone();
        let emoji = match &icon {
            Some(state::ProjectIcon::Emoji(value)) => value.clone(),
            _ => String::new(),
        };
        self.previous_focus = window.focused(cx);
        self.palette_open = false;
        let name = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(name)
                .placeholder("Project name")
        });
        let emoji = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(emoji)
                .placeholder("Emoji or symbol")
        });
        name.read(cx).focus_handle(cx).focus(window, cx);
        self.settings = Some(ProjectSettings {
            id,
            focus: cx.focus_handle(),
            name,
            emoji,
            color: cx.new(|cx| ColorPickerState::new(window, cx).default_value(rgb(color))),
            icon,
            error: None,
        });
        cx.notify();
    }
    fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings = None;
        self.previous_focus
            .take()
            .unwrap_or_else(|| self.focus.clone())
            .focus(window, cx);
        cx.notify();
    }
    fn save_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(draft) = &self.settings else {
            return;
        };
        let name = draft.name.read(cx).value().trim().to_owned();
        if name.is_empty() {
            self.settings.as_mut().unwrap().error = Some("Enter a project name.".into());
            cx.notify();
            return;
        }
        let icon = if matches!(draft.icon, Some(state::ProjectIcon::Image(_))) {
            draft.icon.clone()
        } else {
            let emoji = draft.emoji.read(cx).value().trim().to_owned();
            (!emoji.is_empty()).then_some(state::ProjectIcon::Emoji(emoji))
        };
        if let Some(p) = self.projects.iter_mut().find(|p| p.id == draft.id) {
            p.name = name.clone().into();
            p.custom_name = Some(name);
            if let Some(color) = draft.color.read(cx).value() {
                let rgb = color.to_rgb();
                p.color = ((rgb.r * 255.).round() as u32) << 16
                    | ((rgb.g * 255.).round() as u32) << 8
                    | (rgb.b * 255.).round() as u32;
            }
            p.icon = icon;
        }
        self.persist(cx);
        self.close_settings(window, cx);
    }
    fn choose_project_image(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.settings.as_ref().map(|s| s.id) else {
            return;
        };
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose project icon".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = paths.await
                && let Some(path) = paths.into_iter().next()
            {
                let _ = cx.update(|_, cx| {
                    this.update(cx, |this, cx| {
                        if let Some(draft) = &mut this.settings
                            && draft.id == id
                        {
                            let supported =
                                path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
                                    gpui_kit::Img::extensions()
                                        .contains(&e.to_ascii_lowercase().as_str())
                                });
                            if supported && path.is_file() {
                                draft.icon = Some(state::ProjectIcon::Image(path));
                                draft.error = None;
                            } else {
                                draft.error =
                                    Some("Choose an image such as PNG, JPEG, WebP, or SVG.".into());
                            }
                            cx.notify();
                        }
                    })
                });
            }
        })
        .detach();
    }
    fn render_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let draft = self.settings.as_ref().unwrap();
        let id = draft.id;
        let remove_disabled = self
            .projects
            .iter()
            .find(|p| p.id == id)
            .is_none_or(|p| p.mutating);
        let image = matches!(draft.icon, Some(state::ProjectIcon::Image(_)));
        let image_path = match &draft.icon {
            Some(state::ProjectIcon::Image(p)) => Some(p.clone()),
            _ => None,
        };
        div()
            .id("project-settings-overlay")
            .track_focus(&draft.focus)
            .absolute()
            .size_full()
            .occlude()
            .child(
                div()
                    .id("settings-backdrop")
                    .test_support()
                    .absolute()
                    .size_full()
                    .bg(rgb(0).opacity(0.85))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, w, cx| this.close_settings(w, cx)),
                    ),
            )
            .child(
                h_flex()
                    .absolute()
                    .top(px(100.))
                    .w_full()
                    .justify_center()
                    .child(
                        v_flex()
                            .id("project-settings")
                            .test_support()
                            .w(px(420.))
                            .p_5()
                            .gap_4()
                            .rounded(px(12.))
                            .bg(rgb(0x090909))
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .child(
                                div()
                                    .font_medium()
                                    .text_size(px(14.))
                                    .child("Project settings"),
                            )
                            .child(
                                v_flex()
                                    .gap_2()
                                    .child("Name")
                                    .child(Input::new(&draft.name).id("project-name").w_full()),
                            )
                            .child(
                                h_flex().justify_between().child("Color").child(
                                    div().id("project-color").test_support().child(
                                        ColorPicker::new(&draft.color)
                                            .label("Project color")
                                            .featured_colors(vec![
                                                rgb(0x74d9b5).into(),
                                                rgb(0xff8175).into(),
                                                rgb(0xb9a0e8).into(),
                                                rgb(0x82b6e5).into(),
                                                rgb(0xffd65a).into(),
                                            ]),
                                    ),
                                ),
                            )
                            .child(
                                v_flex()
                                    .gap_2()
                                    .child("Icon")
                                    .child(
                                        h_flex()
                                            .gap_2()
                                            .child(
                                                Button::new("icon-emoji")
                                                    .ghost()
                                                    .label("Emoji")
                                                    .selected(!image)
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        if let Some(s) = &mut this.settings {
                                                            s.icon = None;
                                                        }
                                                        cx.notify();
                                                    })),
                                            )
                                            .child(
                                                Button::new("icon-image")
                                                    .ghost()
                                                    .label("Choose image…")
                                                    .selected(image)
                                                    .on_click(cx.listener(|this, _, w, cx| {
                                                        this.choose_project_image(w, cx)
                                                    })),
                                            ),
                                    )
                                    .when(!image, |box_| {
                                        box_.child(
                                            Input::new(&draft.emoji).id("project-emoji").w_full(),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(12.))
                                                .text_color(cx.theme().muted_foreground)
                                                .child("Use ⌃⌘Space for the macOS emoji picker."),
                                        )
                                    })
                                    .when_some(image_path, |box_, path| {
                                        box_.child(
                                            h_flex()
                                                .gap_3()
                                                .child(
                                                    div()
                                                        .size(px(48.))
                                                        .rounded(px(12.))
                                                        .overflow_hidden()
                                                        .child(
                                                            img(path.clone())
                                                                .size_full()
                                                                .rounded(px(12.))
                                                                .object_fit(ObjectFit::Cover),
                                                        ),
                                                )
                                                .child(
                                                    div().flex_1().min_w_0().truncate().child(
                                                        path.file_name()
                                                            .unwrap_or_default()
                                                            .to_string_lossy()
                                                            .to_string(),
                                                    ),
                                                ),
                                        )
                                    }),
                            )
                            .children(draft.error.as_ref().map(|e| {
                                div()
                                    .text_color(cx.theme().danger)
                                    .text_size(px(12.))
                                    .child(e.clone())
                            }))
                            .child(
                                h_flex()
                                    .justify_between()
                                    .gap_2()
                                    .child(
                                        Button::new("settings-remove")
                                            .ghost()
                                            .label("Remove project")
                                            .text_color(cx.theme().danger)
                                            .disabled(remove_disabled)
                                            .on_click(cx.listener(move |this, _, w, cx| {
                                                if this
                                                    .projects
                                                    .iter()
                                                    .any(|p| p.id == id && p.mutating)
                                                {
                                                    return;
                                                }
                                                this.close_settings(w, cx);
                                                this.remove_project(id, w, cx);
                                            })),
                                    )
                                    .child(
                                        h_flex()
                                            .gap_2()
                                            .child(
                                                Button::new("settings-cancel")
                                                    .ghost()
                                                    .label("Cancel")
                                                    .on_click(cx.listener(|this, _, w, cx| {
                                                        this.close_settings(w, cx)
                                                    })),
                                            )
                                            .child(
                                                Button::new("settings-save")
                                                    .label("Save")
                                                    .on_click(cx.listener(|this, _, w, cx| {
                                                        this.save_settings(w, cx)
                                                    })),
                                            ),
                                    ),
                            ),
                    ),
            )
            .focus_trap("project-settings-trap", &draft.focus)
            .into_any_element()
    }
    fn load(&mut self, mutation: Option<(Vec<DiffFile>, bool)>, cx: &mut Context<Self>) {
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
        p.generation += 1;
        let generation = p.generation;
        p.loading = true;
        p.mutating = mutation.is_some();
        p.error = None;
        p.task = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let mutation_error = mutation.and_then(|(file, stage)| {
                        repository::set_files_staged(&path, &file, stage).err()
                    });
                    (repository::load(&path), mutation_error)
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
                        p.views[0].install(snapshot.staged, cx);
                        p.views[1].install(snapshot.changes, cx);
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
        cx.notify();
    }
    fn refresh_project(&mut self, id: u64, cx: &mut Context<Self>) {
        if let Some(ix) = self.projects.iter().position(|p| p.id == id) {
            let active = self.active;
            self.active = ix;
            self.refresh(cx);
            self.active = active;
        }
    }
    fn start_watching(
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
                cx.background_executor()
                    .timer(Duration::from_millis(150))
                    .await;
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
    fn open_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: true,
            prompt: Some("Open Git repository".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = paths.await {
                let _ = cx.update(|window, cx| {
                    this.update(cx, |this, cx| {
                        for path in paths {
                            this.add_path(path, cx);
                        }
                        this.focus.focus(window, cx);
                    })
                });
            }
        })
        .detach();
    }
    fn switch_project(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if ix < self.projects.len() {
            self.active = ix;
            self.focus.focus(window, cx);
            self.persist(cx);
            cx.notify();
        }
    }
    fn set_group(&mut self, group: ChangeGroup, window: &mut Window, cx: &mut Context<Self>) {
        let Some(p) = self.projects.get_mut(self.active) else {
            return;
        };
        p.group = group;
        self.focus.focus(window, cx);
        self.persist(cx);
        cx.notify();
    }
    fn select_file(&mut self, path: SharedString, window: &mut Window, cx: &mut Context<Self>) {
        let Some(p) = self.projects.get_mut(self.active) else {
            return;
        };
        let view = p.view_mut();
        view.select(path, cx);
        view.focus.focus(window, cx);
        self.persist(cx);
        cx.notify();
    }
    fn move_file(&mut self, direction: isize, cx: &mut Context<Self>) {
        let Some(p) = self.projects.get_mut(self.active) else {
            return;
        };
        let view = p.view_mut();
        if view.files.is_empty() {
            return;
        }
        let current = view
            .files
            .iter()
            .position(|f| Some(f.path()) == view.selected.as_ref())
            .unwrap_or(0);
        let ix = current
            .saturating_add_signed(direction)
            .min(view.files.len() - 1);
        if ix == current {
            return;
        }
        view.select(view.files[ix].path().clone(), cx);
        self.persist_navigation(cx);
        cx.notify();
    }
    fn set_mode(&mut self, mode: DiffMode, cx: &mut Context<Self>) {
        let Some(p) = self.projects.get(self.active) else {
            return;
        };
        for view in &p.views {
            view.diff.update(cx, |d, cx| d.set_mode(mode, cx));
        }
        self.persist(cx);
    }
    fn change(&mut self, next: bool, cx: &mut Context<Self>) {
        let Some(p) = self.projects.get(self.active) else {
            return;
        };
        p.view().diff.update(cx, |d, cx| {
            if next {
                d.next_change(cx)
            } else {
                d.previous_change(cx)
            }
        });
    }
    pub fn selected_additions(&self) -> usize {
        self.projects
            .get(self.active)
            .and_then(|p| p.view().selected_file())
            .map_or(0, |f| f.additions())
    }
    pub fn live_ready(&self) -> bool {
        self.projects
            .get(self.active)
            .is_some_and(|p| p.watcher.is_some())
    }
    pub fn selected_mode(&self, cx: &App) -> DiffMode {
        self.projects
            .get(self.active)
            .map_or(DiffMode::Unified, |p| p.view().diff.read(cx).mode())
    }
    pub fn project_count(&self) -> usize {
        self.projects.len()
    }
    pub fn active_project_id(&self) -> Option<u64> {
        self.projects.get(self.active).map(|p| p.id)
    }
    pub fn palette_query(&self, cx: &App) -> String {
        self.command.read(cx).query(cx).to_string()
    }
    pub fn palette_is_open(&self) -> bool {
        self.palette_open
    }
    fn persist(&mut self, cx: &App) {
        if self.restoring {
            return;
        }
        let Some(path) = &self.state_path else {
            return;
        };
        let state = state::SavedState {
            projects: self
                .projects
                .iter()
                .filter_map(|p| {
                    p.path.as_ref().map(|path| state::SavedProject {
                        path: path.clone(),
                        color: p.color,
                        name: p.custom_name.clone(),
                        icon: p.icon.clone(),
                        staged: p.group == ChangeGroup::Staged,
                        split: p.view().diff.read(cx).mode() == DiffMode::Split,
                        selected: std::array::from_fn(|ix| {
                            p.views[ix].selected.as_ref().map(ToString::to_string)
                        }),
                    })
                })
                .collect(),
            active: self.projects.get(self.active).and_then(|p| p.path.clone()),
            ..Default::default()
        };
        if self.persisted_state.as_ref() == Some(&state) && self.state_error.is_none() {
            return;
        }
        self.state_error = state::save(path, &state).err();
        if self.state_error.is_none() {
            self.persisted_state = Some(state);
        }
    }
    fn stage_path(
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
    fn remove_project(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ix) = self.projects.iter().position(|p| p.id == id) else {
            return;
        };
        if self.projects[ix].mutating {
            return;
        }
        self.projects.remove(ix);
        if self.active > ix {
            self.active -= 1;
        }
        self.active = self.active.min(self.projects.len().saturating_sub(1));
        self.hovered_project = None;
        self.focus.focus(window, cx);
        self.persist(cx);
        cx.notify();
    }
    fn move_project(&mut self, direction: isize, window: &mut Window, cx: &mut Context<Self>) {
        if self.projects.is_empty() {
            return;
        }
        let ix =
            (self.active as isize + direction).rem_euclid(self.projects.len() as isize) as usize;
        self.switch_project(ix, window, cx);
    }
    fn focus_diff(&self, window: &mut Window, cx: &mut App) {
        if let Some(p) = self.projects.get(self.active) {
            p.view().diff.read(cx).focus_handle(cx).focus(window, cx);
        }
    }
    fn open_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings.is_some() {
            return;
        }
        if self.palette_open {
            self.close_palette(true, window, cx);
            return;
        }
        self.previous_focus = window.focused(cx);
        self.palette_open = true;
        self.command.update(cx, |state, cx| {
            state.set_query("", window, cx);
            state.focus(window, cx);
        });
        cx.notify();
    }
    fn close_palette(&mut self, cancel: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.palette_open = false;
        if cancel
            || self
                .command
                .read(cx)
                .focus_handle(cx)
                .contains_focused(window, cx)
        {
            self.previous_focus
                .take()
                .unwrap_or_else(|| self.focus.clone())
                .focus(window, cx);
        } else {
            self.previous_focus = None;
        }
        cx.notify();
    }
    fn render_palette(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = self.projects.get(self.active);
        let has_files = p.is_some_and(|p| !p.view().files.is_empty());
        let mutable = p.is_some_and(|p| p.path.is_some() && !p.loading && has_files);
        let projects = CommandGroup::new()
            .label("Projects")
            .items(self.projects.iter().map(|p| {
                CommandItem::new()
                    .label(p.name.clone())
                    .keywords(p.path.iter().map(|p| p.display().to_string()))
                    .checked(self.active_project_id() == Some(p.id))
                    .action(Box::new(SelectProject { id: p.id }))
            }));
        let item = |label: &str, action: Box<dyn Action>, disabled: bool| {
            CommandItem::new()
                .label(label.to_owned())
                .action(action)
                .disabled(disabled)
        };
        let commands = CommandGroup::new().label("Commands").items([
            item("Open repository", Box::new(OpenProject), false),
            item(
                "Refresh repository",
                Box::new(Refresh),
                !p.is_some_and(|p| p.path.is_some() && !p.loading),
            ),
            item(
                "Next project",
                Box::new(NextProject),
                self.projects.len() < 2,
            ),
            item(
                "Previous project",
                Box::new(PreviousProject),
                self.projects.len() < 2,
            ),
            item("Next file", Box::new(NextFile), !has_files),
            item("Previous file", Box::new(PreviousFile), !has_files),
            item("Next change", Box::new(NextChange), !has_files),
            item("Previous change", Box::new(PreviousChange), !has_files),
            item(
                "Switch Staged / Changes",
                Box::new(ToggleGroup),
                p.is_none(),
            ),
            item("Unified diff", Box::new(UnifiedMode), p.is_none()),
            item("Split diff", Box::new(SplitMode), p.is_none()),
            item(
                if self.selected_group() == ChangeGroup::Changes {
                    "Stage selected file"
                } else {
                    "Unstage selected file"
                },
                Box::new(StageFile),
                !mutable,
            ),
            item(
                if self.selected_group() == ChangeGroup::Changes {
                    "Stage all files"
                } else {
                    "Unstage all files"
                },
                Box::new(StageAll),
                !mutable,
            ),
            item("Focus diff", Box::new(FocusDiff), !has_files),
            item(
                "Remove from Diffy",
                Box::new(RemoveCurrentProject),
                !p.is_some_and(|p| p.path.is_some() && !p.mutating),
            ),
            item(
                "Check for updates",
                Box::new(CheckForUpdates),
                self.updates.busy(),
            ),
            item(
                "Update and restart",
                Box::new(InstallUpdate),
                self.updates.release.is_none() || self.updates.busy(),
            ),
            item("About Diffy", Box::new(About), false),
            item("Quit Diffy", Box::new(Quit), false),
        ]);
        let confirm = cx.entity().downgrade();
        let cancel = confirm.clone();
        div()
            .id("palette-overlay")
            .absolute()
            .size_full()
            .occlude()
            .child(
                div()
                    .id("palette-backdrop")
                    .test_support()
                    .absolute()
                    .size_full()
                    .bg(rgb(0).opacity(0.78))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| this.close_palette(true, window, cx)),
                    ),
            )
            .child(
                h_flex()
                    .absolute()
                    .top(px(70.))
                    .w_full()
                    .justify_center()
                    .child(
                        div()
                            .id("command-palette")
                            .test_support()
                            .w(px(520.))
                            .bg(rgb(0))
                            .rounded(px(10.))
                            .overflow_hidden()
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .child(
                                Command::new(&self.command)
                                    .bordered(false)
                                    .bg(rgb(0))
                                    .w_full()
                                    .max_h(px(360.))
                                    .placeholder("Search projects and commands…")
                                    .group(projects)
                                    .group(commands)
                                    .on_confirm(move |_, window, cx| {
                                        let owner = confirm.clone();
                                        window.defer(cx, move |window, cx| {
                                            let _ = owner.update(cx, |this, cx| {
                                                this.close_palette(false, window, cx)
                                            });
                                        });
                                    })
                                    .on_cancel(move |window, cx| {
                                        let _ = cancel.update(cx, |this, cx| {
                                            this.close_palette(true, window, cx)
                                        });
                                    })
                                    .footer(|_, _, cx| {
                                        h_flex()
                                            .px_3()
                                            .py_2()
                                            .gap_4()
                                            .text_size(px(12.))
                                            .text_color(cx.theme().muted_foreground)
                                            .child("↑↓ Navigate")
                                            .child("Enter Select")
                                            .child("Esc Close")
                                    }),
                            ),
                    ),
            )
            .into_any_element()
    }
    fn render_projects(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let icons = self
            .projects
            .iter()
            .enumerate()
            .map(|(ix, project)| {
                let active = ix == self.active;
                let project_id = project.id;
                let fallback_initial = project
                    .name
                    .chars()
                    .next()
                    .unwrap_or('D')
                    .to_uppercase()
                    .to_string();
                let policy =
                    Transition::new(Duration::from_millis(160)).ease(|t| 1. - (1. - t).powi(4));
                let id = ElementId::from(("project", ix));
                let marker_opacity = transition(
                    (id.clone(), "marker"),
                    if active { 1f32 } else { 0. },
                    policy.clone(),
                    window,
                    cx,
                );
                let animated_diameter = transition(
                    (id, "hover"),
                    if self.hovered_project == Some(ix) {
                        46f32
                    } else {
                        42.
                    },
                    policy,
                    window,
                    cx,
                );
                let diameter = if matches!(project.icon, Some(state::ProjectIcon::Image(_))) {
                    42.
                } else {
                    animated_diameter
                };
                h_flex()
                    .id(("project-row", ix))
                    .relative()
                    .w_full()
                    .h(px(52.))
                    .justify_center()
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        let next = if *hovered {
                            Some(ix)
                        } else if this.hovered_project == Some(ix) {
                            None
                        } else {
                            this.hovered_project
                        };
                        if next != this.hovered_project {
                            this.hovered_project = next;
                            cx.notify();
                        }
                    }))
                    .child(
                        div()
                            .absolute()
                            .left_0()
                            .w(px(3.))
                            .h(px(24.))
                            .opacity(marker_opacity)
                            .rounded(px(2.))
                            .bg(cx.theme().primary),
                    )
                    .child(
                        Button::new(("project", ix))
                            .ghost()
                            .p_0()
                            .border_0()
                            .w(px(48.))
                            .h(px(48.))
                            .rounded(px(12.))
                            .child(
                                div()
                                    .size_full()
                                    .relative()
                                    .child(
                                        div()
                                            .absolute()
                                            .left(px((48. - diameter) / 2.))
                                            .top(px((48. - diameter) / 2.))
                                            .w(px(diameter))
                                            .h(px(diameter))
                                            .rounded(px(12.))
                                            .bg(rgb(project.color)),
                                    )
                                    .text_color(rgb(0x141618))
                                    .text_size(px(17.))
                                    .font_medium()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(match &project.icon {
                                        Some(state::ProjectIcon::Image(path)) => div()
                                            .size(px(diameter))
                                            .rounded(px(12.))
                                            .overflow_hidden()
                                            .child(
                                                img(path.clone())
                                                    .size_full()
                                                    .rounded(px(12.))
                                                    .object_fit(ObjectFit::Cover)
                                                    .with_fallback(move || {
                                                        div()
                                                            .size_full()
                                                            .flex()
                                                            .items_center()
                                                            .justify_center()
                                                            .child(fallback_initial.clone())
                                                            .into_any_element()
                                                    }),
                                            )
                                            .into_any_element(),
                                        Some(state::ProjectIcon::Emoji(emoji)) => div()
                                            .text_size(px(24.))
                                            .child(emoji.clone())
                                            .into_any_element(),
                                        None => div()
                                            .child(
                                                project
                                                    .name
                                                    .chars()
                                                    .next()
                                                    .unwrap_or('D')
                                                    .to_uppercase()
                                                    .to_string(),
                                            )
                                            .into_any_element(),
                                    }),
                            )
                            .accessibility_label(project.name.clone())
                            .tooltip(project.name.clone())
                            .tooltip_placement(Placement::Right)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.switch_project(ix, window, cx)
                            })),
                    )
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, _, w, cx| {
                            cx.stop_propagation();
                            this.open_settings(project_id, w, cx);
                        }),
                    )
                    .into_any_element()
            })
            .collect::<Vec<_>>();
        v_flex()
            .w(px(76.))
            .h_full()
            .flex_none()
            .min_h_0()
            .bg(cx.theme().sidebar)
            .child(
                div()
                    .id("window-drag-area")
                    .w_full()
                    .h(px(42.))
                    .flex_none()
                    .on_mouse_down(MouseButton::Left, |_, window, _| window.start_window_move())
                    .on_double_click(|_, window, _| window.titlebar_double_click()),
            )
            .child(
                v_flex()
                    .id("projects")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .pt_2()
                    .gap_2()
                    .children(icons)
                    .child(
                        h_flex().h(px(56.)).justify_center().child(
                            Button::new("open-project")
                                .ghost()
                                .w(px(42.))
                                .h(px(42.))
                                .rounded(px(21.))
                                .icon(IconName::Plus)
                                .tooltip("Open repository · ⌘O")
                                .tooltip_placement(Placement::Right)
                                .on_click(
                                    cx.listener(|this, _, window, cx| {
                                        this.open_project(window, cx)
                                    }),
                                ),
                        ),
                    ),
            )
            .into_any_element()
    }

    fn render_center(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(p) = self.projects.get(self.active) else {
            return v_flex()
                .size_full()
                .child(
                    div()
                        .id("toolbar-drag-area")
                        .w_full()
                        .h(px(46.))
                        .flex_none()
                        .on_mouse_down(MouseButton::Left, |_, w, _| w.start_window_move())
                        .on_double_click(|_, w, _| w.titlebar_double_click()),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .items_center()
                        .justify_center()
                        .gap_3()
                        .child(
                            Button::new("open-empty")
                                .ghost()
                                .label("Open repository")
                                .icon(IconName::Plus)
                                .on_click(cx.listener(|this, _, w, cx| this.open_project(w, cx))),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(cx.theme().muted_foreground)
                                .child("⌘O  Open · ⌘P  Commands"),
                        ),
                )
                .into_any_element();
        };
        let view = p.view();
        let mode = view.diff.read(cx).mode();
        let empty = view.files.is_empty();
        let scope_buttons = [ChangeGroup::Staged, ChangeGroup::Changes].map(|group| {
            Button::new(if group == ChangeGroup::Staged {
                "staged"
            } else {
                "changes"
            })
            .ghost()
            .small()
            .label(format!(
                "{}  {}",
                group.label(),
                p.views[group.index()].files.len()
            ))
            .selected(p.group == group)
            .toggled(p.group == group)
            .tooltip("Switch view · ⌘⇧S")
            .on_click(cx.listener(move |this, _, window, cx| this.set_group(group, window, cx)))
        });
        v_flex()
            .size_full()
            .min_w_0()
            .min_h_0()
            .child(
                h_flex()
                    .h(px(46.))
                    .flex_none()
                    .px_3()
                    .gap_1()
                    .children(scope_buttons)
                    .child(
                        div()
                            .id("toolbar-drag-area")
                            .flex_1()
                            .h_full()
                            .on_mouse_down(MouseButton::Left, |_, w, _| w.start_window_move())
                            .on_double_click(|_, w, _| w.titlebar_double_click()),
                    )
                    .child(
                        Button::new("previous-change")
                            .ghost()
                            .xsmall()
                            .icon(IconName::ChevronUp)
                            .disabled(empty)
                            .tooltip("Previous change · ⌥↑")
                            .on_click(cx.listener(|this, _, _, cx| this.change(false, cx))),
                    )
                    .child(
                        Button::new("next-change")
                            .ghost()
                            .xsmall()
                            .icon(IconName::ChevronDown)
                            .disabled(empty)
                            .tooltip("Next change · ⌥↓")
                            .on_click(cx.listener(|this, _, _, cx| this.change(true, cx))),
                    )
                    .child(
                        Button::new("unified")
                            .ghost()
                            .small()
                            .label("Unified")
                            .selected(mode == DiffMode::Unified)
                            .toggled(mode == DiffMode::Unified)
                            .on_click(
                                cx.listener(|this, _, _, cx| this.set_mode(DiffMode::Unified, cx)),
                            ),
                    )
                    .child(
                        Button::new("split")
                            .ghost()
                            .small()
                            .label("Split")
                            .selected(mode == DiffMode::Split)
                            .toggled(mode == DiffMode::Split)
                            .on_click(
                                cx.listener(|this, _, _, cx| this.set_mode(DiffMode::Split, cx)),
                            ),
                    )
                    .child(
                        Button::new("refresh")
                            .ghost()
                            .small()
                            .icon(IconName::RotateCw)
                            .disabled(p.path.is_none() || p.loading)
                            .tooltip("Refresh · ⌘R")
                            .on_click(cx.listener(|this, _, _, cx| this.refresh(cx))),
                    ),
            )
            .child(
                h_flex()
                    .id("preview-header")
                    .test_support()
                    .group("preview-header")
                    .h(px(34.))
                    .flex_none()
                    .px_4()
                    .gap_3()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(13.))
                            .child(view.selected.clone().unwrap_or_default()),
                    )
                    .children(view.selected_file().map(|f| {
                        h_flex()
                            .gap_2()
                            .text_size(px(12.))
                            .child(
                                div()
                                    .text_color(cx.theme().success)
                                    .child(format!("+{}", f.additions())),
                            )
                            .child(
                                div()
                                    .text_color(cx.theme().danger)
                                    .child(format!("−{}", f.deletions())),
                            )
                    }))
                    .child(
                        Button::new("stage-file")
                            .ghost()
                            .xsmall()
                            .icon(if p.group == ChangeGroup::Changes {
                                IconName::Plus
                            } else {
                                IconName::Minus
                            })
                            .accessibility_label(if p.group == ChangeGroup::Changes {
                                "Stage file"
                            } else {
                                "Unstage file"
                            })
                            .when(
                                !view
                                    .diff
                                    .read(cx)
                                    .focus_handle(cx)
                                    .contains_focused(window, cx),
                                |button| {
                                    button
                                        .invisible()
                                        .group_hover("preview-header", |button| button.visible())
                                },
                            )
                            .disabled(empty || p.path.is_none() || p.loading)
                            .tooltip(if p.path.is_none() {
                                "Open a repository to stage files"
                            } else {
                                "Stage / unstage file · ⌘S"
                            })
                            .on_click(cx.listener(|this, _, _, cx| this.stage_file(cx))),
                    ),
            )
            .children(p.error.as_ref().map(|error| {
                h_flex()
                    .px_4()
                    .py_2()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(13.))
                            .text_color(cx.theme().danger)
                            .child(error.clone()),
                    )
                    .child(
                        Button::new("retry")
                            .ghost()
                            .small()
                            .label("Retry")
                            .disabled(p.loading)
                            .on_click(cx.listener(|this, _, _, cx| this.refresh(cx))),
                    )
            }))
            .child(if empty {
                v_flex()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .text_color(cx.theme().muted_foreground)
                    .child(if p.loading {
                        "Reading changes…"
                    } else if p.group == ChangeGroup::Staged {
                        "No staged files"
                    } else {
                        "No changes"
                    })
                    .into_any_element()
            } else {
                div()
                    .id("diff-viewport")
                    .relative()
                    .overflow_hidden()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .child(
                        Diff::new(&view.diff)
                            .absolute()
                            .size_full()
                            .border_0()
                            .header_visible(false)
                            .soft_wrap(true)
                            .split_ratio(0.4)
                            .hunk_separator(DiffHunkSeparator::Simple),
                    )
                    .into_any_element()
            })
            .into_any_element()
    }
    fn render_files(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(p) = self.projects.get(self.active) else {
            return div().size_full().bg(rgb(0)).into_any_element();
        };
        let view = p.view();
        let files = view.files.clone();
        let selected = view.selected.clone();
        let owner = cx.entity().downgrade();
        let project_id = p.id;
        let group = p.group;
        let disabled = p.path.is_none() || p.loading;
        let files_focused = view.focus.contains_focused(window, cx);
        v_flex()
            .size_full()
            .min_h_0()
            .bg(cx.theme().sidebar)
            .child(
                h_flex()
                    .h(px(46.))
                    .flex_none()
                    .px_4()
                    .gap_2()
                    .child(
                        div()
                            .font_medium()
                            .text_size(px(14.))
                            .child(p.group.label()),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(cx.theme().muted_foreground)
                            .child(files.len().to_string()),
                    )
                    .child(
                        div()
                            .id("sidebar-drag-area")
                            .flex_1()
                            .h_full()
                            .on_mouse_down(MouseButton::Left, |_, w, _| w.start_window_move())
                            .on_double_click(|_, w, _| w.titlebar_double_click()),
                    ),
            )
            .child(
                div()
                    .id("file-list")
                    .test_support()
                    .flex_1()
                    .min_h_0()
                    .px_2()
                    .track_focus(&view.focus)
                    .key_context("DiffyFiles")
                    .child(
                        uniform_list("files", files.len(), move |range, _, cx| {
                            range
                                .map(|ix| {
                                    let f = &files[ix];
                                    let path = f.path().clone();
                                    let owner = owner.clone();
                                    let action_owner = owner.clone();
                                    let action_path = path.clone();
                                    let tooltip_path = path.clone();
                                    let (directory, filename) = path.rsplit_once('/').map_or(
                                        (None, path.to_string()),
                                        |(dir, name)| (Some(format!("{dir}/")), name.to_owned()),
                                    );
                                    let row_group =
                                        SharedString::from(format!("file-row-{project_id}-{ix}"));
                                    let selected = selected.as_ref() == Some(&path);
                                    let (status, color) = match f.status() {
                                        DiffFileStatus::Added => ("A", cx.theme().success),
                                        DiffFileStatus::Deleted => ("D", cx.theme().danger),
                                        DiffFileStatus::Renamed => ("R", cx.theme().primary),
                                        DiffFileStatus::Copied => ("C", cx.theme().primary),
                                        _ => ("M", cx.theme().muted_foreground),
                                    };
                                    ListItem::new(path.clone())
                                        .group(row_group.clone())
                                        .h(px(30.))
                                        .px_2()
                                        .w_full()
                                        .min_w_0()
                                        .rounded(px(5.))
                                        .selected(selected)
                                        .accessibility_label(path.clone())
                                        .tooltip(move |w, cx| Tooltip::new(tooltip_path.clone()).max_w(px(640.)).whitespace_normal().build(w, cx))
                                        .child(
                                            h_flex()
                                                .w_full()
                                                .min_w_0()
                                                .gap_2()
                                                .child(
                                                    Icon::new(IconName::FileText)
                                                        .xsmall()
                                                        .text_color(cx.theme().muted_foreground),
                                                )
                                                .child(
                                                    h_flex()
                                                        .flex_1()
                                                        .min_w_0()
                                                        .overflow_hidden()
                                                        .text_size(px(13.))
                                                        .children(directory.map(|dir| div()
                                                            .id(("file-directory", ix))
                                                            .min_w_0()
                                                            .truncate()
                                                            .text_ellipsis_middle()
                                                            .text_color(cx.theme().muted_foreground)
                                                            .child(dir)))
                                                        .child(div()
                                                            .id(("file-name", ix))
                                                            .test_support()
                                                            .flex_shrink_0()
                                                            .max_w_full()
                                                            .truncate()
                                                            .text_ellipsis_middle()
                                                            .font_medium()
                                                            .text_color(cx.theme().foreground)
                                                            .role(gpui_kit::accesskit::Role::Label)
                                                            .aria_label(filename.clone())
                                                            .child(filename)),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(px(12.))
                                                        .text_color(color)
                                                        .child(status),
                                                )
                                                .child(
                                                    Button::new(("file-action", ix))
                                                        .ghost()
                                                        .xsmall()
                                                        .icon(if group == ChangeGroup::Changes {
                                                            IconName::Plus
                                                        } else {
                                                            IconName::Minus
                                                        })
                                                        .disabled(disabled)
                                                        .accessibility_label(format!(
                                                            "{} {}",
                                                            if group == ChangeGroup::Changes {
                                                                "Stage"
                                                            } else {
                                                                "Unstage"
                                                            },
                                                            path
                                                        ))
                                                        .tooltip(if group == ChangeGroup::Changes {
                                                            "Stage file"
                                                        } else {
                                                            "Unstage file"
                                                        })
                                                        .when(
                                                            !(selected && files_focused),
                                                            |button| {
                                                                button.invisible().group_hover(
                                                                    row_group,
                                                                    |button| button.visible(),
                                                                )
                                                            },
                                                        )
                                                        .on_click(move |_, _, cx| {
                                                            cx.stop_propagation();
                                                            let _ = action_owner.update(
                                                                cx,
                                                                |this, cx| {
                                                                    this.stage_path(
                                                                        project_id,
                                                                        group,
                                                                        &action_path,
                                                                        cx,
                                                                    )
                                                                },
                                                            );
                                                        }),
                                                ),
                                        )
                                        .on_click(move |_, window, cx| {
                                            let _ = owner.update(cx, |this, cx| {
                                                this.select_file(path.clone(), window, cx)
                                            });
                                        })
                                        .into_any_element()
                                })
                                .collect()
                        })
                        .size_full()
                        .track_scroll(&view.scroll),
                    ),
            )
            .child(
                div().flex_none().px_3().py_3().child(
                    Button::new("stage-all")
                        .ghost()
                        .w_full()
                        .icon(if group == ChangeGroup::Changes {
                            IconName::Plus
                        } else {
                            IconName::Minus
                        })
                        .label(if group == ChangeGroup::Changes {
                            "Stage all"
                        } else {
                            "Unstage all"
                        })
                        .disabled(disabled || view.files.is_empty())
                        .on_click(cx.listener(|this, _, _, cx| this.stage_all(cx))),
                ),
            )
            .into_any_element()
    }
}
impl Render for Diffy {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.projects.get(self.active);
        let hint = |keys: &str, label: &str| {
            h_flex()
                .gap(px(7.))
                .child(div().text_color(rgb(0xb3b8c2)).child(keys.to_owned()))
                .child(
                    div()
                        .text_color(cx.theme().muted_foreground)
                        .child(label.to_owned()),
                )
        };
        let shortcuts = h_flex()
            .gap(px(22.))
            .text_size(px(12.))
            .child(hint("⌘P", "Commands"))
            .child(hint("⌘⇧↑/↓", "Projects"))
            .child(hint("⌘↑/↓", "Files"))
            .child(hint("⌘⇧S", "View"))
            .child(hint(
                "⌘S",
                if self.selected_group() == ChangeGroup::Staged {
                    "Unstage"
                } else {
                    "Stage"
                },
            ));
        let breadcrumb = p.map(|p| {
            let path = p.path.as_ref().map_or_else(
                || p.name.to_string(),
                |path| {
                    let parent = path
                        .parent()
                        .and_then(|p| p.file_name())
                        .map(|n| n.to_string_lossy().into_owned());
                    match parent {
                        Some(parent) => format!("…/{parent}/{}", p.name),
                        None => p.name.to_string(),
                    }
                },
            );
            h_flex()
                .min_w_0()
                .gap(px(9.))
                .text_size(px(12.))
                .text_color(cx.theme().muted_foreground)
                .child(div().min_w_0().truncate().child(path))
                .child("/")
                .child(div().flex_none().child(p.branch.clone()))
        });
        v_flex()
            .id("diffy")
            .relative()
            .key_context("Diffy")
            .track_focus(&self.focus)
            .size_full()
            .min_w_0()
            .min_h_0()
            .bg(rgb(0))
            .font_family(fonts::FAMILY)
            .font_features(fonts::features())
            .text_size(px(13.))
            .text_color(cx.theme().foreground)
            .on_key_up(cx.listener(|this, _, _, _| this.stop_file_repeat()))
            .on_action(cx.listener(|this, a: &EditProject, w, cx| this.open_settings(a.id, w, cx)))
            .on_action(cx.listener(|this, _: &StageAll, _, cx| this.stage_all(cx)))
            .on_action(cx.listener(|this, _: &OpenProject, w, cx| this.open_project(w, cx)))
            .on_action(cx.listener(|this, _: &OpenCommandPalette, w, cx| this.open_palette(w, cx)))
            .on_action(cx.listener(|this, a: &SelectProject, w, cx| {
                if let Some(ix) = this.projects.iter().position(|p| p.id == a.id) {
                    this.switch_project(ix, w, cx);
                }
            }))
            .on_action(
                cx.listener(|this, a: &RemoveProject, w, cx| this.remove_project(a.id, w, cx)),
            )
            .on_action(cx.listener(|this, _: &RemoveCurrentProject, w, cx| {
                if let Some(id) = this.active_project_id() {
                    this.remove_project(id, w, cx);
                }
            }))
            .on_action(
                cx.listener(|this, _: &UnifiedMode, _, cx| this.set_mode(DiffMode::Unified, cx)),
            )
            .on_action(cx.listener(|this, _: &SplitMode, _, cx| this.set_mode(DiffMode::Split, cx)))
            .on_action(cx.listener(|this, _: &Refresh, _, cx| this.refresh(cx)))
            .on_action(cx.listener(|this, _: &StageFile, _, cx| this.stage_file(cx)))
            .on_action(cx.listener(|this, _: &ToggleGroup, w, cx| {
                this.set_group(this.selected_group().other(), w, cx)
            }))
            .on_action(cx.listener(|this, _: &NextProject, w, cx| this.move_project(1, w, cx)))
            .on_action(cx.listener(|this, _: &PreviousProject, w, cx| this.move_project(-1, w, cx)))
            .on_action(cx.listener(|this, _: &NextFile, _, cx| this.move_file(1, cx)))
            .on_action(cx.listener(|this, _: &PreviousFile, _, cx| this.move_file(-1, cx)))
            .on_action(cx.listener(|this, _: &NextChange, _, cx| this.change(true, cx)))
            .on_action(cx.listener(|this, _: &PreviousChange, _, cx| this.change(false, cx)))
            .on_action(cx.listener(|this, _: &FocusDiff, w, cx| this.focus_diff(w, cx)))
            .on_action(cx.listener(|this, _: &CheckForUpdates, _, cx| this.check_updates(true, cx)))
            .on_action(cx.listener(|this, _: &InstallUpdate, _, cx| this.install_update(cx)))
            .on_action(cx.listener(|_, _: &Hide, _, cx| cx.hide()))
            .on_action(cx.listener(|_, _: &HideOthers, _, cx| cx.hide_other_apps()))
            .on_action(cx.listener(|_, _: &ShowAll, _, cx| cx.unhide_other_apps()))
            .on_action(cx.listener(|_, _: &About, w, cx| {
                let prompt = w.prompt(gpui_kit::PromptLevel::Info, "Diffy", Some(concat!("Version ", env!("CARGO_PKG_VERSION"), "\nA small desktop Git diff viewer.\nhttps://github.com/hitSlop/diffy\nMIT License · © 2026 hitSlop")), &["OK"], cx);
                cx.spawn(async move |_, _| { let _ = prompt.await; }).detach();
            }))
            .on_action(cx.listener(|this, _: &Quit, _, cx| {
                if this.updates.blocks_mutations() { return; }
                this.persist(cx);
                cx.quit();
            }))
            .children(self.state_error.as_ref().map(|error| {
                div()
                    .px_3()
                    .py_2()
                    .text_size(px(12.))
                    .text_color(cx.theme().danger)
                    .child(error.clone())
            }))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .child(self.render_projects(window, cx))
                    .child(
                        div().flex_1().min_w_0().min_h_0().h_full().child(
                            h_resizable("workspace")
                                .with_handle_appearance(std::rc::Rc::new(|_, _, _| {
                                    Some(div().into_any_element())
                                }))
                                .child(
                                    resizable_panel()
                                        .size_range(px(360.)..px(2400.))
                                        .child(self.render_center(window, cx)),
                                )
                                .child(
                                    resizable_panel()
                                        .size(px(300.))
                                        .size_range(px(240.)..px(460.))
                                        .child(self.render_files(window, cx)),
                                ),
                        ),
                    ),
            )
            .children(self.render_update_notice(cx))
            .child(
                StatusBar::new()
                    .border_0()
                    .bg(rgb(0))
                    .px(px(14.))
                    .py(px(6.))
                    .left(h_flex().min_w_0().children(breadcrumb))
                    .right(shortcuts),
            )
            .when(self.palette_open, |root| {
                root.child(self.render_palette(cx))
            })
            .when(self.settings.is_some(), |root| {
                root.child(self.render_settings(cx))
            })
    }
}
