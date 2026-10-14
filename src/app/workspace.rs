use super::Diffy;
use crate::repository::ChangeGroup;
use gpui_kit::AnyElement;
use gpui_kit::App;
use gpui_kit::Context;
use gpui_kit::Focusable as _;
use gpui_kit::InteractiveElement as _;
use gpui_kit::IntoElement;
use gpui_kit::MouseButton;
use gpui_kit::ParentElement as _;
use gpui_kit::Styled as _;
use gpui_kit::Window;
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::IconName;
use gpui_kit::component::InteractiveElementExt as _;
use gpui_kit::component::Selectable as _;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::Button;
use gpui_kit::component::button::ButtonVariants as _;
use gpui_kit::component::diff::Diff;
use gpui_kit::component::diff::DiffHunkSeparator;
use gpui_kit::component::diff::DiffMode;
use gpui_kit::component::h_flex;
use gpui_kit::component::kbd::Kbd;
use gpui_kit::component::menu::ContextMenuExt as _;
use gpui_kit::component::toolbar::Toolbar;
use gpui_kit::component::v_flex;
use gpui_kit::div;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::px;

impl Diffy {
    pub(super) fn render_center(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
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
                                .child(
                                    h_flex()
                                        .gap_2()
                                        .children(
                                            Kbd::binding_for_action(
                                                &super::OpenProject,
                                                Some("Diffy"),
                                                window,
                                            )
                                            .map(|k| k.appearance(false)),
                                        )
                                        .child("Open")
                                        .children(
                                            Kbd::binding_for_action(
                                                &super::OpenCommandPalette,
                                                Some("Diffy"),
                                                window,
                                            )
                                            .map(|k| k.appearance(false)),
                                        )
                                        .child("Commands"),
                                ),
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
            .tooltip(super::shortcut_label(
                &super::ToggleGroup,
                window,
                "Switch view",
            ))
            .on_click(cx.listener(move |this, _, window, cx| this.set_group(group, window, cx)))
        });
        v_flex()
            .size_full()
            .min_w_0()
            .min_h_0()
            .child(
                Toolbar::new("review-toolbar")
                    .h(px(46.))
                    .flex_none()
                    .px_3()
                    .gap_1()
                    .contents(scope_buttons.map(IntoElement::into_any_element))
                    .content(
                        div()
                            .id("toolbar-drag-area")
                            .flex_1()
                            .h_full()
                            .on_mouse_down(MouseButton::Left, |_, w, _| w.start_window_move())
                            .on_double_click(|_, w, _| w.titlebar_double_click()),
                    )
                    .content(
                        Button::new("previous-change")
                            .ghost()
                            .xsmall()
                            .icon(IconName::ChevronUp)
                            .disabled(empty)
                            .tooltip(super::shortcut_label(
                                &super::PreviousChange,
                                window,
                                "Previous change",
                            ))
                            .on_click(cx.listener(|this, _, _, cx| this.change(false, cx))),
                    )
                    .content(
                        Button::new("next-change")
                            .ghost()
                            .xsmall()
                            .icon(IconName::ChevronDown)
                            .disabled(empty)
                            .tooltip(super::shortcut_label(
                                &super::NextChange,
                                window,
                                "Next change",
                            ))
                            .on_click(cx.listener(|this, _, _, cx| this.change(true, cx))),
                    )
                    .content(
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
                    .content(
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
                    .content(
                        Button::new("refresh")
                            .ghost()
                            .small()
                            .icon(IconName::RotateCw)
                            .disabled(p.path.is_none() || p.loading)
                            .tooltip(super::shortcut_label(&super::Refresh, window, "Refresh"))
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
                            .child(view.selected.clone().unwrap_or_default())
                            .context_menu({
                                let id = p.id;
                                let path = view.selected.clone().unwrap_or_default();
                                let local = p.path.is_some();
                                move |menu, _, _| {
                                    super::reader::file_menu(menu, id, path.as_str(), local)
                                }
                            }),
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
                            .disabled(
                                empty
                                    || p.path.is_none()
                                    || p.loading
                                    || view.selected_entry().is_some_and(|e| !e.stageable()),
                            )
                            .tooltip(if p.path.is_none() {
                                "Open a repository to stage files".to_owned()
                            } else {
                                super::shortcut_label(
                                    &super::StageFile,
                                    window,
                                    "Stage / unstage file",
                                )
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
            .children(view.preview_error.as_ref().map(|error| {
                h_flex()
                    .px_3()
                    .py_2()
                    .gap_2()
                    .child(div().text_color(cx.theme().danger).child(error.clone()))
                    .child(
                        Button::new("retry-preview")
                            .ghost()
                            .label("Retry")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.ensure_preview(cx);
                            })),
                    )
            }))
            .children(view.selected_entry().filter(|e| !e.stageable()).map(|_| {
                div()
                    .px_3()
                    .py_2()
                    .text_color(cx.theme().danger)
                    .child("Unmerged file · resolve and stage outside Diffy")
            }))
            .child(if !empty && view.preview.is_none() {
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(cx.theme().muted_foreground)
                    .child(if view.preview_error.is_some() && !view.preview_loading {
                        "Preview unavailable"
                    } else {
                        "Loading preview…"
                    })
                    .into_any_element()
            } else if empty {
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
                            .soft_wrap(self.reader.wrap)
                            .split_ratio(0.4)
                            .hunk_separator(DiffHunkSeparator::Simple),
                    )
                    .into_any_element()
            })
            .into_any_element()
    }
    pub(super) fn set_mode(&mut self, mode: DiffMode, cx: &mut Context<Self>) {
        let Some(p) = self.projects.get(self.active) else {
            return;
        };
        for view in &p.views {
            view.diff.update(cx, |d, cx| d.set_mode(mode, cx));
        }
        self.persist(cx);
    }
    pub(super) fn change(&mut self, next: bool, cx: &mut Context<Self>) {
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
    pub(super) fn focus_diff(&self, window: &mut Window, cx: &mut App) {
        if let Some(p) = self.projects.get(self.active) {
            p.view().diff.read(cx).focus_handle(cx).focus(window, cx);
        }
    }
    pub fn selected_mode(&self, cx: &App) -> DiffMode {
        self.projects
            .get(self.active)
            .map_or(DiffMode::Unified, |p| p.view().diff.read(cx).mode())
    }
}
