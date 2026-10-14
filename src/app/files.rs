use super::Diffy;
use crate::repository::ChangeGroup;
use gpui_kit::AnyElement;
use gpui_kit::Context;
use gpui_kit::InteractiveElement as _;
use gpui_kit::IntoElement;
use gpui_kit::MouseButton;
use gpui_kit::ParentElement as _;
use gpui_kit::SharedString;
use gpui_kit::StatefulInteractiveElement as _;
use gpui_kit::Styled as _;
use gpui_kit::Window;
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::Icon;
use gpui_kit::component::IconName;
use gpui_kit::component::InteractiveElementExt as _;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::StyledExt as _;
use gpui_kit::component::button::Button;
use gpui_kit::component::button::ButtonVariants as _;
use gpui_kit::component::diff::DiffFileStatus;
use gpui_kit::component::h_flex;
use gpui_kit::component::list::ListItem;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::v_flex;
use gpui_kit::component::v_virtual_list;
use gpui_kit::div;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::px;
use gpui_kit::rgb;
use std::time::Duration;

impl Diffy {
    pub(super) fn render_files(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(p) = self.projects.get(self.active) else {
            return div().size_full().bg(rgb(0)).into_any_element();
        };
        let view = p.view();
        let files = view.files.clone();
        let rows = view.rows.clone();
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
                    .relative()
                    .test_support()
                    .flex_1()
                    .min_h_0()
                    .px_2()
                    .track_focus(&view.focus)
                    .key_context("DiffyFiles")
                    .child(
                        v_virtual_list(cx.entity(), "files", view.row_sizes.clone(), move |_, range, _, cx| {
                            range
                                .map(|ix| {
                                    let f = &files[ix];
                                    let row = &rows[ix];
                                    let path = f.path().clone();
                                    let owner = owner.clone();
                                    let action_owner = owner.clone();
                                    let action_path = path.clone();
                                    let tooltip_path = path.clone();
                                    let directory = row.directory.clone();
                                    let filename = row.filename.clone();
                                    let row_group = row.group.clone();
                                    let action_label = if group == ChangeGroup::Changes {
                                        row.stage_label.clone()
                                    } else {
                                        row.unstage_label.clone()
                                    };
                                    let selected = selected.as_ref() == Some(&path);
                                    let (status, color) = match f.status() {
                                        DiffFileStatus::Added => ("A", cx.theme().success),
                                        DiffFileStatus::Deleted => ("D", cx.theme().danger),
                                        DiffFileStatus::Renamed => ("R", cx.theme().primary),
                                        DiffFileStatus::Copied => ("C", cx.theme().primary),
                                        _ => ("M", cx.theme().muted_foreground),
                                    };
                                    div().w_full().h(px(30. + row.gap)).pt(px(row.gap)).child(
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
                                                        .accessibility_label(action_label)
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
                                        }))
                                        .into_any_element()
                                })
                                .collect()
                        })
                        .size_full()
                        .track_scroll(&view.scroll),
                    )
                    .vertical_scrollbar(&view.scroll),
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
    pub(super) fn select_file(
        &mut self,
        path: SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(p) = self.projects.get_mut(self.active) else {
            return;
        };
        let view = p.view_mut();
        view.select(path, cx);
        view.focus.focus(window, cx);
        self.persist(cx);
        cx.notify();
    }
    pub(super) fn move_file(&mut self, direction: isize, cx: &mut Context<Self>) {
        let Some(p) = self.projects.get_mut(self.active) else {
            return;
        };
        let view = p.view_mut();
        if view.files.is_empty() {
            return;
        }
        let current = view
            .selected
            .as_ref()
            .and_then(|path| view.indices.get(path))
            .copied()
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
    pub(super) fn start_file_repeat(
        &mut self,
        direction: isize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
    pub(super) fn stop_file_repeat(&mut self) {
        self.repeat_direction = None;
        self.file_repeat = None;
    }
    #[cfg(feature = "ui-tests")]
    pub fn set_file_key_probe(&mut self, probe: fn(isize) -> bool) {
        self.file_key_probe = probe;
    }
    pub fn selected_file(&self) -> Option<&SharedString> {
        self.projects
            .get(self.active)
            .and_then(|p| p.view().selected.as_ref())
    }
}
