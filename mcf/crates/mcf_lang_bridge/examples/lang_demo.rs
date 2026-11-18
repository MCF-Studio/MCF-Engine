//! Démo : affiche le composant `Hud` du dossier `demo/` (ou celui donné en argument) dans une fenêtre Bevy.
//!
//! `cargo run -p mcf_lang_bridge --example lang_demo --features dev [-- dossier Composant]`
//!
//! Avec `MCF_DEMO_SHOT=image.png`, la démo enregistre une capture **de sa propre fenêtre** puis quitte.

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use mcf_lang_bridge::McfLangPlugin;

fn main() {
    let mut args = std::env::args().skip(1);
    let project = args.next().map_or_else(
        || concat!(env!("CARGO_MANIFEST_DIR"), "/demo").into(),
        std::path::PathBuf::from,
    );
    let component = args.next().unwrap_or_else(|| "Hud".to_string());
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(AssetPlugin {
        // Les polices de Bevy sont dans le dossier `assets/` du dépôt.
        file_path: concat!(env!("CARGO_MANIFEST_DIR"), "/../../../assets").into(),
        ..default()
    }))
    .insert_resource(ClearColor(Color::srgb(0.07, 0.07, 0.11)))
    .add_plugins(McfLangPlugin {
        project,
        component,
        font: Some("fonts/FiraSans-Bold.ttf".into()),
    })
    .add_systems(Startup, |mut commands: Commands| {
        commands.spawn(Camera2d);
    });
    if let Ok(path) = std::env::var("MCF_DEMO_SHOT") {
        app.add_systems(
            Update,
            move |mut commands: Commands, mut frame: Local<u32>, mut exit: MessageWriter<AppExit>| {
                *frame += 1;
                if *frame == 120 {
                    commands
                        .spawn(Screenshot::primary_window())
                        .observe(save_to_disk(path.clone()));
                }
                if *frame == 200 {
                    exit.write(AppExit::Success);
                }
            },
        );
    }
    app.run();
}
