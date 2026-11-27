mod cli;
mod config;
mod deps;
mod gamedb;
mod icons;
mod proton;
mod runner;
mod theme;
mod ui;

use gpui_kit::assets::AllAssets;
use gpui_kit::{AppContext as _, Bounds, TitlebarOptions, WindowBounds, WindowOptions, px, size};

fn main() {
    let raw_args: Vec<String> = std::env::args().skip(1).collect();
    match cli::parse_cli_args(&raw_args) {
        Ok(cli::CliCommand::Gui) => {}
        Ok(cmd) => match cli::execute_cli(cmd) {
            Ok(code) => std::process::exit(code),
            Err(err) => {
                eprintln!("umubox: {err:#}");
                std::process::exit(1);
            }
        },
        Err(err) => {
            eprintln!("umubox: {err}");
            std::process::exit(2);
        }
    }

    gpui_kit::application()
        .with_assets(AllAssets)
        .run(move |cx| {
            gpui_kit::init(cx);
            theme::apply_kanagawa_dragon_theme(cx);
            ui::register_keybindings(cx);

            let bounds = Bounds::centered(None, size(px(1360.), px(860.)), cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("UmuBox".into()),
                    ..Default::default()
                }),
                app_id: Some("umubox".to_string()),
                ..Default::default()
            };

            gpui_kit::open_window(options, cx, move |window, cx| {
                window.set_window_title("UmuBox — Proton & UMU Manager");
                cx.new(|cx| ui::UmuBoxApp::new(window, cx))
            })
            .expect("failed to open UmuBox window");
        });
}
