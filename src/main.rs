use diffy::{app, demo, repository};

use std::path::PathBuf;

use gpui_kit::component::TitleBar;
use gpui_kit::{AppContext as _, WindowBounds, WindowOptions, px, size};

fn main() {
    let mut paths = Vec::new();
    let mut check = false;
    let mut demo_mode = false;
    let mut cli = false;
    let mut literal = false;
    for argument in std::env::args_os().skip(1) {
        if literal {
            paths.push(PathBuf::from(argument));
            continue;
        }
        if argument == "--" {
            literal = true;
        } else if argument == "--cli" {
            cli = true;
        } else if argument == "--version" {
            println!("Diffy {}", env!("CARGO_PKG_VERSION"));
            return;
        } else if argument == "--help" {
            println!(
                "Usage: diffy [--demo] [--check] [repository ...]\nRestores saved projects. --demo opens isolated sample projects. Requires Git for repository mode."
            );
            return;
        } else if argument == "--check" {
            check = true;
        } else if argument == "--demo" {
            demo_mode = true;
        } else {
            paths.push(PathBuf::from(argument));
        }
    }
    if check {
        if paths.is_empty() {
            for project in demo::projects() {
                match repository::GroupSnapshot::parse(project.staged).and_then(|staged| {
                    repository::GroupSnapshot::parse(project.changes)
                        .map(|changes| (staged, changes))
                }) {
                    Ok((staged, changes)) => println!(
                        "{}: {} staged, {} changes (demo)",
                        project.name,
                        staged.files.len(),
                        changes.files.len()
                    ),
                    Err(error) => {
                        eprintln!("{error}");
                        std::process::exit(1);
                    }
                }
            }
        } else {
            for path in paths {
                match repository::load(&path) {
                    Ok(snapshot) => println!(
                        "{}: {} staged, {} changes",
                        snapshot.root.display(),
                        snapshot.staged.files.len(),
                        snapshot.changes.files.len()
                    ),
                    Err(error) => {
                        eprintln!("{error}");
                        std::process::exit(1);
                    }
                }
            }
        }
        return;
    }
    if cli {
        if let Err(error) = diffy::cli::launch(&paths) {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return;
    }
    let (requests, receiver) = async_channel::unbounded();
    let reopen = requests.clone();
    let application = gpui_kit::application().with_assets(gpui_kit::assets::Assets);
    application.on_open_urls(move |urls| {
        let _ = requests.try_send(urls);
    });
    application.on_reopen(move |_| {
        let _ = reopen.try_send(Vec::new());
    });
    application.run(move |cx| {
        gpui_kit::init(cx);
        app::init(cx);
        let mut current = match open_window(paths, demo_mode, cx) {
            Ok(current) => current,
            Err(error) => {
                eprintln!("Couldn't open Diffy: {error}");
                cx.quit();
                return;
            }
        };
        // The receiver is registered before launching so cold-start URL events cannot be lost.
        cx.spawn(async move |cx| {
            while let Ok(urls) = receiver.recv().await {
                let mut paths = Vec::new();
                for url in urls {
                    match diffy::cli::paths_from_url(&url) {
                        Ok(request) => paths.extend(request),
                        Err(error) => eprintln!("Couldn't open project: {error}"),
                    }
                }
                cx.update(|cx| {
                    if !cx.windows().contains(&current.0) {
                        match open_window(Vec::new(), false, cx) {
                            Ok(window) => current = window,
                            Err(error) => {
                                eprintln!("Couldn't reopen Diffy: {error}");
                                return;
                            }
                        }
                    }
                    let _ = current.0.update(cx, |_, window, cx| {
                        current
                            .1
                            .update(cx, |view, cx| view.open_paths(paths, window, cx));
                    });
                });
            }
        })
        .detach();
        cx.activate(true);
    });
}

fn open_window(
    paths: Vec<PathBuf>,
    demo: bool,
    cx: &mut gpui_kit::App,
) -> Result<(gpui_kit::AnyWindowHandle, gpui_kit::Entity<app::Diffy>), String> {
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::centered(size(px(1440.), px(900.)), cx)),
        window_min_size: Some(size(px(920.), px(560.))),
        app_id: Some("com.hitslop.diffy".into()),
        ..TitleBar::window_options()
    };
    gpui_kit::open_window(options, cx, |window, cx| {
        cx.new(|cx| app::Diffy::with_options(paths, demo, diffy::state::default_path(), window, cx))
    })
    .map_err(|e| e.to_string())
}
