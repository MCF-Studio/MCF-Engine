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

/// Repères de la 3D toujours au premier plan (sélection, tracé en cours), posés sur le terrain.
#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct GizmosDessus;

/// Les mêmes repères dans le Plan, à plat au-dessus de tout (voir [`Reperes`]).
#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct GizmosPlanDessus;

/// Altitude des repères dans le Plan, au-dessus de l'altitude maximale : au-dessus des calques (+10 à +18 m), du
/// voile des quartiers (+20 m), des routes (+30 m) et du bâti (+34 m) du Plan (`scene`), qui les cachaient.
pub const REPERES_PLAN_M: f64 = 45.0;

/// Repères de sélection et de tracé, dessinés deux fois : dans la 3D aux points donnés (sur le terrain), dans le
/// Plan aux mêmes `(x, z)` mais à l'altitude `y_plan`. En vue verticale, l'altitude ne change pas la place à
/// l'écran, seulement ce qui passe devant.
pub struct Reperes<'a, 'w, 's, 'w2, 's2> {
    /// Groupe de la 3D.
    pub d3: &'a mut Gizmos<'w, 's, GizmosDessus>,
    /// Groupe du Plan.
    pub plan: &'a mut Gizmos<'w2, 's2, GizmosPlanDessus>,
    /// Altitude des repères dans le Plan.
    pub y_plan: f32,
}

impl Reperes<'_, '_, '_, '_, '_> {
    /// Polyligne.
    pub fn linestrip(&mut self, pts: impl IntoIterator<Item = Vec3>, c: Color) {
        let pts: Vec<Vec3> = pts.into_iter().collect();
        let y = self.y_plan;
        self.plan.linestrip(pts.iter().map(|p| p.with_y(y)), c);
        self.d3.linestrip(pts, c);
    }

    /// Cercle (l'isométrie donne son centre et son plan).
    pub fn circle(&mut self, iso: Isometry3d, rayon: f32, c: Color) {
        let mut a_plat = iso;
        a_plat.translation.y = self.y_plan;
        self.plan.circle(a_plat, rayon, c);
        self.d3.circle(iso, rayon, c);
    }

    /// Flèche.
    pub fn arrow(&mut self, de: Vec3, vers: Vec3, c: Color) {
        let y = self.y_plan;
        self.plan.arrow(de.with_y(y), vers.with_y(y), c);
        self.d3.arrow(de, vers, c);
    }
}

pub struct OutilsPlugin;

/// Systèmes d'interaction (souris, clavier, repères) : les autres modules s'exécutent après.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct SystemesOutils;

impl Plugin for OutilsPlugin {
    fn build(&self, app: &mut App) {
        app.init_gizmo_group::<GizmosPlan>()
            .init_gizmo_group::<Gizmos3d>()
            .init_gizmo_group::<GizmosDessus>()
            .init_gizmo_group::<GizmosPlanDessus>()
            .add_systems(Startup, regler_gizmos)
            .add_systems(Update, (souris, clavier, dessiner).chain().in_set(SystemesOutils));
    }
}

fn regler_gizmos(mut store: ResMut<GizmoConfigStore>) {
    let (c, _) = store.config_mut::<GizmosPlan>();
    c.render_layers = RenderLayers::layer(COUCHE_PLAN);
    c.line.width = 1.0;
    let (c, _) = store.config_mut::<Gizmos3d>();
    c.render_layers = RenderLayers::layer(COUCHE_3D);
    c.line.width = 2.0;
    let (c, _) = store.config_mut::<GizmosPlanDessus>();
    c.render_layers = RenderLayers::layer(COUCHE_PLAN);
    c.depth_bias = -1.0;
    c.line.width = 2.5;
    let (c, _) = store.config_mut::<GizmosDessus>();
    c.render_layers = RenderLayers::layer(COUCHE_3D);
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

/// Bâtiment sous le curseur : dans le Plan, celui dont l'empreinte contient `p` ; en 3D, le premier mur ou toit
/// touché par le rayon de la caméra avant le sol (`p`, `h` : point du sol visé).
fn batiment_sous_curseur(
    carte: &mut Carte,
    p: P2,
    h: f64,
    vue: Option<Vue>,
    camera: Option<(&Vue, &Camera, &GlobalTransform)>,
    curseur: Option<Vec2>,
) -> Option<Id> {
    let rayon = match (vue, camera, curseur) {
        (Some(Vue::Perspective), Some((_, cam, gt)), Some(c)) => cam.viewport_to_world(gt, c).ok(),
        _ => None,
    };
    let Some(rayon) = rayon else {
        return batiment_en(carte, p);
    };
    let o = rayon.origin.as_dvec3();
    let d = rayon.direction.as_dvec3();
    let t_sol = (bevy::math::DVec3::new(p[0], h, p[1]) - o).length();
    batiment_vise(carte, o, d, t_sol + 0.5).or_else(|| batiment_en(carte, p))
}

/// Premier bâtiment (prisme de son empreinte, du point le plus bas du sol sous lui jusqu'au toit, comme le maillage
/// de la scène) touché par le rayon `o + t·d` avec `t ≤ t_max`.
fn batiment_vise(carte: &mut Carte, o: bevy::math::DVec3, d: bevy::math::DVec3, t_max: f64) -> Option<Id> {
    let (o2, d2) = ([o.x, o.z], [d.x, d.z]);
    let croix = |a: P2, b: P2| a[0] * b[1] - a[1] * b[0];
    // 1. Candidats en 2D : l'empreinte croise la trace horizontale du rayon (sans le relief).
    let fin = [o2[0] + d2[0] * t_max, o2[1] + d2[1] * t_max];
    let (bmin, bmax) = (
        [o2[0].min(fin[0]), o2[1].min(fin[1])],
        [o2[0].max(fin[0]), o2[1].max(fin[1])],
    );
    let candidats: Vec<(Id, Vec<P2>, f64, f64)> = carte
        .batiments
        .iter()
        .filter(|(_, b)| {
            let (a, c) = polygone::englobant(&b.empreinte);
            !(c[0] < bmin[0] || bmax[0] < a[0] || c[1] < bmin[1] || bmax[1] < a[1])
        })
        .map(|(id, b)| (*id, b.empreinte.clone(), b.hauteur(), b.decalage_y_m))
        .collect();
    let terrain = &carte.derive().terrain;
    let mut meilleur: Option<(f64, Id)> = None;
    for (id, e, hauteur, decalage) in candidats {
        if e.len() < 3 {
            continue;
        }
        let bas = e
            .iter()
            .map(|q| terrain.hauteur(*q))
            .fold(f64::INFINITY, f64::min)
            - 0.3
            + decalage.min(0.0);
        let centre = polygone::centroide(&e);
        let haut = terrain.hauteur(centre).max(bas + 0.3) + decalage + hauteur;
        // Tranche verticale [bas, haut].
        let (mut ta, mut tb) = if d.y.abs() < 1e-9 {
            if (bas..=haut).contains(&o.y) {
                (0.0, t_max)
            } else {
                continue;
            }
        } else {
            let (t1, t2) = ((bas - o.y) / d.y, (haut - o.y) / d.y);
            (t1.min(t2), t1.max(t2))
        };
        ta = ta.max(0.0);
        tb = tb.min(t_max);
        if ta > tb {
            continue;
        }
        let point = |t: f64| [o2[0] + d2[0] * t, o2[1] + d2[1] * t];
        let mut t_entree = polygone::contient(&e, point(ta)).then_some(ta);
        for k in 0..e.len() {
            let (a, b) = (e[k], e[(k + 1) % e.len()]);
            let ab = [b[0] - a[0], b[1] - a[1]];
            let den = croix(d2, ab);
            if den.abs() < 1e-12 {
                continue;
            }
            let ao = [a[0] - o2[0], a[1] - o2[1]];
            let (t, s) = (croix(ao, ab) / den, croix(ao, d2) / den);
            if (0.0..=1.0).contains(&s) && (ta..=tb).contains(&t) && t_entree.is_none_or(|x| t < x) {
                t_entree = Some(t);
            }
        }
        if let Some(t) = t_entree
            && meilleur.is_none_or(|(m, _)| t < m)
        {
            meilleur = Some((t, id));
        }
    }
    meilleur.map(|(_, id)| id)
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
        Selection::LigneElectrique(id) => projet.carte.modifier("Supprimer une ligne électrique", |tx| {
            if tx.reseau().lignes_electriques.remove(id).is_some() {
                Ok("Ligne électrique supprimée.".to_string())
            } else {
                Err("ligne électrique introuvable".into())
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
        Outil::LigneElectrique if points.len() >= 2 => {
            let r = projet.carte.modifier("Ligne électrique", |tx| {
                let (reseau, manifeste) = tx.reseau_et_manifeste();
                reseau.tracer_ligne_electrique(&points, manifeste)
            });
            projet.resultat(r, |_| "Ligne électrique tracée.".into());
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
        Outil::Tracer | Outil::ZoneLibre | Outil::LigneElectrique if !points.is_empty() => {
            edition.points = points;
            projet.echec("Il manque des points (route ou ligne : 2, zone : 3).");
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
    mut orbite: ResMut<super::camera::Orbite>,
    catalogue: Res<super::modeles::Catalogue>,
    // Vue où le glisser en cours a commencé (`Some(None)` : hors des vues).
    mut vue_glisse: Local<Option<Option<Vue>>>,
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
        // Fin dès que le bouton n'est plus enfoncé : un relâchement au-dessus d'un panneau (capté par egui) ou hors
        // de la fenêtre ne doit pas laisser le glisser actif.
        if !boutons.pressed(MouseButton::Left) {
            edition.glisse = None;
            *vue_glisse = None;
            appliquer_glisse(&mut projet, &edition, g, alt);
            // Un tronçon retracé (croisements) change d'identifiant : on ne garde pas une sélection vers le vide.
            nettoyer_selection(&projet, &mut edition);
            return;
        }
        // Le point ne suit le curseur que dans la vue où le glisser a commencé : passé dans l'autre vue, la même
        // position d'écran vise un autre point (et la caméra 3D qui bouge le déplacerait tout seul).
        let origine = *vue_glisse.get_or_insert(vue);
        if vue.is_some()
            && vue == origine
            && let Some(c) = curseur
        {
            g.ecran[1] = c;
            edition.glisse = Some(g);
        }
        if vue.is_some()
            && vue == origine
            && let Some((p, _)) = edition.curseur
        {
            g.actuel = p;
            edition.glisse = Some(g);
        }
        return;
    }
    *vue_glisse = None;

    let Some((p, h)) = edition.curseur else {
        return;
    };
    // F : la caméra 3D va regarder le point sous le curseur (depuis le Plan ou la 3D) ; le Plan seul s'ouvre en
    // Plan + 3D pour qu'on y soit.
    if touches.just_pressed(KeyCode::KeyF) && !zone.clavier_ui {
        orbite.cible = Vec3::new(p[0] as f32, h as f32, p[1] as f32);
        orbite.elevation = 0.0;
        orbite.distance = orbite.distance.min(250.0);
        if edition.disposition == Disposition::Plan {
            edition.disposition = Disposition::Partagee;
        }
        projet.info(format!("Caméra 3D sur ({:.0} ; {:.0}).", p[0], p[1]));
    }
    let appui = boutons.just_pressed(MouseButton::Left);
    let maj = touches.pressed(KeyCode::ShiftLeft) || touches.pressed(KeyCode::ShiftRight);
    // Bâtiment visé au clic : en 3D, le rayon touche d'abord les murs et les toits, pas le sol derrière.
    let bat = if appui {
        batiment_sous_curseur(&mut projet.carte, p, h, vue, camera, curseur)
    } else {
        None
    };
    // Clic sur la mini-carte : elle s'agrandit (vue partagée).
    if appui && vue == Some(Vue::Plan) && edition.disposition == Disposition::Perspective {
        edition.disposition = Disposition::Partagee;
        return;
    }
    // Gizmo des bâtiments sélectionnés : il passe avant tout autre clic.
    if appui
        && matches!(edition.outil, Outil::Selection | Outil::Lotir)
        && let Selection::Batiments(ids) = &edition.selection
        && let (Some((v, cam, gt)), Some(c)) = (camera, curseur)
        && let Some(saisie) = super::gizmo::poignee_sous(*v, cam, gt, c, &mut projet, &ids.clone())
    {
        edition.glisse = Some(Glisse {
            depart: p,
            actuel: p,
            ecran: [c, c],
            cible: CibleGlisse::Gizmo(saisie),
        });
        return;
    }
    // Maj+clic : sélectionner depuis n'importe quel outil.
    let viseur = camera.zip(curseur).map(|((_, cam, gt), c)| (cam, gt, c));
    if appui && maj && edition.outil != Outil::Selection {
        selectionner(&mut projet, &mut edition, p, bat, viseur, false);
        return;
    }
    match edition.outil {
        Outil::Selection => {
            if appui {
                selectionner(&mut projet, &mut edition, p, bat, viseur, ctrl);
            }
        }
        Outil::Tracer | Outil::ZoneLibre | Outil::LigneElectrique => {
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
        Outil::DeplacerReference => {
            if appui {
                match edition.reference_active {
                    Some(id) => {
                        edition.glisse = Some(Glisse {
                            depart: p,
                            actuel: p,
                            ecran: [Vec2::ZERO; 2],
                            cible: CibleGlisse::Reference(id),
                        });
                    }
                    None => projet.echec("Choisissez d'abord un calque de référence (étape 1)."),
                }
            }
        }
        Outil::CalerReference => {
            if appui {
                if edition.points.len() >= 2 {
                    edition.points.clear();
                }
                edition.points.push(p);
                if edition.points.len() == 2 {
                    projet.info("Deux repères posés : saisissez leur distance réelle, puis « Caler ».");
                }
            }
        }
        Outil::Releve => {
            if appui {
                super::releve::analyser(&mut projet, &mut edition, p);
            }
        }
        Outil::PeindreCalque(mode) => {
            let Some(id) = edition.reference_active else {
                if appui {
                    projet.echec("Choisissez d'abord le calque de relief (étape 1).");
                }
                return;
            };
            // Ctrl+clic : prendre le gris sous le curseur comme gris visé par « Niveau ».
            if appui && ctrl {
                let carte = &projet.carte;
                if let (Some(r), Some(image)) = (
                    carte.references.references.get(&id),
                    carte.images_references.get(&id),
                ) {
                    let (px, py) = r.vers_image(p);
                    if let Some(g) = mcf_carte::relief_image::gris(image, px, py) {
                        edition.calque_gris = g;
                        projet.info(format!("Gris visé : {g:.2}."));
                    }
                }
                return;
            }
            if boutons.pressed(MouseButton::Left) && !ctrl {
                // Force par seconde, ramenée à l'image (bornée si l'image ralentit).
                let dt = f64::from(temps.delta_secs()).min(0.05);
                let force = (edition.calque_force * dt).clamp(0.0, 1.0);
                let (rayon, gris) = (edition.calque_rayon_m, edition.calque_gris);
                if let Err(e) = projet.carte.peindre_reference(id, mode, p, rayon, force, gris)
                    && appui
                {
                    // Une seule fois par geste (pas un message par image).
                    projet.echec(e);
                }
            }
        }
        Outil::Poser(i) => {
            if appui {
                match catalogue.fiches.get(i) {
                    Some(f) => {
                        let r = projet.carte.poser_modele(f, p, edition.cap_pose);
                        let nom = f.nom.clone();
                        projet.resultat(r, |_| {
                            format!(
                                "« {nom} » posé. Cliquez pour en poser un autre, R : tourner, Échap : fini."
                            )
                        });
                    }
                    None => edition.outil = Outil::Selection,
                }
            }
        }
        Outil::Lotir => {
            if appui {
                // Un clic sur un bâtiment le sélectionne (pour le déplacer) au lieu de tout relotir.
                if bat.is_some() {
                    selectionner(&mut projet, &mut edition, p, bat, viseur, ctrl);
                    return;
                }
                match quartier_en(&mut projet.carte, p) {
                    Some(id) => {
                        let r = projet.carte.lotir(id);
                        projet.resultat(r, |n| {
                            format!(
                                "{n} bâtiment(s) ont poussé. Cliquez un bâtiment pour le sélectionner et le glisser."
                            )
                        });
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
    bat: Option<Id>,
    viseur: Option<(&Camera, &GlobalTransform, Vec2)>,
    ctrl: bool,
) {
    // 0. Sommets et bords de la zone libre sélectionnée.
    if let Selection::Quartier(id) = edition.selection
        && let Some(z) = projet.carte.quartiers.zones.get(&id)
        && let Some((cam, gt, c)) = viseur
    {
        let poly = z.polygone.clone();
        let terrain = &projet.carte.derive().terrain;
        let ecran = |q: P2| {
            let w = Vec3::new(q[0] as f32, terrain.hauteur(q) as f32, q[1] as f32);
            cam.world_to_viewport(gt, w).ok()
        };
        let sommet = poly
            .iter()
            .position(|q| ecran(*q).is_some_and(|s| s.distance(c) <= 10.0));
        let n = poly.len();
        match (sommet, ctrl) {
            (Some(index), false) => {
                edition.glisse = Some(Glisse {
                    depart: poly[index],
                    actuel: poly[index],
                    ecran: [Vec2::ZERO; 2],
                    cible: CibleGlisse::SommetZone { zone: id, index },
                });
                return;
            }
            (Some(index), true) => {
                let mut nouveau = poly;
                nouveau.remove(index);
                let r = projet.carte.remodeler_zone(id, nouveau);
                projet.resultat(r, |()| {
                    "Sommet retiré (relotir la zone pour refaire le bâti).".into()
                });
                return;
            }
            (None, true) => {
                // Ctrl+clic près d'un bord : ajoute un sommet.
                let bord = (0..n).find(|&k| {
                    let (a, b) = (ecran(poly[k]), ecran(poly[(k + 1) % n]));
                    a.zip(b).is_some_and(|(a, b)| {
                        let ab = b - a;
                        let t = ((c - a).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
                        (a + ab * t).distance(c) <= 8.0
                    })
                });
                if let Some(k) = bord {
                    let mut nouveau = poly;
                    nouveau.insert(k + 1, p);
                    let r = projet.carte.remodeler_zone(id, nouveau);
                    projet.resultat(r, |()| {
                        "Sommet ajouté (glissez-le ; Ctrl+clic dessus pour le retirer).".into()
                    });
                    return;
                }
            }
            (None, false) => {}
        }
    }
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
                    ecran: [Vec2::ZERO; 2],
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
                let marge = marge_ecran_m(projet, p, viseur);
                if projet.carte.derive().geo.troncon_proche(p, &profils, marge) == Some(id) {
                    inserer_point(projet, id, &controle, p);
                    return;
                }
            }
            (None, false) => {}
        }
    }
    // 2. Bâtiment (Ctrl : ajoute / retire de la sélection).
    if let Some(id) = bat.or_else(|| batiment_en(&projet.carte, p)) {
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
                ecran: [Vec2::ZERO; 2],
                cible: CibleGlisse::Batiments,
            });
        }
        suivre_selection(edition);
        return;
    }
    // 3. Ligne électrique (un trait fin, cliqué à quelques pixels près : avant la route qu'elle longe peut-être),
    // 4. tronçon, 5. quartier.
    let profils = projet.carte.manifeste.profils;
    let marge = marge_ecran_m(projet, p, viseur);
    let ligne = projet.carte.reseau.ligne_electrique_proche(p, marge);
    let troncon = projet.carte.derive().geo.troncon_proche(p, &profils, marge);
    edition.selection = if let Some(id) = ligne {
        Selection::LigneElectrique(id)
    } else if let Some(id) = troncon {
        Selection::Troncon(id)
    } else if let Some(id) = quartier_en(&mut projet.carte, p) {
        Selection::Quartier(id)
    } else {
        Selection::Rien
    };
    suivre_selection(edition);
}

/// Distance (m) qui fait `TOLERANCE_PX` pixels à l'écran autour de `p` : une route se clique à quelques pixels
/// près, quel que soit le zoom du Plan ou la distance de la caméra 3D.
fn marge_ecran_m(projet: &mut Projet, p: P2, viseur: Option<(&Camera, &GlobalTransform, Vec2)>) -> f64 {
    const TOLERANCE_PX: f32 = 8.0;
    let Some((cam, gt, _)) = viseur else {
        return 0.0;
    };
    let h = projet.carte.derive().terrain.hauteur(p) as f32;
    let a = Vec3::new(p[0] as f32, h, p[1] as f32);
    match (
        cam.world_to_viewport(gt, a),
        cam.world_to_viewport(gt, a + Vec3::X),
    ) {
        (Ok(sa), Ok(sb)) if sa.distance(sb) > 1e-4 => f64::from(TOLERANCE_PX / sa.distance(sb)).min(500.0),
        _ => 0.0,
    }
}

/// L'éditeur suit la sélection : une route ouvre l'étape Réseau, un bâtiment l'étape Bâti, un quartier l'étape
/// Quartiers (sauf depuis Bâti, qui lotit les quartiers). L'outil devient « sélection », le geste en cours est gardé.
fn suivre_selection(edition: &mut Edition) {
    let cible = match (&edition.selection, edition.etape) {
        (Selection::Troncon(_) | Selection::LigneElectrique(_), _) => Etape::Reseau,
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

fn appliquer_glisse(projet: &mut Projet, edition: &Edition, g: Glisse, alt: bool) {
    let delta = [g.actuel[0] - g.depart[0], g.actuel[1] - g.depart[1]];
    // Les gestes du gizmo qui ne suivent pas le sol (hauteur, inclinaison) ont leur propre seuil.
    if delta[0].hypot(delta[1]) < 0.05 && !matches!(g.cible, CibleGlisse::Gizmo(_)) {
        return;
    }
    match g.cible {
        CibleGlisse::Batiments | CibleGlisse::Gizmo(_) => {
            let (Selection::Batiments(ids), Some(t)) =
                (&edition.selection, super::gizmo::transformation(&g, alt))
            else {
                return;
            };
            if t.est_nulle() {
                return;
            }
            let r = projet.carte.transformer_batiments(ids, &t);
            projet.resultat(r, |n| {
                let quoi = if t.cap_deg != 0.0 {
                    format!("tourné(s) de {:.1}°", t.cap_deg)
                } else if t.tangage_deg != 0.0 || t.roulis_deg != 0.0 {
                    format!("incliné(s) de {:.1}°", t.tangage_deg + t.roulis_deg)
                } else if t.dy_m != 0.0 {
                    format!("monté(s) de {:+.2} m", t.dy_m)
                } else {
                    "déplacé(s)".to_string()
                };
                format!("{n} bâtiment(s) {quoi} (désormais « manuels »).")
            });
        }
        CibleGlisse::Reference(id) => {
            let r = projet
                .carte
                .modifier_reference(id, "Déplacer un calque de référence", |r| {
                    r.coin = [r.coin[0] + delta[0], r.coin[1] + delta[1]];
                    Ok(())
                });
            projet.resultat(r, |()| "Calque déplacé.".into());
        }
        CibleGlisse::SommetZone { zone, index } => {
            let Some(mut poly) = projet
                .carte
                .quartiers
                .zones
                .get(&zone)
                .map(|z| z.polygone.clone())
            else {
                return;
            };
            if let Some(q) = poly.get_mut(index) {
                *q = g.actuel;
            }
            let r = projet.carte.remodeler_zone(zone, poly);
            projet.resultat(r, |()| {
                "Zone modifiée (relotir la zone pour refaire le bâti).".into()
            });
        }
        CibleGlisse::Poignee {
            troncon,
            index,
            nombre,
        } => {
            let p = g.actuel;
            // Même règle que le tracé : une extrémité lâchée près d'un nœud ou d'une route s'y raccorde (sauf Alt ou
            // accroche coupée), et chaque croisement devient un carrefour.
            let accroche = if edition.accroche && !alt {
                mcf_carte::carte_de_test::ACCROCHE_M
            } else {
                0.0
            };
            let r = projet.carte.modifier("Éditer le tracé", |tx| {
                let (de, vers) = match tx.carte().reseau.troncons.get(&troncon) {
                    Some(t) => (t.de, t.vers),
                    None => return Err("tronçon introuvable".to_string()),
                };
                let (reseau, manifeste) = tx.reseau_et_manifeste();
                let noeud = if index == 0 {
                    Some(de)
                } else if index + 1 == nombre {
                    Some(vers)
                } else {
                    None
                };
                let ok = match noeud {
                    Some(n) => reseau.deplacer_noeud(n, p, manifeste),
                    None => reseau.deplacer_point(troncon, index - 1, p, manifeste),
                };
                if !ok {
                    return Err("point refusé (hors de la carte ?)".into());
                }
                let (a_verifier, fusion) = match noeud {
                    Some(n) => {
                        let m = reseau.raccorder_noeud(n, accroche);
                        (reseau.incidents(m), m != n)
                    }
                    None => (vec![troncon], false),
                };
                let mut carrefours = 0;
                for t in a_verifier {
                    if reseau.troncons.contains_key(&t) {
                        carrefours += reseau.couper_croisements(t, manifeste)?.len().saturating_sub(1);
                    }
                }
                Ok((fusion, carrefours))
            });
            projet.resultat(r, |(fusion, carrefours)| {
                let mut m = String::from("Tracé modifié");
                if fusion {
                    m.push_str(", raccordé au réseau");
                }
                if carrefours > 0 {
                    m.push_str(&format!(", {carrefours} carrefour(s) créé(s)"));
                }
                m + "."
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
    if touches.just_pressed(KeyCode::KeyR) {
        let sens = if maj { -90.0 } else { 90.0 };
        tourner_90(&mut projet, &mut edition, sens);
    }
    if touches.just_pressed(KeyCode::Escape) && matches!(edition.outil, Outil::Poser(_)) {
        edition.outil = Outil::Selection;
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
    // ² (AZERTY ; ` en QWERTY), la touche au-dessus de Tab : outil de sélection de l'étape.
    if touches.just_pressed(KeyCode::Backquote) {
        if edition.outil_selection() {
            projet.info("Outil de sélection.");
        } else {
            projet.info("Pas d'outil de sélection dans cette étape.");
        }
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

/// R : tourne de `deg` le bâtiment en cours de pose, sinon les bâtiments sélectionnés (autour de leur centre).
pub fn tourner_90(projet: &mut Projet, edition: &mut Edition, deg: f64) {
    if matches!(edition.outil, Outil::Poser(_)) {
        edition.cap_pose = mcf_carte::batiments::normaliser_cap(edition.cap_pose + deg);
        projet.info(format!("Cap de pose : {:.0}°.", edition.cap_pose));
        return;
    }
    let Selection::Batiments(ids) = &edition.selection else {
        return;
    };
    let Some(pivot) = super::gizmo::pivot(projet, ids) else {
        return;
    };
    let r = projet
        .carte
        .transformer_batiments(ids, &mcf_carte::Transformation::rotation(deg, pivot));
    projet.resultat(r, |n| format!("{n} bâtiment(s) tourné(s) de {deg:.0}°."));
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
        Selection::LigneElectrique(i) if c.reseau.lignes_electriques.contains_key(&i) => {
            Selection::LigneElectrique(i)
        }
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

/// Contour d'un bâtiment (au sol et au toit) ; pour un bâtiment modélisé, flèche de sa façade avant (−Z du modèle).
fn contour_batiment(
    g: &mut Reperes,
    terrain: &mcf_carte::relief::Relief,
    b: &mcf_carte::Batiment,
    couleur: Color,
) {
    let base = terrain.hauteur(b.centre()) + b.decalage_y_m;
    let pts: Vec<Vec3> = b
        .empreinte
        .iter()
        .chain(b.empreinte.first())
        .map(|p| Vec3::new(p[0] as f32, 0.0, p[1] as f32))
        .collect();
    g.linestrip(pts.iter().map(|p| p.with_y(base as f32 + 0.3)), couleur);
    g.linestrip(
        pts.iter().map(|p| p.with_y((base + b.hauteur()) as f32 + 0.3)),
        couleur,
    );
    if let Some(m) = &b.modele {
        let c = b.centre();
        // Demi-profondeur : distance du centre au milieu du côté avant.
        let (a, bb) = (b.empreinte[0], b.empreinte[1]);
        let avant = mcf_carte::batiments::tourner([0.0, -1.0], [0.0, 0.0], m.cap_deg);
        let demi = mcf_geo::distance(c, [(a[0] + bb[0]) * 0.5, (a[1] + bb[1]) * 0.5]).max(1.0);
        let y = base as f32 + 0.4;
        let de = Vec3::new(c[0] as f32, y, c[1] as f32);
        let vers = de + Vec3::new(avant[0] as f32, 0.0, avant[1] as f32) * (demi as f32 + 3.0);
        g.arrow(de, vers, couleur);
    }
}

fn horizontal(c: Vec3) -> Isometry3d {
    Isometry3d::new(c, Quat::from_rotation_arc(Vec3::Z, Vec3::Y))
}

#[allow(clippy::too_many_arguments)]
fn dessiner(
    mut plan: Gizmos<GizmosPlan>,
    mut trois_d: Gizmos<Gizmos3d>,
    mut dessus_3d: Gizmos<GizmosDessus>,
    mut dessus_plan: Gizmos<GizmosPlanDessus>,
    affiche: Res<Affiche>,
    edition: Res<Edition>,
    orbite: Res<super::camera::Orbite>,
    touches: Res<ButtonInput<KeyCode>>,
    catalogue: Res<super::modeles::Catalogue>,
    mut projet: ResMut<Projet>,
) {
    let alt = touches.pressed(KeyCode::AltLeft) || touches.pressed(KeyCode::AltRight);
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
    let lignes: Vec<(Id, Vec<P2>)> = carte
        .reseau
        .lignes_electriques
        .iter()
        .map(|(id, l)| (*id, l.points.clone()))
        .collect();
    let controle_sel = match &edition.selection {
        Selection::Troncon(id) => carte
            .reseau
            .troncons
            .get(id)
            .and_then(|t| carte.reseau.controle(t)),
        _ => None,
    };
    // Calque de référence actif : son cadre, au-dessus des images (Plan).
    let alt_dessus = carte.manifeste.altitude_max_m as f32 + 25.0;
    let mut dessus = Reperes {
        d3: &mut dessus_3d,
        plan: &mut dessus_plan,
        y_plan: (carte.manifeste.altitude_max_m + REPERES_PLAN_M) as f32,
    };
    if let Some(r) = edition.reference_active.and_then(|id| {
        super::references::references_affichees(carte, &edition)
            .into_iter()
            .find(|(i, _)| *i == id)
    }) {
        let c =
            r.1.coins()
                .map(|q| Vec3::new(q[0] as f32, alt_dessus, q[1] as f32));
        plan.linestrip([c[0], c[1], c[2], c[3], c[0]], Color::srgb(0.95, 0.45, 0.1));
    }
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
    // Réseau électrique : dans le Plan au-dessus des routes, en 3D posé sur le sol (un trait, rien de généré).
    let [r, v, b] = mcf_carte::reseau::COULEUR_ELECTRIQUE;
    let orange = Color::srgb_u8(r, v, b);
    for (_, pts) in &lignes {
        let sol = drape(terrain, pts, false, 1.0);
        let y = dessus.y_plan - 2.0;
        dessus.plan.linestrip(sol.iter().map(|p| p.with_y(y)), orange);
        trois_d.linestrip(sol, orange);
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
            let apercu = super::gizmo::apercu(&edition, alt);
            for id in ids {
                if let Some(b) = batiments.get(id) {
                    let mut b = b.clone();
                    if let Some(t) = &apercu
                        && !b.verrou
                    {
                        b.transformer(t);
                    }
                    contour_batiment(&mut dessus, terrain, &b, jaune);
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
        Selection::LigneElectrique(id) => {
            if let Some((_, pts)) = lignes.iter().find(|(i, _)| i == id) {
                dessus.linestrip(drape(terrain, pts, false, 1.2), jaune);
                for q in pts {
                    let c = Vec3::new(q[0] as f32, (terrain.hauteur(*q) + 1.2) as f32, q[1] as f32);
                    dessus.circle(horizontal(c), 1.5, jaune);
                }
            }
        }
        Selection::Quartier(id) => {
            let poly = affectations
                .get(id)
                .and_then(|a| mcf_carte::ilots::ilot_en(&d.ilots, a.repere))
                .map(|i| i.contour.clone())
                .or_else(|| zones.get(id).map(|z| z.polygone.clone()));
            if let Some(mut poly) = poly {
                // Zone libre : sommets en poignées, aperçu pendant le glisser.
                if zones.contains_key(id) {
                    if let Some(Glisse {
                        cible: CibleGlisse::SommetZone { zone, index },
                        actuel,
                        ..
                    }) = edition.glisse
                        && zone == *id
                        && let Some(q) = poly.get_mut(index)
                    {
                        *q = actuel;
                    }
                    for q in &poly {
                        let c = Vec3::new(q[0] as f32, (terrain.hauteur(*q) + 0.6) as f32, q[1] as f32);
                        dessus.circle(horizontal(c), 2.0, Color::WHITE);
                    }
                }
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
        Outil::LigneElectrique if pts.len() >= 2 => {
            dessus.linestrip(drape(terrain, &pts, false, 1.0), orange);
        }
        Outil::CalerReference if pts.len() >= 2 => {
            // Segment entre les deux repères (le second suit le curseur tant qu'il n'est pas posé).
            let (a, b) = (pts[0], pts[1]);
            let y = alt_dessus;
            plan.line(
                Vec3::new(a[0] as f32, y, a[1] as f32),
                Vec3::new(b[0] as f32, y, b[1] as f32),
                rouge,
            );
        }
        Outil::Poser(i) => {
            if let (Some((p, _)), Some(f)) = (edition.curseur, catalogue.fiches.get(i)) {
                contour_batiment(
                    &mut dessus,
                    terrain,
                    &f.batiment(p, edition.cap_pose),
                    Color::WHITE,
                );
            }
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
    super::releve::dessiner(&mut plan, dessus.d3, &edition, terrain, alt_dessus);
    for q in &edition.points {
        let c = Vec3::new(q[0] as f32, (terrain.hauteur(*q) + 0.5) as f32, q[1] as f32);
        dessus.circle(horizontal(c), 1.2, jaune);
    }
}
