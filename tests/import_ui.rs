use diffy::{
    app,
    discovery::{Candidate, Snapshot, Source},
};
use gpui_kit::{
    AppContext as _, Bounds, HeadlessAppContext, ScrollDelta, WindowBounds, WindowOptions,
    assets::Assets, point, px, size, test::TestWindowExt as _,
};
use std::{collections::BTreeSet, path::PathBuf, sync::Arc};

fn main() {
    let mut cx = HeadlessAppContext::with_platform(
        gpui_kit::platform::current_platform(true).text_system(),
        Arc::new(Assets),
        gpui_kit::platform::current_headless_renderer,
    );
    cx.update(|cx| {
        gpui_kit::init(cx);
        app::init(cx);
        cx.set_reduce_motion(true);
    });
    let (handle, view) = cx
        .update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Default::default(),
                        size: size(px(1200.), px(800.)),
                    })),
                    show: false,
                    focus: false,
                    ..Default::default()
                },
                cx,
                |window, cx| cx.new(|cx| app::Diffy::demo(window, cx)),
            )
        })
        .unwrap();
    let snapshot = Snapshot {
        candidates: (0..40)
            .map(|i| Candidate {
                path: PathBuf::from(format!("/tmp/import-fixture/project-{i:02}")),
                sources: if i == 39 {
                    BTreeSet::from([Source::Herdr, Source::Claude])
                } else {
                    BTreeSet::from([Source::Herdr])
                },
                error: None,
                created_at: None,
            })
            .collect(),
        errors: Default::default(),
        herdr_live: false,
    };
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |v, cx| {
            v.open_import_fixture(snapshot.clone(), window, cx)
        });
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    // Finish the dialog entrance animation before clicking its visual bounds.
    for _ in 0..20 {
        cx.advance_clock(std::time::Duration::from_millis(16));
        cx.run_until_parked();
    }
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    std::fs::create_dir_all("target/previews").unwrap();
    cx.capture_screenshot(handle)
        .unwrap()
        .save("target/previews/import-before-click.png")
        .unwrap();
    cx.update_window(handle, |_, window, cx| {
        assert_eq!(view.read(cx).import_selected_count(cx), 40);
        window.click("import-check-0", cx);
        window.render_frame(cx);
        assert_eq!(
            view.read(cx).import_selected_count(cx),
            39,
            "Checkbox pointer clicks must change the selection"
        );
        window.click("import-check-0", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).import_selected_count(cx), 40);
        window.click("import-auto-0", cx);
        window.render_frame(cx);
        assert!(
            view.read(cx).import_automatic(cx)[0],
            "Auto-add checkbox must toggle"
        );
        for _ in 0..8 {
            window.scroll(
                "import-scroll",
                ScrollDelta::Pixels(point(px(0.), px(-400.))),
                cx,
            );
            window.render_frame(cx);
        }
        assert!(
            window.find("import-check-39").bounds().size.height > px(0.),
            "Scroll must reach the last candidate"
        );
        window.click("import-check-39", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).import_selected_count(cx), 39);
        assert!(
            window.find("import-check-39").bounds().size.height > px(0.),
            "Toggling must preserve scroll position"
        );
        assert!(
            window.find("import-check-39-2").bounds().size.height > px(0.),
            "Shared projects must appear under Claude as well"
        );
        window.click("import-check-39-2", cx);
        window.render_frame(cx);
        assert_eq!(
            view.read(cx).import_selected_count(cx),
            40,
            "Shared project rows must use the same selection"
        );
        window.click("import-skip", cx);
        window.render_frame(cx);
        assert!(!view.read(cx).import_is_open());
        assert_eq!(
            view.read(cx).discovery_preferences().automatic,
            [false; 3],
            "Cancel must not commit auto-add changes"
        );
        assert!(view.read(cx).discovery_preferences().onboarding_completed);
    })
    .unwrap();
    std::fs::create_dir_all("target/previews").unwrap();
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |v, cx| v.open_import_fixture(snapshot, window, cx));
        window.render_frame(cx);
    })
    .unwrap();
    for _ in 0..20 {
        cx.advance_clock(std::time::Duration::from_millis(16));
        cx.run_until_parked();
    }
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("import-clear", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).import_selected_count(cx), 0);
    })
    .unwrap();
    cx.capture_screenshot(handle)
        .unwrap()
        .save("target/previews/diffy-import.png")
        .unwrap();
    cx.update_window(handle, |_, window, cx| {
        window.click("import-apply", cx);
        window.render_frame(cx);
        assert!(!view.read(cx).import_is_open());
    })
    .unwrap();
    println!("Native import checkbox, scrolling, cancellation, and selection checks passed.");
}
