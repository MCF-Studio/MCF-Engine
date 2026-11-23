//! Relief : grille d'altitudes (source sculptée), pinceaux, profils en long des routes et terrain final terrassé.
//! Le terrain final est **dérivé** (source + routes) : retracer une route ne détruit jamais le relief sculpté.

use std::collections::BTreeMap;

use mcf_geo::{P2, TableProfils, distance};

use crate::geometrie::GeoReseau;
use crate::ids::{Id, melanger};
use crate::manifeste::Manifeste;

/// Grille régulière d'altitudes.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Relief {
    /// Échantillons en X.
    pub nx: usize,
    /// Échantillons en Z.
    pub nz: usize,
    /// Distance entre échantillons (m).
    pub resolution: f64,
    /// Coin `(x, z)` minimal.
    pub origine: P2,
    /// Altitudes, ligne par ligne (Z puis X).
    pub h: Vec<f32>,
}

/// Rectangle d'échantillons `[x0, x1) × [z0, z1)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Zone {
    /// X début.
    pub x0: usize,
    /// Z début.
    pub z0: usize,
    /// X fin (exclu).
    pub x1: usize,
    /// Z fin (exclu).
    pub z1: usize,
}

impl Zone {
    /// Vide ?
    pub fn vide(&self) -> bool {
        self.x0 >= self.x1 || self.z0 >= self.z1
    }

    /// Union.
    pub fn union(self, o: Zone) -> Zone {
        if self.vide() {
            return o;
        }
        if o.vide() {
            return self;
        }
        Zone {
            x0: self.x0.min(o.x0),
            z0: self.z0.min(o.z0),
            x1: self.x1.max(o.x1),
            z1: self.z1.max(o.z1),
        }
    }
}

/// Outils de sculpture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pinceau {
    /// Monter.
    Lever,
    /// Creuser.
    Abaisser,
    /// Adoucir.
    Lisser,
    /// Ramener à une altitude cible.
    Aplanir,
    /// Relief naturel (bruit fractal).
    Naturel,
}

impl Pinceau {
    /// Tous les pinceaux.
    pub const TOUS: [Self; 5] = [
        Self::Lever,
        Self::Abaisser,
        Self::Lisser,
        Self::Aplanir,
        Self::Naturel,
    ];

    /// Nom affiché.
    pub const fn nom(self) -> &'static str {
        match self {
            Self::Lever => "Lever",
            Self::Abaisser => "Abaisser",
            Self::Lisser => "Lisser",
            Self::Aplanir => "Aplanir",
            Self::Naturel => "Relief naturel",
        }
    }
}

fn lisse(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Bruit de valeur 2D dans `[0, 1]`, déterministe.
fn bruit(graine: u64, x: f64, z: f64) -> f64 {
    let (ix, iz) = (x.floor(), z.floor());
    let (fx, fz) = (lisse(x - ix), lisse(z - iz));
    let v = |a: f64, b: f64| {
        let mut s = graine
            ^ ((a as i64 as u64).wrapping_mul(0x9E37_79B9))
            ^ ((b as i64 as u64).wrapping_mul(0x85EB_CA6B) << 1);
        (melanger(&mut s) >> 11) as f64 / (1u64 << 53) as f64
    };
    let a = v(ix, iz) + (v(ix + 1.0, iz) - v(ix, iz)) * fx;
    let b = v(ix, iz + 1.0) + (v(ix + 1.0, iz + 1.0) - v(ix, iz + 1.0)) * fx;
    a + (b - a) * fz
}

/// Bruit fractal dans `[-1, 1]` (longueur d'onde de base `onde` m).
pub fn fractal(graine: u64, p: P2, onde: f64) -> f64 {
    let (mut somme, mut amp, mut freq, mut total) = (0.0, 1.0, 1.0 / onde, 0.0);
    for octave in 0..5u64 {
        somme += amp * bruit(graine.wrapping_add(octave), p[0] * freq, p[1] * freq);
        total += amp;
        amp *= 0.5;
        freq *= 2.0;
    }
    (somme / total) * 2.0 - 1.0
}

impl Relief {
    /// Relief plat à l'altitude donnée, aux dimensions du manifeste.
    pub fn plat(m: &Manifeste, altitude: f64) -> Self {
        let (nx, nz) = m.echantillons();
        Self {
            nx,
            nz,
            resolution: m.resolution_relief_m,
            origine: m.bornes().0,
            h: vec![altitude as f32; nx * nz],
        }
    }

    /// Indice d'un échantillon.
    pub fn index(&self, ix: usize, iz: usize) -> usize {
        iz * self.nx + ix
    }

    /// Position d'un échantillon.
    pub fn position(&self, ix: usize, iz: usize) -> P2 {
        [
            self.origine[0] + ix as f64 * self.resolution,
            self.origine[1] + iz as f64 * self.resolution,
        ]
    }

    /// Altitude interpolée (bornée aux bords).
    pub fn hauteur(&self, p: P2) -> f64 {
        if self.nx == 0 || self.nz == 0 {
            return 0.0;
        }
        let fx = ((p[0] - self.origine[0]) / self.resolution).clamp(0.0, (self.nx - 1) as f64);
        let fz = ((p[1] - self.origine[1]) / self.resolution).clamp(0.0, (self.nz - 1) as f64);
        let (x0, z0) = (fx.floor() as usize, fz.floor() as usize);
        let (x1, z1) = ((x0 + 1).min(self.nx - 1), (z0 + 1).min(self.nz - 1));
        let (tx, tz) = (fx - x0 as f64, fz - z0 as f64);
        let g = |x: usize, z: usize| f64::from(self.h[self.index(x, z)]);
        let a = g(x0, z0) + (g(x1, z0) - g(x0, z0)) * tx;
        let b = g(x0, z1) + (g(x1, z1) - g(x0, z1)) * tx;
        a + (b - a) * tz
    }

    /// Normale du terrain (Y vers le haut).
    pub fn normale(&self, p: P2) -> [f64; 3] {
        let e = self.resolution;
        let dx = self.hauteur([p[0] + e, p[1]]) - self.hauteur([p[0] - e, p[1]]);
        let dz = self.hauteur([p[0], p[1] + e]) - self.hauteur([p[0], p[1] - e]);
        let n = [-dx, 2.0 * e, -dz];
        let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        [n[0] / l, n[1] / l, n[2] / l]
    }

    /// Zone d'échantillons couvrant un disque.
    pub fn zone_disque(&self, c: P2, r: f64) -> Zone {
        let f = |v: f64, o: f64, n: usize| (((v - o) / self.resolution).floor().max(0.0) as usize).min(n);
        let g =
            |v: f64, o: f64, n: usize| ((((v - o) / self.resolution).ceil() + 1.0).max(0.0) as usize).min(n);
        Zone {
            x0: f(c[0] - r, self.origine[0], self.nx),
            z0: f(c[1] - r, self.origine[1], self.nz),
            x1: g(c[0] + r, self.origine[0], self.nx),
            z1: g(c[1] + r, self.origine[1], self.nz),
        }
    }

    /// Copie des altitudes d'une zone.
    pub fn copier(&self, z: Zone) -> Vec<f32> {
        let mut v = Vec::with_capacity((z.x1 - z.x0) * (z.z1 - z.z0));
        for iz in z.z0..z.z1 {
            let i = self.index(z.x0, iz);
            v.extend_from_slice(&self.h[i..i + (z.x1 - z.x0)]);
        }
        v
    }

    /// Remet des altitudes copiées par `copier`.
    pub fn coller(&mut self, z: Zone, valeurs: &[f32]) {
        let w = z.x1 - z.x0;
        for (k, iz) in (z.z0..z.z1).enumerate() {
            let i = self.index(z.x0, iz);
            self.h[i..i + w].copy_from_slice(&valeurs[k * w..(k + 1) * w]);
        }
    }

    /// Applique un coup de pinceau. `force` : mètres par coup (lever, abaisser, naturel) ou fraction 0–1 (lisser,
    /// aplanir). Renvoie la zone modifiée.
    #[allow(clippy::too_many_arguments)]
    pub fn pinceau(
        &mut self,
        outil: Pinceau,
        centre: P2,
        rayon: f64,
        force: f64,
        cible: f64,
        graine: u64,
        bornes: (f64, f64),
    ) -> Zone {
        if !(rayon.is_finite() && force.is_finite() && cible.is_finite()) || rayon <= 0.0 {
            return Zone {
                x0: 0,
                z0: 0,
                x1: 0,
                z1: 0,
            };
        }
        let zone = self.zone_disque(centre, rayon);
        let avant = if outil == Pinceau::Lisser {
            Some(self.clone_zone_voisine(zone))
        } else {
            None
        };
        for iz in zone.z0..zone.z1 {
            for ix in zone.x0..zone.x1 {
                let p = self.position(ix, iz);
                let d = distance(p, centre);
                if d >= rayon {
                    continue;
                }
                let w = lisse(1.0 - d / rayon);
                let i = self.index(ix, iz);
                let h = f64::from(self.h[i]);
                let nouveau = match outil {
                    Pinceau::Lever => h + force * w,
                    Pinceau::Abaisser => h - force * w,
                    Pinceau::Aplanir => h + (cible - h) * (force.clamp(0.0, 1.0) * w),
                    Pinceau::Naturel => h + force * w * fractal(graine, p, rayon.max(8.0)),
                    Pinceau::Lisser => {
                        let moy = avant
                            .as_ref()
                            .map(|(g, z0)| moyenne_3x3(g, z0, ix, iz))
                            .unwrap_or(h);
                        h + (moy - h) * (force.clamp(0.0, 1.0) * w)
                    }
                };
                self.h[i] = nouveau.clamp(bornes.0, bornes.1) as f32;
            }
        }
        zone
    }

    fn clone_zone_voisine(&self, z: Zone) -> (Relief, Zone) {
        let zz = Zone {
            x0: z.x0.saturating_sub(1),
            z0: z.z0.saturating_sub(1),
            x1: (z.x1 + 1).min(self.nx),
            z1: (z.z1 + 1).min(self.nz),
        };
        let r = Relief {
            nx: zz.x1 - zz.x0,
            nz: zz.z1 - zz.z0,
            resolution: self.resolution,
            origine: self.position(zz.x0, zz.z0),
            h: self.copier(zz),
        };
        (r, zz)
    }
}

fn moyenne_3x3(g: &Relief, z0: &Zone, ix: usize, iz: usize) -> f64 {
    let (lx, lz) = (ix as i64 - z0.x0 as i64, iz as i64 - z0.z0 as i64);
    let (mut s, mut n) = (0.0, 0.0);
    for dz in -1..=1 {
        for dx in -1..=1 {
            let (x, z) = (lx + dx, lz + dz);
            if x >= 0 && z >= 0 && (x as usize) < g.nx && (z as usize) < g.nz {
                s += f64::from(g.h[g.index(x as usize, z as usize)]);
                n += 1.0;
            }
        }
    }
    if n > 0.0 { s / n } else { 0.0 }
}

/// Profil en long d'un tronçon : altitudes à intervalles réguliers le long de l'axe complet.
#[derive(Debug, Clone, PartialEq)]
pub struct ProfilLong {
    /// Pas d'échantillonnage (m).
    pub pas: f64,
    /// Altitudes, de `0` à la longueur de l'axe.
    pub h: Vec<f64>,
    /// La pente maximale de la classe a-t-elle forcé un écart notable (> 1 m) avec le terrain ?
    pub pente_contrainte: bool,
}

impl ProfilLong {
    /// Altitude à l'abscisse `d`.
    pub fn altitude(&self, d: f64) -> f64 {
        if self.h.is_empty() {
            return 0.0;
        }
        let f = (d / self.pas).clamp(0.0, (self.h.len() - 1) as f64);
        let i = f.floor() as usize;
        let j = (i + 1).min(self.h.len() - 1);
        self.h[i] + (self.h[j] - self.h[i]) * (f - i as f64)
    }
}

/// Altitudes du réseau : nœuds et profils en long.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AltitudesReseau {
    /// Altitude de chaque nœud (carrefour plat).
    pub noeuds: BTreeMap<Id, f64>,
    /// Profil de chaque tronçon.
    pub troncons: BTreeMap<Id, ProfilLong>,
}

/// Pas d'échantillonnage des profils en long (m).
pub const PAS_PROFIL_M: f64 = 2.0;

/// Calcule les altitudes du réseau sur le relief source : le terrain lissé, ramené aux nœuds aux deux bouts, puis
/// borné par la pente maximale de la classe.
pub fn altitudes_reseau(
    reseau: &crate::reseau::Reseau,
    geo: &GeoReseau,
    relief: &Relief,
    profils: &TableProfils,
) -> AltitudesReseau {
    let mut alt = AltitudesReseau::default();
    for (id, n) in &geo.noeuds {
        let p = n.position;
        let mut s = 0.0;
        for (dx, dz) in [(0.0, 0.0), (5.0, 0.0), (-5.0, 0.0), (0.0, 5.0), (0.0, -5.0)] {
            s += relief.hauteur([p[0] + dx, p[1] + dz]);
        }
        alt.noeuds.insert(*id, s / 5.0);
    }
    for (id, g) in &geo.troncons {
        let Some(t) = reseau.troncons.get(id) else {
            continue;
        };
        let n = ((g.longueur / PAS_PROFIL_M).ceil() as usize).max(1);
        let pas = g.longueur / n as f64;
        let mut h: Vec<f64> = (0..=n)
            .map(|i| relief.hauteur(mcf_geo::point_a(&g.axe, i as f64 * pas).unwrap_or(g.axe[0])))
            .collect();
        let terrain = h.clone();
        let (ha, hb) = (alt.noeuds.get(&t.de).copied(), alt.noeuds.get(&t.vers).copied());
        for _ in 0..4 {
            let prec = h.clone();
            for (i, hi) in h.iter_mut().enumerate().skip(1).take(n - 1) {
                let a = i.saturating_sub(3);
                let b = (i + 3).min(n);
                *hi = prec[a..=b].iter().sum::<f64>() / (b - a + 1) as f64;
            }
            if let Some(v) = ha {
                h[0] = v;
            }
            if let Some(v) = hb {
                h[n] = v;
            }
        }
        let g_max = profils.get(g.classe).pente_max * pas;
        for _ in 0..2 {
            for i in 1..=n {
                h[i] = h[i].clamp(h[i - 1] - g_max, h[i - 1] + g_max);
            }
            for i in (0..n).rev() {
                h[i] = h[i].clamp(h[i + 1] - g_max, h[i + 1] + g_max);
            }
        }
        let contrainte = h
            .iter()
            .zip(&terrain)
            .any(|(a, b)| (a - b).abs() > 1.0 + 2.0 * g_max)
            && h.windows(2).any(|w| ((w[1] - w[0]).abs() - g_max).abs() < 1e-9);
        alt.troncons.insert(
            *id,
            ProfilLong {
                pas,
                h,
                pente_contrainte: contrainte,
            },
        );
    }
    alt
}

/// Pente maximale des talus (largeur horizontale / hauteur).
pub const TALUS: f64 = 2.0;
/// Largeur maximale d'un talus (m).
pub const TALUS_MAX_M: f64 = 30.0;
/// Accotement terrassé de chaque côté de l'emprise (m).
pub const BANQUETTE_M: f64 = 1.0;

/// Terrain final : relief source terrassé sous les routes (plat en travers, talus à 1 pour 2) et sous les
/// carrefours.
pub fn terrain_final(
    source: &Relief,
    geo: &GeoReseau,
    alt: &AltitudesReseau,
    profils: &TableProfils,
) -> Relief {
    let mut sortie = source.clone();
    let mut meilleur = vec![f32::INFINITY; source.h.len()];
    for (id, g) in &geo.troncons {
        let Some(profil) = alt.troncons.get(id) else {
            continue;
        };
        let w = profils.get(g.classe).emprise_m() * 0.5 + BANQUETTE_M;
        let mut abscisse = 0.0;
        for s in g.axe.windows(2) {
            let l = distance(s[0], s[1]);
            // Morceaux de 16 m au plus : rectangles englobants petits même pour une longue ligne droite.
            let morceaux = ((l / 16.0).ceil() as usize).max(1);
            for k in 0..morceaux {
                let (t0, t1) = (k as f64 / morceaux as f64, (k + 1) as f64 / morceaux as f64);
                let a = [
                    s[0][0] + (s[1][0] - s[0][0]) * t0,
                    s[0][1] + (s[1][1] - s[0][1]) * t0,
                ];
                let b = [
                    s[0][0] + (s[1][0] - s[0][0]) * t1,
                    s[0][1] + (s[1][1] - s[0][1]) * t1,
                ];
                let (da, db) = (abscisse + l * t0, abscisse + l * t1);
                let portee = w + TALUS_MAX_M;
                let c = [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
                let zone = source.zone_disque(c, distance(a, b) * 0.5 + portee);
                for iz in zone.z0..zone.z1 {
                    for ix in zone.x0..zone.x1 {
                        let p = source.position(ix, iz);
                        let (q, t) = projection(p, a, b);
                        let d = distance(p, q);
                        if d > portee {
                            continue;
                        }
                        let i = source.index(ix, iz);
                        if (d as f32) >= meilleur[i] {
                            continue;
                        }
                        let route = profil.altitude(da + (db - da) * t);
                        let h_src = f64::from(source.h[i]);
                        let h = if d <= w {
                            route
                        } else {
                            let largeur = ((h_src - route).abs() * TALUS).min(TALUS_MAX_M);
                            if d - w >= largeur {
                                continue;
                            }
                            route + (h_src - route) * ((d - w) / largeur.max(1e-6))
                        };
                        meilleur[i] = d as f32;
                        sortie.h[i] = h as f32;
                    }
                }
            }
            abscisse += l;
        }
    }
    for (id, n) in &geo.noeuds {
        let poly = &n.carrefour.emprise;
        let Some(&h) = alt.noeuds.get(id) else { continue };
        if poly.is_empty() {
            continue;
        }
        let c = mcf_geo::polygone::centroide(poly);
        let r = poly.iter().map(|p| distance(*p, c)).fold(0.0, f64::max);
        let zone = source.zone_disque(c, r);
        for iz in zone.z0..zone.z1 {
            for ix in zone.x0..zone.x1 {
                if mcf_geo::polygone::contient(poly, source.position(ix, iz)) {
                    let i = source.index(ix, iz);
                    sortie.h[i] = h as f32;
                }
            }
        }
    }
    sortie
}

fn projection(p: P2, a: P2, b: P2) -> (P2, f64) {
    let d = [b[0] - a[0], b[1] - a[1]];
    let l2 = d[0] * d[0] + d[1] * d[1];
    if l2 <= f64::EPSILON {
        return (a, 0.0);
    }
    let t = (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / l2).clamp(0.0, 1.0);
    ([a[0] + d[0] * t, a[1] + d[1] * t], t)
}

/// Segment de courbe de niveau, avec son altitude.
pub type Courbe = (P2, P2, f64);

/// Courbes de niveau tous les `intervalle` mètres, calculées sur une grille sous-échantillonnée d'un facteur `pas`
/// (carrés en marche).
pub fn courbes_de_niveau(r: &Relief, intervalle: f64, pas: usize) -> Vec<Courbe> {
    let pas = pas.max(1);
    let mut v = Vec::new();
    if r.nx < 2 || r.nz < 2 || intervalle <= 0.0 {
        return v;
    }
    let mut iz = 0;
    while iz + pas < r.nz {
        let mut ix = 0;
        while ix + pas < r.nx {
            let coins = [(ix, iz), (ix + pas, iz), (ix + pas, iz + pas), (ix, iz + pas)];
            let h: Vec<f64> = coins
                .iter()
                .map(|&(x, z)| f64::from(r.h[r.index(x, z)]))
                .collect();
            let p: Vec<P2> = coins.iter().map(|&(x, z)| r.position(x, z)).collect();
            let (lo, hi) = h.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &x| {
                (a.min(x), b.max(x))
            });
            let mut niveau = (lo / intervalle).ceil() * intervalle;
            while niveau <= hi {
                let mut pts: Vec<P2> = Vec::with_capacity(4);
                for k in 0..4 {
                    let (a, b) = (k, (k + 1) % 4);
                    if (h[a] < niveau) != (h[b] < niveau) {
                        let t = (niveau - h[a]) / (h[b] - h[a]);
                        pts.push([
                            p[a][0] + (p[b][0] - p[a][0]) * t,
                            p[a][1] + (p[b][1] - p[a][1]) * t,
                        ]);
                    }
                }
                if pts.len() >= 2 {
                    v.push((pts[0], pts[1], niveau));
                }
                if pts.len() == 4 {
                    v.push((pts[2], pts[3], niveau));
                }
                niveau += intervalle;
            }
            ix += pas;
        }
        iz += pas;
    }
    v
}

/// Couleur de plan d'un point du terrain : teinte hypsométrique et estompage (lumière du nord-ouest).
pub fn couleur_plan(h: f64, normale: [f64; 3], eau: Option<f64>) -> [f32; 3] {
    if let Some(e) = eau
        && h < e
    {
        let profondeur = ((e - h) / 20.0).clamp(0.0, 1.0) as f32;
        return [
            0.55 - 0.2 * profondeur,
            0.72 - 0.2 * profondeur,
            0.88 - 0.1 * profondeur,
        ];
    }
    let paliers: [(f64, [f32; 3]); 5] = [
        (0.0, [0.78, 0.86, 0.68]),
        (40.0, [0.86, 0.88, 0.66]),
        (100.0, [0.88, 0.80, 0.62]),
        (200.0, [0.80, 0.70, 0.58]),
        (400.0, [0.95, 0.95, 0.95]),
    ];
    let mut c = paliers[0].1;
    for w in paliers.windows(2) {
        if h >= w[0].0 {
            let t = ((h - w[0].0) / (w[1].0 - w[0].0)).clamp(0.0, 1.0) as f32;
            c = [0, 1, 2].map(|k| w[0].1[k] + (w[1].1[k] - w[0].1[k]) * t);
        }
    }
    let lumiere = [-0.5f64, 0.7, -0.5];
    let l = (lumiere[0] * lumiere[0] + lumiere[1] * lumiere[1] + lumiere[2] * lumiere[2]).sqrt();
    let ombre =
        ((normale[0] * lumiere[0] + normale[1] * lumiere[1] + normale[2] * lumiere[2]) / l).clamp(0.0, 1.0);
    let f = (0.55 + 0.6 * ombre) as f32;
    c.map(|x| (x * f).clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometrie;
    use crate::reseau::Reseau;
    use mcf_geo::ClasseRoute;

    fn m() -> Manifeste {
        Manifeste::nouveau("t", 256.0)
    }

    #[test]
    fn interpolation_et_pinceaux() {
        let m = m();
        let mut r = Relief::plat(&m, 10.0);
        assert_eq!(r.hauteur([3.3, -7.1]), 10.0);
        let z = r.pinceau(Pinceau::Lever, [0.0, 0.0], 10.0, 5.0, 0.0, 1, (-64.0, 448.0));
        assert!((r.hauteur([0.0, 0.0]) - 15.0).abs() < 1e-4);
        assert_eq!(r.hauteur([20.0, 0.0]), 10.0);
        let avant = r.copier(z);
        r.pinceau(Pinceau::Aplanir, [0.0, 0.0], 10.0, 1.0, 10.0, 1, (-64.0, 448.0));
        assert!((r.hauteur([0.0, 0.0]) - 10.0).abs() < 1e-4);
        r.coller(z, &avant);
        assert!((r.hauteur([0.0, 0.0]) - 15.0).abs() < 1e-4);
        r.pinceau(Pinceau::Lever, [0.0, 0.0], 10.0, 1e6, 0.0, 1, (-64.0, 448.0));
        assert_eq!(r.hauteur([0.0, 0.0]), 448.0);
    }

    #[test]
    fn route_terrassee_sur_une_pente() {
        let m = m();
        let mut src = Relief::plat(&m, 0.0);
        // Pente de 20 % vers +X.
        for iz in 0..src.nz {
            for ix in 0..src.nx {
                let p = src.position(ix, iz);
                let i = src.index(ix, iz);
                src.h[i] = (p[0] * 0.2 + 30.0) as f32;
            }
        }
        let mut res = Reseau::default();
        res.tracer(ClasseRoute::Avenue, &[[-100.0, 0.0], [100.0, 0.0]], 2.0, &m)
            .expect("route");
        let geo = geometrie::calculer(&res, &m.profils);
        let alt = altitudes_reseau(&res, &geo, &src, &m.profils);
        let profil = alt.troncons.values().next().expect("profil");
        // Pente bornée à 6 % (avenue).
        for w in profil.h.windows(2) {
            assert!((w[1] - w[0]).abs() <= 0.06 * profil.pas + 1e-9);
        }
        let fin = terrain_final(&src, &geo, &alt, &m.profils);
        // Plat en travers dans l'emprise.
        let (a, b) = (fin.hauteur([10.0, -11.0]), fin.hauteur([10.0, 11.0]));
        assert!((a - b).abs() < 0.05, "{a} / {b}");
        assert!((a - profil.altitude(110.0)).abs() < 0.05);
        // Loin de la route : terrain d'origine.
        assert_eq!(fin.hauteur([10.0, 100.0]), src.hauteur([10.0, 100.0]));
    }

    #[test]
    fn courbes_sur_un_cone() {
        let m = m();
        let mut r = Relief::plat(&m, 0.0);
        r.pinceau(Pinceau::Lever, [0.0, 0.0], 60.0, 30.0, 0.0, 1, (-64.0, 448.0));
        let c = courbes_de_niveau(&r, 5.0, 2);
        assert!(!c.is_empty());
        assert!(c.iter().all(|(_, _, h)| h % 5.0 == 0.0 && *h > 0.0));
    }
}
