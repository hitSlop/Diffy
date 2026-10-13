use diffy::app;

use gpui_kit::{
    AppContext as _, Bounds, Focusable as _, HeadlessAppContext, WindowBounds, WindowOptions,
    assets::Assets, px, size, test::TestWindowExt as _,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
static FILE_KEYS_HELD: AtomicBool = AtomicBool::new(false);
fn file_keys_held(_: isize) -> bool {
    FILE_KEYS_HELD.load(Ordering::SeqCst)
}

fn main() {
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
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Default::default(),
                        size: size(px(1440.), px(900.)),
                    })),
                    show: false,
                    focus: false,
                    ..Default::default()
                },
                cx,
                |window, cx| cx.new(|cx| app::Diffy::demo(window, cx)),
            )
        })
        .expect("open offscreen Diffy window");
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    let unified = cx.capture_screenshot(handle).expect("Metal capture");
    std::fs::create_dir_all("target/previews").unwrap();
    unified.save("target/previews/diffy-unified.png").unwrap();

    cx.update_window(handle, |_, window, cx| {
        window.click("split", cx);
        window.render_frame(cx);
    })
    .unwrap();
    let split = cx.capture_screenshot(handle).unwrap();
    assert!(
        unified.as_raw() != split.as_raw(),
        "Split button must change rendered layout"
    );
    split.save("target/previews/diffy-split.png").unwrap();

    // Quick taps never throttle, and holding repeats even without native repeat/key-up events.
    cx.update_window(handle, |_, w, cx| {
        view.update(cx, |v, _| v.set_file_key_probe(file_keys_held));
        w.press("cmd-down", cx);
        w.render_frame(cx);
        w.press("cmd-down", cx);
        w.render_frame(cx);
        assert_eq!(
            view.read(cx).selected_file().map(|p| p.as_str()),
            Some("tests/timeout_test.rs")
        );
        w.press("cmd-up", cx);
        w.render_frame(cx);
        w.press("cmd-up", cx);
        w.render_frame(cx);
        FILE_KEYS_HELD.store(true, Ordering::SeqCst);
        use gpui_kit::InputEvent as _;
        w.dispatch_event(
            gpui_kit::KeyDownEvent {
                keystroke: gpui_kit::Keystroke::parse("cmd-down").unwrap(),
                is_held: false,
                prefer_character_input: false,
            }
            .to_platform_input(),
            cx,
        );
        w.render_frame(cx);
        assert_eq!(
            view.read(cx).selected_file().map(|p| p.as_str()),
            Some("src/duration.rs")
        );
    })
    .unwrap();
    cx.run_until_parked();
    for _ in 0..18 {
        cx.advance_clock(std::time::Duration::from_millis(16));
        cx.run_until_parked();
    }
    cx.update_window(handle, |_, w, cx| {
        w.render_frame(cx);
        assert_eq!(
            view.read(cx).selected_file().map(|p| p.as_str()),
            Some("src/duration.rs"),
            "Hold has an initial reading delay"
        );
    })
    .unwrap();
    for _ in 0..3 {
        cx.advance_clock(std::time::Duration::from_millis(16));
        cx.run_until_parked();
    }
    cx.update_window(handle, |_, w, cx| {
        w.render_frame(cx);
        assert_eq!(
            view.read(cx).selected_file().map(|p| p.as_str()),
            Some("tests/timeout_test.rs"),
            "Own timer repeats without OS autorepeat"
        );
    })
    .unwrap();
    FILE_KEYS_HELD.store(false, Ordering::SeqCst);
    cx.advance_clock(std::time::Duration::from_millis(16));
    cx.run_until_parked();
    cx.update_window(handle, |_, w, cx| {
        use gpui_kit::InputEvent as _;
        for _ in 0..2 {
            w.dispatch_event(
                gpui_kit::KeyDownEvent {
                    keystroke: gpui_kit::Keystroke::parse("cmd-up").unwrap(),
                    is_held: false,
                    prefer_character_input: false,
                }
                .to_platform_input(),
                cx,
            );
            w.render_frame(cx);
        }
        assert_eq!(
            view.read(cx).selected_file().map(|p| p.as_str()),
            Some("src/capture.rs"),
            "Quick Command taps work even when macOS omits key-up"
        );
    })
    .unwrap();

    cx.update_window(handle, |_, window, cx| {
        window.press("cmd-enter", cx);
        window.press("cmd-shift-down", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    let next = cx.capture_screenshot(handle).unwrap();
    assert!(
        split.as_raw() != next.as_raw(),
        "Project navigation must change visible source"
    );
    next.save("target/previews/diffy-next-project.png").unwrap();
    cx.update_window(handle, |_, window, cx| {
        window.click(("project", 0usize), cx);
        window.render_frame(cx);
    })
    .unwrap();
    let restored = cx.capture_screenshot(handle).unwrap();
    restored.save("target/previews/diffy-restored.png").unwrap();
    let scale = unified.width() / 1440;
    cx.update_window(handle, |_, window, cx| {
        assert_eq!(
            view.read(cx).selected_group(),
            diffy::repository::ChangeGroup::Changes
        );
        assert_eq!(
            view.read(cx).selected_file().map(|p| p.as_str()),
            Some("src/capture.rs")
        );
        window.click("src/duration.rs", cx);
        window.render_frame(cx);
        assert_eq!(
            view.read(cx).selected_file().map(|p| p.as_str()),
            Some("src/duration.rs")
        );
        window.press("cmd-down", cx);
        window.render_frame(cx);
        assert_eq!(
            view.read(cx).selected_file().map(|p| p.as_str()),
            Some("tests/timeout_test.rs")
        );
        window.press("cmd-up", cx);
        window.render_frame(cx);
        window.press("cmd-shift-s", cx);
        window.render_frame(cx);
        assert_eq!(
            view.read(cx).selected_group(),
            diffy::repository::ChangeGroup::Staged
        );
        assert_eq!(
            view.read(cx).selected_file().map(|p| p.as_str()),
            Some("Cargo.toml")
        );
        assert!(
            window.try_find("src/capture.rs").is_none(),
            "Sidebar only lists current scope"
        );
        window.click("src/duration.rs", cx);
        window.render_frame(cx);
        window.press("cmd-s", cx);
        window.render_frame(cx);
        assert_eq!(
            view.read(cx).selected_file().map(|p| p.as_str()),
            Some("src/duration.rs"),
            "Demo mutation disabled"
        );
        window.press("cmd-shift-s", cx);
        window.render_frame(cx);
        assert_eq!(
            view.read(cx).selected_file().map(|p| p.as_str()),
            Some("src/duration.rs"),
            "Scope selection retained"
        );
        window.press("cmd-shift-s", cx);
        window.render_frame(cx);
        window.press("cmd-shift-down", cx);
        window.render_frame(cx);
        assert_eq!(
            view.read(cx).selected_group(),
            diffy::repository::ChangeGroup::Changes
        );
        window.press("cmd-shift-s", cx);
        window.render_frame(cx);
        assert!(
            view.read(cx).selected_file().is_none(),
            "Slide staged scope empty"
        );
        window.press("cmd-shift-up", cx);
        window.render_frame(cx);
        assert_eq!(
            view.read(cx).selected_group(),
            diffy::repository::ChangeGroup::Staged,
            "Project scope retained"
        );
        assert_eq!(
            view.read(cx).selected_file().map(|p| p.as_str()),
            Some("src/duration.rs")
        );
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, w, cx| w.render_frame(cx))
        .unwrap();
    let staged = cx.capture_screenshot(handle).unwrap();
    staged.save("target/previews/diffy-staged.png").unwrap();
    cx.update_window(handle, |_, window, cx| {
        window.hover(("project", 0usize), cx);
    })
    .unwrap();
    cx.advance_clock(std::time::Duration::from_secs(1));
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    let hovered = cx.capture_screenshot(handle).unwrap();
    hovered
        .save("target/previews/diffy-project-tooltip.png")
        .unwrap();
    macro_rules! circle_pixels {
        ($image:expr) => {{
            let image = $image;
            (0..76 * scale)
                .map(|x| {
                    (42 * scale..108 * scale)
                        .filter(|&y| {
                            let p = image.get_pixel(x, y).0;
                            p[0] == 255 && p[1] == 129 && p[2] == 117
                        })
                        .count()
                })
                .sum::<usize>()
        }};
    }
    assert!(
        circle_pixels!(&hovered) > circle_pixels!(&staged),
        "Hover grows the painted circle"
    );
    // With reduced motion, hover adopts its final appearance immediately.
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.update_window(handle, |_, window, cx| {
        window.hover(("project", 1usize), cx);
        window.render_frame(cx);
    })
    .unwrap();
    let immediate = cx.capture_screenshot(handle).unwrap();
    cx.advance_clock(std::time::Duration::from_millis(200));
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    let settled = cx.capture_screenshot(handle).unwrap();
    assert!(
        (12 * scale..64 * scale).all(|x| {
            (108 * scale..164 * scale).all(|y| immediate.get_pixel(x, y) == settled.get_pixel(x, y))
        }),
        "Reduced motion does not animate the hovered project icon"
    );
    macro_rules! palette_confirm {
        ($query:expr) => {{
            cx.update_window(handle, |_, w, cx| {
                w.press("cmd-p", cx);
                w.render_frame(cx);
                assert!(view.read(cx).palette_is_open());
                w.input($query, cx);
            })
            .unwrap();
            cx.run_until_parked();
            cx.update_window(handle, |_, w, cx| {
                w.render_frame(cx);
                assert_eq!(view.read(cx).palette_query(cx), $query);
                w.press("enter", cx);
            })
            .unwrap();
            cx.run_until_parked();
        }};
    }
    palette_confirm!("Slide");
    cx.update_window(handle, |_, w, cx| {
        w.render_frame(cx);
        assert!(!view.read(cx).palette_is_open());
        assert_eq!(view.read(cx).active_project_id(), Some(2));
    })
    .unwrap();
    palette_confirm!("Split diff");
    cx.update_window(handle, |_, w, cx| {
        w.render_frame(cx);
        assert_eq!(
            view.read(cx).selected_mode(cx),
            gpui_kit::component::diff::DiffMode::Split
        );
        assert!(!view.read(cx).palette_is_open());
    })
    .unwrap();
    palette_confirm!("Refresh repository");
    cx.update_window(handle, |_, w, cx| {
        w.render_frame(cx);
        assert!(
            view.read(cx).palette_is_open(),
            "Disabled command cannot confirm"
        );
        w.press("escape", cx);
        w.render_frame(cx);
        assert!(view.read(cx).palette_is_open(), "First Escape clears query");
        w.press("escape", cx);
        w.render_frame(cx);
        assert!(!view.read(cx).palette_is_open());
        w.press("cmd-p", cx);
        w.render_frame(cx);
    })
    .unwrap();
    cx.capture_screenshot(handle)
        .unwrap()
        .save("target/previews/diffy-command-palette.png")
        .unwrap();
    cx.update_window(handle, |_, w, cx| {
        w.click_at("palette-backdrop", gpui_kit::point(px(10.), px(700.)), cx);
        w.render_frame(cx);
        assert!(
            !view.read(cx).palette_is_open(),
            "Click outside dismisses palette"
        );
    })
    .unwrap();
    struct NoUpdates;
    impl diffy::updates::Backend for NoUpdates {
        fn check(&self) -> Result<Option<diffy::updates::Release>, String> {
            Ok(None)
        }
    }
    cx.update_window(handle, |_, _, cx| {
        view.update(cx, |this, _| this.set_update_backend(Arc::new(NoUpdates)));
    })
    .unwrap();
    palette_confirm!("Check for updates");
    for _ in 0..500 {
        cx.run_until_parked();
        let done = cx
            .update_window(handle, |_, w, cx| {
                w.render_frame(cx);
                matches!(view.read(cx).update_phase(), diffy::updates::Phase::Current)
            })
            .unwrap();
        if done {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    cx.update_window(handle, |_, w, cx| {
        w.render_frame(cx);
        assert!(!view.read(cx).palette_is_open());
        assert!(matches!(
            view.read(cx).update_phase(),
            diffy::updates::Phase::Current
        ));
        w.click("dismiss-update", cx);
        w.render_frame(cx);
        assert!(matches!(
            view.read(cx).update_phase(),
            diffy::updates::Phase::Idle
        ));
    })
    .unwrap();
    // Exercise background Git writes through the actual shortcuts in a disposable repo.
    let root = std::env::temp_dir().join(format!("diffy-ui-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    assert!(
        std::process::Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(["init", "--initial-branch=main"])
            .output()
            .unwrap()
            .status
            .success()
    );
    std::fs::write(root.join(".git/info/exclude"), "preferences/\n").unwrap();
    std::fs::write(root.join("a.rs"), "one\n").unwrap();
    std::fs::write(root.join("b.rs"), "two\n").unwrap();
    let prefs = root.join("preferences/state.json");
    let (real_handle, real_view) = cx
        .update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Default::default(),
                        size: size(px(1440.), px(900.)),
                    })),
                    show: false,
                    focus: false,
                    ..Default::default()
                },
                cx,
                |w, cx| {
                    cx.new(|cx| {
                        app::Diffy::with_options(
                            vec![root.clone()],
                            false,
                            Some(prefs.clone()),
                            w,
                            cx,
                        )
                    })
                },
            )
        })
        .unwrap();
    macro_rules! settle {
        () => {{
            for _ in 0..500 {
                cx.run_until_parked();
                let busy = cx
                    .update_window(real_handle, |_, w, cx| {
                        w.render_frame(cx);
                        real_view.read(cx).is_busy()
                    })
                    .unwrap();
                if !busy {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            cx.update_window(real_handle, |_, _, cx| {
                assert!(
                    !real_view.read(cx).is_busy(),
                    "Background operation completed"
                )
            })
            .unwrap();
        }};
    }
    settle!();
    cx.update_window(real_handle, |_, w, cx| {
        let button = w.find("stage-all").bounds();
        assert!(
            button.origin.y > px(750.),
            "Bulk action stays below the scrolling file list"
        );
        w.click("stage-all", cx);
        w.render_frame(cx);
        w.click("stage-all", cx);
    })
    .unwrap();
    settle!();
    cx.update_window(real_handle, |_, w, cx| {
        assert!(real_view.read(cx).selected_file().is_none());
        w.press("cmd-shift-s", cx);
        w.render_frame(cx);
        w.click("stage-all", cx);
    })
    .unwrap();
    settle!();
    cx.update_window(real_handle, |_, w, cx| {
        assert!(real_view.read(cx).selected_file().is_none());
        w.press("cmd-shift-s", cx);
        w.render_frame(cx);
    })
    .unwrap();
    cx.update_window(real_handle, |_, w, cx| {
        assert_eq!(
            real_view.read(cx).selected_file().map(|p| p.as_str()),
            Some("a.rs")
        );
        w.hover("b.rs", cx);
        w.render_frame(cx);
        w.click(("file-action", 1usize), cx);
    })
    .unwrap();
    settle!();
    cx.update_window(real_handle, |_, w, cx| {
        assert_eq!(
            real_view.read(cx).selected_file().map(|p| p.as_str()),
            Some("a.rs"),
            "Staging a different row preserves selection"
        );
        w.press("cmd-s", cx);
        w.press("cmd-s", cx);
    })
    .unwrap();
    settle!();
    cx.update_window(real_handle, |_, w, cx| {
        assert!(real_view.read(cx).selected_file().is_none());
        w.press("cmd-shift-s", cx);
        w.render_frame(cx);
        assert_eq!(
            real_view.read(cx).selected_file().map(|p| p.as_str()),
            Some("b.rs")
        );
        w.click("a.rs", cx);
        w.render_frame(cx);
        w.hover("preview-header", cx);
        w.render_frame(cx);
        w.click("stage-file", cx);
    })
    .unwrap();
    settle!();
    cx.update_window(real_handle, |_, w, cx| {
        assert_eq!(
            real_view.read(cx).selected_file().map(|p| p.as_str()),
            Some("b.rs")
        );
        w.press("cmd-shift-s", cx);
        w.render_frame(cx);
        assert_eq!(
            real_view.read(cx).selected_file().map(|p| p.as_str()),
            Some("a.rs")
        );
    })
    .unwrap();
    std::fs::write(root.join(".git/index.lock"), b"ui test lock").unwrap();
    cx.update_window(real_handle, |_, w, cx| w.press("cmd-s", cx))
        .unwrap();
    settle!();
    cx.update_window(real_handle, |_, w, cx| {
        assert_eq!(
            real_view.read(cx).selected_file().map(|p| p.as_str()),
            Some("a.rs")
        );
        assert!(
            w.try_find("retry").is_some(),
            "Failed mutation exposes retry alongside preview"
        );
    })
    .unwrap();
    std::fs::remove_file(root.join(".git/index.lock")).unwrap();
    cx.update_window(real_handle, |_, w, cx| w.press("cmd-s", cx))
        .unwrap();
    settle!();
    cx.update_window(real_handle, |_, _, cx| {
        assert!(real_view.read(cx).selected_file().is_none())
    })
    .unwrap();
    assert_eq!(std::fs::read_to_string(root.join("a.rs")).unwrap(), "one\n");
    assert_eq!(std::fs::read_to_string(root.join("b.rs")).unwrap(), "two\n");
    cx.update_window(real_handle, |_, w, cx| {
        w.press("cmd-shift-s", cx);
        w.render_frame(cx);
        w.click("b.rs", cx);
        w.dispatch_action(Box::new(app::SplitMode), cx);
        w.render_frame(cx);
    })
    .unwrap();
    // Right-click settings, cancellation, and saved appearance survive restart.
    cx.update_window(real_handle, |_, w, cx| {
        use gpui_kit::InputEvent as _;
        let position = w.find(("project", 0usize)).bounds().center();
        w.dispatch_event(
            gpui_kit::MouseDownEvent {
                button: gpui_kit::MouseButton::Right,
                position,
                modifiers: Default::default(),
                click_count: 1,
                first_mouse: false,
            }
            .to_platform_input(),
            cx,
        );
        w.render_frame(cx);
        assert!(w.try_find("project-settings").is_some());
        w.press("cmd-a", cx);
        w.input("Temporary name", cx);
        w.press("escape", cx);
        w.render_frame(cx);
        assert!(w.try_find("project-settings").is_none());
        w.dispatch_action(
            Box::new(app::EditProject {
                id: real_view.read(cx).active_project_id().unwrap(),
            }),
            cx,
        );
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(real_handle, |_, w, cx| {
        w.render_frame(cx);
        w.press("cmd-a", cx);
        w.input("Test garden", cx);
        w.click("project-emoji", cx);
        w.input("🪴", cx);
        w.render_frame(cx);
        w.click("project-color", cx);
        w.render_frame(cx);
        w.press("escape", cx);
        w.render_frame(cx);
        assert!(
            w.try_find("project-settings").is_some(),
            "Escape closes the color picker before the settings dialog"
        );
    })
    .unwrap();
    cx.update_window(real_handle, |_, w, cx| {
        w.click("project-color", cx);
        w.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(real_handle, |_, w, cx| {
        w.render_frame(cx);
        let color = real_view.read(cx).project_color_state().unwrap();
        let hex = color.read(cx).hex_input().clone();
        hex.read(cx).focus_handle(cx).focus(w, cx);
        w.render_frame(cx);
        w.press("cmd-a", cx);
        w.input("#FF8175", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(real_handle, |_, w, cx| {
        w.render_frame(cx);
        w.press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(real_handle, |_, w, cx| w.render_frame(cx))
        .unwrap();
    cx.capture_screenshot(real_handle)
        .unwrap()
        .save("target/previews/diffy-project-settings.png")
        .unwrap();
    cx.update_window(real_handle, |_, w, cx| {
        w.click("settings-save", cx);
        w.render_frame(cx);
    })
    .unwrap();
    let saved = diffy::state::load(&prefs).unwrap();
    assert_eq!(saved.projects.len(), 1);
    assert!(saved.projects[0].staged);
    assert!(saved.projects[0].split);
    assert_eq!(saved.projects[0].selected[0].as_deref(), Some("b.rs"));
    assert_eq!(saved.projects[0].name.as_deref(), Some("Test garden"));
    assert_eq!(saved.projects[0].color, 0xff8175);
    assert_eq!(
        saved.projects[0].icon,
        Some(diffy::state::ProjectIcon::Emoji("🪴".into()))
    );
    // A deliberately wide image exercises crop, corner masks, and hover stability.
    let mut icon_fixture = cx.capture_screenshot(handle).unwrap();
    let width = icon_fixture.width();
    let height = icon_fixture.height();
    for (index, pixel) in icon_fixture.pixels_mut().enumerate() {
        pixel.0 = [
            ((index as u32 % width) * 255 / width) as u8,
            ((index as u32 / width) * 255 / height) as u8,
            80,
            255,
        ];
    }
    let icon_path = root.join("preferences/icon.png");
    icon_fixture.save(&icon_path).unwrap();
    let mut image_saved = saved.clone();
    image_saved.projects[0].icon = Some(diffy::state::ProjectIcon::Image(icon_path));
    diffy::state::save(&prefs, &image_saved).unwrap();
    let (restored_handle, restored_view) = cx
        .update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    show: false,
                    focus: false,
                    ..Default::default()
                },
                cx,
                |w, cx| {
                    cx.new(|cx| {
                        app::Diffy::with_options(Vec::new(), false, Some(prefs.clone()), w, cx)
                    })
                },
            )
        })
        .unwrap();
    for _ in 0..500 {
        cx.run_until_parked();
        let busy = cx
            .update_window(restored_handle, |_, w, cx| {
                w.render_frame(cx);
                restored_view.read(cx).is_busy()
            })
            .unwrap();
        if !busy {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    for _ in 0..30 {
        cx.run_until_parked();
        cx.advance_clock(std::time::Duration::from_millis(16));
        cx.update_window(restored_handle, |_, w, cx| w.render_frame(cx))
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let (center, scale) = cx
        .update_window(restored_handle, |_, w, _| {
            (
                w.find(("project", 0usize)).bounds().center(),
                w.scale_factor(),
            )
        })
        .unwrap();
    let image_before = cx.capture_screenshot(restored_handle).unwrap();
    cx.update_window(restored_handle, |_, w, cx| {
        w.hover(("project", 0usize), cx);
        w.render_frame(cx);
    })
    .unwrap();
    cx.advance_clock(std::time::Duration::from_millis(300));
    cx.run_until_parked();
    cx.update_window(restored_handle, |_, w, cx| w.render_frame(cx))
        .unwrap();
    let image_hovered = cx.capture_screenshot(restored_handle).unwrap();
    for y in -16..16 {
        for x in -16..16 {
            let sx = ((f32::from(center.x) + x as f32) * scale).round() as u32;
            let sy = ((f32::from(center.y) + y as f32) * scale).round() as u32;
            assert_eq!(
                image_before.get_pixel(sx, sy),
                image_hovered.get_pixel(sx, sy),
                "Hover must not resize or shift image pixels"
            );
        }
    }
    let middle = image_before.get_pixel(
        (f32::from(center.x) * scale) as u32,
        (f32::from(center.y) * scale) as u32,
    );
    assert_eq!(
        middle.0[2], 80,
        "The image is loaded rather than showing a fallback"
    );
    let corner = image_before.get_pixel(
        ((f32::from(center.x) - 20.) * scale) as u32,
        ((f32::from(center.y) - 20.) * scale) as u32,
    );
    assert!(
        corner.0[..3].iter().all(|c| *c < 5),
        "Image corners follow the rounded squircle mask"
    );
    image_hovered
        .save("target/previews/diffy-image-hover.png")
        .unwrap();
    cx.update_window(restored_handle, |_, w, cx| {
        assert!(!restored_view.read(cx).is_busy());
        assert_eq!(restored_view.read(cx).project_count(), 1);
        assert_eq!(
            restored_view.read(cx).selected_group(),
            diffy::repository::ChangeGroup::Staged
        );
        assert_eq!(
            restored_view.read(cx).selected_file().map(|p| p.as_str()),
            Some("b.rs")
        );
        assert_eq!(
            restored_view.read(cx).selected_mode(cx),
            gpui_kit::component::diff::DiffMode::Split
        );
        assert!(
            gpui_kit::base::test_support::snapshots(w)
                .iter()
                .any(|r| r.label() == Some("Test garden")),
            "Custom project name survives background reload and restart"
        );
        w.dispatch_action(
            Box::new(app::EditProject {
                id: restored_view.read(cx).active_project_id().unwrap(),
            }),
            cx,
        );
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(restored_handle, |_, w, cx| {
        w.render_frame(cx);
        w.click("settings-remove", cx);
        w.render_frame(cx);
        assert_eq!(restored_view.read(cx).project_count(), 0);
        assert!(w.try_find("open-empty").is_some());
        w.press("cmd-down", cx);
        w.press("cmd-shift-down", cx);
        w.press("cmd-shift-s", cx);
        w.render_frame(cx);
    })
    .unwrap();
    assert!(diffy::state::load(&prefs).unwrap().projects.is_empty());
    assert_eq!(std::fs::read_to_string(root.join("a.rs")).unwrap(), "one\n");
    // Resolve nested folders to one repository and keep missing roots recoverable.
    let nested = root.join("nested");
    std::fs::create_dir(&nested).unwrap();
    let (dedup_handle, dedup_view) = cx
        .update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    show: false,
                    focus: false,
                    ..Default::default()
                },
                cx,
                |w, cx| {
                    cx.new(|cx| {
                        app::Diffy::with_options(vec![root.clone(), nested], false, None, w, cx)
                    })
                },
            )
        })
        .unwrap();
    for _ in 0..500 {
        cx.run_until_parked();
        let done = cx
            .update_window(dedup_handle, |_, w, cx| {
                w.render_frame(cx);
                dedup_view.read(cx).project_count() == 1 && !dedup_view.read(cx).is_busy()
            })
            .unwrap();
        if done {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    cx.update_window(dedup_handle, |_, _, cx| {
        assert_eq!(dedup_view.read(cx).project_count(), 1)
    })
    .unwrap();
    let (missing_handle, missing_view) = cx
        .update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    show: false,
                    focus: false,
                    ..Default::default()
                },
                cx,
                |w, cx| {
                    cx.new(|cx| {
                        app::Diffy::with_options(vec![root.join("missing")], false, None, w, cx)
                    })
                },
            )
        })
        .unwrap();
    for _ in 0..500 {
        cx.run_until_parked();
        let done = cx
            .update_window(missing_handle, |_, w, cx| {
                w.render_frame(cx);
                !missing_view.read(cx).is_busy()
            })
            .unwrap();
        if done {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    cx.update_window(missing_handle, |_, w, cx| {
        assert_eq!(missing_view.read(cx).project_count(), 1);
        assert!(w.try_find("retry").is_some());
        w.dispatch_action(Box::new(app::RemoveCurrentProject), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(missing_handle, |_, w, cx| {
        w.render_frame(cx);
        assert_eq!(missing_view.read(cx).project_count(), 0);
    })
    .unwrap();
    // Large files must have a bounded viewport and reach their final changed line.
    let large_root = root.join("large-repository");
    std::fs::create_dir(&large_root).unwrap();
    assert!(
        std::process::Command::new("git")
            .arg("-C")
            .arg(&large_root)
            .args(["init", "--initial-branch=main"])
            .output()
            .unwrap()
            .status
            .success()
    );
    let large = format!(
        "{}\n{}TAIL_3000\n",
        "wide".repeat(1000),
        (0..3000).map(|n| format!("line_{n}\n")).collect::<String>()
    );
    std::fs::write(large_root.join("large.txt"), &large).unwrap();
    let (large_handle, large_view) = cx
        .update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Default::default(),
                        size: size(px(1440.), px(900.)),
                    })),
                    show: false,
                    focus: false,
                    ..Default::default()
                },
                cx,
                |w, cx| {
                    cx.new(|cx| {
                        app::Diffy::with_options(vec![large_root.clone()], false, None, w, cx)
                    })
                },
            )
        })
        .unwrap();
    for _ in 0..500 {
        cx.run_until_parked();
        cx.advance_clock(std::time::Duration::from_millis(20));
        let done = cx
            .update_window(large_handle, |_, w, cx| {
                w.render_frame(cx);
                large_view.read(cx).live_ready() && !large_view.read(cx).is_busy()
            })
            .unwrap();
        if done {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    cx.update_window(large_handle, |_, w, cx| {
        assert!(large_view.read(cx).live_ready());
        assert_eq!(large_view.read(cx).selected_additions(), 3002);
        assert!(
            w.find("diff-body").bounds().size.height < px(850.),
            "Diff must be bounded by its pane"
        );
        w.hover("diff-body", cx);
        w.render_frame(cx);
        for _ in 0..8 {
            w.scroll(
                "diff-body",
                gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(-12000.))),
                cx,
            );
            w.render_frame(cx);
        }

        assert!(
            gpui_kit::base::test_support::snapshots(w)
                .iter()
                .any(|row| row.label() == Some("TAIL_3000")),
            "Wheel scrolling reaches the final line despite horizontal overflow"
        );
        w.press("cmd-enter", cx);
        w.press("home", cx);
        w.render_frame(cx);
        w.press("end", cx);
        w.render_frame(cx);
        assert!(
            gpui_kit::base::test_support::snapshots(w)
                .iter()
                .any(|row| row.label() == Some("TAIL_3000")),
            "End reaches the final line"
        );
        w.click("split", cx);
        w.render_frame(cx);
        w.press("cmd-enter", cx);
        w.press("end", cx);
        w.render_frame(cx);
        assert!(
            gpui_kit::base::test_support::snapshots(w)
                .iter()
                .any(|row| row.label() == Some("TAIL_3000")),
            "Split mode reaches the final line"
        );
        let body = w.find("diff-body").bounds();
        let tail = gpui_kit::base::test_support::snapshots(w)
            .into_iter()
            .find(|row| row.label() == Some("TAIL_3000"))
            .unwrap();
        assert!(
            tail.bounds().origin.x < body.origin.x + body.size.width * 0.2,
            "Added files use the full pane rather than an empty original column"
        );
        assert!(
            tail.bounds().origin.x + tail.bounds().size.width <= body.origin.x + body.size.width,
            "The added source fits inside the viewport"
        );
        w.scroll(
            "diff-body",
            gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(200.))),
            cx,
        );
        w.render_frame(cx);
        let rows = gpui_kit::base::test_support::snapshots(w);
        let row = rows
            .iter()
            .find(|r| {
                r.label().is_some_and(|s| s.starts_with("line_"))
                    && r.bounds().origin.y > body.origin.y + px(50.)
                    && r.bounds().origin.y < body.origin.y + body.size.height - px(50.)
            })
            .unwrap();
        let label = row.label().unwrap().to_owned();
        let before_y = row.bounds().origin.y;
        w.scroll(
            "diff-body",
            gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(-8.))),
            cx,
        );
        w.render_frame(cx);
        let after_y = gpui_kit::base::test_support::snapshots(w)
            .into_iter()
            .find(|r| r.label() == Some(label.as_str()))
            .unwrap()
            .bounds()
            .origin
            .y;
        assert!(
            (before_y - after_y - px(8.)).abs() < px(1.),
            "Trackpad scrolling retains pixel precision rather than snapping to rows"
        );
        w.press("end", cx);
        w.render_frame(cx);
    })
    .unwrap();
    // Working-tree edits and new files refresh without keyboard or manual refresh.
    std::fs::write(large_root.join("large.txt"), format!("{large}LIVE_TAIL\n")).unwrap();
    for _ in 0..500 {
        cx.run_until_parked();
        cx.advance_clock(std::time::Duration::from_millis(50));
        let done = cx
            .update_window(large_handle, |_, w, cx| {
                w.render_frame(cx);
                large_view.read(cx).selected_additions() == 3003 && !large_view.read(cx).is_busy()
            })
            .unwrap();
        if done {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    cx.update_window(large_handle, |_, _, cx| {
        assert_eq!(
            large_view.read(cx).selected_additions(),
            3003,
            "Live edit detected"
        )
    })
    .unwrap();
    cx.update_window(large_handle, |_, w, cx| {
        w.render_frame(cx);
        assert!(
            gpui_kit::base::test_support::snapshots(w)
                .iter()
                .any(|r| r.label() == Some("TAIL_3000")),
            "Live edits preserve the reading position"
        );
    })
    .unwrap();
    std::fs::write(large_root.join("new.txt"), "live addition\n").unwrap();
    for _ in 0..500 {
        cx.run_until_parked();
        cx.advance_clock(std::time::Duration::from_millis(50));
        let done = cx
            .update_window(large_handle, |_, w, cx| {
                w.render_frame(cx);
                w.try_find("new.txt").is_some()
            })
            .unwrap();
        if done {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    cx.update_window(large_handle, |_, w, _| {
        assert!(
            w.try_find("new.txt").is_some(),
            "Live untracked file detected"
        )
    })
    .unwrap();
    // Git index changes made outside Diffy also update the two scopes.
    assert!(
        std::process::Command::new("git")
            .arg("-C")
            .arg(&large_root)
            .args(["add", "--", "large.txt"])
            .output()
            .unwrap()
            .status
            .success()
    );
    for _ in 0..500 {
        cx.run_until_parked();
        cx.advance_clock(std::time::Duration::from_millis(50));
        let done = cx
            .update_window(large_handle, |_, w, cx| {
                w.render_frame(cx);
                large_view
                    .read(cx)
                    .selected_file()
                    .is_some_and(|p| p.as_str() == "new.txt")
                    && !large_view.read(cx).is_busy()
            })
            .unwrap();
        if done {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    cx.update_window(large_handle, |_, w, cx| {
        assert_eq!(
            large_view.read(cx).selected_file().map(|p| p.as_str()),
            Some("new.txt")
        );
        w.press("cmd-shift-s", cx);
        w.render_frame(cx);
        assert_eq!(
            large_view.read(cx).selected_file().map(|p| p.as_str()),
            Some("large.txt")
        );
        assert_eq!(large_view.read(cx).selected_additions(), 3003);
    })
    .unwrap();
    let long_path =
        "zz/very-long-directory-name/another-long-directory/deeply-nested-source/capture-build.ts";
    std::fs::create_dir_all(large_root.join(long_path).parent().unwrap()).unwrap();
    std::fs::write(large_root.join(long_path), "export const ready = true;\n").unwrap();
    cx.update_window(large_handle, |_, w, cx| {
        w.press("cmd-shift-s", cx);
        w.render_frame(cx);
    })
    .unwrap();
    for _ in 0..500 {
        cx.run_until_parked();
        cx.advance_clock(std::time::Duration::from_millis(50));
        if cx
            .update_window(large_handle, |_, w, cx| {
                w.render_frame(cx);
                w.try_find(long_path).is_some()
            })
            .unwrap()
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    cx.update_window(large_handle, |_, w, cx| {
        let list = w.find("file-list").bounds();
        let name = gpui_kit::base::test_support::snapshots(w)
            .into_iter()
            .find(|r| r.label() == Some("capture-build.ts"))
            .unwrap();
        assert!(
            name.bounds().size.width >= px(110.),
            "Directory compression leaves enough room for the filename"
        );
        assert!(
            name.bounds().origin.x >= list.origin.x
                && name.bounds().origin.x + name.bounds().size.width
                    <= list.origin.x + list.size.width,
            "Filename stays inside the sidebar"
        );
        w.hover(long_path, cx);
        w.render_frame(cx);
    })
    .unwrap();
    cx.advance_clock(std::time::Duration::from_millis(1000));
    cx.run_until_parked();
    cx.update_window(large_handle, |_, w, cx| w.render_frame(cx))
        .unwrap();
    cx.capture_screenshot(large_handle)
        .unwrap()
        .save("target/previews/diffy-file-paths.png")
        .unwrap();
    std::fs::remove_dir_all(root).unwrap();
    println!("Native render and interaction checks passed; previews in target/previews.");
}
