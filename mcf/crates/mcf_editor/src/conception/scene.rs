//! Construction de la scène à partir du document. Chaque élément visible existe en deux exemplaires aux **positions
//! identiques** : un pour la couche Plan (aplats, sans éclairage, couleurs du plan stylisé) et un pour la couche 3D
//! (éclairé, couleurs « réalistes »). Le contrat d'exactitude 2D = 3D tient donc par construction.
//!
//! La scène n'est reconstruite que pour les familles de données qui ont changé (révisions de `mcf_carte`).

use std::time::Instant;

use bevy::{
    asset::RenderAssetUsages,
    camera::visibility::RenderLayers,
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
    /// Courbes de niveau `(a, b, maîtresse)`, posées sur le terrain final.
    pub courbes: Vec<(Vec3, Vec3, bool)>,
    /// Matériaux partagés.
    materiaux: Option<Materiaux>,
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
    parties: Query<(Entity, &Partie)>,
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
    // Pendant un coup de pinceau, le terrain est reconstruit au plus 12 fois par seconde.
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
    let a_refaire = [
        (Partie::Terrain, terrain),
        (Partie::Eau, change(|x| x.manifeste)),
        (Partie::Routes, terrain),
        (Partie::Bati, terrain || change(|x| x.batiments)),
        (Partie::Quartiers, geometrie || change(|x| x.quartiers)),
    ];
    for (e, p) in &parties {
        if a_refaire.iter().any(|(q, oui)| q == p && *oui) {
            commands.entity(e).despawn();
        }
    }
    let eau = projet.carte.manifeste.niveau_eau_m;
    let (coin, fin) = projet.carte.manifeste.bornes();
    let profils = projet.carte.manifeste.profils;
    let batiments: Vec<_> = projet.carte.batiments.values().cloned().collect();
    let quartiers = projet.carte.quartiers.clone();
    let reseau = projet.carte.reseau.clone();
    let alt_max = projet.carte.manifeste.altitude_max_m as f32;
    let projet_alt_min = projet.carte.manifeste.altitude_min_m as f32;
    let d = projet.carte.derive();
    let mut ajouter = |commands: &mut Commands,
                       partie: Partie,
                       mesh: Mesh,
                       mat: Handle<StandardMaterial>,
                       couche: usize| {
        commands.spawn((
            partie,
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(mat),
            Transform::IDENTITY,
            RenderLayers::layer(couche),
        ));
    };

    if terrain {
        for (plan, trois_d) in maillages_terrain(&d.terrain, eau) {
            ajouter(
                &mut commands,
                Partie::Terrain,
                plan,
                mats.plan.clone(),
                COUCHE_PLAN,
            );
            ajouter(
                &mut commands,
                Partie::Terrain,
                trois_d,
                surfaces.herbe.clone(),
                COUCHE_3D,
            );
        }
        let pas = d.terrain.nx.max(d.terrain.nz).div_ceil(400).max(1);
        affiche.courbes = relief::courbes_de_niveau(&d.terrain, 5.0, pas)
            .into_iter()
            .map(|(a, b, h)| {
                let y = h as f32 + 0.3;
                (
                    Vec3::new(a[0] as f32, y, a[1] as f32),
                    Vec3::new(b[0] as f32, y, b[1] as f32),
                    (h / 25.0).fract().abs() < 1e-6,
                )
            })
            .collect();
        affiche.dernier_terrain = Some(Instant::now());
    }

    if change(|x| x.manifeste) {
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
    if change(|x| x.manifeste)
        && let Some(niveau) = eau
    {
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

    if terrain {
        let (mut plan, mut voirie) = (Tampon::default(), Voirie::default());
        let blanc = [1.0, 1.0, 1.0, 1.0];
        let rang = |c: mcf_geo::ClasseRoute| {
            mcf_geo::ClasseRoute::TOUTES
                .iter()
                .position(|x| *x == c)
                .unwrap_or(0)
        };
        let trottoir_plan = lin([226, 222, 214], 1.0);
        for (id, g) in &d.geo.troncons {
            let profil = profils.get(g.classe);
            let Some(long) = d.altitudes.troncons.get(id) else {
                continue;
            };
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
            let Some(&h) = d.altitudes.noeuds.get(id) else {
                continue;
            };
            let classe = reseau
                .troncons
                .values()
                .filter(|t| t.de == *id || t.vers == *id)
                .map(|t| t.classe)
                .max_by_key(|c| rang(*c))
                .unwrap_or(mcf_geo::ClasseRoute::Rue);
            let profil = profils.get(classe);
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
        if !plan.vide() {
            ajouter(
                &mut commands,
                Partie::Routes,
                plan.maillage(),
                mats.plan.clone(),
                COUCHE_PLAN,
            );
        }
        for (lot, mat) in [
            (voirie.asphalte, &surfaces.asphalte),
            (voirie.terre, &surfaces.terre),
            (voirie.trottoir, &surfaces.trottoir),
            (voirie.herbe, &surfaces.herbe),
        ] {
            if !lot.vide() {
                ajouter(
                    &mut commands,
                    Partie::Routes,
                    lot.maillage(),
                    mat.clone(),
                    COUCHE_3D,
                );
            }
        }
    }

    if terrain || change(|x| x.batiments) {
        let (mut plan, mut toits, mut murs) = (Tampon::default(), Tampon::default(), Tampon::default());
        for b in &batiments {
            bloc(&mut plan, &mut toits, &mut murs, b, &d.terrain);
        }
        if !plan.vide() {
            ajouter(
                &mut commands,
                Partie::Bati,
                plan.maillage(),
                mats.plan.clone(),
                COUCHE_PLAN,
            );
        }
        for (lot, mat) in [(toits, &surfaces.toit), (murs, &surfaces.facade)] {
            if !lot.vide() {
                ajouter(
                    &mut commands,
                    Partie::Bati,
                    lot.maillage(),
                    mat.clone(),
                    COUCHE_3D,
                );
            }
        }
    }

    if geometrie || change(|x| x.quartiers) {
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

    if geometrie || change(|x| x.batiments) {
        edition.conflits = projet.carte.conflits();
    }
    affiche.vu = Some(r);
    edition.duree_scene_ms = debut.elapsed().as_secs_f64() * 1000.0;
}

/// Maillages du terrain par morceaux (plan, 3D). Au plus ~1024 échantillons par côté sont affichés : au-delà, la
/// grille est sous-échantillonnée (le relief source et le terrain final restent à pleine résolution).
fn maillages_terrain(t: &Relief, eau: Option<f64>) -> Vec<(Mesh, Mesh)> {
    let mut v = Vec::new();
    if t.nx < 2 || t.nz < 2 {
        return v;
    }
    let pas = t.nx.max(t.nz).div_ceil(1024).max(1);
    let xs: Vec<usize> = (0..t.nx)
        .step_by(pas)
        .chain((!(t.nx - 1).is_multiple_of(pas)).then_some(t.nx - 1))
        .collect();
    let zs: Vec<usize> = (0..t.nz)
        .step_by(pas)
        .chain((!(t.nz - 1).is_multiple_of(pas)).then_some(t.nz - 1))
        .collect();
    const MORCEAU: usize = 128;
    let mut z0 = 0;
    while z0 + 1 < zs.len() {
        let z1 = (z0 + MORCEAU).min(zs.len() - 1);
        let mut x0 = 0;
        while x0 + 1 < xs.len() {
            let x1 = (x0 + MORCEAU).min(xs.len() - 1);
            let (nx, nz) = (x1 - x0 + 1, z1 - z0 + 1);
            let mut pos = Vec::with_capacity(nx * nz);
            let mut nor = Vec::with_capacity(nx * nz);
            let mut col_plan = Vec::with_capacity(nx * nz);
            let mut col_3d = Vec::with_capacity(nx * nz);
            let mut uv = Vec::with_capacity(nx * nz);
            let mut tan = Vec::with_capacity(nx * nz);
            for &iz in &zs[z0..=z1] {
                for &ix in &xs[x0..=x1] {
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
            v.push((fabriquer(col_plan), trois_d));
            x0 = x1;
        }
        z0 = z1;
    }
    v
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
fn bloc(plan: &mut Tampon, toits: &mut Tampon, murs: &mut Tampon, b: &mcf_carte::Batiment, terrain: &Relief) {
    if b.empreinte.len() < 3 {
        return;
    }
    let hs: Vec<f64> = b.empreinte.iter().map(|p| terrain.hauteur(*p)).collect();
    let bas = hs.iter().copied().fold(f64::INFINITY, f64::min) - 0.3;
    let sol = terrain.hauteur(b.centre()).max(bas + 0.3);
    let haut = sol + b.hauteur();
    let toit = mcf_geo::maillage_polygone(&b.empreinte, |_| haut);
    plan.ajouter(&toit, lin([150, 140, 132], 1.0));
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
