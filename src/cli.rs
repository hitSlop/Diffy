//! User-owned terminal launcher and read-only project-open requests.
use std::path::{Path, PathBuf};
use url::Url;

#[derive(serde::Serialize, serde::Deserialize)]
struct LaunchConfig {
    managed_by: String,
    executable: PathBuf,
}
pub fn launcher_target(config: &Path) -> Result<PathBuf, String> {
    let saved: LaunchConfig =
        serde_json::from_slice(&std::fs::read(config).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    if saved.managed_by != "com.hitslop.diffy" || !saved.executable.is_absolute() {
        return Err("This configuration isn't a Diffy launcher.".into());
    }
    Ok(saved.executable)
}

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
    Err("Run the Diffy executable with repository paths to open projects.".into())
}

pub fn install_current() -> Result<String, String> {
    let executable = crate::updates::launch_executable()?;
    if cfg!(target_os = "macos") && crate::updates::bundle_path(&executable).is_none() {
        return Err("Install the terminal command from a packaged Diffy app.".into());
    }
    let home = directories::BaseDirs::new().ok_or("Home directory unavailable")?;
    #[cfg(unix)]
    let directory = home.home_dir().join(".local/bin");
    #[cfg(windows)]
    let directory = home.data_local_dir().join("hitSlop/Diffy/bin");
    let name = if cfg!(windows) { "diffy.exe" } else { "diffy" };
    install_at(&directory.join(name), &executable)?;
    let on_path = std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|p| p == directory));
    let mut report = format!(
        "Installed {}\n\nRun diffy . to open a repository in this window.\nReinstall this command if you move Diffy.",
        directory.join(name).display()
    );
    if !on_path {
        #[cfg(unix)]
        report.push_str("\n\nIf your terminal cannot find diffy, add this line to your shell configuration and open a new terminal:\n\nexport PATH=\"$HOME/.local/bin:$PATH\"");
        #[cfg(windows)]
        report.push_str(&format!("\n\nAdd {} to your user Path in Windows Environment Variables, then open a new terminal.", directory.display()));
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
        drop(file);
        if replacing {
            fs::rename(&temporary, destination)
        } else {
            fs::hard_link(&temporary, destination)
        }
    })();
    let _ = fs::remove_file(&temporary);
    result.map_err(|e| format!("Couldn't install terminal command: {e}"))
}

#[cfg(windows)]
fn install_at(destination: &Path, executable: &Path) -> Result<(), String> {
    let parent = destination.parent().ok_or("Invalid launcher directory")?;
    let config = parent.join("diffy-launch.json");
    if destination.exists() && launcher_target(&config).is_err() {
        return Err(format!(
            "{} already exists and isn't a Diffy launcher.",
            destination.display()
        ));
    }
    let source = executable
        .parent()
        .ok_or("Invalid application directory")?
        .join("diffy-cli.exe");
    if !source.is_file() {
        return Err("Install the terminal command from a packaged Diffy app.".into());
    }
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    use std::io::Write as _;
    temporary
        .write_all(
            &serde_json::to_vec(&LaunchConfig {
                managed_by: "com.hitslop.diffy".into(),
                executable: executable.to_path_buf(),
            })
            .map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
    temporary.persist(&config).map_err(|e| e.to_string())?;
    let shim = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    std::fs::copy(source, shim.path()).map_err(|e| e.to_string())?;
    shim.persist(destination).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
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
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let destination = root.join("bin/diffy");
        let executable = root.join("app with ' quotes");
        fs::write(&executable, "#!/bin/sh\nprintf '%s\\n' \"$@\"\n").unwrap();
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
        install_at(&destination, &executable).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        let output = loop {
            match std::process::Command::new(&destination)
                .arg("space 日本語 $(nope)")
                .output()
            {
                // Concurrent tests spawn processes while this executable is
                // written. A forked child can briefly inherit its write fd,
                // even after this thread closes it (Linux ETXTBSY).
                Err(error)
                    if cfg!(target_os = "linux")
                        && error.raw_os_error() == Some(26)
                        && std::time::Instant::now() < deadline =>
                {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                result => break result.unwrap(),
            }
        };
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
    }
}
