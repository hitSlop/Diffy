use diffy::{app, demo, repository};

use std::path::PathBuf;

use gpui_kit::component::TitleBar;
use gpui_kit::{AppContext as _, WindowBounds, WindowOptions, px, size};

fn main() {
    let mut paths = Vec::new();
    let mut check = false;
    let mut demo_mode = false;
    for argument in std::env::args_os().skip(1) {
        if argument == "--version" {
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
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            app::init(cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::centered(size(px(1440.), px(900.)), cx)),
                window_min_size: Some(size(px(920.), px(560.))),
                app_id: Some("com.hitslop.diffy".into()),
                ..TitleBar::window_options()
            };
            if let Err(error) = gpui_kit::open_window(options, cx, |window, cx| {
                cx.new(|cx| {
                    app::Diffy::with_options(
                        paths,
                        demo_mode,
                        diffy::state::default_path(),
                        window,
                        cx,
                    )
                })
            }) {
                eprintln!("Couldn't open Diffy: {error}");
                cx.quit();
                return;
            }
            cx.activate(true);
        });
}
