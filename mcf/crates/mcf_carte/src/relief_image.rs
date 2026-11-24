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

/// Niveaux appliqués au gris avant la conversion en altitude : le gris `gris_noir` devient l'altitude du noir, le gris
/// `gris_blanc` celle du blanc (entre les deux : linéaire, puis `courbe`). Une courbe > 1 creuse les plaines et
/// garde les sommets, < 1 relève les plaines.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Niveaux {
    /// Gris (0 à 1) qui devient l'altitude du noir ; plus sombre : altitude du noir aussi.
    pub gris_noir: f64,
    /// Gris (0 à 1) qui devient l'altitude du blanc ; plus clair : altitude du blanc aussi.
    pub gris_blanc: f64,
    /// Exposant de la courbe (0,2 à 5 ; 1 = linéaire).
    pub courbe: f64,
}

impl Default for Niveaux {
    fn default() -> Self {
        Self {
            gris_noir: 0.0,
            gris_blanc: 1.0,
            courbe: 1.0,
        }
    }
}

impl Niveaux {
    /// Bornes (réglages saisis : non fiables).
    pub fn valider(&self) -> Result<(), String> {
        let ok = (0.0..=1.0).contains(&self.gris_noir)
            && (0.0..=1.0).contains(&self.gris_blanc)
            && self.gris_blanc - self.gris_noir >= 1.0 / 255.0
            && (0.2..=5.0).contains(&self.courbe);
        if ok {
            Ok(())
        } else {
            Err("niveaux invalides (gris du noir < gris du blanc, courbe de 0,2 à 5)".into())
        }
    }

    /// Gris corrigé (0 à 1).
    pub fn appliquer(&self, g: f64) -> f64 {
        ((g - self.gris_noir) / (self.gris_blanc - self.gris_noir))
            .clamp(0.0, 1.0)
            .powf(self.courbe)
    }
}

/// Part des pixels opaques **en couleur** (écart entre canaux > 24 sur 255), sur un échantillon d'au plus ~1 Mpx. Une
/// carte de relief est en niveaux de gris : au-delà de quelques pour cent, c'est sans doute la carte couleurs.
pub fn part_coloree(image: &ImageRgba) -> f64 {
    let n = (image.largeur as usize) * (image.hauteur as usize);
    let pas = (n / 1_000_000).max(1);
    let (mut opaques, mut colores) = (0usize, 0usize);
    for p in image.rgba.as_chunks::<4>().0.iter().step_by(pas) {
        if p[3] < ALPHA_DONNEE {
            continue;
        }
        opaques += 1;
        let (mx, mn) = (p[0].max(p[1]).max(p[2]), p[0].min(p[1]).min(p[2]));
        if mx - mn > 24 {
            colores += 1;
        }
    }
    if opaques == 0 {
        0.0
    } else {
        colores as f64 / opaques as f64
    }
}

/// Pinceau sur un calque de relief en niveaux de gris (retouche de la carte de relief avant de l'appliquer).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinceauCalque {
    /// Plus clair = plus haut.
    Eclaircir,
    /// Plus sombre = plus bas.
    Assombrir,
    /// Vers un gris donné (plateau, fond de vallée, niveau d'un lac).
    Niveau,
    /// Moyenne locale : adoucit les ruptures.
    Adoucir,
    /// Rend transparent : « pas de donnée », le relief n'y sera pas touché.
    Effacer,
}

impl PinceauCalque {
    /// Tous les modes.
    pub const TOUS: [Self; 5] = [
        Self::Eclaircir,
        Self::Assombrir,
        Self::Niveau,
        Self::Adoucir,
        Self::Effacer,
    ];

    /// Nom affiché.
    pub const fn nom(self) -> &'static str {
        match self {
            Self::Eclaircir => "Éclaircir (monter)",
            Self::Assombrir => "Assombrir (creuser)",
            Self::Niveau => "Niveau",
            Self::Adoucir => "Adoucir",
            Self::Effacer => "Effacer",
        }
    }
}

/// Un coup de pinceau sur des pixels RGBA gris (`largeur × hauteur`), centré en `(cx, cy)` pixels, de rayon `rayon`
/// pixels. `force` (0 à 1) : part du chemin faite à ce coup au centre (le bord est adouci) ; `cible` : gris visé par
/// « Niveau » (0 à 1). Peindre un pixel transparent le rend opaque (il porte alors une donnée), sauf « Effacer ».
#[allow(clippy::too_many_arguments)]
pub fn peindre(
    rgba: &mut [u8],
    largeur: usize,
    hauteur: usize,
    (cx, cy): (f64, f64),
    rayon: f64,
    mode: PinceauCalque,
    force: f64,
    cible: f64,
) {
    if !(rayon.is_finite() && rayon > 0.0 && cx.is_finite() && cy.is_finite())
        || rgba.len() != largeur * hauteur * 4
    {
        return;
    }
    let x0 = (cx - rayon).floor().max(0.0) as usize;
    let y0 = (cy - rayon).floor().max(0.0) as usize;
    let x1 = ((cx + rayon).ceil().max(0.0) as usize).min(largeur);
    let y1 = ((cy + rayon).ceil().max(0.0) as usize).min(hauteur);
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    // « Adoucir » lit les voisins d'avant le coup.
    let avant: Vec<u8> = if mode == PinceauCalque::Adoucir {
        rgba.to_vec()
    } else {
        Vec::new()
    };
    let gris_avant = |x: usize, y: usize| f64::from(avant[(y * largeur + x) * 4]) / 255.0;
    for y in y0..y1 {
        for x in x0..x1 {
            let d = ((x as f64 + 0.5 - cx).powi(2) + (y as f64 + 0.5 - cy).powi(2)).sqrt();
            if d >= rayon {
                continue;
            }
            let t = 1.0 - d / rayon;
            let w = (t * t * (3.0 - 2.0 * t) * force).clamp(0.0, 1.0);
            let i = (y * largeur + x) * 4;
            if mode == PinceauCalque::Effacer {
                rgba[i + 3] = (f64::from(rgba[i + 3]) * (1.0 - w)).round() as u8;
                continue;
            }
            let g = f64::from(rgba[i]) / 255.0;
            let nouveau = match mode {
                PinceauCalque::Eclaircir => g + w,
                PinceauCalque::Assombrir => g - w,
                PinceauCalque::Niveau => g + (cible - g) * w,
                PinceauCalque::Adoucir => {
                    let (mut s, mut n) = (0.0, 0.0);
                    for yy in y.saturating_sub(2)..(y + 3).min(hauteur) {
                        for xx in x.saturating_sub(2)..(x + 3).min(largeur) {
                            s += gris_avant(xx, yy);
                            n += 1.0;
                        }
                    }
                    g + (s / n - g) * w
                }
                PinceauCalque::Effacer => g,
            };
            let v = (nouveau.clamp(0.0, 1.0) * 255.0).round() as u8;
            rgba[i..i + 4].copy_from_slice(&[v, v, v, 255]);
        }
    }
}

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

/// Altitude lue sous le point `p` de la carte : `bas + niveaux(gris) × (haut − bas)`, ou `None` hors de l'image ou
/// sur un pixel transparent.
pub fn altitude_sous(
    reference: &Reference,
    image: &ImageRgba,
    p: [f64; 2],
    bas: f64,
    haut: f64,
    niveaux: &Niveaux,
) -> Option<f64> {
    let (px, py) = reference.vers_image(p);
    gris(image, px, py).map(|g| bas + niveaux.appliquer(g) * (haut - bas))
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
    fn pinceau_de_calque() {
        let mut px = vec![100u8; 9 * 9 * 4];
        peindre(&mut px, 9, 9, (4.5, 4.5), 3.0, PinceauCalque::Eclaircir, 1.0, 0.0);
        assert_eq!(px[(4 * 9 + 4) * 4], 255, "centre éclairci au maximum");
        assert_eq!(px[0], 100, "coin hors du pinceau intact");
        peindre(&mut px, 9, 9, (4.5, 4.5), 3.0, PinceauCalque::Niveau, 1.0, 0.2);
        assert_eq!(px[(4 * 9 + 4) * 4], 51, "centre ramené au gris 0,2");
        peindre(&mut px, 9, 9, (4.5, 4.5), 3.0, PinceauCalque::Effacer, 1.0, 0.0);
        assert_eq!(px[(4 * 9 + 4) * 4 + 3], 0, "centre transparent");
        // Hors de l'image ou rayon absurde : rien.
        let copie = px.clone();
        peindre(
            &mut px,
            9,
            9,
            (-50.0, -50.0),
            3.0,
            PinceauCalque::Assombrir,
            1.0,
            0.0,
        );
        peindre(
            &mut px,
            9,
            9,
            (4.5, 4.5),
            f64::NAN,
            PinceauCalque::Assombrir,
            1.0,
            0.0,
        );
        assert_eq!(px, copie);
    }

    #[test]
    fn niveaux_et_detection_des_couleurs() {
        let n = Niveaux {
            gris_noir: 0.1,
            gris_blanc: 0.9,
            courbe: 2.0,
        };
        assert_eq!(n.appliquer(0.05), 0.0);
        assert_eq!(n.appliquer(0.95), 1.0);
        assert!((n.appliquer(0.5) - 0.25).abs() < 1e-12, "milieu au carré");
        assert!(n.valider().is_ok());
        assert!(Niveaux { courbe: 0.0, ..n }.valider().is_err());
        assert!(
            Niveaux {
                gris_noir: 0.9,
                gris_blanc: 0.1,
                ..n
            }
            .valider()
            .is_err()
        );
        let gris = image(2, 1, &[[10, 10, 10, 255], [200, 200, 200, 255]]);
        let couleurs = image(2, 1, &[[184, 217, 155, 255], [64, 150, 220, 255]]);
        assert_eq!(part_coloree(&gris), 0.0);
        assert_eq!(part_coloree(&couleurs), 1.0);
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
