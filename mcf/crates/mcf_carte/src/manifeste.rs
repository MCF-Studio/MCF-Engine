//! Manifeste d'un projet de carte (`carte.mcfcarte.json`) : emprise, relief, réglages partagés.

use mcf_geo::{P2, TableProfils};
use serde::{Deserialize, Serialize};

use crate::gabarits::TableGabarits;

/// Valeur du champ `format`.
pub const FORMAT: &str = "mcfcarte";
/// Version du format de projet.
pub const VERSION: u32 = 1;
/// Côté minimal et maximal de la carte (m).
pub const TAILLE_MIN_M: f64 = 128.0;
/// Côté maximal (16 km).
pub const TAILLE_MAX_M: f64 = 16_384.0;
/// Tailles de tuile autorisées (m).
pub const TUILES_M: [u32; 3] = [128, 256, 512];
/// Résolutions de relief autorisées (m par échantillon).
pub const RESOLUTIONS_M: [f64; 4] = [0.5, 1.0, 2.0, 4.0];
/// Échantillons de relief maximum (garde-fou mémoire : 8193² ≈ 268 Mo en f32).
pub const MAX_ECHANTILLONS: usize = 8193 * 8193;

/// Manifeste.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifeste {
    /// Toujours `mcfcarte`.
    pub format: String,
    /// Version du format.
    pub version: u32,
    /// Nom de la carte (identifiant sûr : `a-z`, `0-9`, `_`, `-`).
    pub nom: String,
    /// Côtés X et Z de la carte (m), centrée sur l'origine. Multiples de `tuile_m`.
    pub taille_m: [f64; 2],
    /// Côté d'une tuile de relief (m) : découpage des fichiers et du streaming.
    pub tuile_m: u32,
    /// Distance entre deux échantillons de relief (m).
    pub resolution_relief_m: f64,
    /// Altitude du code 0 des heightmaps 16 bits (m).
    pub altitude_min_m: f64,
    /// Altitude du code 65535 (m).
    pub altitude_max_m: f64,
    /// Niveau de la mer ou du lac global (m), s'il y en a.
    #[serde(default)]
    pub niveau_eau_m: Option<f64>,
    /// Graine des générations déterministes.
    pub graine: u64,
    /// Profils routiers de cette carte.
    pub profils: TableProfils,
    /// Gabarits de bâti de cette carte.
    pub gabarits: TableGabarits,
}

/// Identifiant sûr : 1 à 64 caractères `a-z`, `0-9`, `_`, `-`.
pub fn nom_sur(s: &str) -> bool {
    (1..=64).contains(&s.len())
        && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

impl Manifeste {
    /// Nouveau manifeste avec les valeurs par défaut de la spec.
    pub fn nouveau(nom: &str, taille_m: f64) -> Self {
        Self {
            format: FORMAT.into(),
            version: VERSION,
            nom: nom.into(),
            taille_m: [taille_m, taille_m],
            tuile_m: 256,
            resolution_relief_m: 1.0,
            altitude_min_m: -64.0,
            altitude_max_m: 448.0,
            niveau_eau_m: None,
            graine: crate::ids::Id::nouveau().0,
            profils: TableProfils::DEFAUT,
            gabarits: TableGabarits::defaut(),
        }
    }

    /// Bornes de la carte : coin `(x, z)` minimal et maximal.
    pub fn bornes(&self) -> (P2, P2) {
        let (hx, hz) = (self.taille_m[0] * 0.5, self.taille_m[1] * 0.5);
        ([-hx, -hz], [hx, hz])
    }

    /// Le point est-il dans la carte ?
    pub fn contient(&self, p: P2) -> bool {
        let (a, b) = self.bornes();
        p[0].is_finite() && p[1].is_finite() && (a[0]..=b[0]).contains(&p[0]) && (a[1]..=b[1]).contains(&p[1])
    }

    /// Nombre d'échantillons de relief en X et Z.
    pub fn echantillons(&self) -> (usize, usize) {
        let n = |t: f64| (t / self.resolution_relief_m).round() as usize + 1;
        (n(self.taille_m[0]), n(self.taille_m[1]))
    }

    /// Nombre de tuiles en X et Z.
    pub fn tuiles(&self) -> (usize, usize) {
        let n = |t: f64| (t / f64::from(self.tuile_m)).round() as usize;
        (n(self.taille_m[0]), n(self.taille_m[1]))
    }

    /// Vérifie toutes les bornes (un manifeste est une donnée non fiable).
    pub fn valider(&self) -> Result<(), String> {
        if self.format != FORMAT || self.version != VERSION {
            return Err(format!("format attendu « {FORMAT} » en version {VERSION}"));
        }
        if !nom_sur(&self.nom) {
            return Err("nom de carte invalide (a-z, 0-9, _ et -, 64 caractères au plus)".into());
        }
        if !TUILES_M.contains(&self.tuile_m) {
            return Err("taille de tuile non gérée (128, 256 ou 512 m)".into());
        }
        if !RESOLUTIONS_M.contains(&self.resolution_relief_m) {
            return Err("résolution de relief non gérée (0,5, 1, 2 ou 4 m)".into());
        }
        for t in self.taille_m {
            if !(TAILLE_MIN_M..=TAILLE_MAX_M).contains(&t) || t % f64::from(self.tuile_m) != 0.0 {
                return Err("taille de carte hors bornes ou non multiple de la tuile".into());
            }
        }
        let (nx, nz) = self.echantillons();
        if nx.saturating_mul(nz) > MAX_ECHANTILLONS {
            return Err("relief trop grand pour cette résolution".into());
        }
        let (a, b) = (self.altitude_min_m, self.altitude_max_m);
        if !(a.is_finite() && b.is_finite() && -1000.0 <= a && a + 1.0 <= b && b <= 9000.0 && b - a <= 4096.0) {
            return Err("plage d'altitude invalide".into());
        }
        if let Some(e) = self.niveau_eau_m
            && !(a..=b).contains(&e)
        {
            return Err("niveau d'eau hors de la plage d'altitude".into());
        }
        self.profils.valider()?;
        self.gabarits.valider()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nouveau_valide() {
        let m = Manifeste::nouveau("ville_test", 1536.0);
        assert_eq!(m.valider(), Ok(()));
        assert_eq!(m.echantillons(), (1537, 1537));
        assert_eq!(m.tuiles(), (6, 6));
        assert!(m.contient([768.0, -768.0]));
        assert!(!m.contient([769.0, 0.0]));
    }

    #[test]
    fn refus() {
        let mut m = Manifeste::nouveau("ok", 1536.0);
        m.taille_m[0] = 1000.0;
        assert!(m.valider().is_err());
        let mut m = Manifeste::nouveau("../evil", 1536.0);
        assert!(m.valider().is_err());
        m.nom = "ok".into();
        m.altitude_max_m = f64::INFINITY;
        assert!(m.valider().is_err());
    }
}
