//! Construction de la scène à partir du document. Chaque élément visible existe en deux exemplaires aux **positions
//! identiques** : un pour la couche Plan (aplats, sans éclairage, couleurs du plan stylisé) et un pour la couche 3D
//! (éclairé, couleurs « réalistes »). Le contrat d'exactitude 2D = 3D tient donc par construction.
//!
//! La scène n'est reconstruite que pour les familles de données qui ont changé (révisions de `mcf_carte`).

use std::collections::HashMap;
use std::time::Instant;

use bevy::{
    asset::RenderAssetUsages,
    camera::visibility::{RenderLayers, VisibilityRange},
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};
use mcf_carte::{
    Revisions,
    gabarits::Famille,
    relief::{self, Relief},
};
use mcf_geo::{Cote, Maillage, NatureBande, P2, Revetement, polygone};

use super::{COUCHE_3D, COUCHE_PLAN, Edition, Projet, textures::Surfaces};

/// Parties de la scène reconstruites indépendamment.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Partie {
    /// Terrain final.
    Terrain,
    /// Routes et carrefours.
    Routes,
    /// Bâti.
    Bati,
    /// Teintes des quartiers (Plan seulement).
    Quartiers,
    /// Plan d'eau.
    Eau,
}

/// Données d'affichage dérivées, conservées entre deux reconstructions.
#[derive(Resource, Default)]
pub struct Affiche {
    /// Révisions déjà affichées.
    vu: Option<Revisions>,
    /// Dernière reconstruction du terrain.
    dernier_terrain: Option<Instant>,
    /// Matériaux partagés.
    materiaux: Option<Materiaux>,
    /// Version du terrain final affichée (`mcf_carte::MajTerrain`).
    terrain_vu: Option<u64>,
    /// Bâtiments affichés par case.
    bati: HashMap<Case, Vec<mcf_carte::Batiment>>,
}

#[derive(Clone)]
struct Materiaux {
    plan: Handle<StandardMaterial>,
    voile_plan: Handle<StandardMaterial>,
    eau: Handle<StandardMaterial>,
}

pub struct ScenePlugin;

impl Plugin for ScenePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Affiche>().add_systems(Update, reconstruire);
    }
}

/// Tampon de maillage à couleurs par sommet.
#[derive(Default)]
struct Tampon {
    pos: Vec<[f32; 3]>,
    nor: Vec<[f32; 3]>,
    col: Vec<[f32; 4]>,
    /// Coordonnées de texture en mètres (voir `textures`).
    uv: Vec<[f32; 2]>,
    /// Tangentes (cartes de normales) ; `w` = sens de la bitangente.
    tan: Vec<[f32; 4]>,
    idx: Vec<u32>,
}

/// Tangente d'une surface de normale `n` dont la texture avance selon `u` (direction monde) et `v` : `u` projeté sur
/// la surface, `w` tel que `w · (n × t)` pointe vers les `v` croissants (convention MikkTSpace de Bevy).
/// Côté des morceaux du bâti (m).
const MORCEAU_BATI_M: f64 = 512.0;
/// Distance au-delà de laquelle la 3D ne dessine plus le bâti en blocs (m).
pub const PORTEE_BATI_M: f32 = 4500.0;

fn tangente(n: Vec3, u: Vec3, v: Vec3) -> [f32; 4] {
    let mut t = (u - n * n.dot(u)).normalize_or_zero();
    if t == Vec3::ZERO {
        t = n.any_orthonormal_vector();
    }
    let w = if n.cross(t).dot(v) >= 0.0 { 1.0 } else { -1.0 };
    [t.x, t.y, t.z, w]
}

/// Tangente d'une surface plaquée vue du dessus (u = +X, v = +Z).
fn tangente_sol(n: Vec3) -> [f32; 4] {
    tangente(n, Vec3::X, Vec3::Z)
}

/// Couleur sRGB 8 bits → couleur linéaire de sommet.
fn lin(c: [u8; 3], a: f32) -> [f32; 4] {
    let l = Color::srgb_u8(c[0], c[1], c[2]).to_linear();
    [l.red, l.green, l.blue, a]
}

/// Couleur sRGB flottante → couleur linéaire de sommet.
fn lin_f(c: [f32; 3]) -> [f32; 4] {
    let l = Color::srgb(c[0], c[1], c[2]).to_linear();
    [l.red, l.green, l.blue, 1.0]
}

impl Tampon {
    fn ajouter(&mut self, m: &Maillage, couleur: [f32; 4]) {
        let base = self.pos.len() as u32;
        self.pos.extend(m.positions.iter().map(|p| p.map(|c| c as f32)));
        if m.normales.len() == m.positions.len() {
            self.nor.extend(m.normales.iter().map(|n| n.map(|c| c as f32)));
        } else {
            self.nor
                .extend(std::iter::repeat_n([0.0, 1.0, 0.0], m.positions.len()));
        }
        // Plaquage vu du dessus : routes, toits, carrefours sont (presque) horizontaux.
        self.uv
            .extend(m.positions.iter().map(|p| [p[0] as f32, p[2] as f32]));
        let debut = self.nor.len() - m.positions.len();
        let tangentes: Vec<_> = self.nor[debut..]
            .iter()
            .map(|n| tangente_sol(Vec3::from_array(*n)))
            .collect();
        self.tan.extend(tangentes);
        self.col.extend(std::iter::repeat_n(couleur, m.positions.len()));
        self.idx.extend(m.indices.iter().map(|i| i + base));
    }

    /// Triangle orienté pour que sa face avant regarde vers `n`, coordonnées de texture `uv` (m) et tangente `tan`.
    fn triangle(&mut self, p: [Vec3; 3], uv: [Vec2; 3], n: Vec3, tan: [f32; 4], couleur: [f32; 4]) {
        let base = self.pos.len() as u32;
        let direct = (p[1] - p[0]).cross(p[2] - p[0]).dot(n) >= 0.0;
        for (q, t) in p.into_iter().zip(uv) {
            self.pos.push(q.to_array());
            self.nor.push(n.to_array());
            self.uv.push(t.to_array());
            self.tan.push(tan);
            self.col.push(couleur);
        }
        if direct {
            self.idx.extend([base, base + 1, base + 2]);
        } else {
            self.idx.extend([base, base + 2, base + 1]);
        }
    }

    fn vide(&self) -> bool {
        self.idx.is_empty()
    }

    fn maillage(self) -> Mesh {
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.pos)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.nor)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.col)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uv)
            .with_inserted_attribute(Mesh::ATTRIBUTE_TANGENT, self.tan)
            .with_inserted_indices(Indices::U32(self.idx))
    }
}

/// Maillages 3D des routes, un lot par matériau texturé.
#[derive(Default)]
struct Voirie {
    asphalte: Tampon,
    terre: Tampon,
    trottoir: Tampon,
    herbe: Tampon,
}

fn materiaux(materials: &mut Assets<StandardMaterial>) -> Materiaux {
    Materiaux {
        plan: materials.add(StandardMaterial {
            base_color: Color::WHITE,
            unlit: true,
            ..default()
        }),
        voile_plan: materials.add(StandardMaterial {
            base_color: Color::WHITE,
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            ..default()
        }),
        eau: materials.add(StandardMaterial {
            base_color: Color::srgba(0.22, 0.42, 0.62, 0.82),
            perceptual_roughness: 0.15,
            alpha_mode: AlphaMode::Blend,
            ..default()
        }),
    }
}

/// Case du découpage des routes et du bâti (`MORCEAU_BATI_M`), pour ne refaire que ce qui a changé.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Case(i64, i64);

/// Morceau de terrain grossier : premier échantillon `(x0, z0)` (même découpage que les tuiles fines).
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Morceau(usize, usize);

fn case_de(p: P2) -> Case {
    Case(
        (p[0] / MORCEAU_BATI_M).floor() as i64,
        (p[1] / MORCEAU_BATI_M).floor() as i64,
    )
}

/// Rectangle `(min, max)` d'une case.
fn rect_case(c: Case) -> [P2; 2] {
    let (x, z) = (c.0 as f64 * MORCEAU_BATI_M, c.1 as f64 * MORCEAU_BATI_M);
    [[x, z], [x + MORCEAU_BATI_M, z + MORCEAU_BATI_M]]
}

fn se_touchent(a: &[P2; 2], b: &[P2; 2]) -> bool {
    a[0][0] <= b[1][0] && b[0][0] <= a[1][0] && a[0][1] <= b[1][1] && b[0][1] <= a[1][1]
}

/// Ce qu'il faut refaire : tout, ou seulement ce qui touche ces rectangles.
enum ARefaire {
    Tout,
    Zones(Vec<[P2; 2]>),
}

impl ARefaire {
    fn touche(&self, r: &[P2; 2]) -> bool {
        match self {
            Self::Tout => true,
            Self::Zones(z) => z.iter().any(|z| se_touchent(z, r)),
        }
    }

    fn rien(&self) -> bool {
        matches!(self, Self::Zones(z) if z.is_empty())
    }
}

#[allow(clippy::too_many_arguments)]
fn reconstruire(
    mut commands: Commands,
    mut projet: ResMut<Projet>,
    mut affiche: ResMut<Affiche>,
    mut edition: ResMut<Edition>,
    boutons: Res<ButtonInput<MouseButton>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    surfaces: Res<Surfaces>,
    catalogue: Res<super::modeles::Catalogue>,
    parties: Query<(Entity, &Partie, Option<&Case>, Option<&Morceau>)>,
) {
    let r = projet.carte.revisions;
    if affiche.vu == Some(r) {
        return;
    }
    let mats = affiche
        .materiaux
        .get_or_insert_with(|| materiaux(&mut materials))
        .clone();
    let avant = affiche.vu;
    let change = |f: fn(&Revisions) -> u64| avant.is_none_or(|a| f(&a) != f(&r));
    let geometrie = change(|x| x.reseau) || change(|x| x.manifeste);
    let terrain = geometrie || change(|x| x.relief);
    // Pendant un coup de pinceau sur une grande zone, le terrain est reconstruit au plus 12 fois par seconde.
    if terrain
        && !geometrie
        && boutons.pressed(MouseButton::Left)
        && affiche
            .dernier_terrain
            .is_some_and(|t| t.elapsed().as_millis() < 80)
    {
        return;
    }
    let debut = Instant::now();
    let eau = projet.carte.manifeste.niveau_eau_m;
    let (coin, fin) = projet.carte.manifeste.bornes();
    let profils = projet.carte.manifeste.profils;
    let alt_max = projet.carte.manifeste.altitude_max_m as f32;
    let projet_alt_min = projet.carte.manifeste.altitude_min_m as f32;
    let manifeste_change = change(|x| x.manifeste);
    let rang = |c: mcf_geo::ClasseRoute| {
        mcf_geo::ClasseRoute::TOUTES
            .iter()
            .position(|x| *x == c)
            .unwrap_or(0)
    };
    // Classe la plus haute qui arrive à chaque nœud (couleur du carrefour), en un passage sur les tronçons.
    let mut classe_noeud: HashMap<mcf_carte::Id, mcf_geo::ClasseRoute> = HashMap::new();
    if terrain {
        for t in projet.carte.reseau.troncons.values() {
            for n in [t.de, t.vers] {
                let c = classe_noeud.entry(n).or_insert(t.classe);
                if rang(t.classe) > rang(*c) {
                    *c = t.classe;
                }
            }
        }
    }
    let quartiers = (geometrie || change(|x| x.quartiers)).then(|| projet.carte.quartiers.clone());
    // Terrain, routes et bâti : seulement autour de ce que la carte a recalculé (blocs de terrain), sauf première
    // construction ou changement du manifeste.
    let d = projet.carte.derive();
    let sol = if !terrain {
        ARefaire::Zones(Vec::new())
    } else if manifeste_change {
        ARefaire::Tout
    } else {
        // Les normales du terrain lisent les échantillons voisins : marge d'un échantillon et demi.
        let m = d.terrain.resolution * 1.5;
        match d.maj_terrain.rectangles(affiche.terrain_vu, &d.terrain) {
            Some(z) => ARefaire::Zones(
                z.into_iter()
                    .map(|[a, b]| [[a[0] - m, a[1] - m], [b[0] + m, b[1] + m]])
                    .collect(),
            ),
            None => ARefaire::Tout,
        }
    };
    // Bâti : cases dont le terrain ou les bâtiments ont changé (la liste affichée de chaque case est gardée).
    let mut cases_bati: HashMap<Case, Vec<mcf_carte::Batiment>> = HashMap::new();
    let bati_change = change(|x| x.batiments);
    if bati_change || !sol.rien() {
        for b in projet.carte.batiments.values() {
            cases_bati.entry(case_de(b.centre())).or_default().push(b.clone());
        }
    }
    let mut bati_a_refaire: std::collections::HashSet<Case> = std::collections::HashSet::new();
    if bati_change || !sol.rien() {
        let connues: std::collections::HashSet<Case> =
            cases_bati.keys().chain(affiche.bati.keys()).copied().collect();
        for c in connues {
            let nouveau = cases_bati.get(&c);
            let differe = bati_change && nouveau != affiche.bati.get(&c);
            if differe || sol.touche(&rect_case(c)) {
                bati_a_refaire.insert(c);
            }
        }
    }
    let d = projet.carte.derive();
    for (e, p, case, morceau) in &parties {
        let refaire = match p {
            Partie::Terrain => match (morceau, &sol) {
                (_, ARefaire::Tout) => true,
                (Some(m), z) => z.touche(&rect_morceau(&d.terrain, *m)),
                // Courbes de niveau sans morceau (n'arrive pas) : refaites avec le terrain.
                (None, z) => !z.rien(),
            },
            Partie::Eau => manifeste_change,
            Partie::Routes => match (case, &sol) {
                (_, ARefaire::Tout) => true,
                (Some(c), z) => z.touche(&rect_case(*c)),
                (None, z) => !z.rien(),
            },
            Partie::Bati => case.is_none_or(|c| bati_a_refaire.contains(c)) && !bati_a_refaire.is_empty(),
            Partie::Quartiers => geometrie || change(|x| x.quartiers),
        };
        if refaire {
            commands.entity(e).despawn();
        }
    }
    let mut ajouter = |commands: &mut Commands,
                       partie: Partie,
                       mesh: Mesh,
                       mat: Handle<StandardMaterial>,
                       couche: usize|
     -> Entity {
        // Plan : routes et bâti passent au-dessus des calques de référence (altitude max + 10 à + 18 m) et du voile
        // des quartiers (+ 20 m), sinon un calque opaque les cache. En vue verticale l'altitude ne sert qu'à l'ordre
        // d'affichage : le relief est écrasé (× 0,001, quelques mètres) et posé au-dessus.
        let transform = match (couche, partie) {
            (COUCHE_PLAN, Partie::Routes) => plan_au_dessus(alt_max + 30.0),
            (COUCHE_PLAN, Partie::Bati) => plan_au_dessus(alt_max + 34.0),
            _ => Transform::IDENTITY,
        };
        commands
            .spawn((
                partie,
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(mat),
                transform,
                RenderLayers::layer(couche),
            ))
            .id()
    };

    if !sol.rien() {
        // Courbes de niveau du Plan : un maillage de lignes par morceau, construit avec lui (elles étaient
        // redessinées une à une à chaque image, des centaines de milliers de segments sur une grande carte).
        let pas_courbes = d.terrain.nx.max(d.terrain.nz).div_ceil(400).max(1);
        for (m, xs, zs) in morceaux_terrain(&d.terrain) {
            if !sol.touche(&rect_morceau(&d.terrain, m)) {
                continue;
            }
            let (plan, trois_d) = maillage_morceau(&d.terrain, &xs, &zs, eau);
            let e = ajouter(
                &mut commands,
                Partie::Terrain,
                plan,
                mats.plan.clone(),
                COUCHE_PLAN,
            );
            commands.entity(e).insert(m);
            let (mesh, transform) = recentrer(trois_d);
            let e = ajouter(
                &mut commands,
                Partie::Terrain,
                mesh,
                surfaces.herbe.clone(),
                COUCHE_3D,
            );
            commands
                .entity(e)
                .insert((m, transform, super::terrain_fin::portee_grossier()));
            let zone = relief::Zone {
                x0: m.0,
                z0: m.1,
                x1: *xs.last().unwrap_or(&m.0),
                z1: *zs.last().unwrap_or(&m.1),
            };
            let (mut positions, mut couleurs) = (Vec::new(), Vec::new());
            for (a, b, h) in relief::courbes_de_niveau_zone(&d.terrain, 5.0, pas_courbes, zone) {
                let y = h as f32 + 0.3;
                let alpha = if (h / 25.0).fract().abs() < 1e-6 {
                    0.6
                } else {
                    0.28
                };
                let c = Color::srgba(0.45, 0.32, 0.2, alpha).to_linear().to_f32_array();
                positions.push([a[0] as f32, y, a[1] as f32]);
                positions.push([b[0] as f32, y, b[1] as f32]);
                couleurs.extend([c, c]);
            }
            if !positions.is_empty() {
                let mut lignes = Mesh::new(PrimitiveTopology::LineList, RenderAssetUsages::RENDER_WORLD);
                lignes.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
                lignes.insert_attribute(Mesh::ATTRIBUTE_COLOR, couleurs);
                let e = ajouter(
                    &mut commands,
                    Partie::Terrain,
                    lignes,
                    mats.voile_plan.clone(),
                    COUCHE_PLAN,
                );
                commands.entity(e).insert(m);
            }
        }
        affiche.dernier_terrain = Some(Instant::now());
    }

    if manifeste_change {
        // Fond du Plan (hors carte) : un aplat sous tout, pour que le Plan n'ait jamais à effacer l'image (mini-carte,
        // vue partagée).
        let (w, h) = ((fin[0] - coin[0]) as f32, (fin[1] - coin[1]) as f32);
        let c = Vec3::new(
            ((coin[0] + fin[0]) * 0.5) as f32,
            projet_alt_min - 20.0,
            ((coin[1] + fin[1]) * 0.5) as f32,
        );
        let fond = materials.add(StandardMaterial {
            base_color: Color::srgb(0.86, 0.87, 0.84),
            unlit: true,
            ..default()
        });
        ajouter(
            &mut commands,
            Partie::Eau,
            Plane3d::default()
                .mesh()
                .size(w * 30.0, h * 30.0)
                .build()
                .translated_by(c),
            fond,
            COUCHE_PLAN,
        );
    }
    if manifeste_change && let Some(niveau) = eau {
        let (w, h) = ((fin[0] - coin[0]) as f32, (fin[1] - coin[1]) as f32);
        let c = Vec3::new(
            ((coin[0] + fin[0]) * 0.5) as f32,
            niveau as f32,
            ((coin[1] + fin[1]) * 0.5) as f32,
        );
        ajouter(
            &mut commands,
            Partie::Eau,
            Plane3d::default().mesh().size(w, h).build().translated_by(c),
            mats.eau.clone(),
            COUCHE_3D,
        );
    }

    if !sol.rien() {
        // Routes par case : chaque tronçon va dans la case du centre de son axe, chaque carrefour dans celle de son
        // nœud. Une case est refaite entière dès qu'elle touche une zone recalculée.
        let mut cases: HashMap<Case, (Tampon, Voirie)> = HashMap::new();
        let blanc = [1.0, 1.0, 1.0, 1.0];
        let trottoir_plan = lin([226, 222, 214], 1.0);
        for (id, g) in &d.geo.troncons {
            let [a, b] = relief::englobant(&g.axe);
            let case = case_de([(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5]);
            if !sol.touche(&rect_case(case)) {
                continue;
            }
            let profil = profils.get(g.classe);
            let Some(long) = d.altitudes.troncons.get(id) else {
                continue;
            };
            let (plan, voirie) = cases.entry(case).or_default();
            let altitudes: Vec<f64> = g.abscisses_rognees().iter().map(|s| long.altitude(*s)).collect();
            for b in &g.bandes {
                // Le terre-plein central est un muret béton posé sur la chaussée : plus haut que les autres bandes.
                let surelevation = if b.nature == NatureBande::TerrePlein {
                    0.30
                } else {
                    0.10
                };
                let m = mcf_geo::maillage_bande(b, &altitudes, surelevation);
                let cp = match b.nature {
                    NatureBande::Chaussee => lin(profil.couleur_plan, 1.0),
                    NatureBande::TerrePlein => lin(mcf_geo::COULEUR_TERRE_PLEIN, 1.0),
                    _ => trottoir_plan,
                };
                let lot = match (b.nature, profil.revetement, profil.cote) {
                    (NatureBande::TerrePlein, ..) => &mut voirie.trottoir,
                    (NatureBande::Chaussee, Revetement::Revetu, _) => &mut voirie.asphalte,
                    (NatureBande::Chaussee, Revetement::Terre, _) => &mut voirie.terre,
                    (_, _, Cote::Trottoir) => &mut voirie.trottoir,
                    _ => &mut voirie.terre,
                };
                plan.ajouter(&m, cp);
                lot.ajouter(&m, blanc);
            }
        }
        for (id, n) in &d.geo.noeuds {
            let case = case_de(n.position);
            if !sol.touche(&rect_case(case)) {
                continue;
            }
            let Some(&h) = d.altitudes.noeuds.get(id) else {
                continue;
            };
            let classe = classe_noeud.get(id).copied().unwrap_or(mcf_geo::ClasseRoute::Rue);
            let profil = profils.get(classe);
            let (plan, voirie) = cases.entry(case).or_default();
            if !n.carrefour.emprise.is_empty() {
                let m = mcf_geo::maillage_polygone(&n.carrefour.emprise, |_| h + 0.10);
                plan.ajouter(&m, trottoir_plan);
                voirie.trottoir.ajouter(&m, blanc);
            }
            if !n.carrefour.chaussee.is_empty() {
                let m = mcf_geo::maillage_polygone(&n.carrefour.chaussee, |_| h + 0.12);
                plan.ajouter(&m, lin(profil.couleur_plan, 1.0));
                match profil.revetement {
                    Revetement::Revetu => voirie.asphalte.ajouter(&m, blanc),
                    Revetement::Terre => voirie.terre.ajouter(&m, blanc),
                }
            }
            if !n.ilot.is_empty() {
                let m = mcf_geo::maillage_polygone(&n.ilot, |_| h + 0.25);
                plan.ajouter(&m, lin([170, 214, 150], 1.0));
                voirie.herbe.ajouter(&m, blanc);
            }
        }
        for (case, (plan, voirie)) in cases {
            if !plan.vide() {
                let e = ajouter(
                    &mut commands,
                    Partie::Routes,
                    plan.maillage(),
                    mats.plan.clone(),
                    COUCHE_PLAN,
                );
                commands.entity(e).insert(case);
            }
            for (lot, mat) in [
                (voirie.asphalte, &surfaces.asphalte),
                (voirie.terre, &surfaces.terre),
                (voirie.trottoir, &surfaces.trottoir),
                (voirie.herbe, &surfaces.herbe),
            ] {
                if !lot.vide() {
                    let (mesh, transform) = recentrer(lot.maillage());
                    let e = ajouter(&mut commands, Partie::Routes, mesh, mat.clone(), COUCHE_3D);
                    commands.entity(e).insert((case, transform));
                }
            }
        }
    }

    if !bati_a_refaire.is_empty() {
        // Bâti découpé en cases de `MORCEAU_BATI_M` : le moteur écarte celles hors du champ, la 3D ne dessine pas
        // celles au-delà de `PORTEE_BATI_M`, et seules les cases touchées sont refaites.
        // Bâtiment modélisé : de près le modèle (`modeles`), de loin son bloc (niveau de détail, fondu entre les deux).
        let portee = VisibilityRange {
            start_margin: 0.0..0.0,
            end_margin: (PORTEE_BATI_M - 400.0)..PORTEE_BATI_M,
            use_aabb: true,
        };
        let portee_lod = VisibilityRange {
            start_margin: (super::modeles::PORTEE_MODELE_M - 200.0)..super::modeles::PORTEE_MODELE_M,
            ..portee.clone()
        };
        for case in &bati_a_refaire {
            let Some(liste) = cases_bati.get(case) else {
                continue;
            };
            let mut lots: [(Tampon, Tampon, Tampon); 2] = Default::default();
            for b in liste {
                let modele = catalogue.modele_affiche(b);
                let (plan, toits, murs) = &mut lots[usize::from(modele)];
                bloc(plan, toits, murs, b, &d.terrain, true);
            }
            for (modele, (plan, toits, murs)) in lots.into_iter().enumerate() {
                if !plan.vide() {
                    let e = ajouter(
                        &mut commands,
                        Partie::Bati,
                        plan.maillage(),
                        mats.plan.clone(),
                        COUCHE_PLAN,
                    );
                    commands.entity(e).insert(*case);
                }
                for (lot, mat) in [(toits, &surfaces.toit), (murs, &surfaces.facade)] {
                    if !lot.vide() {
                        let (mesh, transform) = recentrer(lot.maillage());
                        let e = ajouter(&mut commands, Partie::Bati, mesh, mat.clone(), COUCHE_3D);
                        let portee = if modele == 1 {
                            portee_lod.clone()
                        } else {
                            portee.clone()
                        };
                        commands.entity(e).insert((*case, transform, portee));
                    }
                }
            }
        }
        for c in &bati_a_refaire {
            match cases_bati.remove(c) {
                Some(l) => {
                    affiche.bati.insert(*c, l);
                }
                None => {
                    affiche.bati.remove(c);
                }
            }
        }
    }

    if let Some(quartiers) = quartiers {
        let mut voile = Tampon::default();
        // Au-dessus de tout relief : en vue verticale, l'altitude ne déplace rien.
        let y = f64::from(alt_max) + 20.0;
        for a in quartiers.affectations.values() {
            if let Some(i) = mcf_carte::ilots::ilot_en(&d.ilots, a.repere) {
                let poly = i.interieur.as_ref().unwrap_or(&i.contour);
                voile.ajouter(
                    &mcf_geo::maillage_polygone(poly, |_| y),
                    lin(a.famille.couleur_plan(), 0.3),
                );
            }
        }
        for z in quartiers.zones.values() {
            voile.ajouter(
                &mcf_geo::maillage_polygone(&z.polygone, |_| y),
                lin(z.famille.couleur_plan(), 0.3),
            );
        }
        if !voile.vide() {
            ajouter(
                &mut commands,
                Partie::Quartiers,
                voile.maillage(),
                mats.voile_plan.clone(),
                COUCHE_PLAN,
            );
        }
    }

    let version_terrain = d.maj_terrain.a;
    if geometrie || change(|x| x.batiments) {
        edition.conflits = projet.carte.conflits();
    }
    affiche.terrain_vu = Some(version_terrain);
    affiche.vu = Some(r);
    edition.duree_scene_ms = debut.elapsed().as_secs_f64() * 1000.0;
}

/// Rectangle `(min, max)` (m) d'un morceau de terrain grossier.
fn rect_morceau(t: &Relief, m: Morceau) -> [P2; 2] {
    let tuile = super::terrain_fin::TUILE;
    let x1 = (m.0 + tuile).min(t.nx.saturating_sub(1));
    let z1 = (m.1 + tuile).min(t.nz.saturating_sub(1));
    [t.position(m.0, m.1), t.position(x1, z1)]
}

/// Morceaux du terrain grossier : `TUILE` échantillons de côté (même découpage que les tuiles fines, pour que chaque
/// tuile fine remplace exactement son morceau grossier), avec les échantillons affichés. Au plus ~1024 échantillons
/// par côté de carte sont affichés : au-delà, la grille est sous-échantillonnée (le relief source et le terrain final
/// restent à pleine résolution, et la 3D proche est à pleine résolution, `terrain_fin`).
fn morceaux_terrain(t: &Relief) -> Vec<(Morceau, Vec<usize>, Vec<usize>)> {
    let mut v = Vec::new();
    if t.nx < 2 || t.nz < 2 {
        return v;
    }
    let pas = t.nx.max(t.nz).div_ceil(1024).max(1);
    let tuile = super::terrain_fin::TUILE;
    let mut z0 = 0;
    while z0 + 1 < t.nz {
        let z1 = (z0 + tuile).min(t.nz - 1);
        let mut x0 = 0;
        while x0 + 1 < t.nx {
            let x1 = (x0 + tuile).min(t.nx - 1);
            let xs: Vec<usize> = echantillons(x1 - x0 + 1, pas)
                .into_iter()
                .map(|i| x0 + i)
                .collect();
            let zs: Vec<usize> = echantillons(z1 - z0 + 1, pas)
                .into_iter()
                .map(|i| z0 + i)
                .collect();
            v.push((Morceau(x0, z0), xs, zs));
            x0 = x1;
        }
        z0 = z1;
    }
    v
}

/// Ramène les sommets d'un maillage autour de son centre et rend le `Transform` qui le replace. Indispensable avec
/// `VisibilityRange` : le fondu calculé par la carte graphique mesure la distance à l'**origine** de l'objet, pas à sa
/// boîte ; un morceau aux sommets en coordonnées du monde (origine au centre de la carte) serait jugé à des kilomètres.
pub fn recentrer(mut mesh: Mesh) -> (Mesh, Transform) {
    use bevy::mesh::VertexAttributeValues;
    let mut centre = Vec3::ZERO;
    if let Some(VertexAttributeValues::Float32x3(pos)) = mesh.attribute_mut(Mesh::ATTRIBUTE_POSITION)
        && !pos.is_empty()
    {
        let (mut min, mut max) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        for p in pos.iter() {
            min = min.min(Vec3::from(*p));
            max = max.max(Vec3::from(*p));
        }
        centre = (min + max) / 2.0;
        for p in pos.iter_mut() {
            *p = (Vec3::from(*p) - centre).to_array();
        }
    }
    (mesh, Transform::from_translation(centre))
}

/// Échantillons d'un axe de `0` à `n - 1` au pas `pas` (le dernier toujours inclus).
fn echantillons(n: usize, pas: usize) -> Vec<usize> {
    (0..n)
        .step_by(pas)
        .chain((!(n - 1).is_multiple_of(pas)).then_some(n - 1))
        .collect()
}

/// Maillages (Plan, 3D) d'un morceau de terrain : échantillons `xs` × `zs` du relief.
pub fn maillage_morceau(t: &Relief, xs: &[usize], zs: &[usize], eau: Option<f64>) -> (Mesh, Mesh) {
    let (nx, nz) = (xs.len(), zs.len());
    let mut pos = Vec::with_capacity(nx * nz);
    let mut nor = Vec::with_capacity(nx * nz);
    let mut col_plan = Vec::with_capacity(nx * nz);
    let mut col_3d = Vec::with_capacity(nx * nz);
    let mut uv = Vec::with_capacity(nx * nz);
    let mut tan = Vec::with_capacity(nx * nz);
    for &iz in zs {
        for &ix in xs {
            let p = t.position(ix, iz);
            let h = f64::from(t.h[t.index(ix, iz)]);
            let n = t.normale(p);
            pos.push([p[0] as f32, h as f32, p[1] as f32]);
            nor.push(n.map(|c| c as f32));
            col_plan.push(lin_f(relief::couleur_plan(h, n, eau)));
            col_3d.push(lin_f(teinte_terrain(h, n, eau)));
            uv.push([p[0] as f32, p[1] as f32]);
            tan.push(tangente_sol(Vec3::new(n[0] as f32, n[1] as f32, n[2] as f32)));
        }
    }
    let mut idx = Vec::with_capacity((nx - 1) * (nz - 1) * 6);
    for j in 0..nz - 1 {
        for i in 0..nx - 1 {
            let a = (j * nx + i) as u32;
            let b = a + 1;
            let c = a + nx as u32;
            let d = c + 1;
            // Face avant vers +Y (sens direct vu du dessus avec +Z vers le bas de l'écran).
            idx.extend_from_slice(&[a, c, b, b, c, d]);
        }
    }
    let fabriquer = |col: Vec<[f32; 4]>| {
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos.clone())
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, nor.clone())
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, col)
            .with_inserted_indices(Indices::U32(idx.clone()))
    };
    // Le Plan n'a pas de texture : UV et tangentes seulement pour la 3D.
    let trois_d = fabriquer(col_3d)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
        .with_inserted_attribute(Mesh::ATTRIBUTE_TANGENT, tan);
    (fabriquer(col_plan), trois_d)
}

/// Teinte 3D du terrain (multiplie la texture d'herbe) : plus sèche en altitude, minérale dans les pentes fortes,
/// sombre et vaseuse sous l'eau. L'estompage vient de l'éclairage, pas de la teinte.
fn teinte_terrain(h: f64, n: [f64; 3], eau: Option<f64>) -> [f32; 3] {
    if eau.is_some_and(|e| h < e) {
        return [0.55, 0.52, 0.42];
    }
    // Teinte d'altitude du plan, ramenée à son maximum (on garde la nuance, pas la luminosité).
    let c = relief::couleur_plan(h, [0.0, 1.0, 0.0], None);
    let m = c[0].max(c[1]).max(c[2]).max(1e-3);
    let alt = c.map(|x| 0.55 + 0.45 * x / m);
    // Pente : 0 à plat, 1 à la verticale ; la roche apparaît à partir d'environ 30°.
    let pente = (1.0 - n[1].clamp(0.0, 1.0)) as f32;
    let t = ((pente - 0.12) / 0.3).clamp(0.0, 1.0);
    let t = t * t * (3.0 - 2.0 * t);
    let roche = [0.78, 0.70, 0.62];
    [0, 1, 2].map(|k| alt[k] * (1.0 - t) + roche[k] * t)
}

/// Teinte des façades d'une famille (multiplie l'enduit) : la couleur du plan, désaturée et éclaircie en pastel pour
/// rester lisible sans écraser la texture.
fn teinte_3d(f: Famille) -> [f32; 4] {
    let c = f.couleur_plan().map(|x| f32::from(x) / 255.0);
    let gris = (c[0] + c[1] + c[2]) / 3.0;
    lin_f(c.map(|x| 0.45 + 0.55 * (gris * 0.4 + x * 0.6)))
}

/// Un bloc : toit à plat + murs, posé sur le terrain (pieds enterrés jusqu'au point le plus bas).
/// Écrase un maillage du Plan à quelques mètres d'épaisseur et le pose à `y` (l'ordre relatif est gardé).
fn plan_au_dessus(y: f32) -> Transform {
    Transform::from_translation(Vec3::Y * y).with_scale(Vec3::new(1.0, 0.001, 1.0))
}

fn bloc(
    plan: &mut Tampon,
    toits: &mut Tampon,
    murs: &mut Tampon,
    b: &mcf_carte::Batiment,
    terrain: &Relief,
    en_3d: bool,
) {
    if b.empreinte.len() < 3 {
        return;
    }
    let hs: Vec<f64> = b.empreinte.iter().map(|p| terrain.hauteur(*p)).collect();
    // Décalage vertical : socle (vers le haut) ou bâtiment enfoncé (vers le bas, murs prolongés d'autant).
    let bas = hs.iter().copied().fold(f64::INFINITY, f64::min) - 0.3 + b.decalage_y_m.min(0.0);
    let sol = terrain.hauteur(b.centre()).max(bas + 0.3) + b.decalage_y_m;
    let haut = sol + b.hauteur();
    let toit = mcf_geo::maillage_polygone(&b.empreinte, |_| haut);
    plan.ajouter(&toit, lin([150, 140, 132], 1.0));
    if !en_3d {
        return;
    }
    toits.ajouter(&toit, lin([200, 198, 194], 1.0));
    let c = polygone::centroide(&b.empreinte);
    let mur = teinte_3d(b.famille);
    let n = b.empreinte.len();
    let mut u_mur = 0.0f32;
    for k in 0..n {
        let (a, bb): (P2, P2) = (b.empreinte[k], b.empreinte[(k + 1) % n]);
        let (dx, dz) = ((bb[0] - a[0]) as f32, (bb[1] - a[1]) as f32);
        let mut normale = Vec3::new(dz, 0.0, -dx).normalize_or_zero();
        let milieu = Vec3::new(
            ((a[0] + bb[0]) * 0.5 - c[0]) as f32,
            0.0,
            ((a[1] + bb[1]) * 0.5 - c[1]) as f32,
        );
        if normale.dot(milieu) < 0.0 {
            normale = -normale;
        }
        let p = |q: P2, y: f64| Vec3::new(q[0] as f32, y as f32, q[1] as f32);
        let quad = [p(a, bas), p(bb, bas), p(bb, haut), p(a, haut)];
        // Texture : u le long du mur (continu d'un mur au suivant), v vers le bas (haut de l'image en haut du mur).
        let (u0, u1) = (u_mur, u_mur + Vec2::new(dx, dz).length());
        u_mur = u1;
        let (vb, vh) = (-(bas as f32), -(haut as f32));
        let uv = [
            Vec2::new(u0, vb),
            Vec2::new(u1, vb),
            Vec2::new(u1, vh),
            Vec2::new(u0, vh),
        ];
        let tan = tangente(normale, (quad[1] - quad[0]).normalize_or_zero(), Vec3::NEG_Y);
        for k in [[0, 1, 2], [0, 2, 3]] {
            // Murs : 3D seulement (vus de dessus, ils n'ont pas d'épaisseur).
            murs.triangle(k.map(|i| quad[i]), k.map(|i| uv[i]), normale, tan, mur);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `w · (n × t)` doit pointer vers les `v` croissants, sinon les cartes de normales sont inversées.
    fn bitangente(n: Vec3, t: [f32; 4]) -> Vec3 {
        n.cross(Vec3::new(t[0], t[1], t[2])) * t[3]
    }

    #[test]
    fn tangente_du_sol_suit_les_uv_monde() {
        let t = tangente_sol(Vec3::Y);
        assert_eq!([t[0], t[1], t[2]], [1.0, 0.0, 0.0]);
        assert!(bitangente(Vec3::Y, t).abs_diff_eq(Vec3::Z, 1e-6));
        // Pente : la tangente reste dans la surface.
        let n = Vec3::new(0.6, 0.8, 0.0);
        let t = tangente_sol(n);
        assert!(Vec3::new(t[0], t[1], t[2]).dot(n).abs() < 1e-6);
    }

    #[test]
    fn tangente_dun_mur_descend_avec_v() {
        let n = Vec3::Z;
        let t = tangente(n, Vec3::X, Vec3::NEG_Y);
        assert!(bitangente(n, t).abs_diff_eq(Vec3::NEG_Y, 1e-6));
    }

    #[test]
    fn teinte_du_terrain_devient_minerale_dans_les_pentes() {
        let plat = teinte_terrain(10.0, [0.0, 1.0, 0.0], None);
        let raide = teinte_terrain(10.0, [0.8, 0.6, 0.0], None);
        assert!(plat[1] > plat[0], "herbe : vert dominant {plat:?}");
        assert!(raide[0] >= raide[1], "pente : roche {raide:?}");
    }
}
