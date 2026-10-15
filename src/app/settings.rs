use super::Diffy;
use crate::state;
use gpui_kit::AnyElement;
use gpui_kit::AppContext as _;
use gpui_kit::Context;
use gpui_kit::Entity;
use gpui_kit::FocusHandle;
use gpui_kit::Focusable as _;
use gpui_kit::InteractiveElement as _;
use gpui_kit::IntoElement;
use gpui_kit::MouseButton;
use gpui_kit::ObjectFit;
use gpui_kit::ParentElement as _;
use gpui_kit::PathPromptOptions;
use gpui_kit::Styled as _;
use gpui_kit::StyledImage as _;
use gpui_kit::Window;
use gpui_kit::base::TestSupportExt as _;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::FocusTrapElement as _;
use gpui_kit::component::Selectable as _;
use gpui_kit::component::StyledExt as _;
use gpui_kit::component::button::Button;
use gpui_kit::component::button::ButtonVariants as _;
use gpui_kit::component::color_picker::ColorPicker;
use gpui_kit::component::color_picker::ColorPickerState;
use gpui_kit::component::h_flex;
use gpui_kit::component::input::Input;
use gpui_kit::component::input::InputState;
use gpui_kit::component::v_flex;
use gpui_kit::div;
use gpui_kit::img;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::px;
use gpui_kit::rgb;

pub(super) struct ProjectSettings {
    pub(super) id: u64,
    pub(super) focus: FocusHandle,
    pub(super) name: Entity<InputState>,
    pub(super) emoji: Entity<InputState>,
    pub(super) color: Entity<ColorPickerState>,
    pub(super) icon: Option<state::ProjectIcon>,
    pub(super) error: Option<String>,
    image_generation: u64,
    importing_image: bool,
}

impl Diffy {
    pub(super) fn open_settings(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        if !self.projects.iter().any(|p| p.id == id) {
            return;
        }
        self.stop_file_repeat();
        self.reset_file_scroll();
        let p = self.projects.iter().find(|p| p.id == id).unwrap();
        let (name, color, icon) = self.appearance(p);
        let name = name.clone();
        let icon = icon.clone();
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
            image_generation: 0,
            importing_image: false,
        });
        cx.notify();
    }
    pub(super) fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings = None;
        self.previous_focus
            .take()
            .unwrap_or_else(|| self.focus.clone())
            .focus(window, cx);
        cx.notify();
    }
    pub(super) fn save_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(draft) = &self.settings else {
            return;
        };
        if draft.importing_image {
            return;
        }
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
        let color = draft.color.read(cx).value().map(|color| {
            let rgb = color.to_rgb();
            ((rgb.r * 255.).round() as u32) << 16
                | ((rgb.g * 255.).round() as u32) << 8
                | (rgb.b * 255.).round() as u32
        });
        if let Some(ix) = self.projects.iter().position(|p| p.id == draft.id) {
            let common = self.projects[ix].repository.clone();
            if let Some(r) = self
                .repositories
                .iter_mut()
                .find(|r| Some(&r.common_dir) == common.as_ref())
            {
                r.name = name.clone().into();
                r.icon = icon.clone();
                if let Some(color) = color {
                    r.color = color;
                }
                r.mark_appearance_explicit();
                // Keep the main checkout's legacy appearance fields readable by
                // older app versions; sibling names remain independent tab labels.
                if let Some(p) = self
                    .projects
                    .iter_mut()
                    .find(|p| p.repository == common && p.main_worktree)
                {
                    p.name = name.clone().into();
                    p.custom_name = Some(name);
                    p.icon = icon;
                    if let Some(color) = color {
                        p.color = color;
                    }
                }
            } else {
                let p = &mut self.projects[ix];
                p.name = name.clone().into();
                p.custom_name = Some(name);
                p.icon = icon;
                if let Some(color) = color {
                    p.color = color;
                }
            }
        }
        self.persist(cx);
        self.close_settings(window, cx);
    }
    pub(super) fn choose_project_image(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(draft) = self.settings.as_mut() else {
            return;
        };
        draft.image_generation += 1;
        let generation = draft.image_generation;
        // The input entity identifies this particular opening of the dialog.
        let dialog = draft.name.entity_id();
        draft.importing_image = true;
        draft.error = None;
        let state_path = self.state_path.clone();
        cx.notify();
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose project icon".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = match paths.await {
                Ok(Ok(Some(paths))) => match paths.into_iter().next() {
                    Some(path) => {
                        cx.background_executor()
                            .spawn(async move {
                                match state_path {
                                    Some(state_path) => {
                                        state::import_project_icon(&state_path, &path).map(Some)
                                    }
                                    // Demo sessions never write app data.
                                    None => {
                                        let supported = path.extension().and_then(|e| e.to_str())
                                            .is_some_and(|e| gpui_kit::Img::extensions().contains(&e.to_ascii_lowercase().as_str()));
                                        if supported && path.is_file() {
                                            Ok(Some(path))
                                        } else {
                                            Err("Choose an image such as PNG, JPEG, WebP, or SVG.".into())
                                        }
                                    }
                                }
                            })
                            .await
                    }
                    None => Ok(None),
                },
                Ok(Ok(None)) => Ok(None),
                _ => Err("Couldn't open the image picker. Try again.".into()),
            };
            let _ = this.update(cx, |this, cx| {
                if let Some(draft) = &mut this.settings
                    && draft.name.entity_id() == dialog
                    && draft.image_generation == generation
                {
                    draft.importing_image = false;
                    match result {
                        Ok(Some(path)) => draft.icon = Some(state::ProjectIcon::Image(path)),
                        Ok(None) => {}
                        Err(error) => draft.error = Some(error),
                    }
                    cx.notify();
                }
            });
        })
        .detach();
    }
    pub(super) fn render_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let draft = self.settings.as_ref().unwrap();
        let id = draft.id;
        let remove_disabled = self.projects.iter().find(|p| p.id == id).is_none_or(|p| {
            self.repository_mutating(
                self.projects
                    .iter()
                    .position(|other| other.id == p.id)
                    .unwrap(),
            )
        });
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
                                                            s.image_generation += 1;
                                                            s.importing_image = false;
                                                            s.error = None;
                                                        }
                                                        cx.notify();
                                                    })),
                                            )
                                            .child(
                                                Button::new("icon-image")
                                                    .ghost()
                                                    .label("Choose image…")
                                                    .disabled(draft.importing_image)
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
                                                .child(if cfg!(target_os = "macos") {
                                                    "Use ⌃⌘Space for the emoji picker."
                                                } else if cfg!(windows) {
                                                    "Use Win+. for the emoji picker."
                                                } else {
                                                    "Enter or paste an emoji."
                                                }),
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
                                                .child(div().flex_1().min_w_0().truncate().child(
                                                    if path.parent().and_then(|p| p.file_name())
                                                        == Some(std::ffi::OsStr::new(
                                                            "project-icons",
                                                        ))
                                                    {
                                                        "Saved in Diffy".to_owned()
                                                    } else {
                                                        path.file_name()
                                                            .unwrap_or_default()
                                                            .to_string_lossy()
                                                            .to_string()
                                                    },
                                                )),
                                        )
                                    }),
                            )
                            .children(draft.error.as_ref().map(|e| {
                                div()
                                    .text_color(cx.theme().danger)
                                    .text_size(px(12.))
                                    .child(e.clone())
                            }))
                            .when(draft.importing_image, |box_| {
                                box_.child(div().text_size(px(12.)).child("Copying image…"))
                            })
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
                                                    .disabled(draft.importing_image)
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
    #[cfg(feature = "ui-tests")]
    pub fn project_color_state(&self) -> Option<Entity<ColorPickerState>> {
        self.settings.as_ref().map(|s| s.color.clone())
    }
}
