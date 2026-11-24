//! Panneaux egui : barre du haut (fichier, annuler, vues), étapes et outils à gauche, inspecteur à droite, barre
//! d'état en bas. L'espace restant est donné aux vues (`ZoneVues`).

use std::path::{Path, PathBuf};

use bevy::{prelude::*, window::FileDragAndDrop};
use bevy_egui::{EguiContexts, egui};
use mcf_carte::{
    Carte, Densite, Famille, GrapheRoutier, Manifeste, Origine,
    export::{self, StylePlan},
    relief::Pinceau,
};
use mcf_geo::{ClasseRoute, polygone};

use super::{
    Disposition, Edition, Etape, Outil, Projet, Selection, ZoneVues,
    camera::{Orbite, VuePlan},
    fichiers::{Cible, Dialogue},
    modeles::Catalogue,
    outils,
};

fn pastille(ui: &mut egui::Ui, c: [u8; 3]) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
    ui.painter()
        .rect_filled(r, 2.0, egui::Color32::from_rgb(c[0], c[1], c[2]));
}

fn aide(ui: &mut egui::Ui, lignes: &[&str]) {
    ui.add_space(4.0);
    for l in lignes {
        ui.label(egui::RichText::new(*l).small().weak());
    }
}

/// Recentre les deux caméras sur la carte.
fn recentrer(m: &Manifeste, plan: &mut VuePlan, orbite: &mut Orbite) {
    let (a, b) = m.bornes();
    let c = Vec2::new(((a[0] + b[0]) * 0.5) as f32, ((a[1] + b[1]) * 0.5) as f32);
    let cote = (b[0] - a[0]).max(b[1] - a[1]) as f32;
    plan.centre = c;
    plan.m_par_px = cote / 700.0;
    orbite.cible = Vec3::new(c.x, 0.0, c.y);
    orbite.distance = cote * 0.9;
}

fn remplacer(
    projet: &mut Projet,
    edition: &mut Edition,
    carte: Carte,
    plan: &mut VuePlan,
    orbite: &mut Orbite,
) {
    recentrer(&carte.manifeste, plan, orbite);
    projet.carte = carte;
    edition.selection = Selection::Rien;
    edition.points.clear();
    edition.glisse = None;
    edition.reference_active = None;
    edition.reference_saisie = None;
    edition.releve.vider();
}

/// Thème de l'éditeur : police plus grande, accents de couleur.
fn styler(ctx: &egui::Context) {
    use egui::{FontFamily, FontId, TextStyle};
    ctx.all_styles_mut(|s| {
        s.text_styles = [
            (TextStyle::Heading, FontId::new(21.0, FontFamily::Proportional)),
            (TextStyle::Body, FontId::new(15.0, FontFamily::Proportional)),
            (TextStyle::Button, FontId::new(15.0, FontFamily::Proportional)),
            (TextStyle::Small, FontId::new(12.5, FontFamily::Proportional)),
            (TextStyle::Monospace, FontId::new(14.0, FontFamily::Monospace)),
        ]
        .into();
        s.spacing.item_spacing = egui::vec2(8.0, 6.0);
        s.spacing.button_padding = egui::vec2(8.0, 4.0);
        let v = &mut s.visuals;
        let accent = egui::Color32::from_rgb(214, 132, 52);
        v.selection.bg_fill = accent;
        v.selection.stroke = egui::Stroke::new(1.0, egui::Color32::WHITE);
        v.hyperlink_color = accent;
        v.panel_fill = egui::Color32::from_rgb(24, 27, 33);
        v.window_fill = egui::Color32::from_rgb(30, 34, 41);
        v.widgets.inactive.weak_bg_fill = egui::Color32::from_rgb(44, 50, 60);
        v.widgets.hovered.weak_bg_fill = egui::Color32::from_rgb(64, 74, 90);
        v.widgets.active.weak_bg_fill = accent;
    });
}

pub fn interface(
    mut contexts: EguiContexts,
    mut projet: ResMut<Projet>,
    mut edition: ResMut<Edition>,
    mut zone: ResMut<ZoneVues>,
    mut plan: ResMut<VuePlan>,
    mut orbite: ResMut<Orbite>,
    mut style_pose: Local<bool>,
    dialogue: Res<Dialogue>,
    catalogue: Res<Catalogue>,
    mut deposes: MessageReader<FileDragAndDrop>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    if !*style_pose {
        *style_pose = true;
        styler(ctx);
    }
    let mut racine = egui::Ui::new(
        ctx.clone(),
        "racine".into(),
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::background())
            .max_rect(ctx.viewport_rect()),
    );
    let (projet, edition) = (&mut *projet, &mut *edition);
    recevoir_fichiers(projet, edition, &dialogue, &mut deposes);

    egui::Panel::top("haut").show(&mut racine, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.strong("MCF Editor · Conception de carte");
            ui.separator();
            let sale = if projet.carte.sale.propre() { "" } else { " *" };
            ui.label(format!("{}{sale}", projet.carte.manifeste.nom));
            ui.separator();
            if ui.button("Annuler").on_hover_text("Ctrl+Z").clicked() {
                outils::annuler(projet, edition);
            }
            if ui.button("Refaire").on_hover_text("Ctrl+Y").clicked() {
                outils::refaire(projet, edition);
            }
            if ui.button("Enregistrer").on_hover_text("Ctrl+S").clicked() {
                outils::enregistrer(projet, edition);
            }
            ui.separator();
            ui.label("Vue :");
            for (d, nom, astuce) in [
                (
                    Disposition::Perspective,
                    "3D + mini-carte",
                    "Tab. Clic sur la mini-carte : l'agrandir. F : la 3D regarde le point sous la souris.",
                ),
                (
                    Disposition::Partagee,
                    "Plan + 3D",
                    "Maj+Tab. F dans le Plan : la 3D va au point sous la souris.",
                ),
                (Disposition::Plan, "Plan", "Tab"),
            ] {
                if ui
                    .selectable_label(edition.disposition == d, nom)
                    .on_hover_text(astuce)
                    .clicked()
                {
                    edition.disposition = d;
                }
            }
            if ui.button("Recentrer").clicked() {
                recentrer(&projet.carte.manifeste, &mut plan, &mut orbite);
            }
            ui.separator();
            if ui
                .selectable_label(edition.promenade, "🚶 Se promener (P)")
                .on_hover_text(
                    "Un personnage du jeu (1,65 m) marche là où regarde la caméra 3D : ZQSD, Maj pour courir.                      Pour juger l'échelle des rues et des bâtiments.",
                )
                .clicked()
            {
                edition.promenade = !edition.promenade;
            }
        });
    });

    egui::Panel::bottom("etat").show(&mut racine, |ui| {
        ui.horizontal_wrapped(|ui| {
            let texte = egui::RichText::new(&projet.message);
            ui.label(if projet.erreur {
                texte.color(egui::Color32::from_rgb(230, 80, 70))
            } else {
                texte
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(format!("scène : {:.0} ms", edition.duree_scene_ms))
                        .small()
                        .weak(),
                );
                if let Some((p, h)) = edition.curseur {
                    ui.monospace(format!("x {:8.2}  z {:8.2}  alt {:6.2} m", p[0], p[1], h));
                }
            });
        });
    });

    egui::Panel::left("etapes")
        .resizable(true)
        .default_size(380.0)
        .size_range(300.0..=640.0)
        .show(&mut racine, |ui| {
            // Curseurs à la largeur du panneau (libellé et valeur à côté) : rien ne dépasse à droite.
            ui.spacing_mut().slider_width = (ui.available_width() - 200.0).clamp(60.0, 260.0);
            egui::ScrollArea::vertical()
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    // Étapes en cascade : l'étape courante se déplie sous son titre.
                    for e in Etape::TOUTES {
                        let actif = edition.etape == e;
                        let c = e.couleur();
                        let couleur = egui::Color32::from_rgb(c[0], c[1], c[2]);
                        let fond = if actif {
                            couleur
                        } else {
                            couleur.gamma_multiply(0.25)
                        };
                        let rep =
                            egui::Frame::new()
                                .fill(fond)
                                .corner_radius(6)
                                .inner_margin(egui::Margin::symmetric(10, 6))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.label(
                                        egui::RichText::new(e.nom())
                                            .strong()
                                            .size(17.0)
                                            .color(egui::Color32::WHITE),
                                    );
                                    ui.label(egui::RichText::new(e.resume()).size(12.5).color(
                                        egui::Color32::from_white_alpha(if actif { 230 } else { 150 }),
                                    ));
                                })
                                .response
                                .interact(egui::Sense::click())
                                .on_hover_cursor(egui::CursorIcon::PointingHand);
                        if rep.clicked() && !actif {
                            edition.aller_a(e);
                        }
                        if actif {
                            egui::Frame::new()
                                .stroke(egui::Stroke::new(2.0, couleur))
                                .corner_radius(6)
                                .inner_margin(egui::Margin::same(10))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    match e {
                                        Etape::Cadre => {
                                            cadre(ui, projet, edition, &mut plan, &mut orbite, &dialogue)
                                        }
                                        Etape::Relief => etape_relief(ui, edition),
                                        Etape::Reseau => etape_reseau(ui, projet, edition),
                                        Etape::Quartiers => etape_quartiers(ui, edition),
                                        Etape::Bati => etape_bati(ui, projet, edition, &catalogue),
                                        Etape::Export => etape_export(ui, projet, edition),
                                    }
                                });
                        }
                        ui.add_space(4.0);
                    }
                });
        });

    egui::Panel::right("inspecteur")
        .resizable(true)
        .default_size(300.0)
        .size_range(240.0..=520.0)
        .show(&mut racine, |ui| {
            ui.spacing_mut().slider_width = (ui.available_width() - 160.0).clamp(60.0, 220.0);
            egui::ScrollArea::vertical()
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.heading("Inspecteur");
                    inspecteur(ui, projet, edition, &catalogue);
                    ui.separator();
                    statistiques(ui, projet);
                    ui.separator();
                    egui::CollapsingHeader::new("Historique")
                        .default_open(false)
                        .show(ui, |ui| {
                            let (passe, futur) = projet.carte.historique();
                            for l in futur.iter().rev().take(5) {
                                ui.label(egui::RichText::new(*l).weak().italics());
                            }
                            for l in passe.iter().rev().take(15) {
                                ui.label(*l);
                            }
                        });
                });
        });

    let reste = racine.available_rect_before_wrap();
    zone.rect = Rect::new(reste.min.x, reste.min.y, reste.max.x, reste.max.y);
    zone.souris_sur_ui = ctx.is_pointer_over_egui() || ctx.egui_wants_pointer_input();
    zone.clavier_ui = ctx.egui_wants_keyboard_input();
    Ok(())
}

fn cadre(
    ui: &mut egui::Ui,
    projet: &mut Projet,
    edition: &mut Edition,
    plan: &mut VuePlan,
    orbite: &mut Orbite,
    dialogue: &Dialogue,
) {
    ui.strong("Nouvelle carte");
    ui.horizontal_wrapped(|ui| {
        ui.label("Nom");
        ui.text_edit_singleline(&mut edition.nouveau_nom);
    });
    ui.horizontal_wrapped(|ui| {
        ui.label("Côté");
        // Saisie libre en km (taper « 32 » pour 32 km), arrondie au multiple de tuile en sortant du champ.
        let mut km = edition.nouvelle_taille / 1000.0;
        let reponse = ui.add(
            egui::DragValue::new(&mut km)
                .range(0.256..=mcf_carte::manifeste::TAILLE_MAX_M / 1000.0)
                .speed(0.05)
                .max_decimals(3)
                .suffix(" km"),
        );
        if reponse.changed() {
            edition.nouvelle_taille = km * 1000.0;
        }
        if !reponse.has_focus() {
            edition.nouvelle_taille = Manifeste::arrondir_taille(edition.nouvelle_taille);
        }
    })
    .response
    .on_hover_text("Côté de la carte carrée, 0,256 à 32,768 km (arrondi à 256 m). Double-clic pour taper.");
    let resolution = Manifeste::resolution_pour(edition.nouvelle_taille);
    let (couleur, texte) = if resolution > 1.0 {
        (
            egui::Color32::from_rgb(220, 160, 60),
            format!("Très grande carte : relief à {resolution} m par point (garde-fou mémoire).",),
        )
    } else if edition.nouvelle_taille > 2048.0 {
        (
            egui::Color32::from_rgb(220, 160, 60),
            "Grande carte : sculpture plus lente (relief à 1 m).".to_owned(),
        )
    } else {
        (
            ui.visuals().weak_text_color(),
            "Relief à 1 m par point.".to_owned(),
        )
    };
    ui.label(egui::RichText::new(texte).small().color(couleur));
    // Plage d'altitude fixée à la création (codage 16 bits du relief : 4 096 m d'écart au plus).
    ui.horizontal_wrapped(|ui| {
        ui.label("Altitudes");
        let (bas, haut) = &mut edition.nouvelle_plage_m;
        ui.add(
            egui::DragValue::new(bas)
                .range(-1000.0..=8999.0)
                .speed(1.0)
                .suffix(" m"),
        );
        ui.label("à");
        ui.add(
            egui::DragValue::new(haut)
                .range(-999.0..=9000.0)
                .speed(1.0)
                .suffix(" m"),
        );
    })
    .response
    .on_hover_text(
        "Plus bas et plus haut point possibles de la carte (fond marin, sommets). Écart de 4 096 m au plus ; \
         précision du relief = écart / 65 535.",
    );
    let (bas, haut) = edition.nouvelle_plage_m;
    ui.label(
        egui::RichText::new(format!(
            "Précision du relief : {:.1} cm.",
            (haut - bas) / 65_535.0 * 100.0
        ))
        .small()
        .weak(),
    );
    if ui.button("Créer (vierge, plate)").clicked() {
        edition.nouvelle_taille = Manifeste::arrondir_taille(edition.nouvelle_taille);
        let nom = edition.nouveau_nom.trim().to_owned();
        let mut manifeste = Manifeste::nouveau(&nom, edition.nouvelle_taille);
        (manifeste.altitude_min_m, manifeste.altitude_max_m) = edition.nouvelle_plage_m;
        if !mcf_carte::manifeste::nom_sur(&nom) {
            projet.echec("Nom refusé : minuscules, chiffres, « _ » et « - » seulement (64 au plus).");
        } else {
            match Carte::nouvelle(manifeste) {
                Ok(c) => {
                    remplacer(projet, edition, c, plan, orbite);
                    edition.chemin.clear();
                    // On reste dans le Cadre : c'est là qu'on importe et cale les calques de référence.
                    projet.info(format!(
                        "Carte « {nom} » créée. Importez un calque de référence si besoin, puis passez au relief."
                    ));
                }
                Err(e) => projet.echec(e),
            }
        }
    }
    ui.separator();
    ui.strong("Fichier");
    ui.label("Dossier du projet de carte");
    ui.horizontal_wrapped(|ui| {
        ui.text_edit_singleline(&mut edition.chemin);
        if ui
            .add_enabled(!dialogue.ouvert(), egui::Button::new("Parcourir…"))
            .clicked()
        {
            dialogue.ouvrir(Cible::DossierProjet);
        }
    });
    ui.horizontal_wrapped(|ui| {
        if ui.button("Ouvrir").clicked() {
            let chemin = PathBuf::from(edition.chemin.trim());
            if !projet.carte.sale.propre() && !projet.erreur {
                projet.echec(
                    "Modifications non enregistrées : enregistrez, ou cliquez encore pour les abandonner.",
                );
            } else {
                match Carte::ouvrir(&chemin) {
                    Ok(c) => {
                        remplacer(projet, edition, c, plan, orbite);
                        projet.info(format!("Carte ouverte depuis {}.", chemin.display()));
                    }
                    Err(e) => projet.echec(format!("Ouverture refusée : {e}")),
                }
            }
        }
        if ui.button("Enregistrer ici").clicked() {
            outils::enregistrer(projet, edition);
        }
    });
    match &projet.carte.dossier {
        Some(d) => ui.label(egui::RichText::new(format!("Projet : {}", d.display())).small()),
        None => ui.label(
            egui::RichText::new("Pas encore enregistrée : indiquez un dossier neuf ou vide.")
                .small()
                .weak(),
        ),
    };
    if ui.button("Recharger la carte de test").clicked() {
        match mcf_carte::carte_de_test::construire() {
            Ok(c) => {
                remplacer(projet, edition, c, plan, orbite);
                projet.info("Carte de test rechargée.");
            }
            Err(e) => projet.echec(e),
        }
    }
    ui.separator();
    calques_reference(ui, projet, edition, dialogue);
    super::releve::panneau(ui, projet, edition, plan);
    ui.separator();
    ui.strong("Carte");
    let m = projet.carte.manifeste.clone();
    egui::Grid::new("manifeste").num_columns(2).show(ui, |ui| {
        ui.label("Emprise");
        ui.label(format!("{} × {} m", m.taille_m[0], m.taille_m[1]));
        ui.end_row();
        ui.label("Tuiles");
        ui.label(format!("{} m, relief à {} m", m.tuile_m, m.resolution_relief_m));
        ui.end_row();
        ui.label("Altitudes");
        ui.label(format!("{} à {} m", m.altitude_min_m, m.altitude_max_m));
        ui.end_row();
    });
    let mut avec_eau = m.niveau_eau_m.is_some();
    let mut niveau = m.niveau_eau_m.unwrap_or(0.0);
    let mut change = ui
        .checkbox(&mut avec_eau, "Mer / lac (niveau d'eau global)")
        .changed();
    if avec_eau {
        change |= ui
            .add(egui::Slider::new(&mut niveau, m.altitude_min_m..=50.0).text("niveau (m)"))
            .drag_stopped();
    }
    if change {
        let cible = avec_eau.then_some(niveau);
        let r = projet.carte.modifier("Niveau d'eau", |tx| {
            tx.manifeste_mut().niveau_eau_m = cible;
            Ok(())
        });
        projet.resultat(r, |()| "Niveau d'eau modifié.".into());
    }
}

fn etape_relief(ui: &mut egui::Ui, edition: &mut Edition) {
    ui.strong("Pinceaux");
    ui.horizontal_wrapped(|ui| {
        for p in Pinceau::TOUS {
            if ui
                .selectable_label(edition.outil == Outil::Pinceau(p), p.nom())
                .clicked()
            {
                edition.outil = Outil::Pinceau(p);
            }
        }
    });
    ui.add(
        // Jusqu'à 2 km (borne de `Carte::pinceau`) : sur une carte de 32 km, un massif se modèle à grands traits.
        egui::Slider::new(&mut edition.rayon, 2.0..=2000.0)
            .logarithmic(true)
            .text("rayon (m)"),
    );
    let (max, unite) = match edition.outil {
        Outil::Pinceau(Pinceau::Lisser | Pinceau::Aplanir) => (4.0, "force"),
        _ => (150.0, "m/s"),
    };
    edition.force = edition.force.min(max);
    ui.add(egui::Slider::new(&mut edition.force, 0.1..=max).text(unite));
    if edition.outil == Outil::Pinceau(Pinceau::Aplanir) {
        ui.add(egui::DragValue::new(&mut edition.cible).suffix(" m").speed(0.1));
        ui.label(egui::RichText::new("Ctrl+clic : prendre l'altitude sous la souris.").small());
    }
    aide(
        ui,
        &[
            "Clic gauche maintenu : sculpter (un geste = une annulation).",
            "Les routes sont terrassées automatiquement : retracer une route ne détruit pas le relief.",
        ],
    );
}

fn etape_reseau(ui: &mut egui::Ui, projet: &Projet, edition: &mut Edition) {
    ui.horizontal_wrapped(|ui| {
        if ui
            .selectable_label(edition.outil == Outil::Tracer, "✏ Tracer")
            .clicked()
        {
            edition.outil = Outil::Tracer;
        }
        if ui
            .selectable_label(edition.outil == Outil::Selection, "⬉ Éditer / sélectionner")
            .clicked()
        {
            edition.outil = Outil::Selection;
            edition.points.clear();
        }
    });
    ui.strong("Classe de route");
    let profils = projet.carte.manifeste.profils;
    for c in ClasseRoute::TOUTES {
        ui.horizontal_wrapped(|ui| {
            pastille(ui, profils.get(c).couleur_plan);
            if ui
                .selectable_label(
                    edition.classe == c,
                    format!("{} — {} m", c.nom(), profils.get(c).emprise_m()),
                )
                .clicked()
            {
                edition.classe = c;
                edition.outil = Outil::Tracer;
            }
        });
    }
    ui.checkbox(&mut edition.accroche, "Accroche aux routes (2 m)");
    if edition.outil == Outil::Tracer && !edition.points.is_empty() {
        ui.label(format!("{} point(s) posé(s).", edition.points.len()));
    }
    aide(
        ui,
        &[
            "Clics : points du tracé (virages arrondis automatiquement).",
            "Double-clic ou Entrée : terminer. Retour arrière : retirer le dernier point. Échap : abandonner.",
            "Croiser ou toucher une route crée le carrefour. Alt : sans accroche.",
            "Éditer : cliquez une route, glissez ses poignées. Ctrl+clic sur la route : ajouter un point ; Ctrl+clic sur un point : le retirer. Suppr : supprimer le tronçon.",
            "Maj+clic (depuis n'importe quel outil) : sélectionner ; l'éditeur ouvre la bonne étape.",
        ],
    );
}

fn etape_quartiers(ui: &mut egui::Ui, edition: &mut Edition) {
    ui.horizontal_wrapped(|ui| {
        for (o, nom) in [
            (Outil::Affecter, "Affecter un îlot"),
            (Outil::ZoneLibre, "Zone libre"),
            (Outil::Selection, "Sélection"),
        ] {
            if ui.selectable_label(edition.outil == o, nom).clicked() {
                edition.outil = o;
                edition.points.clear();
            }
        }
    });
    familles(ui, edition);
    aide(
        ui,
        &[
            "Affecter : cliquez dans un îlot fermé par des routes.",
            "Zone libre : clics, puis Entrée (port, industrie, champs…).",
            "Modifier une zone libre : sélectionnez-la, glissez ses sommets ; Ctrl+clic sur un bord : ajouter un \
             sommet ; Ctrl+clic sur un sommet : le retirer. Un îlot affecté suit ses routes : modifiez les routes.",
            "Étape 5 pour faire pousser le bâti.",
        ],
    );
}

fn familles(ui: &mut egui::Ui, edition: &mut Edition) {
    ui.strong("Famille");
    for f in Famille::TOUTES {
        ui.horizontal_wrapped(|ui| {
            pastille(ui, f.couleur_plan());
            if ui.selectable_label(edition.famille == f, f.nom()).clicked() {
                edition.famille = f;
            }
        });
    }
    ui.strong("Densité");
    ui.horizontal_wrapped(|ui| {
        for d in Densite::TOUTES {
            if ui.selectable_label(edition.densite == d, d.nom()).clicked() {
                edition.densite = d;
            }
        }
    });
}

fn etape_bati(ui: &mut egui::Ui, projet: &mut Projet, edition: &mut Edition, catalogue: &Catalogue) {
    ui.horizontal_wrapped(|ui| {
        if ui
            .selectable_label(edition.outil == Outil::Lotir, "Lotir un quartier")
            .clicked()
        {
            edition.outil = Outil::Lotir;
        }
        if ui
            .selectable_label(edition.outil == Outil::Selection, "⬉ Ajuster")
            .clicked()
        {
            edition.outil = Outil::Selection;
        }
    });
    if ui.button("Lotir tous les quartiers").clicked() {
        let r = projet.carte.lotir_tout();
        projet.resultat(r, |n| {
            format!("{n} bâtiment(s) ont poussé (les bâtiments manuels n'ont pas bougé).")
        });
    }
    if !edition.conflits.is_empty() {
        ui.label(
            egui::RichText::new(format!(
                "⚠ {} bâtiment(s) empiètent sur une route (contour rouge).",
                edition.conflits.len()
            ))
            .color(egui::Color32::from_rgb(230, 80, 70)),
        );
    }
    aide(
        ui,
        &[
            "Lotir : cliquez un quartier (façades sur les bords, rangées et allées à l'intérieur) ; relancer ne \
             touche que les blocs « générés ».",
            "Ajuster : cliquez un bâtiment (Plan ou 3D, même avec « Lotir »), glissez-le (il devient « manuel »). \
             Ctrl+clic : sélection multiple.",
            "Gizmo : flèche rouge = axe X, bleue = axe Z, carré = libre, anneau vert = tourner (crans de 5°, Alt : \
             libre). R / Maj+R : tourner de 90°.",
            "Suppr : supprimer. Verrou, position, cap et niveaux dans l'inspecteur.",
        ],
    );
    ui.separator();
    liste_batiments(ui, edition, catalogue);
}

/// Liste des bâtiments modélisés du catalogue : un clic arme la pose.
fn liste_batiments(ui: &mut egui::Ui, edition: &mut Edition, catalogue: &Catalogue) {
    ui.strong("Bâtiments modélisés");
    if let Some(e) = &catalogue.erreur {
        ui.label(
            egui::RichText::new(format!("⚠ Catalogue refusé : {e}"))
                .color(egui::Color32::from_rgb(230, 80, 70)),
        );
        return;
    }
    if catalogue.fiches.is_empty() {
        ui.label(egui::RichText::new("Catalogue vide.").weak());
        return;
    }
    for (i, f) in catalogue.fiches.iter().enumerate() {
        let dispo = catalogue.disponibles.contains(&f.id);
        let texte = format!(
            "{}{} · {:.1} × {:.1} m, {:.1} m de haut",
            if dispo { "" } else { "⚠ " },
            f.nom,
            f.largeur_m,
            f.profondeur_m,
            f.hauteur_m
        );
        let r = ui.selectable_label(edition.outil == Outil::Poser(i), texte);
        let r = if dispo {
            r.on_hover_text(format!("{} ({})", f.fichier, f.famille.nom()))
        } else {
            r.on_hover_text(format!(
                "Modèle absent : {}. Copiez-le du jeu dans assets/batiments (le bâtiment s'affiche en bloc).",
                f.fichier
            ))
        };
        if r.clicked() {
            edition.selection = Selection::Rien;
            edition.outil = if edition.outil == Outil::Poser(i) {
                Outil::Selection
            } else {
                Outil::Poser(i)
            };
        }
    }
    if let Outil::Poser(_) = edition.outil {
        ui.horizontal(|ui| {
            ui.add(
                egui::DragValue::new(&mut edition.cap_pose)
                    .range(0.0..=359.9)
                    .speed(1.0)
                    .suffix("°")
                    .prefix("cap : "),
            );
            ui.label(egui::RichText::new("clic : poser · R : +90° · Échap : fini").weak());
        });
    }
}

fn etape_export(ui: &mut egui::Ui, projet: &mut Projet, edition: &mut Edition) {
    ui.strong("Aperçu (PNG, ≤ 2048 px)");
    ui.horizontal_wrapped(|ui| {
        for (s, nom) in [(StylePlan::Complet, "Plan stylisé"), (StylePlan::Jeu, "Minimap")] {
            if ui.button(nom).clicked() {
                let r = export::exporter_apercu(&mut projet.carte, s);
                projet.resultat(r, |p| format!("Aperçu écrit : {}", p.display()));
            }
        }
    });
    ui.separator();
    ui.strong("Exports complets");
    ui.checkbox(&mut edition.ecraser_export, "Remplacer un export existant");
    if ui.button("Plan en tuiles (0,5 m/px)").clicked() {
        let r = export::exporter(&mut projet.carte, 0.5, StylePlan::Complet, edition.ecraser_export);
        projet.resultat(r, |v| format!("{} tuile(s) exportée(s).", v.len()));
    }
    if ui.button("Pyramide minimap / GPS").clicked() {
        let r = export::exporter(&mut projet.carte, 0.5, StylePlan::Jeu, edition.ecraser_export);
        projet.resultat(r, |v| format!("{} tuile(s) de minimap exportée(s).", v.len()));
    }
    ui.separator();
    ui.strong("Graphe routier (GPS, IA)");
    let g = GrapheRoutier::construire(&projet.carte.reseau, &projet.carte.manifeste.profils);
    ui.label(format!(
        "{} nœud(s), {} arête(s), {} composante(s){}",
        g.noeuds.len(),
        g.aretes.len(),
        g.composantes(),
        if g.connexe() { " — tout est relié" } else { "" }
    ));
    aide(
        ui,
        &["Les exports exigent une carte enregistrée ; ils vont dans <projet>/exports/."],
    );
}

fn inspecteur(ui: &mut egui::Ui, projet: &mut Projet, edition: &mut Edition, catalogue: &Catalogue) {
    match edition.selection.clone() {
        Selection::Rien => {
            ui.label(egui::RichText::new("Rien de sélectionné.").weak());
        }
        Selection::Batiments(ids) if ids.len() == 1 => {
            let id = ids[0];
            let Some(b) = projet.carte.batiments.get(&id).cloned() else {
                return;
            };
            let fiche = b.modele.as_ref().map(|m| {
                catalogue
                    .fiche(&m.fiche)
                    .map_or_else(|| format!("{} (fiche absente)", m.fiche), |f| f.nom.clone())
            });
            match &fiche {
                Some(nom) => ui.strong(format!("{nom} · {}", b.famille.nom())),
                None => ui.strong(format!("Bâtiment · {}", b.famille.nom())),
            };
            position_et_cap(ui, projet, id, &b);
            egui::Grid::new("bat").num_columns(2).show(ui, |ui| {
                ui.label("Origine");
                ui.label(match b.origine {
                    Origine::Genere => "générée",
                    Origine::Manuel => "manuelle",
                });
                ui.end_row();
                ui.label("Emprise au sol");
                ui.label(format!("{:.0} m²", polygone::aire(&b.empreinte)));
                ui.end_row();
                ui.label("Hauteur");
                ui.label(format!("{:.1} m", b.hauteur()));
                ui.end_row();
            });
            let mut niveaux = b.niveaux;
            if b.modele.is_none()
                && ui
                    .add(
                        egui::DragValue::new(&mut niveaux)
                            .range(1..=80)
                            .prefix("niveaux : "),
                    )
                    .changed()
            {
                let r = projet.carte.modifier("Niveaux", |tx| {
                    if let Some(x) = tx.batiment(id) {
                        x.niveaux = niveaux;
                        x.origine = Origine::Manuel;
                    }
                    Ok(())
                });
                projet.resultat(r, |()| format!("{niveaux} niveau(x)."));
            }
            let mut verrou = b.verrou;
            if ui.checkbox(&mut verrou, "Verrouillé").changed() {
                let r = projet.carte.modifier("Verrou", |tx| {
                    if let Some(x) = tx.batiment(id) {
                        x.verrou = verrou;
                    }
                    Ok(())
                });
                projet.resultat(r, |()| {
                    if verrou { "Verrouillé." } else { "Déverrouillé." }.into()
                });
            }
            if ui.button("Supprimer").clicked() {
                outils::supprimer(projet, edition);
            }
        }
        Selection::Batiments(ids) => {
            ui.strong(format!("{} bâtiments", ids.len()));
            ui.horizontal_wrapped(|ui| {
                for (v, nom) in [(true, "Verrouiller"), (false, "Déverrouiller")] {
                    if ui.button(nom).clicked() {
                        let r = projet.carte.modifier("Verrou", |tx| {
                            for i in &ids {
                                if let Some(x) = tx.batiment(*i) {
                                    x.verrou = v;
                                }
                            }
                            Ok(())
                        });
                        projet.resultat(r, |()| "Fait.".into());
                    }
                }
            });
            if ui.button("Supprimer").clicked() {
                outils::supprimer(projet, edition);
            }
        }
        Selection::Troncon(id) => {
            let Some(t) = projet.carte.reseau.troncons.get(&id).cloned() else {
                return;
            };
            let longueur = projet
                .carte
                .derive()
                .geo
                .troncons
                .get(&id)
                .map(|g| g.longueur)
                .unwrap_or(0.0);
            ui.strong(format!("Tronçon · {}", t.classe.nom()));
            ui.label(format!(
                "{:.1} m, {} point(s) de forme, emprise {} m",
                longueur,
                t.points.len(),
                projet.carte.manifeste.profils.get(t.classe).emprise_m()
            ));
            let mut classe = t.classe;
            egui::ComboBox::from_label("Classe")
                .selected_text(classe.nom())
                .show_ui(ui, |ui| {
                    for c in ClasseRoute::TOUTES {
                        ui.selectable_value(&mut classe, c, c.nom());
                    }
                });
            if classe != t.classe {
                let r = projet
                    .carte
                    .modifier(&format!("Classe : {}", classe.nom()), |tx| {
                        if let Some(x) = tx.reseau().troncons.get_mut(&id) {
                            x.classe = classe;
                        }
                        Ok(())
                    });
                projet.resultat(r, |()| "Classe changée : profil et carrefours recalculés.".into());
            }
            // Le texte saisi vit dans l'état d'édition (sinon il serait remplacé à chaque image).
            if edition.nom_saisi.as_ref().is_none_or(|(i, _)| *i != id) {
                edition.nom_saisi = Some((id, t.nom.clone().unwrap_or_default()));
            }
            let mut valider = false;
            ui.label("Nom de la voie");
            ui.horizontal_wrapped(|ui| {
                if let Some((_, texte)) = &mut edition.nom_saisi {
                    let rep = ui.add(egui::TextEdit::singleline(texte).desired_width(160.0));
                    valider |= rep.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                }
                valider |= ui.button("Valider").clicked();
            });
            if valider && let Some((_, texte)) = edition.nom_saisi.clone() {
                let valeur = (!texte.trim().is_empty()).then(|| texte.trim().to_owned());
                let ancien = t.nom.clone();
                // Une voie traverse plusieurs carrefours : tous les tronçons qui portaient le même nom suivent.
                let r = projet.carte.modifier("Nommer la voie", |tx| {
                    let mut n = 0;
                    for (i, x) in tx.reseau().troncons.iter_mut() {
                        if *i == id || (ancien.is_some() && x.nom == ancien) {
                            x.nom = valeur.clone();
                            n += 1;
                        }
                    }
                    Ok(n)
                });
                projet.resultat(r, |n| format!("Voie nommée ({n} tronçon(s))."));
            }
            ui.label(
                egui::RichText::new("Glissez les poignées (blanches : carrefours / extrémités).")
                    .small()
                    .weak(),
            );
            if ui.button("Supprimer le tronçon").clicked() {
                outils::supprimer(projet, edition);
            }
        }
        Selection::Quartier(id) => {
            let (famille, densite, libre) = if let Some(a) = projet.carte.quartiers.affectations.get(&id) {
                (a.famille, a.densite, false)
            } else if let Some(z) = projet.carte.quartiers.zones.get(&id) {
                (z.famille, z.densite, true)
            } else {
                return;
            };
            ui.strong(if libre { "Zone libre" } else { "Îlot affecté" });
            let (mut f, mut d) = (famille, densite);
            egui::ComboBox::from_label("Famille")
                .selected_text(f.nom())
                .show_ui(ui, |ui| {
                    for x in Famille::TOUTES {
                        ui.selectable_value(&mut f, x, x.nom());
                    }
                });
            egui::ComboBox::from_label("Densité")
                .selected_text(d.nom())
                .show_ui(ui, |ui| {
                    for x in Densite::TOUTES {
                        ui.selectable_value(&mut d, x, x.nom());
                    }
                });
            if (f, d) != (famille, densite) {
                let r = projet.carte.modifier("Changer le quartier", |tx| {
                    let q = tx.quartiers();
                    if let Some(a) = q.affectations.get_mut(&id) {
                        a.famille = f;
                        a.densite = d;
                    } else if let Some(z) = q.zones.get_mut(&id) {
                        z.famille = f;
                        z.densite = d;
                    }
                    Ok(())
                });
                projet.resultat(r, |()| {
                    "Quartier modifié. « Lotir » pour régénérer son bâti.".into()
                });
            }
            let n = projet
                .carte
                .batiments
                .values()
                .filter(|b| b.affectation == Some(id))
                .count();
            ui.label(format!("{n} bâtiment(s) rattaché(s)."));
            ui.horizontal_wrapped(|ui| {
                if ui.button("Lotir").clicked() {
                    let r = projet.carte.lotir(id);
                    projet.resultat(r, |n| format!("{n} bâtiment(s) ont poussé."));
                }
                if ui.button("Retirer").clicked() {
                    outils::supprimer(projet, edition);
                }
            });
        }
    }
}

/// Position du centre et orientation, au clavier : une étape d'historique par valeur saisie.
fn position_et_cap(ui: &mut egui::Ui, projet: &mut Projet, id: mcf_carte::Id, b: &mcf_carte::Batiment) {
    let c = b.centre();
    let (mut x, mut z) = (c[0], c[1]);
    // Bloc : pas de cap stocké, on applique une rotation relative.
    let cap = b.modele.as_ref().map(|m| m.cap_deg);
    let mut cap_saisi = cap.unwrap_or(0.0);
    let mut deg = 0.0;
    ui.add_enabled_ui(!b.verrou, |ui| {
        ui.horizontal(|ui| {
            let rx = ui.add(
                egui::DragValue::new(&mut x)
                    .speed(0.1)
                    .fixed_decimals(2)
                    .prefix("X : ")
                    .suffix(" m"),
            );
            let rz = ui.add(
                egui::DragValue::new(&mut z)
                    .speed(0.1)
                    .fixed_decimals(2)
                    .prefix("Z : ")
                    .suffix(" m"),
            );
            if rx.changed() || rz.changed() {
                let r = projet
                    .carte
                    .transformer_batiments(&[id], [x - c[0], z - c[1]], 0.0, c);
                projet.resultat(r, |_| format!("Centre en ({x:.2} ; {z:.2})."));
            }
        });
        ui.horizontal(|ui| {
            if let Some(avant) = cap {
                if ui
                    .add(
                        egui::DragValue::new(&mut cap_saisi)
                            .range(0.0..=359.9)
                            .speed(0.5)
                            .fixed_decimals(1)
                            .prefix("cap : ")
                            .suffix("°"),
                    )
                    .changed()
                {
                    deg = cap_saisi - avant;
                }
            } else {
                ui.label("tourner :");
            }
            for (texte, d) in [
                ("⟲ 90°", 90.0),
                ("⟳ 90°", -90.0),
                ("⟲ 15°", 15.0),
                ("⟳ 15°", -15.0),
            ] {
                if ui.small_button(texte).clicked() {
                    deg = d;
                }
            }
        });
    });
    if deg != 0.0 {
        let r = projet.carte.transformer_batiments(&[id], [0.0, 0.0], deg, c);
        projet.resultat(r, |_| format!("Tourné de {deg:.1}°."));
    }
}

fn statistiques(ui: &mut egui::Ui, projet: &mut Projet) {
    let c = &projet.carte;
    let carrefours = c
        .reseau
        .noeuds
        .keys()
        .filter(|n| c.reseau.degre(**n) >= 3)
        .count();
    let (troncons, batiments) = (c.reseau.troncons.len(), c.batiments.len());
    let quartiers = c.quartiers.affectations.len() + c.quartiers.zones.len();
    let ilots = projet.carte.derive().ilots.len();
    egui::Grid::new("stats").num_columns(2).show(ui, |ui| {
        for (k, v) in [
            ("Tronçons", troncons),
            ("Carrefours", carrefours),
            ("Îlots", ilots),
            ("Quartiers", quartiers),
            ("Bâtiments", batiments),
        ] {
            ui.label(k);
            ui.label(v.to_string());
            ui.end_row();
        }
    });
}

/// Importe un PNG comme calque de référence (validation stricte dans `mcf_carte`) et ouvre l'étape où on le règle.
fn importer_image(projet: &mut Projet, edition: &mut Edition, chemin: &Path) {
    edition.chemin_image = chemin.display().to_string();
    match projet.carte.importer_reference_fichier(chemin) {
        Ok(id) => {
            if edition.etape != Etape::Cadre {
                edition.aller_a(Etape::Cadre);
            }
            edition.reference_active = Some(id);
            edition.reference_saisie = None;
            edition.disposition = Disposition::Partagee;
            projet.info(
                "Calque importé sur toute la largeur de la carte. Calez-le : « Caler » (deux repères \
                 de distance connue), puis « Déplacer ».",
            );
        }
        Err(e) => projet.echec(format!("Import refusé : {e}")),
    }
}

/// Convertit une image (vue satellite, plan) vers la convention des cartes sources et importe la carte couleurs et
/// la carte de relief proposée ; prépare les altitudes du noir et du blanc et le lissage conseillé.
fn convertir_image(projet: &mut Projet, edition: &mut Edition, chemin: &Path) {
    let debut = std::time::Instant::now();
    match projet.carte.convertir_et_importer_fichier(chemin) {
        Ok((_couleurs, relief, c)) => {
            if edition.etape != Etape::Cadre {
                edition.aller_a(Etape::Cadre);
            }
            // Le relief proposé est actif : « Appliquer comme relief » vise le bon calque (le calque couleurs lu en gris
            // inverserait montagnes et plaines). Les deux calques ont le même calage à l'import.
            edition.reference_active = Some(relief);
            edition.relief_niveaux = mcf_carte::relief_image::Niveaux::default();
            edition.reference_saisie = None;
            edition.disposition = Disposition::Partagee;
            edition.relief_noir_m = c.noir_m;
            edition.relief_blanc_m = c.blanc_m;
            let m_par_px = projet
                .carte
                .references
                .references
                .get(&relief)
                .map_or(1.0, |r| r.m_par_px);
            edition.relief_lissage_m =
                mcf_carte::relief_image::lissage_conseille(c.noir_m, c.blanc_m, m_par_px);
            let parts: Vec<String> = c
                .parts
                .iter()
                .filter(|(_, p)| *p >= 0.005)
                .map(|(k, p)| format!("{} {:.0} %", k.nom(), p * 100.0))
                .collect();
            let m = &projet.carte.manifeste;
            let plage = if m.altitude_min_m <= c.noir_m && c.blanc_m <= m.altitude_max_m {
                String::new()
            } else {
                format!(
                    " ⚠ La plage d'altitude de la carte ({} à {} m) ne contient pas {} à {} m : créez la carte avec \
                     ces altitudes pour appliquer le relief.",
                    m.altitude_min_m, m.altitude_max_m, c.noir_m, c.blanc_m
                )
            };
            projet.info(format!(
                "Converti en {:.1} s : {}. Le relief proposé est le calque actif : réglez noir, blanc et niveaux, puis \
                 « Appliquer comme relief » (relief déduit, à vérifier ; routes non extraites). Si vous recalez le \
                 calque couleurs, recopiez son calage sur le relief (« Même calage que… »).{plage}",
                debut.elapsed().as_secs_f64(),
                parts.join(", ")
            ));
        }
        Err(e) => projet.echec(format!("Conversion refusée : {e}")),
    }
}

/// Chemins choisis dans une fenêtre native ou déposés sur la fenêtre de l'éditeur.
fn recevoir_fichiers(
    projet: &mut Projet,
    edition: &mut Edition,
    dialogue: &Dialogue,
    deposes: &mut MessageReader<FileDragAndDrop>,
) {
    let mut chemins: Vec<(Option<Cible>, PathBuf)> = dialogue
        .recevoir()
        .map(|(cible, chemin)| (Some(cible), chemin))
        .into_iter()
        .collect();
    chemins.extend(deposes.read().filter_map(|m| match m {
        FileDragAndDrop::DroppedFile { path_buf, .. } => Some((None, path_buf.clone())),
        _ => None,
    }));
    for (cible, chemin) in chemins {
        // Un dossier déposé est un projet de carte ; un fichier déposé, une image de référence.
        let cible = cible.unwrap_or(if chemin.is_dir() {
            Cible::DossierProjet
        } else {
            Cible::ImageReference
        });
        match cible {
            Cible::ImageReference => importer_image(projet, edition, &chemin),
            Cible::ImageAConvertir => convertir_image(projet, edition, &chemin),
            Cible::DossierProjet => {
                edition.chemin = chemin.display().to_string();
                if edition.etape != Etape::Cadre {
                    edition.aller_a(Etape::Cadre);
                }
                projet.info(format!(
                    "Dossier choisi : {}. « Ouvrir » pour charger la carte qu'il contient, « Enregistrer ici » \
                     s'il est neuf ou vide.",
                    chemin.display()
                ));
            }
        }
    }
}

/// Consignes à donner à une IA pour produire une carte source (`docs/PROMPT_CARTE_IA.md`).
const PROMPT_CARTE_IA: &str = include_str!("../../../../../docs/PROMPT_CARTE_IA.md");

/// Calques de référence : importer une image, la régler, la caler (spec §8). Les réglages s'affichent en direct et
/// ne sont écrits (une étape d'historique) qu'avec « Appliquer ».
fn calques_reference(ui: &mut egui::Ui, projet: &mut Projet, edition: &mut Edition, dialogue: &Dialogue) {
    ui.strong("Calques de référence");
    ui.label(
        egui::RichText::new(
            "Une image (plan, satmap, croquis) posée dans le Plan pour décalquer routes, relief et bâti. \
             Elle ne crée rien toute seule.",
        )
        .small()
        .weak(),
    );
    ui.horizontal_wrapped(|ui| {
        let ouvert = dialogue.ouvert();
        if ui
            .add_enabled(!ouvert, egui::Button::new("Importer une image…"))
            .on_hover_text("Choisir un PNG (ou le glisser-déposer sur la fenêtre)")
            .clicked()
        {
            dialogue.ouvrir(Cible::ImageReference);
        }
        if ui
            .add_enabled(!ouvert, egui::Button::new("🪄 Convertir une carte…"))
            .on_hover_text(
                "Vue satellite ou plan illustré → carte couleurs (aplats de la convention) + relief proposé (déduit, \
                 à vérifier). Les routes ne sont pas extraites.",
            )
            .clicked()
        {
            dialogue.ouvrir(Cible::ImageAConvertir);
        }
        if ouvert {
            ui.spinner();
        }
    });
    if ui
        .button("📋 Copier la convention")
        .on_hover_text(
            "Copie les consignes (palette, carte couleurs, carte de relief en gris) à donner à une IA pour produire \
             une carte que l'éditeur sait relever.",
        )
        .clicked()
    {
        ui.ctx().copy_text(PROMPT_CARTE_IA.to_owned());
        projet.info("Convention copiée : collez-la dans la demande à l'IA (remplir les valeurs entre crochets).");
    }
    ui.collapsing("Ou saisir le chemin", |ui| {
        ui.text_edit_singleline(&mut edition.chemin_image);
        if ui.button("Importer ce chemin").clicked() {
            let chemin = PathBuf::from(edition.chemin_image.trim().trim_matches('"'));
            importer_image(projet, edition, &chemin);
        }
    });
    // Calque retiré (annuler, autre carte) : on oublie la saisie.
    if edition
        .reference_active
        .is_some_and(|id| !projet.carte.references.references.contains_key(&id))
    {
        edition.reference_active = None;
        edition.reference_saisie = None;
    }
    let liste: Vec<_> = projet
        .carte
        .references
        .references
        .iter()
        .map(|(id, r)| (*id, r.nom.clone(), r.visible))
        .collect();
    for (id, nom, visible) in liste {
        ui.horizontal_wrapped(|ui| {
            let mut v = visible;
            if ui.checkbox(&mut v, "").on_hover_text("Afficher").changed() {
                let r = projet.carte.modifier_reference(id, "Afficher un calque", |r| {
                    r.visible = v;
                    Ok(())
                });
                if let Err(e) = r {
                    projet.echec(e);
                }
            }
            let actif = edition.reference_active == Some(id);
            if ui.selectable_label(actif, nom).clicked() && !actif {
                edition.reference_active = Some(id);
                edition.reference_saisie = None;
            }
            if ui
                .small_button("🗑")
                .on_hover_text("Retirer (annulable)")
                .clicked()
            {
                let r = projet.carte.retirer_reference(id);
                projet.resultat(r, |()| "Calque retiré (Ctrl+Z pour l'annuler).".into());
            }
        });
    }
    let Some(id) = edition.reference_active else {
        return;
    };
    let Some(enregistre) = projet.carte.references.references.get(&id).cloned() else {
        return;
    };
    if edition.reference_saisie.as_ref().is_none_or(|(i, _)| *i != id) {
        edition.reference_saisie = Some((id, enregistre.clone()));
    }
    let Some((_, saisie)) = edition.reference_saisie.as_mut() else {
        return;
    };
    let (min, max) = mcf_carte::references::ECHELLE_M_PAR_PX;
    egui::Grid::new("reference").num_columns(2).show(ui, |ui| {
        ui.label("Opacité");
        ui.add(egui::Slider::new(&mut saisie.opacite, 0.05..=1.0));
        ui.end_row();
        ui.label("Échelle");
        let vitesse = saisie.m_par_px * 0.002;
        ui.add(
            egui::DragValue::new(&mut saisie.m_par_px)
                .speed(vitesse)
                .range(min..=max)
                .suffix(" m/pixel"),
        );
        ui.end_row();
        ui.label("Rotation");
        ui.add(
            egui::DragValue::new(&mut saisie.rotation_deg)
                .speed(0.1)
                .range(-360.0..=360.0)
                .suffix(" °"),
        );
        ui.end_row();
        ui.label("Coin haut gauche");
        ui.horizontal_wrapped(|ui| {
            ui.add(egui::DragValue::new(&mut saisie.coin[0]).speed(1.0).prefix("x "));
            ui.add(egui::DragValue::new(&mut saisie.coin[1]).speed(1.0).prefix("z "));
        });
        ui.end_row();
    });
    let largeur = f64::from(saisie.largeur_px) * saisie.m_par_px;
    let hauteur = f64::from(saisie.hauteur_px) * saisie.m_par_px;
    ui.label(
        egui::RichText::new(format!(
            "Image : {} × {} pixels, soit {largeur:.0} × {hauteur:.0} m.",
            saisie.largeur_px, saisie.hauteur_px
        ))
        .small(),
    );
    let modifie = *saisie != enregistre;
    let nouveau = saisie.clone();
    ui.horizontal_wrapped(|ui| {
        if ui.add_enabled(modifie, egui::Button::new("Appliquer")).clicked() {
            let r = projet
                .carte
                .modifier_reference(id, "Régler un calque de référence", |r| {
                    *r = nouveau;
                    Ok(())
                });
            projet.resultat(r, |()| "Réglages du calque appliqués.".into());
            edition.reference_saisie = None;
        }
        if ui.add_enabled(modifie, egui::Button::new("Rétablir")).clicked() {
            edition.reference_saisie = None;
        }
    });
    ui.horizontal_wrapped(|ui| {
        for (outil, texte, aide) in [
            (
                Outil::DeplacerReference,
                "✋ Déplacer",
                "Glisser dans le Plan pour déplacer le calque.",
            ),
            (
                Outil::CalerReference,
                "📏 Caler",
                "Cliquer deux repères de l'image dont on connaît la distance (graduations de l'échelle, deux villes…).",
            ),
        ] {
            if ui
                .selectable_label(edition.outil == outil, texte)
                .on_hover_text(aide)
                .clicked()
            {
                edition.outil = outil;
                edition.points.clear();
                edition.reference_saisie = None;
            }
        }
    });
    if edition.outil == Outil::CalerReference {
        ui.label(egui::RichText::new(format!("Repères posés : {} / 2", edition.points.len().min(2))).small());
        ui.horizontal_wrapped(|ui| {
            ui.label("Distance réelle");
            ui.add(
                egui::DragValue::new(&mut edition.distance_calage)
                    .speed(10.0)
                    .range(0.1..=1_000_000.0)
                    .suffix(" m"),
            );
        });
        if ui
            .add_enabled(edition.points.len() >= 2, egui::Button::new("Caler l'échelle"))
            .clicked()
        {
            let (a, b, dist) = (edition.points[0], edition.points[1], edition.distance_calage);
            let r = projet
                .carte
                .modifier_reference(id, "Caler un calque de référence", |r| {
                    r.caler_distance(a, b, dist)
                });
            projet.resultat(r, |()| {
                "Échelle calée : le premier repère n'a pas bougé. « Déplacer » pour le poser au bon endroit.".into()
            });
            edition.points.clear();
        }
    }
    meme_calage(ui, projet, edition, id);
    relief_depuis_calque(ui, projet, edition, id);
}

/// Recopie le calage d'un autre calque de même cadrage (carte couleurs et carte de relief d'une même zone) : même coin,
/// même rotation, et une échelle ramenée à la largeur en pixels de ce calque.
fn meme_calage(ui: &mut egui::Ui, projet: &mut Projet, edition: &mut Edition, id: mcf_carte::Id) {
    let autres: Vec<_> = projet
        .carte
        .references
        .references
        .iter()
        .filter(|(i, _)| **i != id)
        .map(|(i, r)| (*i, r.clone()))
        .collect();
    if autres.is_empty() {
        return;
    }
    ui.menu_button("Même calage que…", |ui| {
        for (_, modele) in autres {
            if ui.button(&modele.nom).clicked() {
                let r = projet.carte.modifier_reference(id, "Recopier un calage", |r| {
                    r.coin = modele.coin;
                    r.rotation_deg = modele.rotation_deg;
                    r.m_par_px = modele.m_par_px * f64::from(modele.largeur_px) / f64::from(r.largeur_px);
                    Ok(())
                });
                projet.resultat(r, |()| {
                    format!("Calage recopié de « {} » (même cadrage supposé).", modele.nom)
                });
                edition.reference_saisie = None;
                ui.close();
            }
        }
    })
    .response
    .on_hover_text("Pour deux images du même cadrage : la carte couleurs déjà calée et sa carte de relief.");
}

/// Applique le calque actif comme carte de relief en niveaux de gris (noir = bas, blanc = haut, transparent = rien).
fn relief_depuis_calque(ui: &mut egui::Ui, projet: &mut Projet, edition: &mut Edition, id: mcf_carte::Id) {
    ui.add_space(4.0);
    ui.strong("Relief depuis ce calque");
    ui.label(
        egui::RichText::new(
            "Image en niveaux de gris : noir = altitude du noir, blanc = altitude du blanc, transparent = relief \
             inchangé. Calez-la d'abord (« Même calage que… »).",
        )
        .small()
        .weak(),
    );
    let (min, max) = (
        projet.carte.manifeste.altitude_min_m,
        projet.carte.manifeste.altitude_max_m,
    );
    egui::Grid::new("relief_calque").num_columns(2).show(ui, |ui| {
        ui.label("Noir");
        ui.add(
            egui::DragValue::new(&mut edition.relief_noir_m)
                .range(min..=max)
                .speed(1.0)
                .suffix(" m"),
        );
        ui.end_row();
        ui.label("Blanc");
        ui.add(
            egui::DragValue::new(&mut edition.relief_blanc_m)
                .range(min..=max)
                .speed(1.0)
                .suffix(" m"),
        );
        ui.end_row();
        ui.label("Lissage");
        ui.add(
            egui::DragValue::new(&mut edition.relief_lissage_m)
                .range(0.0..=mcf_carte::relief_image::LISSAGE_MAX_M)
                .speed(1.0)
                .suffix(" m"),
        )
        .on_hover_text("Adoucit les marches des 256 niveaux de gris (0 = aucun).");
        ui.end_row();
        // Niveaux : équilibrer clair et foncé sans retoucher l'image.
        let n = &mut edition.relief_niveaux;
        ui.label("Gris du noir");
        ui.add(egui::Slider::new(&mut n.gris_noir, 0.0..=0.99).fixed_decimals(2))
            .on_hover_text("Tout gris plus sombre que celui-ci prend l'altitude du noir.");
        ui.end_row();
        ui.label("Gris du blanc");
        ui.add(egui::Slider::new(&mut n.gris_blanc, 0.01..=1.0).fixed_decimals(2))
            .on_hover_text("Tout gris plus clair que celui-ci prend l'altitude du blanc : baissez-le pour monter les reliefs.");
        ui.end_row();
        if n.gris_blanc <= n.gris_noir + 0.01 {
            n.gris_blanc = (n.gris_noir + 0.01).min(1.0);
        }
        ui.label("Courbe");
        ui.add(egui::Slider::new(&mut n.courbe, 0.2..=5.0).logarithmic(true).fixed_decimals(2))
            .on_hover_text("1 = linéaire. Plus de 1 : plaines plus basses, sommets gardés. Moins de 1 : plaines relevées.");
        ui.end_row();
    });
    ui.label(egui::RichText::new("Peindre le calque (Ctrl+clic : prendre le gris)").small());
    ui.horizontal_wrapped(|ui| {
        for m in mcf_carte::relief_image::PinceauCalque::TOUS {
            let outil = Outil::PeindreCalque(m);
            if ui.selectable_label(edition.outil == outil, m.nom()).clicked() {
                edition.outil = outil;
            }
        }
    });
    ui.add(
        egui::Slider::new(&mut edition.calque_rayon_m, 5.0..=5000.0)
            .logarithmic(true)
            .text("rayon (m)"),
    );
    ui.add(egui::Slider::new(&mut edition.calque_force, 0.05..=1.0).text("force / s"));
    ui.add(egui::Slider::new(&mut edition.calque_gris, 0.0..=1.0).text("gris visé (Niveau)"));
    if ui.small_button("Niveaux par défaut").clicked() {
        edition.relief_niveaux = mcf_carte::relief_image::Niveaux::default();
    }
    let conseille = projet.carte.references.references.get(&id).map_or(0.0, |r| {
        mcf_carte::relief_image::lissage_conseille(edition.relief_noir_m, edition.relief_blanc_m, r.m_par_px)
    });
    ui.horizontal_wrapped(|ui| {
        ui.label(
            egui::RichText::new(format!(
                "Plage de la carte : {min} à {max} m. Lissage conseillé : {conseille} m (efface les marches des \
                 256 niveaux de gris)."
            ))
            .small()
            .weak(),
        );
        if ui.small_button("Prendre").clicked() {
            edition.relief_lissage_m = conseille;
        }
    });
    if ui.button("⛰ Appliquer comme relief").clicked() {
        let r = projet.carte.relief_depuis_reference(
            id,
            edition.relief_noir_m,
            edition.relief_blanc_m,
            edition.relief_lissage_m,
            &edition.relief_niveaux,
        );
        projet.resultat(r, |n| {
            format!(
                "Relief appliqué : {n} points modifiés. Réglez et réappliquez autant que nécessaire (chaque \
                 application remplace la précédente, Ctrl+Z pour annuler)."
            )
        });
    }
}
