//! Panneaux egui : barre du haut (fichier, annuler, vues), étapes et outils à gauche, inspecteur à droite, barre
//! d'état en bas. L'espace restant est donné aux vues (`ZoneVues`).

use std::path::PathBuf;

use bevy::prelude::*;
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
    outils,
};

const TAILLES: [f64; 8] = [256.0, 512.0, 1024.0, 1536.0, 2048.0, 3072.0, 4096.0, 5120.0];

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
}

pub fn interface(
    mut contexts: EguiContexts,
    mut projet: ResMut<Projet>,
    mut edition: ResMut<Edition>,
    mut zone: ResMut<ZoneVues>,
    mut plan: ResMut<VuePlan>,
    mut orbite: ResMut<Orbite>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    let mut racine = egui::Ui::new(
        ctx.clone(),
        "racine".into(),
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::background())
            .max_rect(ctx.viewport_rect()),
    );
    let (projet, edition) = (&mut *projet, &mut *edition);

    egui::Panel::top("haut").show(&mut racine, |ui| {
        ui.horizontal(|ui| {
            ui.strong("MCF Editor · Conception de carte");
            ui.separator();
            let sale = if projet.carte.sale.propre() { "" } else { " *" };
            ui.label(format!("{}{sale}", projet.carte.manifeste.nom));
            ui.separator();
            if ui.button("↶ Annuler").on_hover_text("Ctrl+Z").clicked() {
                outils::annuler(projet, edition);
            }
            if ui.button("↷ Refaire").on_hover_text("Ctrl+Y").clicked() {
                outils::refaire(projet, edition);
            }
            if ui.button("💾 Enregistrer").on_hover_text("Ctrl+S").clicked() {
                outils::enregistrer(projet, edition);
            }
            ui.separator();
            ui.label("Vue :");
            for (d, nom, astuce) in [
                (Disposition::Plan, "Plan", "Tab"),
                (Disposition::Perspective, "3D", "Tab"),
                (Disposition::Partagee, "Plan + 3D", "Maj+Tab"),
            ] {
                if ui
                    .selectable_label(edition.disposition == d, nom)
                    .on_hover_text(astuce)
                    .clicked()
                {
                    edition.disposition = d;
                }
            }
            if ui.button("⌖ Recentrer").clicked() {
                recentrer(&projet.carte.manifeste, &mut plan, &mut orbite);
            }
        });
    });

    egui::Panel::bottom("etat").show(&mut racine, |ui| {
        ui.horizontal(|ui| {
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
        .resizable(false)
        .exact_size(300.0)
        .show(&mut racine, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.heading("Étapes");
                ui.horizontal_wrapped(|ui| {
                    for e in Etape::TOUTES {
                        if ui.selectable_label(edition.etape == e, e.nom()).clicked() {
                            edition.aller_a(e);
                        }
                    }
                });
                ui.separator();
                match edition.etape {
                    Etape::Cadre => cadre(ui, projet, edition, &mut plan, &mut orbite),
                    Etape::Relief => etape_relief(ui, edition),
                    Etape::Reseau => etape_reseau(ui, projet, edition),
                    Etape::Quartiers => etape_quartiers(ui, edition),
                    Etape::Bati => etape_bati(ui, projet, edition),
                    Etape::Export => etape_export(ui, projet, edition),
                }
            });
        });

    egui::Panel::right("inspecteur")
        .resizable(false)
        .exact_size(290.0)
        .show(&mut racine, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.heading("Inspecteur");
                inspecteur(ui, projet, edition);
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
) {
    ui.strong("Nouvelle carte");
    ui.horizontal(|ui| {
        ui.label("Nom");
        ui.text_edit_singleline(&mut edition.nouveau_nom);
    });
    egui::ComboBox::from_label("Côté")
        .selected_text(format!("{} m", edition.nouvelle_taille))
        .show_ui(ui, |ui| {
            for t in TAILLES {
                ui.selectable_value(&mut edition.nouvelle_taille, t, format!("{t} m"));
            }
        });
    if edition.nouvelle_taille > 2048.0 {
        ui.label(
            egui::RichText::new("Grande carte : sculpture plus lente (relief à 1 m).")
                .small()
                .color(egui::Color32::from_rgb(220, 160, 60)),
        );
    }
    if ui.button("Créer (vierge, plate)").clicked() {
        let nom = edition.nouveau_nom.trim().to_owned();
        if !mcf_carte::manifeste::nom_sur(&nom) {
            projet.echec("Nom refusé : minuscules, chiffres, « _ » et « - » seulement (64 au plus).");
        } else {
            match Carte::nouvelle(Manifeste::nouveau(&nom, edition.nouvelle_taille)) {
                Ok(c) => {
                    remplacer(projet, edition, c, plan, orbite);
                    edition.chemin.clear();
                    edition.aller_a(Etape::Relief);
                    projet.info(format!(
                        "Carte « {nom} » créée. Sculptez le relief, puis tracez le réseau."
                    ));
                }
                Err(e) => projet.echec(e),
            }
        }
    }
    ui.separator();
    ui.strong("Fichier");
    ui.label("Dossier du projet de carte");
    ui.text_edit_singleline(&mut edition.chemin);
    ui.horizontal(|ui| {
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
        egui::Slider::new(&mut edition.rayon, 2.0..=300.0)
            .logarithmic(true)
            .text("rayon (m)"),
    );
    let (max, unite) = match edition.outil {
        Outil::Pinceau(Pinceau::Lisser | Pinceau::Aplanir) => (4.0, "force"),
        _ => (40.0, "m/s"),
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
    ui.horizontal(|ui| {
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
        ui.horizontal(|ui| {
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
            "Éditer : cliquez une route, puis glissez ses poignées. Suppr : supprimer.",
        ],
    );
}

fn etape_quartiers(ui: &mut egui::Ui, edition: &mut Edition) {
    ui.horizontal(|ui| {
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
            "Étape 5 pour faire pousser le bâti.",
        ],
    );
}

fn familles(ui: &mut egui::Ui, edition: &mut Edition) {
    ui.strong("Famille");
    for f in Famille::TOUTES {
        ui.horizontal(|ui| {
            pastille(ui, f.couleur_plan());
            if ui.selectable_label(edition.famille == f, f.nom()).clicked() {
                edition.famille = f;
            }
        });
    }
    ui.strong("Densité");
    ui.horizontal(|ui| {
        for d in Densite::TOUTES {
            if ui.selectable_label(edition.densite == d, d.nom()).clicked() {
                edition.densite = d;
            }
        }
    });
}

fn etape_bati(ui: &mut egui::Ui, projet: &mut Projet, edition: &mut Edition) {
    ui.horizontal(|ui| {
        if ui
            .selectable_label(edition.outil == Outil::Lotir, "🏗 Lotir un quartier")
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
            "Lotir : cliquez un quartier ; relancer ne touche que les blocs « générés ».",
            "Ajuster : glissez un bâtiment (il devient « manuel »). Ctrl+clic : sélection multiple.",
            "Suppr : supprimer. Verrou et niveaux dans l'inspecteur.",
        ],
    );
}

fn etape_export(ui: &mut egui::Ui, projet: &mut Projet, edition: &mut Edition) {
    ui.strong("Aperçu (PNG, ≤ 2048 px)");
    ui.horizontal(|ui| {
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

fn inspecteur(ui: &mut egui::Ui, projet: &mut Projet, edition: &mut Edition) {
    match edition.selection.clone() {
        Selection::Rien => {
            ui.label(egui::RichText::new("Rien de sélectionné.").weak());
        }
        Selection::Batiments(ids) if ids.len() == 1 => {
            let id = ids[0];
            let Some(b) = projet.carte.batiments.get(&id).cloned() else {
                return;
            };
            ui.strong(format!("Bâtiment · {}", b.famille.nom()));
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
            if ui
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
            if ui.checkbox(&mut verrou, "🔒 Verrouillé").changed() {
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
            ui.horizontal(|ui| {
                for (v, nom) in [(true, "🔒 Verrouiller"), (false, "Déverrouiller")] {
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
            let mut nom = t.nom.clone().unwrap_or_default();
            ui.horizontal(|ui| {
                ui.label("Nom");
                let rep = ui.text_edit_singleline(&mut nom);
                if rep.lost_focus() && Some(nom.trim()) != t.nom.as_deref() {
                    let valeur = (!nom.trim().is_empty()).then(|| nom.trim().to_owned());
                    let r = projet.carte.modifier("Nommer la voie", |tx| {
                        if let Some(x) = tx.reseau().troncons.get_mut(&id) {
                            x.nom = valeur;
                        }
                        Ok(())
                    });
                    projet.resultat(r, |()| "Voie nommée.".into());
                }
            });
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
            ui.horizontal(|ui| {
                if ui.button("🏗 Lotir").clicked() {
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
