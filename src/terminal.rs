//! Console launcher, also used as the installed Windows command shim.
fn main() {
    let result = run();
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.iter().any(|arg| arg == "--version") {
        println!("Diffy {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if args.iter().any(|arg| arg == "--help") {
        println!(
            "Usage: diffy [--check] [repository ...]\nOpen repositories in Diffy. --check validates repositories without opening a window."
        );
        return Ok(());
    }
    let check = args.first().is_some_and(|arg| arg == "--check");
    let paths = args
        .into_iter()
        .skip(usize::from(check))
        .map(std::path::PathBuf::from)
        .map(|path| diffy::repository::resolve_root(&path))
        .collect::<Result<Vec<_>, _>>()?;
    if check {
        for path in paths {
            let snapshot = diffy::repository::load_metadata(&path)?;
            println!(
                "{}: {} staged, {} changes",
                snapshot.root.display(),
                snapshot.groups[0].len(),
                snapshot.groups[1].len()
            );
        }
        return Ok(());
    }
    let current = std::env::current_exe().map_err(|e| e.to_string())?;
    let parent = current.parent().ok_or("Couldn't locate the launcher")?;
    let config = parent.join("diffy-launch.json");
    let executable: std::path::PathBuf = if config.exists() {
        diffy::cli::launcher_target(&config)?
    } else {
        parent.join(if cfg!(windows) { "diffy.exe" } else { "diffy" })
    };
    if !executable.is_file() || executable == current {
        return Err("Reinstall the terminal command from Diffy.".into());
    }
    std::process::Command::new(executable)
        .args(paths)
        .spawn()
        .map_err(|e| format!("Couldn't open Diffy: {e}"))?;
    Ok(())
}
