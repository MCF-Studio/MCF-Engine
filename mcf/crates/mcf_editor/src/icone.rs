//! Icône de la fenêtre de l'éditeur : le logo du moteur (`mcf/branding/icone_256.png`), inclus dans l'exécutable.

use bevy::{ecs::system::NonSendMarker, prelude::*, window::PrimaryWindow, winit::WINIT_WINDOWS};

/// Logo réduit à 256 px (source : `mcf/branding/logo_source.png`).
const ICONE_PNG: &[u8] = include_bytes!("../../../branding/icone_256.png");

/// Pose l'icône sur la fenêtre principale dès qu'elle existe.
pub struct IconePlugin;

impl Plugin for IconePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, poser);
    }
}

/// Image de l'icône, décodée par le décodeur PNG validé de `mcf_carte`.
fn icone() -> Result<winit::window::Icon, String> {
    let image = mcf_carte::references::decoder_png(ICONE_PNG)?;
    winit::window::Icon::from_rgba(image.rgba.to_vec(), image.largeur, image.hauteur)
        .map_err(|e| e.to_string())
}

// Les fenêtres winit ne se manipulent que sur le fil principal (`NonSendMarker`).
fn poser(
    _fil_principal: NonSendMarker,
    fenetre: Option<Single<Entity, With<PrimaryWindow>>>,
    mut posee: Local<bool>,
) {
    if *posee {
        return;
    }
    let Some(fenetre) = fenetre else {
        return;
    };
    WINIT_WINDOWS.with_borrow(|fenetres| {
        // La fenêtre winit est créée quelques images après l'entité : on réessaie jusque-là.
        let Some(f) = fenetres.get_window(*fenetre) else {
            return;
        };
        match icone() {
            Ok(i) => {
                // Sous Windows, la barre des tâches (et Alt+Tab) utilise une icône distincte de celle du titre.
                #[cfg(windows)]
                {
                    use winit::platform::windows::WindowExtWindows;
                    f.set_taskbar_icon(Some(i.clone()));
                }
                f.set_window_icon(Some(i));
            }
            Err(e) => warn!("icône de fenêtre non posée : {e}"),
        }
        *posee = true;
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn icone_incluse_valide() {
        assert!(super::icone().is_ok());
    }
}
