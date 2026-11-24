//! Choix de fichiers : fenêtres natives « ouvrir » (rfd) et glisser-déposer sur la fenêtre de l'éditeur.
//!
//! La fenêtre native est ouverte sur un fil à part : l'éditeur continue de se dessiner pendant qu'elle est
//! ouverte. Le chemin choisi revient par un canal, lu à chaque image par l'interface. Ce module ne fait que
//! **désigner** un chemin : la validation stricte (format, taille, budget de pixels) reste celle de `mcf_carte`.

use std::{
    path::PathBuf,
    sync::{Mutex, mpsc},
    thread,
};

use bevy::prelude::*;

/// Ce que l'utilisateur est en train de choisir.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cible {
    /// Image PNG d'un calque de référence (plan, satmap, croquis).
    ImageReference,
    /// Dossier d'un projet de carte (à ouvrir, ou neuf / vide pour enregistrer).
    DossierProjet,
    /// Image quelconque (vue satellite, plan) à convertir vers la convention des cartes sources.
    ImageAConvertir,
}

/// Fenêtre de choix en cours (une seule à la fois).
#[derive(Resource, Default)]
pub struct Dialogue(Mutex<Option<(Cible, mpsc::Receiver<Option<PathBuf>>)>>);

impl Dialogue {
    /// Une fenêtre de choix est ouverte.
    pub fn ouvert(&self) -> bool {
        self.0.lock().is_ok_and(|g| g.is_some())
    }

    /// Ouvre la fenêtre native de choix pour `cible` (sans effet si une autre est déjà ouverte).
    pub fn ouvrir(&self, cible: Cible) {
        let Ok(mut en_cours) = self.0.lock() else {
            return;
        };
        if en_cours.is_some() {
            return;
        }
        let (envoi, reception) = mpsc::channel();
        let lance = thread::Builder::new()
            .name("choix-fichier".into())
            .spawn(move || {
                let choix = match cible {
                    Cible::ImageReference => rfd::FileDialog::new()
                        .set_title("Importer un calque de référence (PNG)")
                        .add_filter("Image PNG", &["png"])
                        .pick_file(),
                    Cible::ImageAConvertir => rfd::FileDialog::new()
                        .set_title("Convertir une carte (vue satellite, plan) vers la convention")
                        .add_filter("Image PNG", &["png"])
                        .pick_file(),
                    Cible::DossierProjet => rfd::FileDialog::new()
                        .set_title("Dossier du projet de carte")
                        .pick_folder(),
                };
                // L'éditeur a pu se fermer entre-temps : rien à faire.
                let _ = envoi.send(choix);
            });
        if lance.is_ok() {
            *en_cours = Some((cible, reception));
        }
    }

    /// Chemin choisi, une seule fois, quand la fenêtre se ferme (`None` tant qu'elle est ouverte, ou si
    /// l'utilisateur a annulé).
    pub fn recevoir(&self) -> Option<(Cible, PathBuf)> {
        let mut en_cours = self.0.lock().ok()?;
        let (cible, reception) = en_cours.as_ref()?;
        let cible = *cible;
        let choix = match reception.try_recv() {
            Ok(choix) => choix,
            Err(mpsc::TryRecvError::Empty) => return None,
            // Fil terminé sans réponse : on libère la place.
            Err(mpsc::TryRecvError::Disconnected) => None,
        };
        *en_cours = None;
        choix.map(|c| (cible, c))
    }
}
