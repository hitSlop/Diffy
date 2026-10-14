//! Installed project destinations, independent of rendering and shell setup.
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize)]
pub enum ProjectApp {
    ITerm,
    Warp,
    Ghostty,
    Terminal,
    VSCode,
    VSCodeInsiders,
    Cursor,
    Zed,
    Finder,
}

impl ProjectApp {
    pub const TERMINALS: [Self; 4] = [Self::ITerm, Self::Warp, Self::Ghostty, Self::Terminal];
    pub const EDITORS: [Self; 4] = [Self::VSCode, Self::VSCodeInsiders, Self::Cursor, Self::Zed];

    pub fn label(self) -> &'static str {
        match self {
            Self::ITerm => "Open in iTerm",
            Self::Warp => "Open in Warp",
            Self::Ghostty => "Open in Ghostty",
            Self::Terminal => "Open in Terminal",
            Self::VSCode => "Open in VS Code",
            Self::VSCodeInsiders => "Open in VS Code Insiders",
            Self::Cursor => "Open in Cursor",
            Self::Zed => "Open in Zed",
            Self::Finder => "Open in Finder",
        }
    }

    #[cfg(target_os = "macos")]
    fn bundle_id(self) -> &'static str {
        match self {
            Self::ITerm => "com.googlecode.iterm2",
            Self::Warp => "dev.warp.Warp-Stable",
            Self::Ghostty => "com.mitchellh.ghostty",
            Self::Terminal => "com.apple.Terminal",
            Self::VSCode => "com.microsoft.VSCode",
            Self::VSCodeInsiders => "com.microsoft.VSCodeInsiders",
            Self::Cursor => "com.todesktop.230313mzl4w4u92",
            Self::Zed => "dev.zed.Zed",
            Self::Finder => "com.apple.finder",
        }
    }
}

/// Implementations run on a background worker. Tests replace this boundary so
/// inspecting a menu never starts an editor or terminal.
pub trait Launcher: Send + Sync {
    fn installed(&self) -> Vec<ProjectApp>;
    fn open(&self, app: ProjectApp, root: &Path) -> Result<(), String>;
}

pub struct SystemLauncher;

impl Launcher for SystemLauncher {
    fn installed(&self) -> Vec<ProjectApp> {
        ProjectApp::TERMINALS
            .into_iter()
            .chain(ProjectApp::EDITORS)
            .filter(|app| application_path(*app).is_some())
            .collect()
    }

    fn open(&self, app: ProjectApp, root: &Path) -> Result<(), String> {
        if !root.is_absolute() || !root.is_dir() {
            return Err("The project folder is unavailable. Check that it still exists and Diffy has access.".into());
        }
        let application = application_path(app).ok_or_else(|| {
            format!(
                "{} is unavailable. Check that the app is still installed.",
                app.label().trim_start_matches("Open in ")
            )
        })?;
        let output = open_command(&application, root)
            .output()
            .map_err(|e| format!("Couldn't {}: {e}", app.label().to_lowercase()))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(format!(
                "Couldn't {}: {}",
                app.label().to_lowercase(),
                String::from_utf8_lossy(&output.stderr).trim()
            ))
        }
    }
}

fn open_command(application: &Path, root: &Path) -> std::process::Command {
    let mut command = std::process::Command::new("/usr/bin/open");
    command
        .arg("-a")
        .arg(application)
        .arg(root)
        .current_dir("/");
    command
}

#[cfg(target_os = "macos")]
fn application_path(app: ProjectApp) -> Option<PathBuf> {
    // LaunchServices also knows about apps outside /Applications. Querying
    // known bundle IDs needs neither Spotlight nor access to user documents.
    objc2::rc::autoreleasepool(|_| {
        let workspace = objc2_app_kit::NSWorkspace::sharedWorkspace();
        let id = objc2_foundation::NSString::from_str(app.bundle_id());
        let url = workspace.URLForApplicationWithBundleIdentifier(&id)?;
        let path = PathBuf::from(url.path()?.to_string());
        path.is_dir().then_some(path)
    })
}

#[cfg(not(target_os = "macos"))]
fn application_path(_: ProjectApp) -> Option<PathBuf> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launching_uses_literal_arguments_and_a_safe_working_directory() {
        let app = Path::new("/Applications/Visual Studio Code.app");
        let root = Path::new("/tmp/space ' quote $HOME `command` $(command) ü");
        let command = open_command(app, root);
        assert_eq!(command.get_program(), "/usr/bin/open");
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            vec![
                std::ffi::OsStr::new("-a"),
                app.as_os_str(),
                root.as_os_str()
            ]
        );
        assert_eq!(command.get_current_dir(), Some(Path::new("/")));
    }

    #[test]
    fn unavailable_folders_fail_before_attempting_to_launch() {
        let root =
            std::env::temp_dir().join(format!("diffy-missing-launch-{}", std::process::id()));
        assert!(
            SystemLauncher
                .open(ProjectApp::Terminal, &root)
                .unwrap_err()
                .contains("folder is unavailable")
        );
        assert!(
            SystemLauncher
                .open(ProjectApp::VSCode, Path::new("relative"))
                .is_err()
        );
    }
}
