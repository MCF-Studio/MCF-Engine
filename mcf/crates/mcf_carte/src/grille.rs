//! Index spatial par grille régulière : trouver vite les polygones proches d'une boîte, au lieu de tout parcourir
//! (169 000 bâtiments × des centaines de milliers de morceaux de route = des milliards de tests sans index).

use std::collections::HashMap;

use mcf_geo::P2;

/// Boîte englobante `[min, max]`.
pub type Boite = [P2; 2];

/// Boîte englobante d'un polygone (vide : boîte inversée qui ne touche rien).
pub fn boite(poly: &[P2]) -> Boite {
    let mut b = [[f64::MAX, f64::MAX], [f64::MIN, f64::MIN]];
    for p in poly {
        b[0][0] = b[0][0].min(p[0]);
        b[0][1] = b[0][1].min(p[1]);
        b[1][0] = b[1][0].max(p[0]);
        b[1][1] = b[1][1].max(p[1]);
    }
    b
}

/// Les deux boîtes se touchent.
pub fn se_touchent(a: &Boite, b: &Boite) -> bool {
    a[0][0] <= b[1][0] && b[0][0] <= a[1][0] && a[0][1] <= b[1][1] && b[0][1] <= a[1][1]
}

/// Grille d'indices : chaque case garde les éléments dont la boîte la touche.
#[derive(Debug, Clone)]
pub struct Grille {
    cote: f64,
    cases: HashMap<(i64, i64), Vec<u32>>,
    boites: Vec<Boite>,
}

impl Grille {
    /// Grille vide, cases de `cote` mètres.
    pub fn new(cote: f64) -> Self {
        Self {
            cote: cote.max(1.0),
            cases: HashMap::new(),
            boites: Vec::new(),
        }
    }

    /// Grille remplie avec les boîtes de ces polygones (indices = rang dans la liste).
    pub fn de_polygones(polys: &[Vec<P2>], cote: f64) -> Self {
        let mut g = Self::new(cote);
        for p in polys {
            g.ajouter(boite(p));
        }
        g
    }

    fn plage(&self, b: &Boite) -> (i64, i64, i64, i64) {
        let c = |v: f64| (v / self.cote).floor() as i64;
        (c(b[0][0]), c(b[0][1]), c(b[1][0]), c(b[1][1]))
    }

    /// Ajoute un élément ; rend son indice.
    pub fn ajouter(&mut self, b: Boite) -> u32 {
        let i = self.boites.len() as u32;
        self.boites.push(b);
        if b[0][0] <= b[1][0] && b[0][1] <= b[1][1] {
            let (x0, z0, x1, z1) = self.plage(&b);
            // Un élément démesuré (erreur de données) ne remplit pas des millions de cases.
            if (x1 - x0 + 1) * (z1 - z0 + 1) <= 4096 {
                for x in x0..=x1 {
                    for z in z0..=z1 {
                        self.cases.entry((x, z)).or_default().push(i);
                    }
                }
            }
        }
        i
    }

    /// Indices des éléments dont la boîte touche `b`, sans doublon, dans l'ordre croissant.
    pub fn proches(&self, b: &Boite) -> Vec<u32> {
        let (x0, z0, x1, z1) = self.plage(b);
        let mut v = Vec::new();
        for x in x0..=x1 {
            for z in z0..=z1 {
                if let Some(c) = self.cases.get(&(x, z)) {
                    v.extend(
                        c.iter()
                            .copied()
                            .filter(|i| se_touchent(&self.boites[*i as usize], b)),
                    );
                }
            }
        }
        v.sort_unstable();
        v.dedup();
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trouve_les_voisins_et_eux_seuls() {
        let carre = |x: f64, z: f64| vec![[x, z], [x + 1.0, z], [x + 1.0, z + 1.0], [x, z + 1.0]];
        let polys = vec![
            carre(0.0, 0.0),
            carre(10.0, 0.0),
            carre(100.0, 100.0),
            carre(-50.0, 3.0),
        ];
        let g = Grille::de_polygones(&polys, 16.0);
        assert_eq!(g.proches(&[[0.5, 0.5], [10.5, 0.5]]), vec![0, 1]);
        assert_eq!(g.proches(&[[99.0, 99.0], [99.5, 99.5]]), Vec::<u32>::new());
        assert_eq!(g.proches(&[[-60.0, -10.0], [200.0, 200.0]]), vec![0, 1, 2, 3]);
    }
}
