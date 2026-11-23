//! Interaction : point de la carte sous la souris, outils de chaque étape, raccourcis, repères visuels (gizmos).
//!
//! Toutes les commandes passent par l'API de `mcf_carte` (une étape d'historique par geste, validation avant
//! d'appliquer). Un glisser n'écrit rien avant le relâchement : une seule annulation par geste.

use std::path::PathBuf;

use bevy::{camera::visibility::RenderLayers, prelude::*, window::PrimaryWindow};
use mcf_carte::{Carte, Id, relief::Pinceau};
use mcf_geo::{P2, polygone};

use super::{
    COUCHE_3D, COUCHE_PLAN, CibleGlisse, Disposition, Edition, Etape, Glisse, Outil, Projet, Selection,
    ZoneVues,
    camera::{Vue, vue_sous},
    scene::Affiche,
};

/// Repères visibles seulement dans le Plan.
#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct GizmosPlan;

/// Repères visibles seulement dans la 3D.
#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct Gizmos3d;

/// Repères visibles partout, toujours au premier plan (sélection, tracé en cours).
#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct GizmosDessus;

pub struct OutilsPlugin;

impl Plugin for OutilsPlugin {
    fn build(&self, app: &mut App) {
        app.init_gizmo_group::<GizmosPlan>()
            .init_gizmo_group::<Gizmos3d>()
            .init_gizmo_group::<GizmosDessus>()
            .add_systems(Startup, regler_gizmos)
            .add_systems(Update, (souris, clavier, dessiner).chain());
    }
}

fn regler_gizmos(mut store: ResMut<GizmoConfigStore>) {
    let (c, _) = store.config_mut::<GizmosPlan>();
    c.render_layers = RenderLayers::layer(COUCHE_PLAN);
    c.line.width = 1.0;
    let (c, _) = store.config_mut::<Gizmos3d>();
    c.render_layers = RenderLayers::layer(COUCHE_3D);
    c.line.width = 2.0;
    let (c, _) = store.config_mut::<GizmosDessus>();
    c.render_layers = RenderLayers::from_layers(&[COUCHE_PLAN, COUCHE_3D]);
    c.depth_bias = -1.0;
    c.line.width = 2.5;
}

/// Point de la carte visé par le curseur dans une vue, et son altitude (terrain final).
fn viser(
    vue: Vue,
    camera: &Camera,
    gt: &GlobalTransform,
    curseur: Vec2,
    carte: &mut Carte,
) -> Option<(P2, f64)> {
    let rayon = camera.viewport_to_world(gt, curseur).ok()?;
    let manifeste_contient = |p: P2, c: &Carte| c.manifeste.contient(p);
    match vue {
        Vue::Plan => {
            let p = [f64::from(rayon.origin.x), f64::from(rayon.origin.z)];
            if !manifeste_contient(p, carte) {
                return None;
            }
            let h = carte.derive().terrain.hauteur(p);
            Some((p, h))
        }
        Vue::Perspective => {
            let manifeste = carte.manifeste.clone();
            let terrain = &carte.derive().terrain;
            let o = rayon.origin.as_dvec3();
            let d = rayon.direction.as_dvec3();
            let dessous = |t: f64| {
                let q = o + d * t;
                q.y <= terrain.hauteur([q.x, q.z])
            };
            // Marche à pas de 2 m puis dichotomie : précis au centimètre.
            let (mut t0, mut t) = (0.0, 0.0);
            while t < 30_000.0 {
                if dessous(t) {
                    let mut t1 = t;
                    for _ in 0..24 {
                        let m = (t0 + t1) * 0.5;
                        if dessous(m) {
                            t1 = m;
                        } else {
                            t0 = m;
                        }
                    }
                    let q = o + d * t1;
                    let p = [q.x, q.z];
                    return manifeste.contient(p).then(|| (p, terrain.hauteur(p)));
                }
                t0 = t;
                t += 2.0f64.max(t * 0.002);
            }
            None
        }
    }
}

/// Affectation ou zone libre qui contient `p`.
pub fn quartier_en(carte: &mut Carte, p: P2) -> Option<Id> {
    let affectations = carte.quartiers.affectations.clone();
    let zones = carte.quartiers.zones.clone();
    let d = carte.derive();
    if let Some(i) = mcf_carte::ilots::ilot_en(&d.ilots, p)
        && let Some((id, _)) = affectations
            .iter()
            .find(|(_, a)| polygone::contient(&i.contour, a.repere))
    {
        return Some(*id);
    }
    zones
        .iter()
        .find(|(_, z)| polygone::contient(&z.polygone, p))
        .map(|(id, _)| *id)
}

fn batiment_en(carte: &Carte, p: P2) -> Option<Id> {
    carte
        .batiments
        .iter()
        .find(|(_, b)| polygone::contient(&b.empreinte, p))
        .map(|(id, _)| *id)
}

/// Enregistre le projet (dossier courant, sinon le chemin saisi, qui doit être neuf ou vide).
pub fn enregistrer(projet: &mut Projet, edition: &Edition) {
    let chemin = edition.chemin.trim();
    let cible = match (&projet.carte.dossier, chemin.is_empty()) {
        (Some(_), _) => None,
        (None, false) => Some(PathBuf::from(chemin)),
        (None, true) => {
            projet.echec("Indiquez d'abord un dossier de projet (neuf ou vide) dans « Fichier ».");
            return;
        }
    };
    let r = projet.carte.enregistrer(cible.as_deref());
    let dossier = projet.carte.dossier.clone();
    projet.resultat(r, |()| match dossier {
        Some(d) => format!("Enregistré dans {}.", d.display()),
        None => "Enregistré.".into(),
    });
}

/// Supprime la sélection (une seule étape d'historique).
pub fn supprimer(projet: &mut Projet, edition: &mut Edition) {
    let sel = std::mem::take(&mut edition.selection);
    let r = match &sel {
        Selection::Rien => return,
        Selection::Batiments(ids) => projet.carte.modifier("Supprimer des bâtiments", |tx| {
            let mut n = 0;
            for id in ids {
                n += usize::from(tx.retirer_batiment(*id).is_some());
            }
            Ok(format!("{n} bâtiment(s) supprimé(s)."))
        }),
        Selection::Troncon(id) => projet.carte.modifier("Supprimer un tronçon", |tx| {
            if tx.reseau().supprimer_troncon(*id) {
                Ok("Tronçon supprimé.".to_string())
            } else {
                Err("tronçon introuvable".into())
            }
        }),
        Selection::Quartier(id) => projet.carte.modifier("Retirer un quartier", |tx| {
            let q = tx.quartiers();
            if q.affectations.remove(id).is_some() || q.zones.remove(id).is_some() {
                Ok("Quartier retiré (ses bâtiments restent, à supprimer à part).".to_string())
            } else {
                Err("quartier introuvable".into())
            }
        }),
    };
    projet.resultat(r, |m| m);
}

/// Termine le tracé ou la zone en cours.
pub fn terminer(projet: &mut Projet, edition: &mut Edition, sans_accroche: bool) {
    let points = std::mem::take(&mut edition.points);
    match edition.outil {
        Outil::Tracer if points.len() >= 2 => {
            let accroche = if edition.accroche && !sans_accroche {
                mcf_carte::carte_de_test::ACCROCHE_M
            } else {
                0.0
            };
            let r = projet.carte.tracer_route(edition.classe, &points, accroche);
            let classe = edition.classe.nom();
            projet.resultat(r, |ids| format!("{classe} tracée ({} tronçon(s)).", ids.len()));
        }
        Outil::ZoneLibre if points.len() >= 3 => {
            if !polygone::est_simple(&points) {
                projet.echec("Zone refusée : ses bords se croisent.");
                return;
            }
            let r = projet
                .carte
                .ajouter_zone(points, edition.famille, edition.densite);
            projet.resultat(r, |_| "Zone libre ajoutée. Outil « Lotir » pour la bâtir.".into());
        }
        Outil::Tracer | Outil::ZoneLibre if !points.is_empty() => {
            edition.points = points;
            projet.echec("Il manque des points (route : 2, zone : 3).");
        }
        _ => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn souris(
    window: Single<&Window, With<PrimaryWindow>>,
    boutons: Res<ButtonInput<MouseButton>>,
    touches: Res<ButtonInput<KeyCode>>,
    temps: Res<Time>,
    zone: Res<ZoneVues>,
    cameras: Query<(&Vue, &Camera, &GlobalTransform)>,
    mut projet: ResMut<Projet>,
    mut edition: ResMut<Edition>,
) {
    let ctrl = touches.pressed(KeyCode::ControlLeft) || touches.pressed(KeyCode::ControlRight);
    let alt = touches.pressed(KeyCode::AltLeft) || touches.pressed(KeyCode::AltRight);
    let curseur = window.cursor_position();
    let vue = curseur.and_then(|c| vue_sous(c, &zone, edition.disposition));
    let camera = vue.and_then(|v| cameras.iter().find(|(x, ..)| **x == v));
    edition.curseur = match (curseur, camera) {
        (Some(c), Some((v, cam, gt))) => viser(*v, cam, gt, c, &mut projet.carte),
        _ => None,
    };
    // Coup de pinceau en 3D : on vise le plan horizontal pris au début du geste.
    let pinceau = matches!(edition.outil, Outil::Pinceau(_)) && boutons.pressed(MouseButton::Left);
    match (pinceau, edition.plan_pinceau, curseur, camera) {
        (true, None, _, Some((Vue::Perspective, ..))) => {
            edition.plan_pinceau = edition.curseur.map(|(_, h)| h)
        }
        (true, Some(y), Some(c), Some((Vue::Perspective, cam, gt))) => {
            if let Ok(r) = cam.viewport_to_world(gt, c) {
                let (o, d) = (r.origin.as_dvec3(), r.direction.as_dvec3());
                if d.y.abs() > 1e-6 {
                    let q = o + d * ((y - o.y) / d.y);
                    let p = [q.x, q.z];
                    if projet.carte.manifeste.contient(p) {
                        let h = projet.carte.derive().terrain.hauteur(p);
                        edition.curseur = Some((p, h));
                    }
                }
            }
        }
        (false, ..) => edition.plan_pinceau = None,
        _ => {}
    }

    // Fin d'un geste de sculpture : une seule étape d'historique.
    if boutons.just_released(MouseButton::Left) {
        projet.carte.fin_trait();
    }

    // Glisser en cours.
    if let Some(mut g) = edition.glisse {
        if let Some((p, _)) = edition.curseur {
            g.actuel = p;
            edition.glisse = Some(g);
        }
        if boutons.just_released(MouseButton::Left) {
            edition.glisse = None;
            appliquer_glisse(&mut projet, &edition, g);
        }
        return;
    }

    let Some((p, h)) = edition.curseur else {
        return;
    };
    let appui = boutons.just_pressed(MouseButton::Left);
    let maj = touches.pressed(KeyCode::ShiftLeft) || touches.pressed(KeyCode::ShiftRight);
    // Clic sur la mini-carte : elle s'agrandit (vue partagée).
    if appui && vue == Some(Vue::Plan) && edition.disposition == Disposition::Perspective {
        edition.disposition = Disposition::Partagee;
        return;
    }
    // Maj+clic : sélectionner depuis n'importe quel outil.
    if appui && maj && edition.outil != Outil::Selection {
        selectionner(&mut projet, &mut edition, p, None, false);
        return;
    }
    match edition.outil {
        Outil::Selection => {
            if appui {
                let viseur = camera.zip(curseur).map(|((_, cam, gt), c)| (cam, gt, c));
                selectionner(&mut projet, &mut edition, p, viseur, ctrl);
            }
        }
        Outil::Tracer | Outil::ZoneLibre => {
            if appui {
                // Double-clic (même point) : termine.
                if edition
                    .points
                    .last()
                    .is_some_and(|q| mcf_geo::distance(*q, p) < 0.75)
                {
                    terminer(&mut projet, &mut edition, alt);
                } else {
                    edition.points.push(p);
                }
            }
        }
        Outil::Pinceau(outil) => {
            if appui && ctrl {
                edition.cible = h;
                projet.info(format!("Altitude cible : {h:.1} m."));
                return;
            }
            if boutons.pressed(MouseButton::Left) && !ctrl {
                let dt = f64::from(temps.delta_secs()).min(0.05);
                let force = match outil {
                    Pinceau::Lever | Pinceau::Abaisser | Pinceau::Naturel => edition.force * dt,
                    Pinceau::Lisser | Pinceau::Aplanir => (edition.force * 0.25 * dt).clamp(0.0, 1.0),
                };
                let (rayon, cible) = (edition.rayon, edition.cible);
                projet.carte.pinceau(outil, p, rayon, force, cible);
            }
        }
        Outil::Affecter => {
            if appui {
                let (f, d) = (edition.famille, edition.densite);
                let r = projet.carte.affecter(p, f, d);
                projet.resultat(r, |_| {
                    format!(
                        "Îlot affecté : {} · {}. Outil « Lotir » pour faire pousser le bâti.",
                        f.nom(),
                        d.nom()
                    )
                });
            }
        }
        Outil::Lotir => {
            if appui {
                match quartier_en(&mut projet.carte, p) {
                    Some(id) => {
                        let r = projet.carte.lotir(id);
                        projet.resultat(r, |n| format!("{n} bâtiment(s) ont poussé."));
                    }
                    None => projet.echec("Aucun quartier ici : affectez d'abord l'îlot (étape 4)."),
                }
            }
        }
    }
}

/// Clic de sélection. `viseur` (caméra, transformée, curseur) permet d'attraper les poignées du tronçon
/// sélectionné ; `ctrl` : ajoute / retire (bâtiments), insère / supprime un point (tronçon sélectionné).
fn selectionner(
    projet: &mut Projet,
    edition: &mut Edition,
    p: P2,
    viseur: Option<(&Camera, &GlobalTransform, Vec2)>,
    ctrl: bool,
) {
    // 1. Poignées et tracé du tronçon sélectionné.
    if let Selection::Troncon(id) = edition.selection {
        let controle = projet
            .carte
            .reseau
            .troncons
            .get(&id)
            .and_then(|t| projet.carte.reseau.controle(t))
            .map(|(pts, _)| pts)
            .unwrap_or_default();
        let terrain = &projet.carte.derive().terrain;
        let proche = viseur.and_then(|(cam, gt, c)| {
            controle.iter().enumerate().find(|(_, q)| {
                let w = Vec3::new(q[0] as f32, terrain.hauteur(**q) as f32, q[1] as f32);
                cam.world_to_viewport(gt, w).is_ok_and(|s| s.distance(c) <= 10.0)
            })
        });
        match (proche, ctrl) {
            (Some((index, _)), true) => {
                supprimer_point(projet, id, index, controle.len());
                return;
            }
            (Some((index, q)), false) => {
                edition.glisse = Some(Glisse {
                    depart: *q,
                    actuel: *q,
                    cible: CibleGlisse::Poignee {
                        troncon: id,
                        index,
                        nombre: controle.len(),
                    },
                });
                return;
            }
            (None, true) => {
                let profils = projet.carte.manifeste.profils;
                if projet.carte.derive().geo.troncon_sous(p, &profils) == Some(id) {
                    inserer_point(projet, id, &controle, p);
                    return;
                }
            }
            (None, false) => {}
        }
    }
    // 2. Bâtiment (Ctrl : ajoute / retire de la sélection).
    if let Some(id) = batiment_en(&projet.carte, p) {
        let mut ids = match &edition.selection {
            Selection::Batiments(v) => v.clone(),
            _ => Vec::new(),
        };
        if ctrl {
            if let Some(i) = ids.iter().position(|x| *x == id) {
                ids.remove(i);
            } else {
                ids.push(id);
            }
            edition.selection = if ids.is_empty() {
                Selection::Rien
            } else {
                Selection::Batiments(ids)
            };
        } else {
            if !ids.contains(&id) {
                ids = vec![id];
            }
            edition.selection = Selection::Batiments(ids);
            edition.glisse = Some(Glisse {
                depart: p,
                actuel: p,
                cible: CibleGlisse::Batiments,
            });
        }
        suivre_selection(edition);
        return;
    }
    // 3. Tronçon, 4. quartier.
    let profils = projet.carte.manifeste.profils;
    let troncon = projet.carte.derive().geo.troncon_sous(p, &profils);
    edition.selection = if let Some(id) = troncon {
        Selection::Troncon(id)
    } else if let Some(id) = quartier_en(&mut projet.carte, p) {
        Selection::Quartier(id)
    } else {
        Selection::Rien
    };
    suivre_selection(edition);
}

/// L'éditeur suit la sélection : une route ouvre l'étape Réseau, un bâtiment l'étape Bâti, un quartier l'étape
/// Quartiers (sauf depuis Bâti, qui lotit les quartiers). L'outil devient « sélection », le geste en cours est gardé.
fn suivre_selection(edition: &mut Edition) {
    let cible = match (&edition.selection, edition.etape) {
        (Selection::Troncon(_), _) => Etape::Reseau,
        (Selection::Batiments(_), _) => Etape::Bati,
        (Selection::Quartier(_), Etape::Bati) => Etape::Bati,
        (Selection::Quartier(_), _) => Etape::Quartiers,
        (Selection::Rien, e) => e,
    };
    if cible != edition.etape || edition.outil != Outil::Selection {
        edition.etape = cible;
        edition.outil = Outil::Selection;
        edition.points.clear();
    }
}

/// Insère un point de forme sur le segment du tracé le plus proche de `p`.
fn inserer_point(projet: &mut Projet, id: Id, controle: &[P2], p: P2) {
    let Some(k) = (0..controle.len().saturating_sub(1)).min_by(|a, b| {
        let da = mcf_geo::distance_segment(p, controle[*a], controle[*a + 1]);
        let db = mcf_geo::distance_segment(p, controle[*b], controle[*b + 1]);
        da.total_cmp(&db)
    }) else {
        return;
    };
    let r = projet.carte.modifier("Ajouter un point", |tx| {
        let t = tx
            .reseau()
            .troncons
            .get_mut(&id)
            .ok_or_else(|| "tronçon introuvable".to_string())?;
        let rayon = mcf_carte::reseau::rayon_defaut(t.classe);
        t.points.insert(k, p);
        t.rayons.insert(k.min(t.rayons.len()), rayon);
        Ok(())
    });
    projet.resultat(r, |()| {
        "Point ajouté (glissez-le ; Ctrl+clic dessus pour le retirer).".into()
    });
}

/// Supprime un point de forme (les extrémités sont des carrefours : on ne les supprime pas ici).
fn supprimer_point(projet: &mut Projet, id: Id, index: usize, nombre: usize) {
    if index == 0 || index + 1 >= nombre {
        projet.echec("Une extrémité ne se supprime pas : supprimez le tronçon (Suppr) ou déplacez-la.");
        return;
    }
    let r = projet.carte.modifier("Retirer un point", |tx| {
        let t = tx
            .reseau()
            .troncons
            .get_mut(&id)
            .ok_or_else(|| "tronçon introuvable".to_string())?;
        if index - 1 < t.points.len() {
            t.points.remove(index - 1);
            t.rayons.remove(index - 1);
        }
        Ok(())
    });
    projet.resultat(r, |()| "Point retiré.".into());
}

fn appliquer_glisse(projet: &mut Projet, edition: &Edition, g: Glisse) {
    let delta = [g.actuel[0] - g.depart[0], g.actuel[1] - g.depart[1]];
    if delta[0].hypot(delta[1]) < 0.05 {
        return;
    }
    match g.cible {
        CibleGlisse::Batiments => {
            let Selection::Batiments(ids) = &edition.selection else {
                return;
            };
            let r = projet.carte.deplacer_batiments(ids, delta);
            projet.resultat(r, |n| {
                format!("{n} bâtiment(s) déplacé(s) (désormais « manuels »).")
            });
        }
        CibleGlisse::Poignee {
            troncon,
            index,
            nombre,
        } => {
            let p = g.actuel;
            let r = projet.carte.modifier("Éditer le tracé", |tx| {
                let (de, vers) = match tx.carte().reseau.troncons.get(&troncon) {
                    Some(t) => (t.de, t.vers),
                    None => return Err("tronçon introuvable".to_string()),
                };
                let (reseau, manifeste) = tx.reseau_et_manifeste();
                let ok = if index == 0 {
                    reseau.deplacer_noeud(de, p, manifeste)
                } else if index + 1 == nombre {
                    reseau.deplacer_noeud(vers, p, manifeste)
                } else {
                    reseau.deplacer_point(troncon, index - 1, p, manifeste)
                };
                if ok {
                    Ok(())
                } else {
                    Err("point refusé (hors de la carte ?)".into())
                }
            });
            projet.resultat(r, |()| {
                "Tracé modifié : rubans, carrefours et îlots recalculés.".into()
            });
        }
    }
}

fn clavier(
    touches: Res<ButtonInput<KeyCode>>,
    zone: Res<ZoneVues>,
    mut projet: ResMut<Projet>,
    mut edition: ResMut<Edition>,
) {
    if zone.clavier_ui {
        return;
    }
    let ctrl = touches.pressed(KeyCode::ControlLeft) || touches.pressed(KeyCode::ControlRight);
    let maj = touches.pressed(KeyCode::ShiftLeft) || touches.pressed(KeyCode::ShiftRight);
    let alt = touches.pressed(KeyCode::AltLeft) || touches.pressed(KeyCode::AltRight);
    if ctrl {
        if touches.just_pressed(KeyCode::KeyZ) && !maj || touches.just_pressed(KeyCode::KeyW) && !maj {
            // Ctrl+Z (AZERTY : la touche physique W porte le Z).
            annuler(&mut projet, &mut edition);
        } else if touches.just_pressed(KeyCode::KeyY) || (touches.just_pressed(KeyCode::KeyZ) && maj) {
            refaire(&mut projet, &mut edition);
        } else if touches.just_pressed(KeyCode::KeyS) {
            enregistrer(&mut projet, &edition);
        }
        return;
    }
    if touches.just_pressed(KeyCode::Escape) {
        if edition.points.is_empty() {
            edition.selection = Selection::Rien;
        }
        edition.points.clear();
        edition.glisse = None;
    }
    if touches.just_pressed(KeyCode::Enter) || touches.just_pressed(KeyCode::NumpadEnter) {
        terminer(&mut projet, &mut edition, alt);
    }
    if touches.just_pressed(KeyCode::Backspace) {
        edition.points.pop();
    }
    if touches.just_pressed(KeyCode::Delete) {
        supprimer(&mut projet, &mut edition);
    }
    for (k, e) in [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
    ]
    .into_iter()
    .zip(Etape::TOUTES)
    {
        if touches.just_pressed(k) {
            edition.aller_a(e);
        }
    }
}

/// Annule (et oublie une sélection qui ne correspondrait plus à rien).
pub fn annuler(projet: &mut Projet, edition: &mut Edition) {
    edition.points.clear();
    match projet.carte.annuler() {
        Some(l) => projet.info(format!("Annulé : {l}.")),
        None => projet.info("Rien à annuler."),
    }
    nettoyer_selection(projet, edition);
}

/// Refait.
pub fn refaire(projet: &mut Projet, edition: &mut Edition) {
    match projet.carte.refaire() {
        Some(l) => projet.info(format!("Refait : {l}.")),
        None => projet.info("Rien à refaire."),
    }
    nettoyer_selection(projet, edition);
}

fn nettoyer_selection(projet: &Projet, edition: &mut Edition) {
    let c = &projet.carte;
    edition.selection = match std::mem::take(&mut edition.selection) {
        Selection::Batiments(v) => {
            let v: Vec<Id> = v.into_iter().filter(|i| c.batiments.contains_key(i)).collect();
            if v.is_empty() {
                Selection::Rien
            } else {
                Selection::Batiments(v)
            }
        }
        Selection::Troncon(i) if c.reseau.troncons.contains_key(&i) => Selection::Troncon(i),
        Selection::Quartier(i)
            if c.quartiers.affectations.contains_key(&i) || c.quartiers.zones.contains_key(&i) =>
        {
            Selection::Quartier(i)
        }
        _ => Selection::Rien,
    };
}

/// Polyligne posée sur le terrain final (un point tous les 2 m au plus).
fn drape(terrain: &mcf_carte::relief::Relief, pts: &[P2], ferme: bool, dessus: f64) -> Vec<Vec3> {
    let mut v = Vec::new();
    let n = pts.len();
    if n < 2 {
        return v;
    }
    let fin = if ferme { n } else { n - 1 };
    for k in 0..fin {
        let (a, b) = (pts[k], pts[(k + 1) % n]);
        let pas = (mcf_geo::distance(a, b) / 2.0).ceil().clamp(1.0, 500.0) as usize;
        for i in 0..pas {
            let t = i as f64 / pas as f64;
            let q = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
            v.push(Vec3::new(
                q[0] as f32,
                (terrain.hauteur(q) + dessus) as f32,
                q[1] as f32,
            ));
        }
    }
    let q = if ferme { pts[0] } else { pts[n - 1] };
    v.push(Vec3::new(
        q[0] as f32,
        (terrain.hauteur(q) + dessus) as f32,
        q[1] as f32,
    ));
    v
}

fn horizontal(c: Vec3) -> Isometry3d {
    Isometry3d::new(c, Quat::from_rotation_arc(Vec3::Z, Vec3::Y))
}

#[allow(clippy::too_many_arguments)]
fn dessiner(
    mut plan: Gizmos<GizmosPlan>,
    mut trois_d: Gizmos<Gizmos3d>,
    mut dessus: Gizmos<GizmosDessus>,
    affiche: Res<Affiche>,
    edition: Res<Edition>,
    orbite: Res<super::camera::Orbite>,
    mut projet: ResMut<Projet>,
) {
    // Où regarde la caméra 3D (repère dans le Plan et la mini-carte).
    if edition.disposition != Disposition::Plan {
        let y = projet.carte.manifeste.altitude_max_m as f32 + 30.0;
        let c = orbite.cible.with_y(y);
        let (s, co) = bevy::math::ops::sin_cos(orbite.lacet);
        let avant = Vec3::new(-s, 0.0, -co);
        let droite = Vec3::new(co, 0.0, -s);
        let r = (orbite.distance * 0.25).max(4.0);
        let oeil = c - avant * orbite.distance * bevy::math::ops::cos(orbite.tangage);
        let rouge = Color::srgb(0.9, 0.2, 0.15);
        plan.circle(horizontal(c), r * 0.3, rouge);
        plan.line(oeil, c + avant * r + droite * r, rouge);
        plan.line(oeil, c + avant * r - droite * r, rouge);
    }
    let jaune = Color::srgb(1.0, 0.82, 0.1);
    let rouge = Color::srgb(0.95, 0.15, 0.1);
    let violet = Color::srgb(0.65, 0.3, 0.95);
    // Courbes de niveau (Plan).
    for (a, b, maitresse) in &affiche.courbes {
        let c = if *maitresse {
            Color::srgba(0.45, 0.32, 0.2, 0.6)
        } else {
            Color::srgba(0.45, 0.32, 0.2, 0.28)
        };
        plan.line(*a, *b, c);
    }
    let carte = &mut projet.carte;
    let (coin, fin) = carte.manifeste.bornes();
    let affectations = carte.quartiers.affectations.clone();
    let zones = carte.quartiers.zones.clone();
    let contours: Vec<_> = carte.contours.contours.values().cloned().collect();
    let batiments = carte.batiments.clone();
    let profils = carte.manifeste.profils;
    let controle_sel = match &edition.selection {
        Selection::Troncon(id) => carte
            .reseau
            .troncons
            .get(id)
            .and_then(|t| carte.reseau.controle(t)),
        _ => None,
    };
    let d = carte.derive();
    let terrain = &d.terrain;
    // Limites de la carte.
    let y = 0.0;
    plan.linestrip(
        [
            Vec3::new(coin[0] as f32, y, coin[1] as f32),
            Vec3::new(fin[0] as f32, y, coin[1] as f32),
            Vec3::new(fin[0] as f32, y, fin[1] as f32),
            Vec3::new(coin[0] as f32, y, fin[1] as f32),
            Vec3::new(coin[0] as f32, y, coin[1] as f32),
        ],
        Color::srgb(0.2, 0.2, 0.2),
    );
    // Quartiers en 3D : contours posés au sol.
    for a in affectations.values() {
        if let Some(i) = mcf_carte::ilots::ilot_en(&d.ilots, a.repere) {
            let poly = i.interieur.as_ref().unwrap_or(&i.contour);
            let c = a.famille.couleur_plan();
            trois_d.linestrip(drape(terrain, poly, true, 0.4), Color::srgb_u8(c[0], c[1], c[2]));
        }
    }
    for z in zones.values() {
        let c = z.famille.couleur_plan();
        trois_d.linestrip(
            drape(terrain, &z.polygone, true, 0.4),
            Color::srgb_u8(c[0], c[1], c[2]),
        );
    }
    // Contours nommés (entrées de souterrain).
    for c in &contours {
        dessus.linestrip(drape(terrain, &c.polygone, true, 0.5), violet);
    }
    // Conflits bâti / routes.
    for id in &edition.conflits {
        if let Some(b) = batiments.get(id) {
            dessus.linestrip(drape(terrain, &b.empreinte, true, 0.6), rouge);
        }
    }
    // Sélection.
    match &edition.selection {
        Selection::Batiments(ids) => {
            let decal = match edition.glisse {
                Some(Glisse {
                    cible: CibleGlisse::Batiments,
                    depart,
                    actuel,
                }) => [actuel[0] - depart[0], actuel[1] - depart[1]],
                _ => [0.0, 0.0],
            };
            for id in ids {
                if let Some(b) = batiments.get(id) {
                    let base = terrain.hauteur(b.centre());
                    let pts: Vec<Vec3> = b
                        .empreinte
                        .iter()
                        .chain(b.empreinte.first())
                        .map(|p| Vec3::new((p[0] + decal[0]) as f32, 0.0, (p[1] + decal[1]) as f32))
                        .collect();
                    dessus.linestrip(pts.iter().map(|p| p.with_y(base as f32 + 0.3)), jaune);
                    dessus.linestrip(
                        pts.iter().map(|p| p.with_y((base + b.hauteur()) as f32 + 0.3)),
                        jaune,
                    );
                }
            }
        }
        Selection::Troncon(id) => {
            if let Some(g) = d.geo.troncons.get(id) {
                dessus.linestrip(drape(terrain, &g.axe, false, 0.6), jaune);
            }
            if let Some((mut pts, _)) = controle_sel {
                if let Some(Glisse {
                    cible: CibleGlisse::Poignee { index, .. },
                    actuel,
                    ..
                }) = edition.glisse
                    && let Some(q) = pts.get_mut(index)
                {
                    *q = actuel;
                    dessus.linestrip(drape(terrain, &pts, false, 0.6), Color::WHITE);
                }
                for (i, q) in pts.iter().enumerate() {
                    let c = Vec3::new(q[0] as f32, (terrain.hauteur(*q) + 0.6) as f32, q[1] as f32);
                    let extremite = i == 0 || i + 1 == pts.len();
                    dessus.circle(horizontal(c), 2.0, if extremite { Color::WHITE } else { jaune });
                }
            }
        }
        Selection::Quartier(id) => {
            let poly = affectations
                .get(id)
                .and_then(|a| mcf_carte::ilots::ilot_en(&d.ilots, a.repere))
                .map(|i| i.contour.clone())
                .or_else(|| zones.get(id).map(|z| z.polygone.clone()));
            if let Some(poly) = poly {
                dessus.linestrip(drape(terrain, &poly, true, 0.6), jaune);
            }
        }
        Selection::Rien => {}
    }
    // Tracé ou zone en cours, avec la position du curseur.
    let mut pts = edition.points.clone();
    if let Some((p, _)) = edition.curseur
        && !pts.is_empty()
    {
        pts.push(p);
    }
    match edition.outil {
        Outil::Tracer if pts.len() >= 2 => {
            // Aperçu exact : mêmes fonctions que le noyau (arrondis par défaut de la classe, profil en travers).
            let rayons: Vec<f64> = (0..pts.len())
                .map(|i| {
                    if i == 0 || i + 1 == pts.len() {
                        0.0
                    } else {
                        mcf_carte::reseau::rayon_defaut(edition.classe)
                    }
                })
                .collect();
            let axe = mcf_geo::discretiser(&pts, &rayons, 0.25);
            for b in mcf_geo::bandes(&axe, profils.get(edition.classe)) {
                dessus.linestrip(drape(terrain, &b.gauche, false, 0.5), Color::WHITE);
                dessus.linestrip(drape(terrain, &b.droite, false, 0.5), Color::WHITE);
            }
            dessus.linestrip(drape(terrain, &axe, false, 0.5), jaune);
        }
        Outil::ZoneLibre if pts.len() >= 2 => {
            dessus.linestrip(drape(terrain, &pts, true, 0.5), jaune);
        }
        Outil::Pinceau(_) => {
            if let Some((p, h)) = edition.curseur {
                let c = Vec3::new(p[0] as f32, h as f32 + 0.5, p[1] as f32);
                dessus.circle(horizontal(c), edition.rayon as f32, jaune);
                dessus.circle(horizontal(c), (edition.rayon * 0.5) as f32, jaune.with_alpha(0.4));
            }
        }
        _ => {}
    }
    for q in &edition.points {
        let c = Vec3::new(q[0] as f32, (terrain.hauteur(*q) + 0.5) as f32, q[1] as f32);
        dessus.circle(horizontal(c), 1.2, jaune);
    }
}
