use gpui_kit::{App, FontFeatures};
use std::borrow::Cow;

pub const FAMILY: &str = "CommitMonoV143";
pub fn init(cx: &App) {
    cx.text_system()
        .add_fonts(vec![
            Cow::Borrowed(
                include_bytes!("../assets/fonts/CommitMonoV143-400Regular.otf").as_slice(),
            ),
            Cow::Borrowed(
                include_bytes!("../assets/fonts/CommitMonoV143-500Regular.otf").as_slice(),
            ),
        ])
        .expect("bundled Commit Mono fonts must load");
}
pub fn features() -> FontFeatures {
    FontFeatures(std::sync::Arc::new(vec![
        ("calt".into(), 0),
        ("liga".into(), 0),
    ]))
}
