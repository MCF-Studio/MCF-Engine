//! Conversion d'une image quelconque (vue satellite, plan illustré) vers les deux images de la convention des cartes
//! sources (`docs/CONVENTION_CARTES_SOURCE.md`) : une **carte couleurs** en aplats de la palette, et une **carte de
//! relief proposée** en niveaux de gris.
//!
//! Ce n'est qu'une **proposition** à vérifier et retoucher :
//! - les couleurs viennent d'une classification pixel par pixel (teinte, saturation, luminosité) calibrée sur
//!   `map_sat.png`, nettoyée par un filtre de majorité ;
//! - le relief est **déduit**, pas mesuré (une image ne contient pas d'altitudes) : le terrain monte avec la distance à
//!   la mer, les massifs viennent de la densité de neige et de roche, les lacs sont plats ;
//! - les routes ne sont **pas** extraites (trop fines et trop variées sur une vue satellite) : relevé assisté sur un
//!   plan, ou tracé à la main.
//!
//! Coûts bornés : [`MAX_PIXELS`] au plus (refus au-delà), mémoire linéaire.

use std::collections::VecDeque;
use std::sync::Arc;

use crate::references::ImageRgba;
use crate::relief_image::lisser;

/// Pixels convertis au plus (au-delà : réduire l'image avant).
pub const MAX_PIXELS: u64 = 16 * 1024 * 1024;

/// Catégories de la carte couleurs (palette de la convention).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Classe {
    /// Mer, lac, rivière.
    Eau,
    /// Fond : prairie, sable, garrigue.
    Campagne,
    /// Forêt dense.
    Foret,
    /// Bâti dense (toits).
    Urbain,
    /// Zones grises : industrie, logistique, aéroports.
    Industrie,
    /// Champs.
    Agriculture,
    /// Roche et neige (massifs).
    Roche,
}

impl Classe {
    /// Toutes les classes.
    pub const TOUTES: [Self; 7] = [
        Self::Eau,
        Self::Campagne,
        Self::Foret,
        Self::Urbain,
        Self::Industrie,
        Self::Agriculture,
        Self::Roche,
    ];

    /// Couleur de la palette de la convention (sRGB).
    pub const fn couleur(self) -> [u8; 3] {
        match self {
            Self::Eau => [64, 150, 220],
            Self::Campagne => [184, 217, 155],
            Self::Foret => [90, 150, 90],
            Self::Urbain => [232, 131, 107],
            Self::Industrie => [186, 160, 210],
            Self::Agriculture => [240, 222, 110],
            Self::Roche => [170, 170, 170],
        }
    }

    /// Nom affiché.
    pub const fn nom(self) -> &'static str {
        match self {
            Self::Eau => "eau",
            Self::Campagne => "campagne",
            Self::Foret => "forêt",
            Self::Urbain => "urbain",
            Self::Industrie => "industrie",
            Self::Agriculture => "agriculture",
            Self::Roche => "roche / neige",
        }
    }
}

/// Réglages de la conversion.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reglages {
    /// Altitude du noir de la carte de relief (m).
    pub noir_m: f64,
    /// Altitude du blanc (m) : sommet le plus haut.
    pub blanc_m: f64,
    /// Altitude du fond marin proposé (m).
    pub fond_mer_m: f64,
    /// Altitude atteinte loin de la côte, hors massif (m).
    pub plateau_m: f64,
    /// Côté du filtre de majorité (pixels, impair).
    pub majorite_px: usize,
}

impl Default for Reglages {
    fn default() -> Self {
        Self {
            noir_m: -50.0,
            blanc_m: 2500.0,
            fond_mer_m: -30.0,
            plateau_m: 250.0,
            majorite_px: 5,
        }
    }
}

/// Résultat : les deux images de la convention et ce qu'il faut taper dans l'éditeur.
#[derive(Debug, Clone)]
pub struct Conversion {
    /// Carte couleurs (aplats).
    pub couleurs: ImageRgba,
    /// Carte de relief proposée (gris : noir = `noir_m`, blanc = `blanc_m`).
    pub relief: ImageRgba,
    /// Altitude du noir (m).
    pub noir_m: f64,
    /// Altitude du blanc (m).
    pub blanc_m: f64,
    /// Part de chaque classe (0 à 1), dans l'ordre de [`Classe::TOUTES`].
    pub parts: Vec<(Classe, f64)>,
}

/// Teinte (degrés), saturation et luminosité (0 à 1) d'une couleur sRGB.
fn tsl(r: f64, g: f64, b: f64) -> (f64, f64, f64) {
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let v = max / 255.0;
    let s = if max > 0.0 { (max - min) / max } else { 0.0 };
    let d = max - min;
    let h = if d <= 0.0 {
        0.0
    } else if max == r {
        60.0 * (((g - b) / d).rem_euclid(6.0))
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    (h, s, v)
}

/// Classe d'un pixel (moyenné sur son voisinage). Seuils calibrés sur `map_sat.png` (échantillons du 2026-11-24 :
/// mer h 203 s 1,00 ; rivière h 184 s 0,74 ; neige s 0,02 v 0,90 ; roche h 192 s 0,16 v 0,36 ; forêt h 127 v 0,24 ;
/// prairie h 81 ; champs h 54 s 0,63 v 0,64 ; villes h 21–29 s 0,28–0,30 ; industrie s 0,15 ; sable h 41 s 0,42).
pub fn classer(r: f64, g: f64, b: f64) -> Classe {
    let (h, s, v) = tsl(r, g, b);
    if (165.0..=240.0).contains(&h) && s > 0.5 {
        Classe::Eau
    } else if (s < 0.12 && v > 0.78) || (s < 0.3 && v < 0.5 && (150.0..=260.0).contains(&h)) {
        // Neige (blanc sans couleur) ou roche (gris bleuté sombre).
        Classe::Roche
    } else if s < 0.2 && (0.4..=0.8).contains(&v) {
        Classe::Industrie
    } else if (90.0..=165.0).contains(&h) && v < 0.33 {
        Classe::Foret
    } else if (45.0..=68.0).contains(&h) && s > 0.45 && v > 0.5 {
        Classe::Agriculture
    } else if (8.0..=32.0).contains(&h) && (0.18..=0.5).contains(&s) && (0.4..=0.78).contains(&v) {
        Classe::Urbain
    } else {
        Classe::Campagne
    }
}

/// Convertit une image. Refuse les images de plus de [`MAX_PIXELS`] pixels.
pub fn convertir(image: &ImageRgba, reglages: &Reglages) -> Result<Conversion, String> {
    let (l, h) = (image.largeur as usize, image.hauteur as usize);
    if l < 8 || h < 8 {
        return Err("image trop petite (8 × 8 pixels au moins)".into());
    }
    if (l * h) as u64 > MAX_PIXELS {
        return Err(format!(
            "image trop grande pour la conversion ({MAX_PIXELS} pixels au plus) : réduisez-la"
        ));
    }
    let r = reglages;
    if !(r.noir_m.is_finite() && r.blanc_m.is_finite() && r.noir_m < r.blanc_m)
        || !(r.noir_m..=r.blanc_m).contains(&r.fond_mer_m)
        || !(0.0..=r.blanc_m).contains(&r.plateau_m)
        || r.majorite_px.is_multiple_of(2)
        || r.majorite_px > 15
    {
        return Err("réglages de conversion invalides".into());
    }

    // 1. Classes, sur l'image moyennée 3 × 3 (atténue le grain des textures).
    let px = |x: usize, y: usize, c: usize| f64::from(image.rgba[(y * l + x) * 4 + c]);
    let mut classes = vec![Classe::Campagne; l * h];
    for y in 0..h {
        for x in 0..l {
            let mut s = [0.0; 3];
            let mut n = 0.0;
            for yy in y.saturating_sub(1)..=(y + 1).min(h - 1) {
                for xx in x.saturating_sub(1)..=(x + 1).min(l - 1) {
                    for (c, v) in s.iter_mut().enumerate() {
                        *v += px(xx, yy, c);
                    }
                    n += 1.0;
                }
            }
            classes[y * l + x] = classer(s[0] / n, s[1] / n, s[2] / n);
        }
    }
    // 2. Filtre de majorité : retire le bruit isolé (toits épars, ombres, petits bateaux).
    let mut classes = majorite(&classes, l, h, r.majorite_px / 2);
    // 3. Contexte des massifs : là où la roche est dense, le gris est de la roche (pas une zone d'activité) et les
    //    petites « eaux » intérieures sont des versants à l'ombre (bleu sombre comme la mer sur une vue satellite).
    let cote = l.max(h) as f64;
    let rayon = (cote * 0.02).round().max(1.0) as usize;
    // Inverse : du gris « roche » au milieu de la ville est du bâti (gratte-ciel, toits gris). Seulement la densité
    // de toits « ville » : les massifs sont pleins de gris « activité », pas de toits brique.
    let ville = densite(&classes, l, h, Classe::Urbain, rayon);
    for i in 0..l * h {
        if classes[i] == Classe::Roche && ville[i] > SEUIL_VILLE {
            classes[i] = Classe::Industrie;
        }
    }
    let roche = densite(&classes, l, h, Classe::Roche, rayon);
    for i in 0..l * h {
        if classes[i] == Classe::Industrie && roche[i] > SEUIL_MASSIF {
            classes[i] = Classe::Roche;
        }
    }
    ombres_en_roche(&mut classes, &roche, l, h);

    // 4. Carte couleurs.
    let mut rgba = Vec::with_capacity(l * h * 4);
    for c in &classes {
        let [a, b, d] = c.couleur();
        rgba.extend_from_slice(&[a, b, d, 255]);
    }
    let parts = Classe::TOUTES
        .iter()
        .map(|k| {
            (
                *k,
                classes.iter().filter(|c| *c == k).count() as f64 / (l * h) as f64,
            )
        })
        .collect();

    // 5. Relief proposé.
    let altitudes = relief_propose(&classes, l, h, r);
    let mut gris = Vec::with_capacity(l * h * 4);
    for a in altitudes {
        let g = ((f64::from(a) - r.noir_m) / (r.blanc_m - r.noir_m) * 255.0)
            .round()
            .clamp(0.0, 255.0) as u8;
        gris.extend_from_slice(&[g, g, g, 255]);
    }
    let image = |v| ImageRgba {
        largeur: image.largeur,
        hauteur: image.hauteur,
        rgba: Arc::new(v),
    };
    Ok(Conversion {
        couleurs: image(rgba),
        relief: image(gris),
        noir_m: r.noir_m,
        blanc_m: r.blanc_m,
        parts,
    })
}

/// Densité de roche au-delà de laquelle on est dans un massif.
const SEUIL_MASSIF: f32 = 0.08;
/// Densité de toits « ville » au-delà de laquelle le gris est du bâti, pas de la roche.
const SEUIL_VILLE: f32 = 0.1;

/// Part des pixels de la classe `k` dans un carré de rayon `rayon` autour de chaque pixel.
fn densite(classes: &[Classe], l: usize, h: usize, k: Classe, rayon: usize) -> Vec<f32> {
    let mut d: Vec<f32> = classes.iter().map(|c| if *c == k { 1.0 } else { 0.0 }).collect();
    lisser(&mut d, l, h, rayon);
    d
}

/// Composantes d'eau qui ne touchent pas le bord de l'image (donc pas la mer) et dont la densité de roche moyenne
/// dépasse le seuil des massifs : des versants à l'ombre, pas des lacs.
fn ombres_en_roche(classes: &mut [Classe], roche: &[f32], l: usize, h: usize) {
    let mut vu = vec![false; l * h];
    for depart in 0..l * h {
        if classes[depart] != Classe::Eau || vu[depart] {
            continue;
        }
        vu[depart] = true;
        let mut file = VecDeque::from([depart]);
        let (mut membres, mut bord, mut somme) = (Vec::new(), false, 0.0f64);
        while let Some(i) = file.pop_front() {
            membres.push(i);
            somme += f64::from(roche[i]);
            let (x, y) = (i % l, i / l);
            bord |= x == 0 || y == 0 || x + 1 == l || y + 1 == h;
            for (ok, j) in [
                (x > 0, i.wrapping_sub(1)),
                (x + 1 < l, i + 1),
                (y > 0, i.wrapping_sub(l)),
                (y + 1 < h, i + l),
            ] {
                if ok && !vu[j] && classes[j] == Classe::Eau {
                    vu[j] = true;
                    file.push_back(j);
                }
            }
        }
        if !bord && somme / membres.len() as f64 > f64::from(SEUIL_MASSIF) {
            for i in membres {
                classes[i] = Classe::Roche;
            }
        }
    }
}

/// Classe majoritaire dans un carré de rayon `rayon` (égalité : la classe du centre est gardée).
fn majorite(classes: &[Classe], l: usize, h: usize, rayon: usize) -> Vec<Classe> {
    if rayon == 0 {
        return classes.to_vec();
    }
    let mut sortie = classes.to_vec();
    for y in 0..h {
        for x in 0..l {
            let mut compte = [0u16; 7];
            for yy in y.saturating_sub(rayon)..=(y + rayon).min(h - 1) {
                for xx in x.saturating_sub(rayon)..=(x + rayon).min(l - 1) {
                    compte[classes[yy * l + xx] as usize] += 1;
                }
            }
            let centre = classes[y * l + x];
            let mut meilleure = centre;
            for k in Classe::TOUTES {
                if compte[k as usize] > compte[meilleure as usize] {
                    meilleure = k;
                }
            }
            sortie[y * l + x] = meilleure;
        }
    }
    sortie
}

/// Relief proposé (m) : mer au fond, terre qui monte avec la distance à la côte jusqu'au plateau, massifs d'après la
/// densité de roche et de neige, lacs et rivières plats à l'altitude de leur rive la plus basse.
fn relief_propose(classes: &[Classe], l: usize, h: usize, r: &Reglages) -> Vec<f32> {
    let n = l * h;
    let cote = l.max(h) as f64;
    // Mer = eau reliée au bord de l'image ; le reste de l'eau = lacs et rivières.
    let eau: Vec<bool> = classes.iter().map(|c| *c == Classe::Eau).collect();
    let mut mer = vec![false; n];
    let mut file = VecDeque::new();
    for x in 0..l {
        for y in [0, h - 1] {
            if eau[y * l + x] && !mer[y * l + x] {
                mer[y * l + x] = true;
                file.push_back(y * l + x);
            }
        }
    }
    for y in 0..h {
        for x in [0, l - 1] {
            if eau[y * l + x] && !mer[y * l + x] {
                mer[y * l + x] = true;
                file.push_back(y * l + x);
            }
        }
    }
    voisins_remplir(&eau, &mut mer, &mut file, l, h);

    // Distance à la mer (pixels, chanfrein 3-4).
    let masque = crate::releve::Masque { l, h, v: mer.clone() };
    let distance = crate::releve::distance_a(&masque, true);
    let pas = f64::from(crate::releve::PAS_DROIT);
    // Montée côtière sur ~6 % du côté de l'image.
    let echelle = (cote * 0.06).max(1.0);

    // Densité de roche et neige, très lissée (massifs continus).
    let mut massif: Vec<f32> = classes
        .iter()
        .map(|c| if *c == Classe::Roche { 1.0 } else { 0.0 })
        .collect();
    lisser(&mut massif, l, h, (cote * 0.025).round().max(1.0) as usize);
    lisser(&mut massif, l, h, (cote * 0.025).round().max(1.0) as usize);
    let pic = massif.iter().copied().fold(0.0f32, f32::max).max(1e-6);

    let haut = r.blanc_m - 10.0;
    let mut alt = vec![0.0f32; n];
    for i in 0..n {
        if mer[i] {
            alt[i] = r.fond_mer_m as f32;
            continue;
        }
        let d = f64::from(distance[i]) / pas;
        let cotier = 1.0 - (-d / echelle).exp();
        // Sous la moitié du seuil des massifs : pas de montagne (roche éparse des villes, toits gris bleutés).
        let seuil = SEUIL_MASSIF * 0.5;
        let m = f64::from((massif[i] - seuil) / (pic - seuil).max(1e-6)).clamp(0.0, 1.0);
        let montagne = m.powf(0.7);
        alt[i] = (2.0 + r.plateau_m * cotier + (haut - r.plateau_m) * montagne * cotier) as f32;
    }
    // Lissage de la terre (la mer reste au fond).
    let mut terre: Vec<f32> = alt
        .iter()
        .zip(&mer)
        .map(|(a, m)| if *m { f32::NAN } else { *a })
        .collect();
    lisser(&mut terre, l, h, (cote * 0.004).round().max(1.0) as usize);
    for i in 0..n {
        if !mer[i] {
            alt[i] = terre[i];
        }
    }
    aplanir_lacs(&eau, &mer, &mut alt, l, h);
    alt
}

/// Propage `vu` à travers les pixels `dedans` voisins (4-connexité).
fn voisins_remplir(dedans: &[bool], vu: &mut [bool], file: &mut VecDeque<usize>, l: usize, h: usize) {
    while let Some(i) = file.pop_front() {
        let (x, y) = (i % l, i / l);
        let mut pousser = |j: usize| {
            if dedans[j] && !vu[j] {
                vu[j] = true;
                file.push_back(j);
            }
        };
        if x > 0 {
            pousser(i - 1);
        }
        if x + 1 < l {
            pousser(i + 1);
        }
        if y > 0 {
            pousser(i - l);
        }
        if y + 1 < h {
            pousser(i + l);
        }
    }
}

/// Chaque lac (composante d'eau hors mer) est mis à plat à l'altitude de sa rive la plus basse.
fn aplanir_lacs(eau: &[bool], mer: &[bool], alt: &mut [f32], l: usize, h: usize) {
    let lac: Vec<bool> = eau.iter().zip(mer).map(|(e, m)| *e && !m).collect();
    let mut vu = vec![false; l * h];
    for depart in 0..l * h {
        if !lac[depart] || vu[depart] {
            continue;
        }
        vu[depart] = true;
        let mut file = VecDeque::from([depart]);
        let mut membres = Vec::new();
        let mut rive = f32::INFINITY;
        while let Some(i) = file.pop_front() {
            membres.push(i);
            let (x, y) = (i % l, i / l);
            for (ok, j) in [
                (x > 0, i.wrapping_sub(1)),
                (x + 1 < l, i + 1),
                (y > 0, i.wrapping_sub(l)),
                (y + 1 < h, i + l),
            ] {
                if !ok {
                    continue;
                }
                if lac[j] {
                    if !vu[j] {
                        vu[j] = true;
                        file.push_back(j);
                    }
                } else if !mer[j] {
                    rive = rive.min(alt[j]);
                }
            }
        }
        if rive.is_finite() {
            for i in membres {
                alt[i] = rive;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes_des_echantillons_de_map_sat() {
        // Moyennes mesurées sur map_sat.png (voir `classer`).
        for (rgb, attendue) in [
            ([0.0, 46.0, 74.0], Classe::Eau),
            ([28.0, 103.0, 108.0], Classe::Eau),
            ([226.0, 227.0, 230.0], Classe::Roche),
            ([77.0, 88.0, 92.0], Classe::Roche),
            ([32.0, 61.0, 36.0], Classe::Foret),
            ([94.0, 121.0, 45.0], Classe::Campagne),
            ([164.0, 154.0, 61.0], Classe::Agriculture),
            ([152.0, 123.0, 107.0], Classe::Urbain),
            ([151.0, 129.0, 109.0], Classe::Urbain),
            ([129.0, 128.0, 110.0], Classe::Industrie),
            ([166.0, 144.0, 95.0], Classe::Campagne),
            ([159.0, 154.0, 153.0], Classe::Industrie),
        ] {
            assert_eq!(classer(rgb[0], rgb[1], rgb[2]), attendue, "{rgb:?}");
        }
    }

    #[test]
    fn ile_avec_lac_et_montagne() {
        // 64 × 64 : mer autour, île au centre, lac à l'ouest, neige au nord-est.
        let (l, h) = (64usize, 64usize);
        let mut rgba = Vec::with_capacity(l * h * 4);
        for y in 0..h {
            for x in 0..l {
                let (dx, dy) = (x as f64 - 32.0, y as f64 - 32.0);
                let c = if dx.hypot(dy) > 24.0 {
                    [0, 46, 74]
                } else if (dx + 10.0).hypot(dy) < 5.0 {
                    [1, 61, 86]
                } else if (dx - 8.0).hypot(dy + 8.0) < 7.0 {
                    [226, 227, 230]
                } else {
                    [94, 121, 45]
                };
                rgba.extend_from_slice(&[c[0], c[1], c[2], 255]);
            }
        }
        let img = ImageRgba {
            largeur: l as u32,
            hauteur: h as u32,
            rgba: Arc::new(rgba),
        };
        let r = Reglages::default();
        let c = convertir(&img, &r).expect("conversion");
        let gris = |x: usize, y: usize| c.relief.rgba[(y * l + x) * 4];
        let couleur = |x: usize, y: usize| {
            let i = (y * l + x) * 4;
            [c.couleurs.rgba[i], c.couleurs.rgba[i + 1], c.couleurs.rgba[i + 2]]
        };
        assert_eq!(couleur(2, 2), Classe::Eau.couleur());
        assert_eq!(couleur(40, 24), Classe::Roche.couleur());
        assert_eq!(couleur(22, 32), Classe::Eau.couleur(), "le lac reste de l'eau");
        // Mer au fond, sommet le plus haut, lac plat et au-dessus de la mer.
        let mer = gris(2, 2);
        assert!(gris(40, 24) > gris(32, 45), "le massif domine la plaine");
        assert!(gris(22, 32) > mer, "le lac est au-dessus de la mer");
        assert_eq!(gris(21, 32), gris(23, 32), "le lac est plat");
        assert!(convertir(&img, &Reglages { majorite_px: 4, ..r }).is_err());
    }
}
