use super::Diffy;
use crate::state;
use gpui_kit::AnyElement;
use gpui_kit::Context;
use gpui_kit::ElementId;
use gpui_kit::InteractiveElement as _;
use gpui_kit::IntoElement;
use gpui_kit::MouseButton;
use gpui_kit::ObjectFit;
use gpui_kit::ParentElement as _;
use gpui_kit::StatefulInteractiveElement as _;
use gpui_kit::Styled as _;
use gpui_kit::StyledImage as _;
use gpui_kit::Window;
use gpui_kit::base::motion::Transition;
use gpui_kit::base::motion::transition;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::IconName;
use gpui_kit::component::InteractiveElementExt as _;
use gpui_kit::component::Placement;
use gpui_kit::component::StyledExt as _;
use gpui_kit::component::badge::Badge;
use gpui_kit::component::button::Button;
use gpui_kit::component::button::ButtonVariants as _;
use gpui_kit::component::h_flex;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::v_flex;
use gpui_kit::div;
use gpui_kit::img;
use gpui_kit::px;
use gpui_kit::rgb;
use std::time::Duration;

impl Diffy {
    pub(super) fn render_projects(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
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
                        Badge::new()
                            .count(project.changed_count)
                            .color(rgb(0x34363c))
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
                                    .tooltip(format!(
                                        "{} · {} changed files",
                                        project.name, project.changed_count
                                    ))
                                    .tooltip_placement(Placement::Right)
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        cx.stop_propagation();
                                        this.switch_project(ix, window, cx)
                                    })),
                            ),
                    )
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.switch_project(ix, window, cx)),
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
                    .overflow_y_scrollbar()
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
                                .tooltip(super::shortcut_label(
                                    &super::OpenProject,
                                    window,
                                    "Open repository",
                                ))
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
}
