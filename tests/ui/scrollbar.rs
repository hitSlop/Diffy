use gpui_kit::{
    App, AppContext as _, Context, Entity, HeadlessAppContext, IntoElement, ParentElement as _,
    Pixels, Quad, Render, Rgba, ScrollDelta, Size, Styled as _, Window, WindowBounds,
    WindowOptions,
    component::{
        ActiveTheme as _,
        diff::{Diff, DiffFile, DiffMode, DiffState},
    },
    div, point, px, size,
    test::TestWindowExt as _,
};

struct Fixture {
    diff: Entity<DiffState>,
    size: Size<Pixels>,
}

impl Render for Fixture {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().p_4().child(
            div()
                .relative()
                .w(self.size.width)
                .h(self.size.height)
                .child(
                    Diff::new(&self.diff)
                        .size_full()
                        .border_0()
                        .header_visible(false)
                        .syntax_highlight(false)
                        .soft_wrap(true),
                ),
        )
    }
}

fn long_file() -> Vec<DiffFile> {
    let mut patch = String::from("--- a/marks.txt\n+++ b/marks.txt\n@@ -1,239 +1,239 @@\n");
    for line in 0..240 {
        match line {
            0 => patch.push_str("-removed_at_start\n"),
            40 => patch.push_str("+added_near_top\n"),
            80 => patch.push_str(&format!(" {}\n", "wrapped context ".repeat(80))),
            120 => patch.push_str("-old_middle\n+new_middle\n"),
            239 => patch.push_str("-old_tail\n+new_tail\n"),
            _ => patch.push_str(&format!(" context_{line}\n")),
        }
    }
    DiffFile::parse(&patch).unwrap()
}

fn short_file() -> Vec<DiffFile> {
    DiffFile::parse("--- a/short.txt\n+++ b/short.txt\n@@ -1 +1 @@\n-old\n+new\n").unwrap()
}

fn marks(window: &Window, cx: &App) -> Vec<Quad> {
    let body = window
        .find("diff-body")
        .bounds()
        .scale(window.scale_factor());
    window
        .painted_quads()
        .into_iter()
        .filter(|quad| {
            (quad.background == cx.theme().danger.into()
                || quad.background == cx.theme().success.into())
                && quad.bounds.left() >= body.right() - px(16.).scale(window.scale_factor())
                && quad.bounds.size.width == px(3.).scale(window.scale_factor())
        })
        .collect()
}

#[track_caller]
fn assert_marks(window: &Window, cx: &App, expected: bool) {
    let marks = marks(window, cx);
    assert_eq!(!marks.is_empty(), expected, "scrollbar marks visibility");
    if !expected {
        return;
    }
    let scale = window.scale_factor();
    let body = window.find("diff-body").bounds().scale(scale);
    for color in [cx.theme().danger, cx.theme().success] {
        assert!(marks.iter().any(|quad| quad.background == color.into()));
    }
    for quad in &marks {
        assert!(quad.bounds.top() >= body.top());
        assert!(
            quad.bounds.bottom() <= body.bottom(),
            "mark {:?}, body {:?}",
            quad.bounds,
            body
        );
        assert!(quad.bounds.right() <= body.right());
        assert!(quad.bounds.size.height >= px(2.).scale(scale));
        assert!(
            quad.bounds.left() >= body.right() - px(3.).scale(scale),
            "all marks use one strip to the right of the thumb"
        );
    }
    assert!(
        marks
            .iter()
            .any(|quad| quad.bounds.top() <= body.top() + px(5.).scale(scale)),
        "first mark {:?}, body {:?}",
        marks[0].bounds,
        body
    );
    assert!(
        marks
            .iter()
            .any(|quad| quad.bounds.bottom() == body.bottom())
    );
}

fn visible_context(window: &Window) -> String {
    gpui_kit::base::test_support::snapshots(window)
        .iter()
        .filter_map(|row| row.label())
        .filter(|label| label.starts_with("context_") || *label == "new_tail")
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn run(cx: &mut HeadlessAppContext) {
    let (handle, fixture) = cx
        .update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(gpui_kit::Bounds::new(
                        Default::default(),
                        size(px(900.), px(700.)),
                    ))),
                    show: false,
                    focus: false,
                    ..Default::default()
                },
                cx,
                |_, cx| {
                    let diff = cx.new(|cx| {
                        DiffState::new(long_file(), cx)
                            .with_mode(DiffMode::Unified)
                            .with_context_lines(None)
                    });
                    cx.new(|_| Fixture {
                        diff,
                        size: size(px(760.), px(580.)),
                    })
                },
            )
        })
        .unwrap();
    // A cold first frame must work without a second render or a scroll event.
    cx.update_window(handle, |_, w, cx| {
        w.render_frame(cx);
        assert_marks(w, cx, true);
    })
    .unwrap();

    // Check actual rendered pixels, as well as the scene geometry.
    let samples = cx
        .update_window(handle, |_, w, cx| {
            marks(w, cx)
                .iter()
                .map(|quad| {
                    let color: Rgba = if quad.background == cx.theme().danger.into() {
                        cx.theme().danger.into()
                    } else {
                        cx.theme().success.into()
                    };
                    let center = quad.bounds.center();
                    (
                        center.x.0 as u32,
                        center.y.0 as u32,
                        [color.r, color.g, color.b].map(|channel| (channel * 255.).round() as u8),
                    )
                })
                .collect::<Vec<_>>()
        })
        .unwrap();
    let image = cx.capture_screenshot(handle).unwrap();
    for (x, y, expected) in samples {
        let actual = image.get_pixel(x, y);
        for channel in 0..3 {
            assert!(
                actual[channel].abs_diff(expected[channel]) <= 3,
                "marker pixel at ({x}, {y}): {actual:?}, expected {expected:?}"
            );
        }
    }
    std::fs::create_dir_all("target/previews").unwrap();
    image
        .save("target/previews/diffy-scrollbar-unified.png")
        .unwrap();

    let diff = cx
        .update_window(handle, |_, _, cx| fixture.read(cx).diff.clone())
        .unwrap();
    cx.update_window(handle, |_, w, cx| {
        w.render_frame(cx);
        let line = gpui_kit::base::test_support::snapshots(w)
            .into_iter()
            .find(|row| row.label() == Some("context_1"))
            .unwrap()
            .bounds();
        w.drag(
            point(line.left() + px(1.), line.center().y),
            point(line.right() - px(1.), line.center().y),
            cx,
        );
        assert!(
            diff.read(cx).selected_text(cx).contains("context_"),
            "text remains selectable"
        );
        // Wheel, thumb drag, and track clicks must still reach the list.
        let before = visible_context(w);
        w.scroll(
            "diff-body",
            ScrollDelta::Pixels(point(px(0.), px(-400.))),
            cx,
        );
        w.render_frame(cx);
        assert_ne!(
            visible_context(w),
            before,
            "wheel scroll changes visible rows"
        );
        assert_marks(w, cx, true);
        diff.update(cx, |diff, cx| diff.scroll_to_file("marks.txt", cx));
        w.render_frame(cx);
        let body = w.find("diff-body").bounds();
        let from = point(body.right() - px(7.), body.top() + px(15.));
        let before = visible_context(w);
        w.drag(from, point(from.x, body.bottom() - px(30.)), cx);
        w.render_frame(cx);
        assert_ne!(visible_context(w), before, "thumb drag scrolls the list");
        diff.update(cx, |diff, cx| diff.scroll_to_file("marks.txt", cx));
        w.render_frame(cx);
        let before = visible_context(w);
        // Click directly on a colored gutter mark rather than in the thumb lane.
        let mark = marks(w, cx)
            .into_iter()
            .find(|quad| quad.bounds.center().y.0 / w.scale_factor() > f32::from(body.center().y))
            .unwrap();
        let center = mark.bounds.center();
        w.click_at(
            "diff-body",
            point(
                px(center.x.0 / w.scale_factor()) - body.left(),
                px(center.y.0 / w.scale_factor()) - body.top(),
            ),
            cx,
        );
        w.render_frame(cx);
        assert_ne!(
            visible_context(w),
            before,
            "colored gutter passes track clicks through"
        );

        diff.update(cx, |diff, cx| diff.set_mode(DiffMode::Split, cx));
        w.render_frame(cx);
        assert_marks(w, cx, true);
        // Narrowing also changes the height of the wrapped context line.
        fixture.update(cx, |view, cx| {
            view.size = size(px(420.), px(420.));
            cx.notify();
        });
        w.render_frame(cx);
        assert_marks(w, cx, true);
    })
    .unwrap();
    cx.capture_screenshot(handle)
        .unwrap()
        .save("target/previews/diffy-scrollbar-split.png")
        .unwrap();

    cx.update_window(handle, |_, w, cx| {
        diff.update(cx, |diff, cx| diff.set_context_lines(Some(0), cx));
        w.render_frame(cx);
        assert_marks(w, cx, false);
        diff.update(cx, |diff, cx| diff.expand_unchanged(cx));
        w.render_frame(cx);
        assert_marks(w, cx, true);
        diff.update(cx, |diff, cx| diff.set_files(short_file(), cx));
        w.render_frame(cx);
        assert_marks(w, cx, false);
        diff.update(cx, |diff, cx| {
            diff.set_files(long_file(), cx);
            diff.set_context_lines(None, cx);
        });
        w.render_frame(cx);
        assert_marks(w, cx, true);
        diff.update(cx, |diff, cx| diff.set_files([], cx));
        w.render_frame(cx);
        assert_marks(w, cx, false);
    })
    .unwrap();
}
