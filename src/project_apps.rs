//! Installed project destinations, independent of rendering and shell setup.
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize)]
pub enum ProjectApp {
    ITerm,
    Warp,
    Ghostty,
    Terminal,
    WindowsTerminal,
    PowerShell,
    Alacritty,
    VSCode,
    VSCodeInsiders,
    Cursor,
    Zed,
    Finder,
}

impl ProjectApp {
    #[cfg(target_os = "macos")]
    pub const TERMINALS: [Self; 4] = [Self::ITerm, Self::Warp, Self::Ghostty, Self::Terminal];
    #[cfg(windows)]
    pub const TERMINALS: [Self; 4] = [
        Self::WindowsTerminal,
        Self::PowerShell,
        Self::Warp,
        Self::Ghostty,
    ];
    #[cfg(all(unix, not(target_os = "macos")))]
    pub const TERMINALS: [Self; 4] = [Self::Terminal, Self::Alacritty, Self::Warp, Self::Ghostty];
    pub const EDITORS: [Self; 4] = [Self::VSCode, Self::VSCodeInsiders, Self::Cursor, Self::Zed];

    pub fn label(self) -> &'static str {
        match self {
            Self::ITerm => "Open in iTerm",
            Self::Warp => "Open in Warp",
            Self::Ghostty => "Open in Ghostty",
            Self::Terminal => "Open in Terminal",
            Self::WindowsTerminal => "Open in Windows Terminal",
            Self::PowerShell => "Open in PowerShell",
            Self::Alacritty => "Open in Alacritty",
            Self::VSCode => "Open in VS Code",
            Self::VSCodeInsiders => "Open in VS Code Insiders",
            Self::Cursor => "Open in Cursor",
            Self::Zed => "Open in Zed",
            Self::Finder => folder_label(),
        }
    }

    #[cfg(target_os = "macos")]
    fn bundle_id(self) -> &'static str {
        match self {
            Self::ITerm => "com.googlecode.iterm2",
            Self::Warp => "dev.warp.Warp-Stable",
            Self::Ghostty => "com.mitchellh.ghostty",
            Self::Terminal => "com.apple.Terminal",
            Self::WindowsTerminal | Self::PowerShell | Self::Alacritty => "",
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
        let mut command = project_command(app, &application, root);
        #[cfg(not(target_os = "macos"))]
        {
            // Terminals can remain attached for the life of the window. Spawning
            // avoids holding a background worker until the user closes them.
            command
                .spawn()
                .map_err(|e| format!("Couldn't {}: {e}", app.label().to_lowercase()))?;
            Ok(())
        }
        #[cfg(target_os = "macos")]
        {
            let output = command
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
}

#[cfg(target_os = "macos")]
fn open_command(application: &Path, root: &Path) -> std::process::Command {
    let mut command = std::process::Command::new("/usr/bin/open");
    command
        .arg("-a")
        .arg(application)
        .arg(root)
        .current_dir("/");
    command
}

pub fn folder_label() -> &'static str {
    if cfg!(target_os = "macos") {
        "Open in Finder"
    } else if cfg!(windows) {
        "Open in Explorer"
    } else {
        "Open in File Manager"
    }
}
pub fn reveal_label() -> &'static str {
    if cfg!(target_os = "macos") {
        "Reveal in Finder"
    } else if cfg!(windows) {
        "Reveal in Explorer"
    } else {
        "Reveal in File Manager"
    }
}
fn project_command(app: ProjectApp, application: &Path, root: &Path) -> std::process::Command {
    #[cfg(target_os = "macos")]
    {
        let _ = app;
        open_command(application, root)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let mut command = std::process::Command::new(application);
        command.current_dir(root);
        match app {
            ProjectApp::WindowsTerminal => {
                command.arg("-d").arg(root);
            }
            ProjectApp::PowerShell => {
                use base64::Engine as _;
                let script = format!(
                    "Set-Location -LiteralPath '{}'",
                    root.to_string_lossy().replace('\'', "''")
                );
                let encoded: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
                command
                    .args(["-NoExit", "-EncodedCommand"])
                    .arg(base64::engine::general_purpose::STANDARD.encode(encoded));
            }
            ProjectApp::Ghostty => {
                command.arg(format!("--working-directory={}", root.display()));
            }
            ProjectApp::Alacritty => {
                command.arg("--working-directory").arg(root);
            }
            ProjectApp::Terminal => {
                match application
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or_default()
                {
                    "konsole" => {
                        command.arg("--workdir").arg(root);
                    }
                    "gnome-terminal" | "xfce4-terminal" => {
                        command.arg("--working-directory").arg(root);
                    }
                    _ => {}
                }
            }
            _ => {
                command.arg(root);
            }
        }
        command
    }
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

#[cfg(target_os = "linux")]
fn application_path(app: ProjectApp) -> Option<PathBuf> {
    let names: &[&str] = match app {
        ProjectApp::Terminal => &[
            "gnome-terminal",
            "konsole",
            "xfce4-terminal",
            "x-terminal-emulator",
            "xterm",
        ],
        ProjectApp::Warp => &["warp-terminal"],
        ProjectApp::Ghostty => &["ghostty"],
        ProjectApp::Alacritty => &["alacritty"],
        ProjectApp::VSCode => &["code"],
        ProjectApp::VSCodeInsiders => &["code-insiders"],
        ProjectApp::Cursor => &["cursor"],
        ProjectApp::Zed => &["zed", "zeditor"],
        ProjectApp::Finder => &["xdg-open"],
        _ => &[],
    };
    names.iter().find_map(|name| which::which(name).ok())
}
#[cfg(windows)]
fn application_path(app: ProjectApp) -> Option<PathBuf> {
    let names: &[&str] = match app {
        ProjectApp::WindowsTerminal => &["wt.exe"],
        ProjectApp::PowerShell => &["pwsh.exe", "powershell.exe"],
        ProjectApp::Warp => &["warp.exe"],
        ProjectApp::VSCode => &["Code.exe"],
        ProjectApp::VSCodeInsiders => &["Code - Insiders.exe"],
        ProjectApp::Cursor => &["Cursor.exe"],
        ProjectApp::Zed => &["zed.exe"],
        ProjectApp::Finder => &["explorer.exe"],
        _ => &[],
    };
    if let Some(path) = names.iter().find_map(|name| which::which(name).ok()) {
        return Some(path);
    }
    let suffixes: &[&str] = match app {
        ProjectApp::VSCode => &["Microsoft VS Code/Code.exe"],
        ProjectApp::VSCodeInsiders => &["Microsoft VS Code Insiders/Code - Insiders.exe"],
        ProjectApp::Cursor => &["cursor/Cursor.exe"],
        ProjectApp::Zed => &["Zed/zed.exe"],
        ProjectApp::Warp => &["Warp/warp.exe"],
        _ => &[],
    };
    let mut bases: Vec<PathBuf> = ["ProgramFiles", "ProgramFiles(x86)"]
        .into_iter()
        .filter_map(std::env::var_os)
        .map(PathBuf::from)
        .collect();
    if let Some(dirs) = directories::BaseDirs::new() {
        bases.push(dirs.data_local_dir().join("Programs"));
    }
    bases
        .iter()
        .flat_map(|base| suffixes.iter().map(move |suffix| base.join(suffix)))
        .find(|path| path.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
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
