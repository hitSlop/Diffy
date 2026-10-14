use super::{Diffy, EditProject, OpenProjectIn, RemoveProject};
use crate::project_apps::{Launcher, ProjectApp, SystemLauncher};
use gpui_kit::component::{
    WindowExt as _,
    menu::{PopupMenu, PopupMenuItem},
    notification::Notification,
};
use gpui_kit::{AppContext as _, Context, Subscription, Task, Window};
use std::sync::Arc;

pub(super) struct Runtime {
    pub(super) installed: Vec<ProjectApp>,
    launcher: Arc<dyn Launcher>,
    detecting: bool,
    detection_task: Option<Task<()>>,
    pub(super) activation: Option<Subscription>,
}

impl Default for Runtime {
    fn default() -> Self {
        Self {
            installed: Vec::new(),
            launcher: Arc::new(SystemLauncher),
            detecting: false,
            detection_task: None,
            activation: None,
        }
    }
}

pub(super) fn project_menu(
    mut menu: PopupMenu,
    id: u64,
    installed: &[ProjectApp],
    local: bool,
    mutating: bool,
) -> PopupMenu {
    menu = menu.menu("Edit Project…", Box::new(EditProject { id }));
    for group in [ProjectApp::TERMINALS, ProjectApp::EDITORS] {
        let apps = group
            .into_iter()
            .filter(|app| installed.contains(app))
            .collect::<Vec<_>>();
        if !apps.is_empty() {
            menu = menu.separator();
            for app in apps {
                menu = menu.menu_with_disabled(
                    app.label(),
                    Box::new(OpenProjectIn { id, app }),
                    !local,
                );
            }
        }
    }
    menu.separator()
        .menu_with_disabled(
            "Open in Finder",
            Box::new(OpenProjectIn {
                id,
                app: ProjectApp::Finder,
            }),
            !local || !cfg!(target_os = "macos"),
        )
        .item(
            PopupMenuItem::new("Remove Project")
                .danger()
                .action(Box::new(RemoveProject { id }))
                .disabled(mutating),
        )
}

impl Diffy {
    pub(super) fn detect_project_apps(&mut self, cx: &mut Context<Self>) {
        if self.project_apps.detecting {
            return;
        }
        self.project_apps.detecting = true;
        let launcher = self.project_apps.launcher.clone();
        self.project_apps.detection_task = Some(cx.spawn(async move |owner, cx| {
            let installed = cx
                .background_spawn(async move { launcher.installed() })
                .await;
            let _ = owner.update(cx, |this, cx| {
                this.project_apps.installed = installed;
                this.project_apps.detecting = false;
                cx.notify();
            });
        }));
    }

    pub(super) fn open_project_in(
        &self,
        action: &OpenProjectIn,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self
            .projects
            .iter()
            .find(|p| p.id == action.id)
            .and_then(|p| p.path.clone())
        else {
            return;
        };
        let app = action.app;
        let launcher = self.project_apps.launcher.clone();
        cx.spawn_in(window, async move |owner, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { launcher.open(app, &root) })
                .await;
            if let Err(error) = result {
                let _ = cx.update(|window, cx| {
                    if owner.upgrade().is_some() {
                        window.push_notification(Notification::error(error), cx);
                    }
                });
            }
        })
        .detach();
    }
}

#[cfg(feature = "ui-tests")]
impl Diffy {
    pub fn set_project_launcher(&mut self, launcher: Arc<dyn Launcher>, cx: &mut Context<Self>) {
        self.project_apps.detection_task = None;
        self.project_apps.detecting = false;
        self.project_apps.launcher = launcher;
        self.detect_project_apps(cx);
    }

    pub fn project_apps_ready(&self) -> bool {
        !self.project_apps.detecting
    }
}
