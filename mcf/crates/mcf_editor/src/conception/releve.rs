//! Relevé assisté sur le calque de référence actif (étape 1 « Cadre ») : l'auteur choisit ce qu'il relève (eau,
//! taches, routes), clique la couleur dans le Plan, puis **valide ou rejette chaque proposition**. L'analyse vient
//! de `mcf_carte::releve` ; chaque validation passe par les commandes habituelles de la carte (une étape
//! d'historique, annulable). Les propositions ne sont jamais enregistrées.

use std::sync::Arc;

use bevy::prelude::*;
use bevy_egui::egui;
use mcf_carte::{
    Famille, Id, Reference,
    releve::{self, Grille, Proposition, Reglages, Sorte},
};
use mcf_geo::{ClasseRoute, P2};

use super::{
    Edition, Outil, Projet,
    camera::VuePlan,
    outils::{GizmosDessus, GizmosPlan},
};

/// État du relevé (non enregistré).
pub struct EtatReleve {
    /// Ce qu'on relève.
    pub sorte: Sorte,
    /// Réglages de l'analyse.
    pub reglages: Reglages,
    /// Propositions en attente.
    pub propositions: Vec<Proposition>,
    /// Proposition mise en avant.
    pub courante: usize,
    /// Dernier clic (calque, point), pour relancer avec d'autres réglages.
    pub graine: Option<(Id, P2)>,
    /// Eau : niveau (m).
    pub niveau: f64,
    /// Eau : profondeur sous le niveau (m).
    pub profondeur: f64,
    /// Eau : largeur de la rive en pente douce (m).
    pub rive: f64,
    /// Grille d'analyse du calque (recalculée si le calque change).
    grille: Option<(Id, Reference, Arc<Grille>)>,
}

impl Default for EtatReleve {
    fn default() -> Self {
        Self {
            sorte: Sorte::Eau,
            reglages: Sorte::Eau.reglages(),
            propositions: Vec::new(),
            courante: 0,
            graine: None,
            niveau: -1.0,
            profondeur: 6.0,
            rive: 40.0,
            grille: None,
        }
    }
}

impl EtatReleve {
    /// Oublie tout (autre carte).
    pub fn vider(&mut self) {
        self.propositions.clear();
        self.courante = 0;
        self.graine = None;
        self.grille = None;
    }
}

/// Clic de l'outil « Relevé » : analyse la couleur sous `p` sur le calque actif.
pub fn analyser(projet: &mut Projet, edition: &mut Edition, p: P2) {
    let Some(id) = edition.reference_active else {
        projet.echec("Choisissez d'abord un calque de référence.");
        return;
    };
    let carte = &projet.carte;
    let (Some(r), Some(image)) = (
        carte.references.references.get(&id),
        carte.images_references.get(&id),
    ) else {
        projet.echec("Calque de référence introuvable.");
        return;
    };
    let etat = &mut edition.releve;
    // La grille ne dépend que de l'image et du placement : l'opacité ou la visibilité n'y changent rien.
    let meme = |a: &Reference, b: &Reference| {
        a.coin == b.coin && a.m_par_px == b.m_par_px && a.rotation_deg == b.rotation_deg
    };
    let grille = match &etat.grille {
        Some((i, gr, g)) if *i == id && meme(gr, r) => g.clone(),
        _ => match Grille::preparer(image, r, &carte.manifeste) {
            Ok(g) => {
                let g = Arc::new(g);
                etat.grille = Some((id, r.clone(), g.clone()));
                g
            }
            Err(e) => {
                projet.echec(format!("Relevé impossible : {e}"));
                return;
            }
        },
    };
    let debut = std::time::Instant::now();
    match grille.analyser(etat.sorte, p, &etat.reglages) {
        Ok(props) => {
            let n = props.len();
            etat.propositions = props;
            etat.courante = 0;
            etat.graine = Some((id, p));
            let plein = if n >= releve::MAX_PROPOSITIONS {
                " (plafond atteint : les plus grandes)"
            } else {
                ""
            };
            projet.info(format!(
                "{n} proposition(s){plein} en {:.0} ms : validez ou rejetez-les une par une (étape 1).",
                debut.elapsed().as_secs_f64() * 1000.0
            ));
        }
        Err(e) => {
            etat.propositions.clear();
            etat.graine = Some((id, p));
            projet.echec(format!("Relevé : {e}."));
        }
    }
}

/// Applique la proposition `k` à la carte. Retirée de la liste si la carte l'accepte.
fn valider(projet: &mut Projet, edition: &mut Edition, k: usize) -> bool {
    let Some(prop) = edition.releve.propositions.get(k).cloned() else {
        return false;
    };
    let m = projet.carte.manifeste.clone();
    let etat = &edition.releve;
    let r = match &prop {
        Proposition::Eau(eau) => projet
            .carte
            .creuser_eau(eau, etat.niveau, etat.profondeur, etat.rive)
            .map(|n| {
                format!(
                    "Plan d'eau creusé ({n} points de relief abaissés), niveau {:.1} m.",
                    etat.niveau
                )
            }),
        Proposition::Zone { polygone, .. } => {
            let (f, d) = (edition.famille, edition.densite);
            projet
                .carte
                .ajouter_zone(releve::borner(polygone, &m), f, d)
                .map(|_| format!("Zone libre ajoutée : {} · {}.", f.nom(), d.nom()))
        }
        Proposition::Route { trace, .. } => {
            let c = edition.classe;
            projet
                .carte
                .tracer_route(c, &releve::borner(trace, &m), 4.0)
                .map(|ids| format!("{} : {} tronçon(s) tracé(s).", c.nom(), ids.len()))
        }
    };
    match r {
        Ok(msg) => {
            projet.info(msg);
            let etat = &mut edition.releve;
            etat.propositions.remove(k);
            etat.courante = etat.courante.min(etat.propositions.len().saturating_sub(1));
            true
        }
        Err(e) => {
            projet.echec(format!("Proposition refusée par la carte : {e}"));
            false
        }
    }
}

/// Panneau du relevé (sous les calques de référence).
pub fn panneau(ui: &mut egui::Ui, projet: &mut Projet, edition: &mut Edition, plan: &mut VuePlan) {
    let Some(id) = edition.reference_active else {
        return;
    };
    ui.add_space(6.0);
    ui.strong("Relevé assisté");
    ui.label(
        egui::RichText::new(
            "Calez d'abord le calque. Choisissez quoi relever, puis cliquez la couleur dans le Plan : l'éditeur \
             propose, vous validez une par une. Rien n'est fait tout seul.",
        )
        .small()
        .weak(),
    );
    let etat = &mut edition.releve;
    ui.horizontal(|ui| {
        for s in Sorte::TOUTES {
            if ui.selectable_label(etat.sorte == s, s.nom()).clicked() && etat.sorte != s {
                etat.sorte = s;
                etat.reglages = s.reglages();
                etat.propositions.clear();
            }
        }
    });
    let pipette = edition.outil == Outil::Releve;
    if ui
        .selectable_label(pipette, "🎯 Cliquer une couleur du calque")
        .on_hover_text("Puis cliquez dans le Plan sur la mer, une tache de ville, une route…")
        .clicked()
    {
        edition.outil = Outil::Releve;
        edition.points.clear();
    }
    let etat = &mut edition.releve;
    let mut relancer = false;
    egui::Grid::new("releve").num_columns(2).show(ui, |ui| {
        ui.label("Tolérance couleur");
        relancer |= ui
            .add(egui::Slider::new(&mut etat.reglages.tolerance, 2.0..=60.0))
            .on_hover_text("Écart de couleur accepté (≈ 2 : identique, 10 : proche, 40 : très large).")
            .drag_stopped();
        ui.end_row();
        ui.label("Combler");
        relancer |= ui
            .add(egui::Slider::new(&mut etat.reglages.combler_px, 0..=releve::MAX_COMBLER_PX).suffix(" px"))
            .on_hover_text("Referme les textes et traits fins qui coupent une tache (pixels du calque).")
            .drag_stopped();
        ui.end_row();
        match etat.sorte {
            Sorte::Eau | Sorte::Zone => {
                ui.label("Aire minimale");
                relancer |= ui
                    .add(
                        egui::DragValue::new(&mut etat.reglages.aire_min_m2)
                            .speed(500.0)
                            .range(0.0..=1e9)
                            .suffix(" m²"),
                    )
                    .on_hover_text(if etat.sorte == Sorte::Eau {
                        "Taches plus petites ignorées ; îles plus petites comblées."
                    } else {
                        "Taches plus petites ignorées."
                    })
                    .drag_stopped();
                ui.end_row();
            }
            Sorte::Routes => {
                ui.label("Longueur minimale");
                relancer |= ui
                    .add(
                        egui::DragValue::new(&mut etat.reglages.longueur_min_m)
                            .speed(5.0)
                            .range(0.0..=100_000.0)
                            .suffix(" m"),
                    )
                    .on_hover_text("Branches plus courtes ignorées, ergots retirés.")
                    .drag_stopped();
                ui.end_row();
            }
        }
    });
    relancer |= ui
        .checkbox(&mut etat.reglages.toutes, "Toutes les taches de cette couleur")
        .changed();
    if let Some((gid, p)) = etat.graine
        && gid == id
        && (relancer || ui.small_button("↻ Relancer avec ces réglages").clicked())
    {
        analyser(projet, edition, p);
    }

    let etat = &mut edition.releve;
    if etat.propositions.is_empty() {
        return;
    }
    ui.separator();
    // Réglages appliqués à la validation.
    match etat.sorte {
        Sorte::Eau => {
            egui::Grid::new("releve_eau").num_columns(2).show(ui, |ui| {
                let m = &projet.carte.manifeste;
                ui.label("Niveau de l'eau");
                ui.add(
                    egui::DragValue::new(&mut etat.niveau)
                        .speed(0.1)
                        .range(m.altitude_min_m..=m.altitude_max_m)
                        .suffix(" m"),
                );
                ui.end_row();
                ui.label("Profondeur");
                ui.add(
                    egui::DragValue::new(&mut etat.profondeur)
                        .speed(0.2)
                        .range(0.5..=500.0)
                        .suffix(" m"),
                );
                ui.end_row();
                ui.label("Rive en pente");
                ui.add(
                    egui::DragValue::new(&mut etat.rive)
                        .speed(1.0)
                        .range(0.0..=2000.0)
                        .suffix(" m"),
                );
                ui.end_row();
            });
            ui.label(
                egui::RichText::new(
                    "Le relief est abaissé sous l'eau, jamais relevé : une terre plus basse que le niveau sera \
                     inondée.",
                )
                .small()
                .weak(),
            );
        }
        Sorte::Zone => {
            ui.horizontal(|ui| {
                ui.label("Usage");
                egui::ComboBox::from_id_salt("releve_famille")
                    .selected_text(edition.famille.nom())
                    .show_ui(ui, |ui| {
                        for f in Famille::TOUTES {
                            ui.selectable_value(&mut edition.famille, f, f.nom());
                        }
                    });
                egui::ComboBox::from_id_salt("releve_densite")
                    .selected_text(edition.densite.nom())
                    .show_ui(ui, |ui| {
                        for d in mcf_carte::Densite::TOUTES {
                            ui.selectable_value(&mut edition.densite, d, d.nom());
                        }
                    });
            });
        }
        Sorte::Routes => {
            ui.horizontal(|ui| {
                ui.label("Classe");
                egui::ComboBox::from_id_salt("releve_classe")
                    .selected_text(edition.classe.nom())
                    .show_ui(ui, |ui| {
                        for c in ClasseRoute::TOUTES {
                            ui.selectable_value(&mut edition.classe, c, c.nom());
                        }
                    });
            });
        }
    }
    let etat = &mut edition.releve;
    let n = etat.propositions.len();
    etat.courante = etat.courante.min(n - 1);
    let k = etat.courante;
    ui.horizontal(|ui| {
        if ui.small_button("◀").clicked() {
            etat.courante = (k + n - 1) % n;
        }
        ui.label(format!("{} / {n} : {}", k + 1, etat.propositions[k].resume()));
        if ui.small_button("▶").clicked() {
            etat.courante = (k + 1) % n;
        }
    });
    let centre = etat.propositions[k].centre();
    let mut action = None;
    ui.horizontal(|ui| {
        if ui
            .button("✔ Valider")
            .on_hover_text("Applique cette proposition (une étape, Ctrl+Z pour l'annuler).")
            .clicked()
        {
            action = Some(true);
        }
        if ui.button("✖ Rejeter").clicked() {
            action = Some(false);
        }
        if ui
            .button("◎ Voir")
            .on_hover_text("Centre le Plan dessus.")
            .clicked()
        {
            plan.centre = Vec2::new(centre[0] as f32, centre[1] as f32);
        }
    });
    match action {
        Some(true) => {
            valider(projet, edition, k);
        }
        Some(false) => {
            let etat = &mut edition.releve;
            etat.propositions.remove(k);
            etat.courante = etat.courante.min(etat.propositions.len().saturating_sub(1));
            projet.info("Proposition rejetée.");
        }
        None => {}
    }
    let n = edition.releve.propositions.len();
    if n > 1 {
        ui.horizontal(|ui| {
            if ui
                .small_button(format!("Valider les {n}"))
                .on_hover_text("Une étape d'historique par proposition ; s'arrête à la première refusée.")
                .clicked()
            {
                let mut faites = 0;
                while !edition.releve.propositions.is_empty() && valider(projet, edition, 0) {
                    faites += 1;
                }
                if !projet.erreur {
                    projet.info(format!("{faites} proposition(s) validée(s)."));
                }
            }
            if ui.small_button("Tout rejeter").clicked() {
                edition.releve.propositions.clear();
                projet.info("Propositions rejetées.");
            }
        });
    }
}

/// Repères des propositions : toutes en pâle dans le Plan, la courante en vif (Plan et 3D).
pub fn dessiner(
    plan: &mut Gizmos<GizmosPlan>,
    dessus: &mut Gizmos<GizmosDessus>,
    edition: &Edition,
    terrain: &mcf_carte::relief::Relief,
    y_plan: f32,
) {
    let etat = &edition.releve;
    let couleur = match etat.sorte {
        Sorte::Eau => Color::srgb(0.1, 0.55, 1.0),
        Sorte::Zone => Color::srgb(0.85, 0.25, 0.85),
        Sorte::Routes => Color::srgb(1.0, 0.45, 0.05),
    };
    let point = |q: &P2, y: f32| Vec3::new(q[0] as f32, y, q[1] as f32);
    for (k, prop) in etat.propositions.iter().enumerate() {
        let courante = k == etat.courante;
        for (ligne, ferme) in prop.lignes() {
            let mut pts: Vec<Vec3> = ligne.iter().map(|q| point(q, y_plan)).collect();
            if ferme && let Some(p) = pts.first().copied() {
                pts.push(p);
            }
            let c = if courante {
                couleur
            } else {
                couleur.with_alpha(0.45)
            };
            plan.linestrip(pts, c);
            if courante {
                let pts: Vec<Vec3> = ligne
                    .iter()
                    .chain(ferme.then(|| ligne.first()).flatten())
                    .map(|q| point(q, (terrain.hauteur(*q) + 0.8) as f32))
                    .collect();
                dessus.linestrip(pts, couleur);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use mcf_carte::Carte;

    use super::*;

    /// Carte de 512 m, calque de 256 px à 2 m : mer à gauche, route jaune horizontale.
    fn projet() -> (Projet, Edition) {
        let mut carte = Carte::nouvelle(mcf_carte::Manifeste::nouveau("t", 512.0)).expect("carte");
        let mut rgba = Vec::new();
        for y in 0..256u32 {
            for x in 0..256u32 {
                let p: [u8; 3] = if x < 64 {
                    [60, 130, 200]
                } else if (160..=163).contains(&y) {
                    [240, 200, 40]
                } else {
                    [150, 200, 110]
                };
                rgba.extend_from_slice(&[p[0], p[1], p[2], 255]);
            }
        }
        let png = mcf_carte::references::encoder_png(&mcf_carte::ImageRgba {
            largeur: 256,
            hauteur: 256,
            rgba: Arc::new(rgba),
        })
        .expect("png");
        let id = carte.importer_reference("plan", &png).expect("import");
        let edition = Edition {
            reference_active: Some(id),
            ..Default::default()
        };
        let projet = Projet {
            carte,
            message: String::new(),
            erreur: false,
        };
        (projet, edition)
    }

    #[test]
    fn clic_propose_et_rien_n_est_ecrit_avant_validation() {
        let (mut projet, mut edition) = projet();
        edition.releve.sorte = Sorte::Routes;
        edition.releve.reglages = Sorte::Routes.reglages();
        let r = projet.carte.references.references[&edition.reference_active.expect("id")].clone();
        analyser(&mut projet, &mut edition, r.vers_carte(128.0, 161.5));
        assert!(!projet.erreur, "{}", projet.message);
        assert_eq!(edition.releve.propositions.len(), 1);
        assert!(
            projet.carte.reseau.troncons.is_empty(),
            "rien d'écrit avant validation"
        );
        assert!(valider(&mut projet, &mut edition, 0), "{}", projet.message);
        assert!(!projet.carte.reseau.troncons.is_empty());
        assert!(edition.releve.propositions.is_empty());
        projet.carte.annuler();
        assert!(projet.carte.reseau.troncons.is_empty());
    }

    #[test]
    fn eau_validee_puis_clic_hors_calque_refuse() {
        let (mut projet, mut edition) = projet();
        let r = projet.carte.references.references[&edition.reference_active.expect("id")].clone();
        analyser(&mut projet, &mut edition, r.vers_carte(10.0, 10.0));
        assert_eq!(edition.releve.propositions.len(), 1, "{}", projet.message);
        assert!(valider(&mut projet, &mut edition, 0), "{}", projet.message);
        assert_eq!(projet.carte.manifeste.niveau_eau_m, Some(-1.0));
        analyser(&mut projet, &mut edition, [10_000.0, 0.0]);
        assert!(projet.erreur);
        assert!(edition.releve.propositions.is_empty());
    }
}
