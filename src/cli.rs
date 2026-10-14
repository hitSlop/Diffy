//! User-owned terminal launcher and read-only project-open requests.
use std::path::{Path, PathBuf};
use url::Url;

#[cfg(unix)]
const MARKER: &str = "# Managed by com.hitslop.diffy\n";

pub fn open_url(paths: &[PathBuf]) -> Result<String, String> {
    let mut url = Url::parse("diffy://open").expect("static URL");
    for path in paths {
        if !path.is_absolute() {
            return Err("Repository paths must be absolute.".into());
        }
        url.query_pairs_mut().append_pair(
            "path",
            path.to_str().ok_or("Non-UTF-8 paths are not supported.")?,
        );
    }
    Ok(url.into())
}

pub fn paths_from_url(value: &str) -> Result<Vec<PathBuf>, String> {
    let url = Url::parse(value).map_err(|_| "Invalid Diffy URL")?;
    if url.scheme() != "diffy"
        || url.host_str() != Some("open")
        || !matches!(url.path(), "" | "/")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.fragment().is_some()
    {
        return Err("Unsupported Diffy URL.".into());
    }
    url.query_pairs()
        .map(|(key, value)| {
            let path = PathBuf::from(value.as_ref());
            if key != "path" || !path.is_absolute() {
                Err("Invalid repository path in Diffy URL.".into())
            } else {
                Ok(path)
            }
        })
        .collect()
}

#[cfg(target_os = "macos")]
pub fn launch(paths: &[PathBuf]) -> Result<(), String> {
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    let bundle = crate::updates::bundle_path(&executable)
        .ok_or("Install the terminal command from a packaged Diffy app.")?;
    let roots = paths
        .iter()
        .map(|path| crate::repository::resolve_root(path))
        .collect::<Result<Vec<_>, _>>()?;
    let mut command = std::process::Command::new("/usr/bin/open");
    command.arg("-a").arg(bundle);
    if !roots.is_empty() {
        command.arg(open_url(&roots)?);
    }
    let output = command
        .output()
        .map_err(|e| format!("Couldn't open Diffy: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "Couldn't open Diffy: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

#[cfg(not(target_os = "macos"))]
pub fn launch(_: &[PathBuf]) -> Result<(), String> {
    Err("The terminal launcher currently supports macOS.".into())
}

pub fn install_current() -> Result<String, String> {
    if !cfg!(target_os = "macos") {
        return Err("The terminal launcher currently supports macOS.".into());
    }
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    if crate::updates::bundle_path(&executable).is_none() {
        return Err("Install the terminal command from a packaged Diffy app.".into());
    }
    let home = std::env::var_os("HOME").ok_or("Home directory unavailable")?;
    let directory = PathBuf::from(home).join(".local/bin");
    install_at(&directory.join("diffy"), &executable)?;
    let on_path = std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|p| p == directory));
    let mut report = format!(
        "Installed {}\n\nRun diffy . to open a repository in this window.\nReinstall this command if you move Diffy.app.",
        directory.join("diffy").display()
    );
    if !on_path {
        report.push_str("\n\nIf your terminal cannot find diffy, add this line to your shell configuration and open a new terminal:\n\nexport PATH=\"$HOME/.local/bin:$PATH\"");
    }
    Ok(report)
}

#[cfg(unix)]
fn shell_quote(path: &Path) -> Result<String, String> {
    let value = path
        .to_str()
        .ok_or("Non-UTF-8 launcher paths are not supported.")?;
    Ok(format!("'{}'", value.replace('\'', "'\"'\"'")))
}

#[cfg(unix)]
fn install_at(destination: &Path, executable: &Path) -> Result<(), String> {
    use std::{fs, io::Write as _, os::unix::fs::PermissionsExt as _};
    let script = format!(
        "#!/bin/sh\n{MARKER}exec {} --cli \"$@\"\n",
        shell_quote(executable)?
    );
    let replacing = match fs::symlink_metadata(destination) {
        Ok(metadata)
            if metadata.is_file()
                && fs::read_to_string(destination)
                    .is_ok_and(|s| s.starts_with(&format!("#!/bin/sh\n{MARKER}"))) =>
        {
            true
        }
        Ok(_) => {
            return Err(format!(
                "{} already exists and isn't a Diffy launcher. Move it or choose another command name before installing.",
                destination.display()
            ));
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(e) => return Err(e.to_string()),
    };
    let parent = destination.parent().ok_or("Invalid launcher directory")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let temporary = parent.join(format!(
        ".diffy-{}-{}.tmp",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(script.as_bytes())?;
        file.set_permissions(fs::Permissions::from_mode(0o755))?;
        file.sync_all()?;
        if replacing {
            fs::rename(&temporary, destination)
        } else {
            fs::hard_link(&temporary, destination)
        }
    })();
    let _ = fs::remove_file(&temporary);
    result.map_err(|e| format!("Couldn't install terminal command: {e}"))
}

#[cfg(not(unix))]
fn install_at(_: &Path, _: &Path) -> Result<(), String> {
    Err("macOS is required.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn open_requests_round_trip_without_shell_interpolation() {
        let paths = vec![
            PathBuf::from("/tmp/日本語 spaces/#?%&'$(touch nope)"),
            PathBuf::from("/tmp/another"),
        ];
        assert_eq!(paths_from_url(&open_url(&paths).unwrap()).unwrap(), paths);
        for bad in [
            "diffy://stage?path=/tmp/repo",
            "diffy://open?path=relative",
            "diffy://open?command=git",
            "https://open?path=/tmp/repo",
            "diffy://user@open?path=/tmp/repo",
        ] {
            assert!(paths_from_url(bad).is_err(), "{bad}");
        }
    }
    #[cfg(unix)]
    #[test]
    fn installer_preserves_conflicts_and_quotes_arguments() {
        use std::fs;
        let root = std::env::temp_dir().join(format!("diffy-cli-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let destination = root.join("bin/diffy");
        let executable = root.join("app with ' quotes");
        fs::write(&executable, "#!/bin/sh\nprintf '%s\\n' \"$@\"\n").unwrap();
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
        install_at(&destination, &executable).unwrap();
        let output = std::process::Command::new(&destination)
            .arg("space 日本語 $(nope)")
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            "--cli\nspace 日本語 $(nope)\n"
        );
        install_at(&destination, &executable).unwrap();
        fs::write(&destination, "unrelated command").unwrap();
        assert!(install_at(&destination, &executable).is_err());
        assert_eq!(
            fs::read_to_string(&destination).unwrap(),
            "unrelated command"
        );
        fs::remove_file(&destination).unwrap();
        std::os::unix::fs::symlink(&executable, &destination).unwrap();
        assert!(install_at(&destination, &executable).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
