//! Peuple un projet de carte à partir d'un plan extrait d'une carte couleurs (`mcf/tools/carte/extraire_plan.py`) :
//! relief retravaillé (massif plus marqué, collines, plateaux sous les villes), mer, routes, quartiers et bâti en
//! blocs. Chaque élément passe par l'API validée de `mcf_carte` ; le projet est enregistré après chaque phase.
//!
//! ```text
//! cargo run -p mcf_carte --example peupler_carte --release -- <projet> <plan.json> [stats|relief|routes|quartiers|bati|tout]
//! ```
//!
//! Les bâtiments déjà posés à la main (modélisés ou déplacés) ne sont jamais touchés : `lotir` ne remplace que le bâti
//! généré de chaque quartier. Les emplacements sont **plausibles** (lus sur le plan illustré), pas un relevé réel.

use std::path::PathBuf;
use std::time::Instant;

use mcf_carte::relief::fractal;
use mcf_carte::{Carte, Densite, Famille};
use mcf_geo::ClasseRoute;
use serde::Deserialize;

#[derive(Deserialize)]
struct Plan {
    routes: Vec<Route>,
    zones: Vec<Zone>,
    /// Lacs (contours) : creusés sous le niveau de l'eau.
    #[serde(default)]
    lacs: Vec<Vec<[f64; 2]>>,
    niveau_eau_m: f64,
}

#[derive(Deserialize)]
struct Route {
    classe: ClasseRoute,
    points: Vec<[f64; 2]>,
}

#[derive(Deserialize)]
struct Zone {
    famille: Famille,
    densite: Densite,
    points: Vec<[f64; 2]>,
}

/// Grille grossière (pas `PAS` échantillons de relief) pour les masques et les flous.
const PAS: usize = 4;

struct Grille {
    nx: usize,
    nz: usize,
    v: Vec<f32>,
}

impl Grille {
    fn new(nx: usize, nz: usize) -> Self {
        Self {
            nx,
            nz,
            v: vec![0.0; nx * nz],
        }
    }
    fn at(&self, x: usize, z: usize) -> f32 {
        self.v[z * self.nx + x]
    }
    /// Valeur bilinéaire en coordonnées de grille.
    fn bilin(&self, fx: f64, fz: f64) -> f32 {
        let fx = fx.clamp(0.0, (self.nx - 1) as f64);
        let fz = fz.clamp(0.0, (self.nz - 1) as f64);
        let (x0, z0) = (fx.floor() as usize, fz.floor() as usize);
        let (x1, z1) = ((x0 + 1).min(self.nx - 1), (z0 + 1).min(self.nz - 1));
        let (tx, tz) = ((fx - x0 as f64) as f32, (fz - z0 as f64) as f32);
        let a = self.at(x0, z0) + (self.at(x1, z0) - self.at(x0, z0)) * tx;
        let b = self.at(x0, z1) + (self.at(x1, z1) - self.at(x0, z1)) * tx;
        a + (b - a) * tz
    }
    /// Flou par boîtes (3 passes ≈ gaussien), rayon en cases.
    fn flou(&mut self, r: usize) {
        for _ in 0..3 {
            self.boite(r, true);
            self.boite(r, false);
        }
    }
    fn boite(&mut self, r: usize, en_x: bool) {
        let (n, m) = if en_x {
            (self.nx, self.nz)
        } else {
            (self.nz, self.nx)
        };
        let idx = |i: usize, j: usize| if en_x { j * self.nx + i } else { i * self.nx + j };
        let mut out = vec![0.0f32; self.v.len()];
        let mut ligne = vec![0.0f64; n + 1];
        for j in 0..m {
            for i in 0..n {
                ligne[i + 1] = ligne[i] + f64::from(self.v[idx(i, j)]);
            }
            for i in 0..n {
                let a = i.saturating_sub(r);
                let b = (i + r + 1).min(n);
                out[idx(i, j)] = ((ligne[b] - ligne[a]) / (b - a) as f64) as f32;
            }
        }
        self.v = out;
    }
}

/// Remplit un polygone (coordonnées de grille) à `valeur` (règle pair-impair, centres de cases).
fn remplir(g: &mut Grille, poly: &[[f64; 2]], valeur: f32) {
    let zmin = poly
        .iter()
        .map(|p| p[1])
        .fold(f64::MAX, f64::min)
        .floor()
        .max(0.0) as usize;
    let zmax = (poly.iter().map(|p| p[1]).fold(f64::MIN, f64::max).ceil() as usize).min(g.nz - 1);
    let mut xs = Vec::new();
    for z in zmin..=zmax {
        let y = z as f64 + 0.5;
        xs.clear();
        for i in 0..poly.len() {
            let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
            if (a[1] <= y) != (b[1] <= y) {
                xs.push(a[0] + (y - a[1]) / (b[1] - a[1]) * (b[0] - a[0]));
            }
        }
        xs.sort_by(f64::total_cmp);
        for p in xs.chunks(2) {
            if let [x0, x1] = p {
                let a = (x0 - 0.5).ceil().max(0.0) as usize;
                let b = ((x1 - 0.5).floor() as i64).min(g.nx as i64 - 1);
                for x in a as i64..=b {
                    g.v[z * g.nx + x as usize] = valeur;
                }
            }
        }
    }
}

fn quantiles(mut v: Vec<f32>) -> String {
    if v.is_empty() {
        return "—".into();
    }
    v.sort_by(f32::total_cmp);
    let q = |t: f64| v[((v.len() - 1) as f64 * t) as usize];
    format!(
        "min {:.0} | 10 % {:.0} | 50 % {:.0} | 90 % {:.0} | 99 % {:.0} | max {:.0} m",
        q(0.0),
        q(0.1),
        q(0.5),
        q(0.9),
        q(0.99),
        q(1.0)
    )
}

fn stats(c: &Carte) {
    let r = &c.relief;
    let terre: Vec<f32> = r.h.iter().step_by(37).copied().filter(|h| *h > 0.5).collect();
    let mer = r.h.iter().step_by(37).filter(|h| **h <= 0.5).count() as f64 / (r.h.len() / 37) as f64;
    println!(
        "relief {} × {} à {} m ; part ≤ 0,5 m : {:.0} %",
        r.nx,
        r.nz,
        r.resolution,
        mer * 100.0
    );
    println!("terre : {}", quantiles(terre));
    for (nom, p) in [
        ("lac central", [-1210.0, -3250.0]),
        ("lac nord-ouest", [-6150.0, -8550.0]),
        ("massif (neige)", [-9100.0, -13400.0]),
        ("ville centrale", [3500.0, 3800.0]),
        ("ville sud-ouest", [-11000.0, 4700.0]),
        ("plaine", [-2000.0, 1500.0]),
    ] {
        println!("  {nom:<16} {:.1} m", r.hauteur(p));
    }
}

/// Relief : massif plus marqué, collines dans la campagne, plateaux sous les villes. Écrit directement dans le
/// relief source (une seule passe sur toute la carte), toutes les tuiles marquées à réécrire.
fn relief(c: &mut Carte, plan: &Plan) {
    let (amin, amax) = (
        c.manifeste.altitude_min_m as f32,
        c.manifeste.altitude_max_m as f32,
    );
    let graine = c.manifeste.graine;
    let eau = plan.niveau_eau_m as f32;
    let r = &mut c.relief;
    let (gx, gz) = (r.nx.div_ceil(PAS), r.nz.div_ceil(PAS));
    let cell = r.resolution * PAS as f64;
    let vers_grille = |p: [f64; 2]| [(p[0] - r.origine[0]) / cell, (p[1] - r.origine[1]) / cell];

    // Masque des villes (1 dedans), adouci : bord de plateau en pente douce sur ~150 m.
    let mut villes = Grille::new(gx, gz);
    for z in &plan.zones {
        let poly: Vec<[f64; 2]> = z.points.iter().map(|p| vers_grille(*p)).collect();
        remplir(&mut villes, &poly, 1.0);
    }
    let mut masque = Grille {
        nx: gx,
        nz: gz,
        v: villes.v.clone(),
    };
    masque.flou((60.0 / cell).ceil() as usize);

    // Terre grossière (pour la distance à la côte et le relief lissé).
    let mut grossier = Grille::new(gx, gz);
    for z in 0..gz {
        for x in 0..gx {
            grossier.v[z * gx + x] = r.h[r.index((x * PAS).min(r.nx - 1), (z * PAS).min(r.nz - 1))];
        }
    }
    // Part de terre lissée sur ~400 m : 0 près de la côte, 1 à l'intérieur.
    let mut interieur = Grille {
        nx: gx,
        nz: gz,
        v: grossier
            .v
            .iter()
            .map(|h| f32::from(u8::from(*h > eau + 0.5)))
            .collect(),
    };
    interieur.flou((250.0 / cell).ceil() as usize);
    // Relief lissé sur ~500 m : altitude cible des plateaux.
    let mut lisse = Grille {
        nx: gx,
        nz: gz,
        v: grossier.v.clone(),
    };
    lisse.flou((350.0 / cell).ceil() as usize);
    // Éloignement des villes sur ~1,5 km : les collines naissent hors des villes et de leurs abords.
    let mut abords = Grille {
        nx: gx,
        nz: gz,
        v: villes.v.clone(),
    };
    abords.flou((600.0 / cell).ceil() as usize);

    // Lacs : fond à 6 m sous l'eau, berges en pente sur ~200 m.
    let mut lacs = Grille::new(gx, gz);
    for l in &plan.lacs {
        let poly: Vec<[f64; 2]> = l.iter().map(|p| vers_grille(*p)).collect();
        remplir(&mut lacs, &poly, 1.0);
    }
    lacs.flou((80.0 / cell).ceil() as usize);

    // Collines nommées : quelques buttes franches dans la campagne (centre, rayon m, hauteur m).
    let buttes: [([f64; 2], f64, f32); 7] = [
        ([-6500.0, -1500.0], 900.0, 70.0),
        ([-3000.0, 6500.0], 700.0, 45.0),
        ([9500.0, 1200.0], 800.0, 55.0),
        ([-9500.0, -4500.0], 1100.0, 80.0),
        ([1500.0, 10500.0], 900.0, 60.0),
        ([-13000.0, -1500.0], 800.0, 50.0),
        ([5200.0, -8800.0], 1000.0, 65.0),
    ];

    let debut = Instant::now();
    for iz in 0..r.nz {
        for ix in 0..r.nx {
            let i = r.index(ix, iz);
            let h0 = r.h[i];
            if h0 <= eau + 0.5 {
                continue; // mer, lacs creusés : inchangés
            }
            let p = r.position(ix, iz);
            let (fx, fz) = ((ix as f64) / PAS as f64, (iz as f64) / PAS as f64);
            let terre = interieur.bilin(fx, fz).clamp(0.0, 1.0);
            let loin_ville = 1.0 - abords.bilin(fx, fz).clamp(0.0, 1.0) * 1.6;
            let loin_ville = loin_ville.clamp(0.0, 1.0);
            let mut h = h0;
            // Massif : au-dessus de 180 m, il se redresse (398 m → ~1 150 m) et des arêtes apparaissent.
            if h > 180.0 {
                let e = h - 180.0;
                let arete = 1.0 - (fractal(graine ^ 0xA2E7, p, 900.0) as f32).abs();
                h = 180.0 + e * 1.8 + e * e / 80.0 + arete * (e / 150.0).min(1.0) * 110.0;
            }
            // Collines : ondulations de ~2 km, plus fortes loin des villes et de la côte.
            let onde = fractal(graine ^ 0x5EED, p, 2200.0) as f32;
            h += onde * 28.0 * terre * loin_ville;
            for (centre, rayon, haut) in buttes {
                let d = ((p[0] - centre[0]).powi(2) + (p[1] - centre[1]).powi(2)).sqrt() / rayon;
                if d < 1.0 {
                    let t = (1.0 - d * d) as f32;
                    h += haut * t * t * loin_ville * terre;
                }
            }
            // Plateaux : sous les villes, le sol rejoint le relief lissé (jamais sous l'eau).
            let w = masque.bilin(fx, fz).clamp(0.0, 1.0);
            if w > 0.0 {
                let cible = lisse.bilin(fx, fz).max(eau + 2.5);
                let w = w * w * (3.0 - 2.0 * w);
                h += (cible - h) * w;
            }
            // La côte reste où elle est : la terre ne passe jamais sous l'eau, sauf dans les lacs.
            h = h.max(eau + 0.6);
            let l = lacs.bilin(fx, fz).clamp(0.0, 1.0);
            if l > 0.0 {
                let t = (l * 1.6).min(1.0);
                let t = t * t * (3.0 - 2.0 * t);
                h += (eau - 6.0 - h) * t;
            }
            r.h[i] = h.clamp(amin, amax);
        }
    }
    let (tx, tz) = c.manifeste.tuiles();
    c.sale
        .relief
        .extend((0..tz).flat_map(|z| (0..tx).map(move |x| (x, z))));
    c.revisions.relief += 1;
    println!("relief retravaillé en {:?}", debut.elapsed());
}

fn routes(c: &mut Carte, plan: &Plan) {
    // Les grandes voies d'abord : les petites viennent s'y accrocher.
    let ordre = [
        ClasseRoute::Autoroute,
        ClasseRoute::Route,
        ClasseRoute::Rue,
        ClasseRoute::CheminTerre,
    ];
    let debut = Instant::now();
    for classe in ordre {
        let (mut ok, mut refus) = (0, 0);
        let mut premier_refus = None;
        for r in plan.routes.iter().filter(|r| r.classe == classe) {
            match c.tracer_route(classe, &r.points, 20.0) {
                Ok(_) => ok += 1,
                Err(e) => {
                    refus += 1;
                    premier_refus.get_or_insert(e);
                }
            }
        }
        println!(
            "{:<14} {ok} tracée(s), {refus} refusée(s){}",
            classe.nom(),
            premier_refus.map(|e| format!(" (ex. : {e})")).unwrap_or_default()
        );
    }
    println!("routes en {:?}", debut.elapsed());
}

fn quartiers(c: &mut Carte, plan: &Plan) {
    let debut = Instant::now();
    let mut ids = Vec::new();
    let mut refus = 0;
    for z in &plan.zones {
        match c.ajouter_zone(z.points.clone(), z.famille, z.densite) {
            Ok(id) => ids.push(id),
            Err(_) => refus += 1,
        }
    }
    println!("{} zone(s), {refus} refusée(s)", ids.len());
    let mut n = 0;
    for id in ids {
        n += c.lotir(id).unwrap_or(0);
    }
    println!(
        "{n} bâtiment(s) en blocs ont poussé en {:?} ; total {}",
        debut.elapsed(),
        c.batiments.len()
    );
}

/// Choix d'un modèle selon l'usage du quartier : (fiche, poids). Les rues de pavillons restent aérées.
fn modeles_pour(famille: Famille, densite: Densite) -> &'static [(&'static str, u32)] {
    use Densite::*;
    use Famille::*;
    match (famille, densite) {
        (Residentiel, Faible) => &[("maison_a", 4), ("maison_b", 3), ("maison_c", 4)],
        (Residentiel, Moyenne) => &[
            ("immeuble_petit", 4),
            ("immeuble_moyen", 3),
            ("maison_b", 2),
            ("maison_c", 1),
        ],
        (Residentiel, Forte) => &[
            ("immeuble_moyen", 4),
            ("immeuble_grand", 4),
            ("cite_barre", 1),
            ("cite_tour", 1),
        ],
        (Residentiel, Centre) => &[
            ("immeuble_grand", 4),
            ("tour_appartements", 2),
            ("tour_bureaux", 2),
            ("hotel_affaires", 1),
        ],
        (Commerce, _) => &[
            ("superette", 3),
            ("cite_commerces", 2),
            ("immeuble_petit", 2),
            ("immeuble_moyen", 1),
        ],
        (Activite, _) => &[("usine", 2), ("garage", 3), ("hangar_ouvert", 3)],
        (Services, _) => &[("immeuble_moyen", 1)],
        _ => &[],
    }
}

/// Part des places le long d'une rue qui reçoivent un bâtiment.
fn remplissage(densite: Densite) -> f64 {
    match densite {
        Densite::Faible => 0.45,
        Densite::Moyenne => 0.6,
        Densite::Forte => 0.7,
        Densite::Centre => 0.8,
    }
}

/// Retrait de la façade derrière le bord de l'emprise (m) et écart entre voisins (m).
fn retrait_ecart(famille: Famille, densite: Densite) -> (f64, f64) {
    match (famille, densite) {
        (Famille::Residentiel, Densite::Faible) => (6.0, 7.0),
        (Famille::Activite, _) => (10.0, 14.0),
        (_, Densite::Centre) => (1.5, 3.0),
        _ => (3.0, 4.0),
    }
}

/// Petit générateur déterministe (xorshift).
struct Hasard(u64);
impl Hasard {
    fn suivant(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
    fn choisir<'a>(&mut self, liste: &'a [(&'a str, u32)]) -> Option<&'a str> {
        let total: u32 = liste.iter().map(|x| x.1).sum();
        if total == 0 {
            return None;
        }
        let mut t = (self.suivant() * f64::from(total)) as u32;
        for (n, p) in liste {
            if t < *p {
                return Some(n);
            }
            t -= p;
        }
        liste.last().map(|x| x.0)
    }
}

/// Retire le bâti généré (blocs) dont le quartier n'existe plus ; garde les bâtiments posés à la main et ceux des
/// quartiers présents (dessinés par l'auteur).
fn vider_bati(c: &mut Carte) -> Result<usize, String> {
    let q = &c.quartiers;
    let ids: Vec<mcf_carte::Id> = c
        .batiments
        .iter()
        .filter(|(_, b)| b.origine == mcf_carte::Origine::Genere)
        .filter(|(_, b)| {
            b.affectation
                .is_none_or(|a| !q.zones.contains_key(&a) && !q.affectations.contains_key(&a))
        })
        .map(|(i, _)| *i)
        .collect();
    let n = ids.len();
    c.modifier("Retirer le bâti en blocs", |tx| {
        for i in &ids {
            tx.retirer_batiment(*i);
        }
        Ok(())
    })?;
    Ok(n)
}

/// Pose des bâtiments modélisés le long des rues et routes des quartiers du plan (façade vers la rue).
fn bati(c: &mut Carte, plan: &Plan, catalogue: &mcf_carte::CatalogueBatiments) -> Result<(), String> {
    use mcf_carte::grille::{Grille, boite};
    use mcf_geo::polygone;
    let debut = Instant::now();
    let retires = vider_bati(c)?;
    println!("{retires} bloc(s) retiré(s)");
    // Quartiers du plan : usage et densité en un point.
    let mut index_zones = Grille::new(256.0);
    for z in &plan.zones {
        index_zones.ajouter(boite(&z.points));
    }
    let zone_en = |p: [f64; 2]| {
        index_zones
            .proches(&[p, p])
            .into_iter()
            .map(|i| &plan.zones[i as usize])
            .find(|z| polygone::contient(&z.points, p))
            .map(|z| (z.famille, z.densite))
    };
    let emprises = c.derive().geo.polygones_emprise();
    let index_routes = Grille::de_polygones(&emprises, 64.0);
    let mut poses: Vec<Vec<[f64; 2]>> = c.batiments.values().map(|b| b.empreinte.clone()).collect();
    let mut index_poses = Grille::de_polygones(&poses, 64.0);
    let eau = c.manifeste.niveau_eau_m.unwrap_or(f64::MIN);
    let profils = c.manifeste.profils;
    let mut hasard = Hasard(c.manifeste.graine | 1);
    let troncons: Vec<_> = c
        .reseau
        .troncons
        .values()
        .filter(|t| {
            matches!(
                t.classe,
                ClasseRoute::Rue | ClasseRoute::Route | ClasseRoute::Avenue | ClasseRoute::Ruelle
            )
        })
        .filter_map(|t| c.reseau.controle(t).map(|(pts, _)| (t.classe, pts)))
        .collect();
    let mut a_poser: Vec<(String, [f64; 2], f64)> = Vec::new();
    for (classe, pts) in &troncons {
        let demi = profils.get(*classe).emprise_m() / 2.0;
        for seg in pts.windows(2) {
            let (a, b) = (seg[0], seg[1]);
            let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
            let long = dx.hypot(dz);
            if long < 8.0 {
                continue;
            }
            let (ux, uz) = (dx / long, dz / long);
            for cote in [-1.0, 1.0] {
                // Normale vers ce côté de la rue.
                let (nx, nz) = (-uz * cote, ux * cote);
                let mut s = 6.0;
                while s < long - 6.0 {
                    let pied = [a[0] + ux * s, a[1] + uz * s];
                    let sonde = [pied[0] + nx * (demi + 12.0), pied[1] + nz * (demi + 12.0)];
                    let Some((famille, densite)) = zone_en(sonde) else {
                        s += 25.0;
                        continue;
                    };
                    let (retrait, ecart) = retrait_ecart(famille, densite);
                    let Some(f) = hasard
                        .choisir(modeles_pour(famille, densite))
                        .and_then(|id| catalogue.fiche(id))
                    else {
                        s += 25.0;
                        continue;
                    };
                    let (larg, prof) = (f.largeur_m, f.profondeur_m);
                    if s + larg / 2.0 > long - 4.0 {
                        break;
                    }
                    let centre_le_long = s + larg / 2.0;
                    let recul = demi + retrait + prof / 2.0;
                    let centre = [
                        a[0] + ux * centre_le_long + nx * recul,
                        a[1] + uz * centre_le_long + nz * recul,
                    ];
                    // Façade (+Z du modèle) tournée vers la rue.
                    let cap = (-nx).atan2(-nz).to_degrees();
                    let b = f.batiment(centre, cap);
                    let marge = polygone::reduit(&b.empreinte, -1.0);
                    let bt = boite(&marge);
                    let libre_route = !index_routes
                        .proches(&bt)
                        .into_iter()
                        .any(|i| polygone::se_chevauchent(&marge, &emprises[i as usize]));
                    let libre_bati = !index_poses
                        .proches(&bt)
                        .into_iter()
                        .any(|i| polygone::se_chevauchent(&marge, &poses[i as usize]));
                    let hs: Vec<f64> = b.empreinte.iter().map(|p| c.relief.hauteur(*p)).collect();
                    let (hmin, hmax) = hs
                        .iter()
                        .fold((f64::MAX, f64::MIN), |(lo, hi), h| (lo.min(*h), hi.max(*h)));
                    let sec = hmin > eau + 0.8 && hmax - hmin < 3.5;
                    let dedans = b.empreinte.iter().all(|p| c.manifeste.contient(*p));
                    let garde = hasard.suivant() < remplissage(densite);
                    if libre_route && libre_bati && sec && dedans {
                        if garde {
                            index_poses.ajouter(boite(&b.empreinte));
                            poses.push(b.empreinte.clone());
                            a_poser.push((f.id.clone(), centre, cap));
                        }
                        s += larg + ecart;
                    } else {
                        s += 4.0;
                    }
                }
            }
        }
    }
    println!("{} bâtiment(s) à poser ({:?})", a_poser.len(), debut.elapsed());
    let mut par_fiche = std::collections::BTreeMap::new();
    c.modifier("Poser les bâtiments de la bibliothèque", |tx| {
        for (id, centre, cap) in &a_poser {
            if let Some(f) = catalogue.fiche(id) {
                tx.poser_batiment(mcf_carte::Id::nouveau(), f.batiment(*centre, *cap));
                *par_fiche.entry(id.clone()).or_insert(0) += 1;
            }
        }
        Ok(())
    })?;
    println!(
        "{} bâtiment(s) modélisé(s) posé(s) en {:?} : {par_fiche:?}",
        a_poser.len(),
        debut.elapsed()
    );
    Ok(())
}

fn enregistrer(c: &mut Carte, phase: &str) -> Result<(), String> {
    let debut = Instant::now();
    c.enregistrer(None)?;
    println!("— enregistré après « {phase} » ({:?})", debut.elapsed());
    Ok(())
}

fn main() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let usage = "usage : peupler_carte <projet> <plan.json> [stats|relief|routes|quartiers|bati|tout]";
    let projet = PathBuf::from(args.next().ok_or(usage)?);
    let plan = PathBuf::from(args.next().ok_or(usage)?);
    let quoi = args.next().unwrap_or_else(|| "stats".into());
    let plan: Plan = serde_json::from_slice(&std::fs::read(&plan).map_err(|e| e.to_string())?)
        .map_err(|e| format!("plan : {e}"))?;
    let debut = Instant::now();
    let mut c = Carte::ouvrir(&projet)?;
    println!(
        "« {} » ouverte en {:?} : {} tronçons, {} bâtiments, {} zones",
        c.manifeste.nom,
        debut.elapsed(),
        c.reseau.troncons.len(),
        c.batiments.len(),
        c.quartiers.zones.len()
    );
    stats(&c);
    let tout = quoi == "tout";
    if tout || quoi == "relief" {
        c.modifier("Niveau de la mer", |tx| {
            tx.manifeste_mut().niveau_eau_m = Some(plan.niveau_eau_m);
            Ok(())
        })?;
        relief(&mut c, &plan);
        stats(&c);
        enregistrer(&mut c, "relief")?;
    }
    if tout || quoi == "routes" {
        routes(&mut c, &plan);
        enregistrer(&mut c, "routes")?;
    }
    if tout || quoi == "quartiers" {
        quartiers(&mut c, &plan);
        enregistrer(&mut c, "quartiers")?;
    }
    if quoi == "bati" {
        let chemin = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../mcf_editor/assets/batiments/catalogue.json");
        let texte = std::fs::read_to_string(&chemin).map_err(|e| format!("{} : {e}", chemin.display()))?;
        let catalogue = mcf_carte::CatalogueBatiments::lire(&texte)?;
        bati(&mut c, &plan, &catalogue)?;
        enregistrer(&mut c, "bâti")?;
    }
    Ok(())
}
