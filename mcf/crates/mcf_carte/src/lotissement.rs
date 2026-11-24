//! Lotissement : découpe les façades d'un îlot en parcelles et y fait « pousser » des blocs selon le gabarit, puis
//! remplit l'intérieur par des rangées desservies par des allées (une grande zone libre n'a pas de rues dedans).
//! Déterministe (même graine, même résultat) et respectueux des obstacles (bâtiments manuels, emprises).

use std::collections::HashMap;

use mcf_geo::{P2, polygone};

use crate::gabarits::Gabarit;
use crate::ids::Alea;

/// Bâtiments maximum générés par îlot ou zone.
pub const MAX_PAR_ILOT: usize = 20_000;
/// Marge de tolérance des contacts (mitoyenneté) en mètres.
const MARGE: f64 = 0.02;
/// Largeur d'une allée de desserte intérieure (m).
pub const ALLEE_M: f64 = 8.0;
/// Fond de parcelle laissé entre deux rangées dos à dos (m).
pub const JARDIN_M: f64 = 4.0;
/// Côté d'une case de l'index spatial (m).
const CASE_M: f64 = 32.0;
/// Au-delà de ce nombre de cases, un polygone (longue emprise de route) est testé à part.
const MAX_CASES: i64 = 4_096;

/// Un bloc proposé : empreinte et niveaux.
pub type Bloc = (Vec<P2>, u32);

fn rectangle(a: P2, d: P2, n: P2, x: f64, w: f64, s: f64, p: f64) -> Vec<P2> {
    let pt = |u: f64, v: f64| [a[0] + d[0] * u + n[0] * v, a[1] + d[1] * u + n[1] * v];
    vec![pt(x, s), pt(x + w, s), pt(x + w, s + p), pt(x, s + p)]
}

fn dans(ilot: &[P2], r: &[P2]) -> bool {
    r.iter().all(|p| polygone::contient(ilot, *p))
        && (0..r.len()).all(|i| {
            let (a, b) = (r[i], r[(i + 1) % r.len()]);
            (0..ilot.len()).all(|j| !polygone::segments_se_coupent(a, b, ilot[j], ilot[(j + 1) % ilot.len()]))
        })
}

/// Index spatial des polygones déjà posés (obstacles et blocs) : une grande zone compte des milliers de blocs.
#[derive(Default)]
struct Index {
    polys: Vec<(P2, P2, Vec<P2>)>,
    cases: HashMap<(i64, i64), Vec<usize>>,
    gros: Vec<usize>,
}

impl Index {
    fn cases_de(a: P2, b: P2) -> (i64, i64, i64, i64) {
        let c = |v: f64| (v / CASE_M).floor() as i64;
        (c(a[0]), c(a[1]), c(b[0]), c(b[1]))
    }

    fn ajouter(&mut self, p: Vec<P2>) {
        let (a, b) = polygone::englobant(&p);
        let k = self.polys.len();
        self.polys.push((a, b, p));
        let (x0, z0, x1, z1) = Self::cases_de(a, b);
        if (x1 - x0 + 1) * (z1 - z0 + 1) > MAX_CASES {
            self.gros.push(k);
            return;
        }
        for x in x0..=x1 {
            for z in z0..=z1 {
                self.cases.entry((x, z)).or_default().push(k);
            }
        }
    }

    fn chevauche(&self, r: &[P2]) -> bool {
        let (a, b) = polygone::englobant(r);
        let touche = |k: usize| {
            let (pa, pb, p) = &self.polys[k];
            !(pb[0] < a[0] || b[0] < pa[0] || pb[1] < a[1] || b[1] < pa[1]) && polygone::se_chevauchent(r, p)
        };
        if self.gros.iter().any(|&k| touche(k)) {
            return true;
        }
        let (x0, z0, x1, z1) = Self::cases_de(a, b);
        (x0..=x1).any(|x| {
            (z0..=z1).any(|z| {
                self.cases
                    .get(&(x, z))
                    .is_some_and(|v| v.iter().any(|&k| touche(k)))
            })
        })
    }
}

/// État d'un lotissement en cours.
struct Lot<'a> {
    ilot: Vec<P2>,
    gabarit: &'a Gabarit,
    alea: Alea,
    index: Index,
    blocs: Vec<Bloc>,
}

impl Lot<'_> {
    fn libre(&self, r: &[P2]) -> bool {
        let r = polygone::reduit(r, MARGE);
        dans(&self.ilot, &r) && !self.index.chevauche(&r)
    }

    /// Une rangée de parcelles le long du segment `a → a + d·l`, côté `n`, à `recul` m du segment (plus le retrait
    /// du gabarit).
    fn rangee(&mut self, a: P2, d: P2, n: P2, l: f64, recul: f64) {
        let g = self.gabarit;
        let mut x = 0.0;
        while x + g.facade_m[0] <= l && self.blocs.len() < MAX_PAR_ILOT {
            let w = self.alea.entre(g.facade_m[0], g.facade_m[1]).min(l - x);
            let s = recul + self.alea.entre(g.retrait_m[0], g.retrait_m[1]);
            let profondeur = self.alea.entre(g.profondeur_m[0], g.profondeur_m[1]);
            let niveaux = self.alea.entier(g.niveaux[0], g.niveaux[1]);
            let place = [profondeur, g.profondeur_m[0]]
                .into_iter()
                .map(|p| polygone::oriente_positif(&rectangle(a, d, n, x, w, s, p)))
                .find(|r| self.libre(r));
            match place {
                Some(r) => {
                    self.index.ajouter(polygone::reduit(&r, MARGE));
                    self.blocs.push((r, niveaux));
                    x += w + if g.mitoyen { 0.0 } else { self.alea.entre(2.0, 6.0) };
                }
                None => x += 2.0,
            }
        }
    }
}

/// Lotit un îlot. `interieur` : surface constructible (sens trigonométrique), `obstacles` : polygones à éviter.
///
/// 1. Façades : une rangée le long de chaque côté (les rues de l'îlot).
/// 2. Intérieur : des allées parallèles au plus long côté, tous les `2 × (retrait + profondeur) + allée + jardin`
///    mètres, bordées d'une rangée de chaque côté. Un petit îlot de centre-ville n'en reçoit pas (pas la place) ; une
///    grande zone libre se remplit entièrement.
pub fn lotir(interieur: &[P2], gabarit: &Gabarit, graine: u64, obstacles: &[Vec<P2>]) -> Vec<Bloc> {
    let ilot = polygone::oriente_positif(interieur);
    let n = ilot.len();
    if n < 3 {
        return Vec::new();
    }
    let (imin, imax) = polygone::englobant(&ilot);
    let mut index = Index::default();
    for o in obstacles {
        let (a, b) = polygone::englobant(o);
        if !(b[0] < imin[0] || imax[0] < a[0] || b[1] < imin[1] || imax[1] < a[1]) {
            index.ajouter(polygone::reduit(o, MARGE));
        }
    }
    let mut lot = Lot {
        ilot,
        gabarit,
        alea: Alea::new(graine),
        index,
        blocs: Vec::new(),
    };

    // 1. Façades sur rue.
    let mut plus_long = (0.0, [1.0, 0.0]);
    for i in 0..n {
        let (a, b) = (lot.ilot[i], lot.ilot[(i + 1) % n]);
        let l = mcf_geo::distance(a, b);
        if l < 1e-9 {
            continue;
        }
        let d = [(b[0] - a[0]) / l, (b[1] - a[1]) / l];
        if l > plus_long.0 {
            plus_long = (l, d);
        }
        if l >= gabarit.facade_m[0] {
            lot.rangee(a, d, [-d[1], d[0]], l, 0.0);
        }
    }

    // 2. Intérieur : allées parallèles au plus long côté, dans le repère (u le long de d, v le long de nrm).
    let d = plus_long.1;
    let nrm = [-d[1], d[0]];
    let (mut umin, mut umax, mut vmin, mut vmax) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for p in &lot.ilot {
        let (u, v) = (p[0] * d[0] + p[1] * d[1], p[0] * nrm[0] + p[1] * nrm[1]);
        (umin, umax, vmin, vmax) = (umin.min(u), umax.max(u), vmin.min(v), vmax.max(v));
    }
    let bande = gabarit.retrait_m[1] + gabarit.profondeur_m[1];
    let pas = 2.0 * bande + ALLEE_M + JARDIN_M;
    // Première allée derrière la rangée de façade du bord bas, son fond de parcelle et sa propre rangée.
    let mut v = vmin + bande + JARDIN_M + bande + ALLEE_M * 0.5;
    while v < vmax && lot.blocs.len() < MAX_PAR_ILOT {
        let a = [d[0] * umin + nrm[0] * v, d[1] * umin + nrm[1] * v];
        let l = umax - umin;
        lot.rangee(a, d, nrm, l, ALLEE_M * 0.5);
        lot.rangee(a, d, [-nrm[0], -nrm[1]], l, ALLEE_M * 0.5);
        v += pas;
    }
    lot.blocs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gabarits::{Densite, Famille, TableGabarits};

    fn ilot() -> Vec<P2> {
        vec![[0.0, 0.0], [86.0, 0.0], [86.0, 86.0], [0.0, 86.0]]
    }

    #[test]
    fn blocs_dans_l_ilot_sans_chevauchement() {
        let t = TableGabarits::defaut();
        for (f, d) in [
            (Famille::Residentiel, Densite::Moyenne),
            (Famille::Residentiel, Densite::Faible),
        ] {
            let g = t.get(f, d).expect("gabarit");
            let blocs = lotir(&ilot(), g, 42, &[]);
            assert!(blocs.len() >= 4, "{f:?} {d:?} : {}", blocs.len());
            for (i, (b, niv)) in blocs.iter().enumerate() {
                assert!((g.niveaux[0]..=g.niveaux[1]).contains(niv));
                assert!(b.iter().all(|p| polygone::contient(&ilot(), *p) || on_edge(*p)));
                for (c, _) in &blocs[i + 1..] {
                    assert!(!polygone::se_chevauchent(
                        &polygone::reduit(b, 0.05),
                        &polygone::reduit(c, 0.05)
                    ));
                }
            }
            // Déterministe.
            assert_eq!(blocs, lotir(&ilot(), g, 42, &[]));
        }
    }

    fn on_edge(p: P2) -> bool {
        p[0].abs() < 1e-6 || p[1].abs() < 1e-6 || (p[0] - 86.0).abs() < 1e-6 || (p[1] - 86.0).abs() < 1e-6
    }

    #[test]
    fn obstacle_respecte() {
        let t = TableGabarits::defaut();
        let g = t.get(Famille::Residentiel, Densite::Moyenne).expect("gabarit");
        let obstacle = vec![[0.0, 0.0], [86.0, 0.0], [86.0, 20.0], [0.0, 20.0]];
        let blocs = lotir(&ilot(), g, 1, std::slice::from_ref(&obstacle));
        assert!(
            blocs
                .iter()
                .all(|(b, _)| !polygone::se_chevauchent(&polygone::reduit(b, 0.05), &obstacle))
        );
    }

    /// Une grande zone libre (sans rues dedans) se remplit jusqu'au centre, pas seulement sur son pourtour.
    #[test]
    fn grande_zone_remplie_jusqu_au_centre() {
        let t = TableGabarits::defaut();
        let g = t.get(Famille::Residentiel, Densite::Moyenne).expect("gabarit");
        let zone = vec![[0.0, 0.0], [400.0, 0.0], [400.0, 300.0], [0.0, 300.0]];
        let blocs = lotir(&zone, g, 7, &[]);
        let loin_du_bord = blocs
            .iter()
            .filter(|(b, _)| {
                let c = polygone::centroide(b);
                c[0].min(400.0 - c[0]).min(c[1]).min(300.0 - c[1]) > 60.0
            })
            .count();
        assert!(
            loin_du_bord >= 20,
            "{loin_du_bord} bloc(s) à plus de 60 m du bord sur {}",
            blocs.len()
        );
        for (i, (b, _)) in blocs.iter().enumerate() {
            for (c, _) in &blocs[i + 1..] {
                assert!(!polygone::se_chevauchent(
                    &polygone::reduit(b, 0.05),
                    &polygone::reduit(c, 0.05)
                ));
            }
        }
        assert_eq!(blocs, lotir(&zone, g, 7, &[]));
    }

    /// Une zone de 2 km de côté reste rapide (index spatial) et sous le plafond.
    #[test]
    fn zone_de_deux_km() {
        let t = TableGabarits::defaut();
        let g = t.get(Famille::Residentiel, Densite::Moyenne).expect("gabarit");
        let zone = vec![[0.0, 0.0], [2000.0, 0.0], [2000.0, 2000.0], [0.0, 2000.0]];
        let debut = std::time::Instant::now();
        let blocs = lotir(&zone, g, 3, &[]);
        eprintln!(
            "2 km : {} blocs en {:.0} ms",
            blocs.len(),
            debut.elapsed().as_secs_f64() * 1000.0
        );
        assert!(blocs.len() > 1_000 && blocs.len() <= MAX_PAR_ILOT);
    }
}
