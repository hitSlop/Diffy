#[path = "../benches/support/fixture.rs"]
mod fixture;
use diffy::{
    app,
    project_apps::{Launcher, ProjectApp},
    state,
};
use gpui_kit::{
    AppContext as _, HeadlessAppContext, InputEvent as _, MouseButton, Role, WindowOptions,
    assets::Assets, base::test_support::snapshots, component::WindowExt as _,
    test::TestWindowExt as _,
};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

#[derive(Default)]
struct MockLauncher {
    installed: Mutex<Vec<ProjectApp>>,
    opened: Mutex<Vec<(ProjectApp, PathBuf)>>,
    fail: AtomicBool,
}
impl Launcher for MockLauncher {
    fn installed(&self) -> Vec<ProjectApp> {
        self.installed.lock().unwrap().clone()
    }
    fn open(&self, app: ProjectApp, root: &Path) -> Result<(), String> {
        self.opened.lock().unwrap().push((app, root.to_owned()));
        if self.fail.load(Ordering::SeqCst) {
            Err("The destination app is unavailable.".into())
        } else {
            Ok(())
        }
    }
}

fn labels(window: &gpui_kit::Window) -> Vec<String> {
    let mut rows = snapshots(window)
        .into_iter()
        .filter(|s| s.role() == Some(Role::MenuItem))
        .filter(|s| s.label().is_some())
        .collect::<Vec<_>>();
    rows.sort_by(|a, b| f32::from(a.bounds().origin.y).total_cmp(&f32::from(b.bounds().origin.y)));
    rows.into_iter()
        .map(|s| s.label().unwrap().to_owned())
        .collect()
}
fn click_menu(window: &mut gpui_kit::Window, label: &str, cx: &mut gpui_kit::App) {
    let row = snapshots(window)
        .into_iter()
        .find(|s| s.role() == Some(Role::MenuItem) && s.label() == Some(label))
        .unwrap();
    window
        .within("popup-menu")
        .click(row.path().last().unwrap().clone(), cx);
    window.render_frame(cx);
}
fn right_click(window: &mut gpui_kit::Window, ix: usize, cx: &mut gpui_kit::App) {
    window.render_frame(cx);
    let position = window.find(("project", ix)).bounds().center();
    window.dispatch_event(
        gpui_kit::MouseDownEvent {
            position,
            button: MouseButton::Right,
            click_count: 1,
            ..Default::default()
        }
        .to_platform_input(),
        cx,
    );
    window.dispatch_event(
        gpui_kit::MouseUpEvent {
            position,
            button: MouseButton::Right,
            click_count: 1,
            ..Default::default()
        }
        .to_platform_input(),
        cx,
    );
    window.render_frame(cx);
}

fn main() {
    let first = fixture::Fixture::new("project-menu-first");
    let second = fixture::Fixture::new("project-menu-second");
    let first_root = first.0.canonicalize().unwrap();
    let second_root = second.0.canonicalize().unwrap();
    first.write("first.txt", "first\n");
    second.write("second.txt", "second\n");
    let storage =
        std::env::temp_dir().join(format!("diffy-project-menu-prefs-{}", std::process::id()));
    let prefs = storage.join("state.json");
    std::fs::create_dir_all(&storage).unwrap();
    let owned_icon = state::import_project_icon(&prefs, Path::new("assets/icon.png")).unwrap();
    let mut initial = state::SavedState {
        active: Some(second_root.clone()),
        ..Default::default()
    };
    initial.discovery.onboarding_completed = true;
    for (path, name) in [(&first_root, "First"), (&second_root, "Second")] {
        initial.projects.push(state::SavedProject {
            path: path.clone(),
            name: Some(name.into()),
            icon: Some(state::ProjectIcon::Image(owned_icon.clone())),
            color: 0xffd65a,
            staged: false,
            split: true,
            selected: [None, None],
        });
    }
    state::save(&prefs, &initial).unwrap();
    let launcher = Arc::new(MockLauncher::default());
    *launcher.installed.lock().unwrap() = vec![
        ProjectApp::Cursor,
        ProjectApp::TERMINALS[0],
        ProjectApp::Cursor,
    ];
    let mut cx = HeadlessAppContext::with_platform(
        gpui_kit::platform::current_platform(true).text_system(),
        Arc::new(Assets),
        gpui_kit::platform::current_headless_renderer,
    );
    cx.update(|cx| {
        gpui_kit::init(cx);
        app::init(cx);
    });
    let (handle, view) = cx
        .update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    show: false,
                    focus: false,
                    ..Default::default()
                },
                cx,
                |window, cx| {
                    cx.new(|cx| {
                        app::Diffy::with_options(vec![], false, Some(prefs.clone()), window, cx)
                    })
                },
            )
        })
        .unwrap();
    cx.update(|cx| {
        view.update(cx, |view, cx| {
            view.set_project_launcher(launcher.clone(), cx)
        })
    });
    macro_rules! settle {
        () => {{
            for _ in 0..300 {
                cx.run_until_parked();
                let ready = cx
                    .update_window(handle, |_, window, cx| {
                        window.render_frame(cx);
                        !view.read(cx).is_busy() && view.read(cx).project_apps_ready()
                    })
                    .unwrap();
                if ready {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }};
    }
    settle!();
    let active = cx.update(|cx| view.read(cx).active_project_id().unwrap());
    let selected = cx.update(|cx| view.read(cx).selected_file().cloned());
    macro_rules! open_menu {
        () => {{
            cx.update_window(handle, |_, window, cx| right_click(window, 0, cx))
                .unwrap();
            settle!();
        }};
    }
    open_menu!();
    cx.update_window(handle, |_, window, cx| {
        assert!(window.try_find("project-settings").is_none());
        assert_eq!(
            labels(window),
            [
                "Edit Project…",
                ProjectApp::TERMINALS[0].label(),
                "Open in Cursor",
                diffy::project_apps::folder_label(),
                "Remove Project"
            ]
        );
        assert_eq!(view.read(cx).active_project_id(), Some(active));
        window.press("escape", cx);
        window.render_frame(cx);
    })
    .unwrap();
    settle!();
    cx.update_window(handle, |_, window, _| assert!(labels(window).is_empty()))
        .unwrap();
    open_menu!();
    cx.update_window(handle, |_, window, cx| {
        click_menu(window, "Edit Project…", cx)
    })
    .unwrap();
    settle!();
    cx.update_window(handle, |_, window, cx| {
        assert!(window.try_find("project-settings").is_some());
        assert_eq!(view.read(cx).active_project_id(), Some(active));
        assert_eq!(window.find("project-name").value(), Some("First"));
        window.press("escape", cx);
        window.render_frame(cx);
    })
    .unwrap();
    for app in [
        ProjectApp::TERMINALS[0],
        ProjectApp::Cursor,
        ProjectApp::Finder,
    ] {
        open_menu!();
        cx.update_window(handle, |_, window, cx| click_menu(window, app.label(), cx))
            .unwrap();
        for _ in 0..100 {
            cx.run_until_parked();
            if launcher.opened.lock().unwrap().last() == Some(&(app, first_root.clone())) {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(
            launcher.opened.lock().unwrap().last(),
            Some(&(app, first_root.clone()))
        );
        cx.update(|cx| {
            assert_eq!(view.read(cx).active_project_id(), Some(active));
            assert_eq!(view.read(cx).selected_file().cloned(), selected);
        });
    }
    launcher.fail.store(true, Ordering::SeqCst);
    open_menu!();
    cx.update_window(handle, |_, window, cx| {
        click_menu(window, "Open in Cursor", cx)
    })
    .unwrap();
    for _ in 0..100 {
        cx.run_until_parked();
        let shown = cx
            .update_window(handle, |_, window, cx| !window.notifications(cx).is_empty())
            .unwrap();
        if shown {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    cx.update_window(handle, |_, window, cx| {
        assert_eq!(
            window.notifications(cx).len(),
            1,
            "Launch errors appear in the current window"
        );
        window.clear_notifications(cx);
    })
    .unwrap();
    // Removing apps while Diffy is running is reflected by the next detection.
    launcher.installed.lock().unwrap().clear();
    cx.update(|cx| {
        view.update(cx, |view, cx| {
            view.set_project_launcher(launcher.clone(), cx)
        })
    });
    settle!();
    open_menu!();
    cx.update_window(handle, |_, window, cx| {
        assert_eq!(
            labels(window),
            [
                "Edit Project…",
                diffy::project_apps::folder_label(),
                "Remove Project"
            ]
        );
        click_menu(window, "Remove Project", cx);
    })
    .unwrap();
    settle!();
    cx.update(|cx| {
        assert_eq!(view.read(cx).project_count(), 1);
        assert_eq!(view.read(cx).active_project_id(), Some(active));
    });
    let flush = cx.update(|cx| view.read(cx).preferences_flush()).unwrap();
    flush.wait().unwrap();
    let saved = state::load(&prefs).unwrap();
    assert_eq!(saved.projects.len(), 1);
    assert_eq!(saved.projects[0].path, second_root);
    assert_eq!(
        saved.projects[0].icon,
        Some(state::ProjectIcon::Image(owned_icon.clone()))
    );
    assert!(owned_icon.exists());
    assert_eq!(
        std::fs::read_to_string(first.0.join("first.txt")).unwrap(),
        "first\n"
    );
    // Demo destinations remain inert, even if an action is dispatched directly.
    let (demo_handle, demo) = cx
        .update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    show: false,
                    focus: false,
                    ..Default::default()
                },
                cx,
                |window, cx| cx.new(|cx| app::Diffy::demo(window, cx)),
            )
        })
        .unwrap();
    cx.update(|cx| {
        demo.update(cx, |view, cx| {
            view.set_project_launcher(launcher.clone(), cx)
        })
    });
    let calls = launcher.opened.lock().unwrap().len();
    cx.update_window(demo_handle, |_, window, cx| {
        let id = demo.read(cx).active_project_id().unwrap();
        window.dispatch_action(
            Box::new(app::OpenProjectIn {
                id,
                app: ProjectApp::Finder,
            }),
            cx,
        );
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(launcher.opened.lock().unwrap().len(), calls);
    std::fs::remove_dir_all(storage).unwrap();
    println!(
        "Project menus, app filtering, inactive project targets, launch errors, removal, and icon preservation passed."
    );
}
