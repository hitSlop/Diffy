#[path = "../benches/support/fixture.rs"]
mod fixture;
use diffy::app;
use gpui_kit::{
    AppContext as _, HeadlessAppContext, WindowOptions, assets::Assets, test::TestWindowExt as _,
};
use std::{sync::Arc, time::Duration};

fn main() {
    let fixture = fixture::Fixture::new("ui");
    let scroll_report = std::env::var_os("DIFFY_SCROLL_REPORT").is_some();
    // Fixture root is the repository; preferences live outside it and its watcher.
    let prefs =
        std::env::temp_dir().join(format!("diffy-reader-prefs-{}.json", std::process::id()));
    let mut initial = diffy::state::SavedState::default();
    initial.discovery.onboarding_completed = true;
    diffy::state::save(&prefs, &initial).unwrap();
    fixture.write("a.rs", "base\n");
    fixture.write("b.rs", "base\n");
    fixture.git(&["add", "."]);
    fixture.git(&["commit", "-m", "base"]);
    fixture.write("a.rs", "one\n");
    fixture.write("b.rs", "one\ntwo\n");
    fixture.write(
        "large.txt",
        &(0..5000).map(|n| format!("line_{n}\n")).collect::<String>(),
    );
    if scroll_report {
        for n in 0..1000 {
            fixture.write(&format!("z-files/{n:04}.txt"), "small file\n");
        }
    }
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
                        app::Diffy::with_options(
                            vec![fixture.0.clone()],
                            false,
                            Some(prefs.clone()),
                            window,
                            cx,
                        )
                    })
                },
            )
        })
        .unwrap();
    let wait = |cx: &mut HeadlessAppContext| {
        for _ in 0..500 {
            cx.run_until_parked();
            let ready = cx
                .update_window(handle, |_, window, cx| {
                    window.render_frame(cx);
                    view.read(cx).live_ready() && !view.read(cx).is_busy()
                })
                .unwrap();
            if ready {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("Preview never became ready");
    };
    let dispatch = |cx: &mut HeadlessAppContext, action: Box<dyn gpui_kit::Action>| {
        cx.update_window(handle, |_, window, cx| window.dispatch_action(action, cx))
            .unwrap();
        cx.run_until_parked();
    };
    wait(&mut cx);
    assert_eq!(cx.update(|cx| view.read(cx).selected_additions()), 1);
    dispatch(&mut cx, Box::new(app::NextFile));
    wait(&mut cx);
    assert_eq!(cx.update(|cx| view.read(cx).selected_additions()), 2);
    let requests = cx.update(|cx| view.read(cx).preview_request_count());
    dispatch(&mut cx, Box::new(app::PreviousFile));
    assert_eq!(cx.update(|cx| view.read(cx).selected_additions()), 1);
    assert_eq!(
        cx.update(|cx| view.read(cx).preview_request_count()),
        requests,
        "Cache hit never starts a preview load"
    );
    dispatch(&mut cx, Box::new(app::CopyRelativePath));
    assert_eq!(
        cx.update(|cx| cx.read_from_clipboard().unwrap().text().unwrap()),
        "a.rs"
    );
    let id = cx.update(|cx| view.read(cx).active_project_id().unwrap());
    dispatch(
        &mut cx,
        Box::new(app::CopyPath {
            id,
            path: "b.rs".into(),
            absolute: false,
        }),
    );
    assert_eq!(
        cx.update(|cx| cx.read_from_clipboard().unwrap().text().unwrap()),
        "b.rs"
    );
    assert_eq!(
        cx.update(|cx| view.read(cx).selected_file().unwrap().to_string()),
        "a.rs",
        "Row utilities do not select another file"
    );
    cx.update_window(handle, |_, window, cx| {
        window.press("cmd-=", cx);
        assert_eq!(view.read(cx).reader_settings().font_size, 14.);
        window.press("cmd-0", cx);
        assert_eq!(view.read(cx).reader_settings().font_size, 13.);
        window.press("cmd-shift-=", cx);
        assert_eq!(view.read(cx).reader_settings().font_size, 14.);
    })
    .unwrap();
    dispatch(&mut cx, Box::new(app::ToggleWrap));
    cx.update(|cx| {
        assert_eq!(view.read(cx).reader_settings().font_size, 14.);
        assert!(!view.read(cx).reader_settings().wrap);
    });
    // Queue competing requests, then return to the cached first path before completion.
    cx.update_window(handle, |_, window, cx| {
        window.dispatch_action(Box::new(app::NextFile), cx);
        window.dispatch_action(Box::new(app::NextFile), cx);
        window.dispatch_action(Box::new(app::PreviousFile), cx);
        window.dispatch_action(Box::new(app::PreviousFile), cx);
    })
    .unwrap();
    wait(&mut cx);
    cx.update(|cx| {
        assert_eq!(
            view.read(cx).selected_preview_path(),
            view.read(cx).selected_file()
        );
        assert_eq!(
            view.read(cx).selected_additions(),
            1,
            "Late results cannot replace the newer selection"
        );
    });
    if let Some(flush) = cx.update(|cx| view.read(cx).preferences_flush()) {
        flush.wait().unwrap();
    }
    let saved = diffy::state::load(&prefs).unwrap();
    assert_eq!(saved.reader.font_size, 14.);
    assert!(!saved.reader.wrap);
    let before = cx.update(|cx| view.read(cx).preview_request_count());
    fixture.write("b.rs", "unrelated change\n");
    for _ in 0..25 {
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(20));
    }
    wait(&mut cx);
    assert_eq!(
        cx.update(|cx| view.read(cx).preview_request_count()),
        before,
        "Unrelated edits do not reload the visible preview"
    );
    dispatch(&mut cx, Box::new(app::NextFile));
    dispatch(&mut cx, Box::new(app::NextFile));
    wait(&mut cx);
    if scroll_report {
        let requests = cx.update(|cx| view.read(cx).preview_request_count());
        for target in ["diff-body", "file-list"] {
            cx.update_window(handle, |_, window, cx| window.hover(target, cx))
                .unwrap();
            let mut samples = Vec::new();
            for n in 0..80 {
                let start = std::time::Instant::now();
                cx.update_window(handle, |_, window, cx| {
                    use gpui_kit::InputEvent as _;
                    window.dispatch_event(
                        gpui_kit::ScrollWheelEvent {
                            position: window.find(target).bounds().center(),
                            delta: gpui_kit::ScrollDelta::Pixels(gpui_kit::point(
                                gpui_kit::px(0.),
                                gpui_kit::px(-24.),
                            )),
                            ..Default::default()
                        }
                        .to_platform_input(),
                        cx,
                    );
                    window.render_frame(cx);
                })
                .unwrap();
                cx.run_until_parked();
                if n >= 20 {
                    samples.push(start.elapsed().as_secs_f64() * 1000.);
                }
            }
            samples.sort_by(f64::total_cmp);
            println!(
                "{target} scroll event + forced headless frame: median={:.2}ms p95={:.2}ms",
                samples[30], samples[56]
            );
        }
        assert_eq!(
            cx.update(|cx| view.read(cx).preview_request_count()),
            requests,
            "Scrolling must not start preview loads"
        );
    }
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.hover("diff-body", cx);
        window.scroll(
            "diff-body",
            gpui_kit::ScrollDelta::Pixels(gpui_kit::point(gpui_kit::px(0.), gpui_kit::px(-4000.))),
            cx,
        );
        window.render_frame(cx);
    })
    .unwrap();
    let anchor = cx
        .update(|cx| view.read(cx).selected_top_line(cx))
        .expect("Scrolled preview has a source anchor");
    assert!(anchor > 20);
    dispatch(&mut cx, Box::new(app::ZoomIn));
    dispatch(&mut cx, Box::new(app::ToggleWrap));
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    assert_eq!(
        cx.update(|cx| view.read(cx).selected_top_line(cx)),
        Some(anchor),
        "Zoom and wrapping retain the visible source line"
    );
    dispatch(&mut cx, Box::new(app::ToggleGroup));
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    dispatch(&mut cx, Box::new(app::ToggleGroup));
    wait(&mut cx);
    assert_eq!(
        cx.update(|cx| view.read(cx).selected_top_line(cx)),
        Some(anchor),
        "Unloading a preview retains its reading anchor"
    );
    if let Some(flush) = cx.update(|cx| view.read(cx).preferences_flush()) {
        flush.wait().unwrap();
    }
    let _ = std::fs::remove_file(prefs);
    // A failed restore must never replace the original with empty defaults,
    // including after reader actions enqueue ordinary navigation saves.
    let invalid_prefs =
        std::env::temp_dir().join(format!("diffy-invalid-prefs-{}.json", std::process::id()));
    let original = b"{unreadable preferences that must survive}";
    std::fs::write(&invalid_prefs, original).unwrap();
    let (failed_handle, failed_view) = cx
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
                        app::Diffy::with_options(
                            vec![],
                            false,
                            Some(invalid_prefs.clone()),
                            window,
                            cx,
                        )
                    })
                },
            )
        })
        .unwrap();
    cx.run_until_parked();
    cx.update_window(failed_handle, |_, window, cx| {
        window.render_frame(cx);
        window.dispatch_action(Box::new(app::ZoomIn), cx);
        window.dispatch_action(Box::new(app::ToggleWrap), cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(
        cx.update(|cx| failed_view.read(cx).preferences_flush())
            .is_none()
    );
    assert_eq!(std::fs::read(&invalid_prefs).unwrap(), original);
    std::fs::remove_file(invalid_prefs).unwrap();
    println!(
        "Lazy previews, cache navigation, stale responses, reader controls and background saves passed."
    );
}
