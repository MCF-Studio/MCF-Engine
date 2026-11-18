//! Client du jeu RP MCF (phase 1 : squelette).

use bevy::prelude::*;
use mcf_core::McfCorePlugin;

fn main() -> AppExit {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "MCF RP".into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(McfCorePlugin)
        .add_systems(Startup, setup)
        .run()
}

fn setup(mut commands: Commands) {
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 2.0, 6.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        Text::new("MCF RP — phase 1 (squelette)"),
        Node {
            position_type: PositionType::Absolute,
            top: px(12),
            left: px(12),
            ..default()
        },
    ));
}
