//! Relief tiré d'une image en niveaux de gris posée en calque de référence (convention :
//! `docs/CONVENTION_CARTES_SOURCE.md`). Noir = altitude basse, blanc = altitude haute, pixel transparent = pas de
//! donnée. Ce module ne fait que des calculs purs ; l'écriture dans la carte (bornes, historique) est dans
//! `Carte::relief_depuis_reference`.
//!
//! Limite connue : les calques sont en 8 bits, soit 256 niveaux. Sur 2 500 m de dénivelé, un niveau vaut ~10 m ;
//! l'interpolation entre pixels et le lissage adoucissent ces marches, sans recréer le détail absent de l'image.

use crate::references::{ImageRgba, Reference};

/// Lissage maximal accepté (m).
pub const LISSAGE_MAX_M: f64 = 500.0;
/// Seuil d'opacité sous lequel un pixel ne porte pas de donnée (0 à 255).
pub const ALPHA_DONNEE: u8 = 128;

/// Lissage conseillé (m) pour effacer les marches des 256 niveaux : ~25 fois la hauteur d'une marche
/// (`(blanc − noir) / 255`), et au moins 2 pixels de l'image. Constaté à l'écran sur l'exemple de la convention
/// (2,5 m par niveau, 4 m par pixel) : 8 m laisse des arcs le long des courbes de niveau, 48 m les efface ; un critère
/// de pente seule (> 28°) ne les voyait pas, l'éclairage les révèle bien avant.
pub fn lissage_conseille(noir_m: f64, blanc_m: f64, m_par_px: f64) -> f64 {
    let marche = (blanc_m - noir_m).abs() / 255.0;
    (25.0 * marche)
        .max(2.0 * m_par_px)
        .clamp(0.0, LISSAGE_MAX_M)
        .round()
}

/// Gris (0 = noir, 1 = blanc) d'un pixel, ou `None` s'il est transparent. Luminance Rec. 709 des valeurs codées :
/// une image en niveaux de gris redonne exactement son niveau.
fn gris_pixel(image: &ImageRgba, x: usize, y: usize) -> Option<f64> {
    let i = (y * image.largeur as usize + x) * 4;
    let p = image.rgba.get(i..i + 4)?;
    if p[3] < ALPHA_DONNEE {
        return None;
    }
    // Gris pur : sa valeur exacte (la somme des coefficients flottants ne fait pas exactement 1).
    if p[0] == p[1] && p[1] == p[2] {
        return Some(f64::from(p[0]) / 255.0);
    }
    Some((0.2126 * f64::from(p[0]) + 0.7152 * f64::from(p[1]) + 0.0722 * f64::from(p[2])) / 255.0)
}

/// Gris interpolé (bilinéaire entre les centres de pixels) au point `(px, py)` de l'image, en pixels réels. `None`
/// hors de l'image ou sur un pixel transparent. Près d'un pixel transparent, on garde le pixel le plus proche.
pub fn gris(image: &ImageRgba, px: f64, py: f64) -> Option<f64> {
    let (l, h) = (f64::from(image.largeur), f64::from(image.hauteur));
    if !(px.is_finite() && py.is_finite()) || px < 0.0 || py < 0.0 || px > l || py > h {
        return None;
    }
    let proche = (
        (px.floor() as usize).min(image.largeur as usize - 1),
        (py.floor() as usize).min(image.hauteur as usize - 1),
    );
    let centre = gris_pixel(image, proche.0, proche.1)?;
    let fx = (px - 0.5).clamp(0.0, l - 1.0);
    let fy = (py - 0.5).clamp(0.0, h - 1.0);
    let (x0, y0) = (fx.floor() as usize, fy.floor() as usize);
    let (x1, y1) = (
        (x0 + 1).min(image.largeur as usize - 1),
        (y0 + 1).min(image.hauteur as usize - 1),
    );
    let (tx, ty) = (fx - x0 as f64, fy - y0 as f64);
    let v = |x, y| gris_pixel(image, x, y);
    match (v(x0, y0), v(x1, y0), v(x0, y1), v(x1, y1)) {
        (Some(a), Some(b), Some(c), Some(d)) => {
            let haut = a + (b - a) * tx;
            let bas = c + (d - c) * tx;
            Some(haut + (bas - haut) * ty)
        }
        _ => Some(centre),
    }
}

/// Altitude lue sous le point `p` de la carte : `bas + gris × (haut − bas)`, ou `None` hors de l'image ou sur un
/// pixel transparent.
pub fn altitude_sous(
    reference: &Reference,
    image: &ImageRgba,
    p: [f64; 2],
    bas: f64,
    haut: f64,
) -> Option<f64> {
    let (px, py) = reference.vers_image(p);
    gris(image, px, py).map(|g| bas + g * (haut - bas))
}

/// Moyenne glissante séparable (carré de côté `2 × rayon + 1`) sur une grille `largeur × hauteur` où `NaN` = pas de
/// donnée : les cases sans donnée restent sans donnée et ne comptent pas dans les moyennes. Coût linéaire (sommes
/// cumulées), quel que soit le rayon.
pub fn lisser(v: &mut [f32], largeur: usize, hauteur: usize, rayon: usize) {
    if rayon == 0 || largeur == 0 || hauteur == 0 || v.len() != largeur * hauteur {
        return;
    }
    let mut ligne = Vec::new();
    // Lignes, puis colonnes.
    for z in 0..hauteur {
        ligne.clear();
        ligne.extend_from_slice(&v[z * largeur..(z + 1) * largeur]);
        passe(&ligne, rayon, |x, r| v[z * largeur + x] = r);
    }
    let mut colonne = vec![0.0f32; hauteur];
    for x in 0..largeur {
        for z in 0..hauteur {
            colonne[z] = v[z * largeur + x];
        }
        passe(&colonne, rayon, |z, r| v[z * largeur + x] = r);
    }
}

/// Une passe 1D : écrit la moyenne des valeurs définies dans la fenêtre, pour chaque case définie.
fn passe(entree: &[f32], rayon: usize, mut ecrire: impl FnMut(usize, f32)) {
    let n = entree.len();
    let mut somme = vec![0.0f64; n + 1];
    let mut compte = vec![0u32; n + 1];
    for (i, &x) in entree.iter().enumerate() {
        let defini = !x.is_nan();
        somme[i + 1] = somme[i] + if defini { f64::from(x) } else { 0.0 };
        compte[i + 1] = compte[i] + u32::from(defini);
    }
    for (i, &x) in entree.iter().enumerate() {
        if x.is_nan() {
            continue;
        }
        let (a, b) = (i.saturating_sub(rayon), (i + rayon + 1).min(n));
        let c = compte[b] - compte[a];
        if c > 0 {
            ecrire(i, ((somme[b] - somme[a]) / f64::from(c)) as f32);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    fn image(largeur: u32, hauteur: u32, pixels: &[[u8; 4]]) -> ImageRgba {
        ImageRgba {
            largeur,
            hauteur,
            rgba: Arc::new(pixels.iter().flatten().copied().collect()),
        }
    }

    #[test]
    fn noir_blanc_et_transparent() {
        let img = image(2, 1, &[[0, 0, 0, 255], [255, 255, 255, 255]]);
        assert_eq!(gris(&img, 0.5, 0.5), Some(0.0));
        assert_eq!(gris(&img, 1.5, 0.5), Some(1.0));
        // Entre les deux centres : interpolé.
        assert!((gris(&img, 1.0, 0.5).expect("gris") - 0.5).abs() < 1e-12);
        assert_eq!(gris(&img, 2.5, 0.5), None);
        let trou = image(2, 1, &[[0, 0, 0, 0], [255, 255, 255, 255]]);
        assert_eq!(gris(&trou, 0.5, 0.5), None);
        // À côté du trou : le pixel le plus proche, sans mélange avec le trou.
        assert_eq!(gris(&trou, 1.2, 0.5), Some(1.0));
    }

    #[test]
    fn lissage_conseille_selon_la_marche() {
        // Exemple de la convention : 640 m sur 255 niveaux = 2,5 m par marche → 63 m.
        assert_eq!(lissage_conseille(-40.0, 600.0, 4.0), 63.0);
        // map_1 : 2 600 m de dénivelé, 25 m par pixel → ~255 m.
        assert_eq!(lissage_conseille(-100.0, 2500.0, 25.0), 255.0);
        // Faible dénivelé, gros pixels : 2 pixels au moins.
        assert_eq!(lissage_conseille(0.0, 10.0, 30.0), 60.0);
    }

    #[test]
    fn lissage_garde_les_trous_et_moyenne_le_reste() {
        let mut v = vec![0.0, 10.0, f32::NAN, 30.0, 40.0];
        lisser(&mut v, 5, 1, 1);
        assert_eq!(v[0], 5.0);
        assert_eq!(v[1], 5.0);
        assert!(v[2].is_nan());
        assert_eq!(v[3], 35.0);
        assert_eq!(v[4], 35.0);
        // Un plan incliné reste un plan incliné (au bord près).
        let mut rampe: Vec<f32> = (0..9).map(|i| (i % 3) as f32).collect();
        lisser(&mut rampe, 3, 3, 1);
        assert_eq!(rampe[4], 1.0);
    }
}
