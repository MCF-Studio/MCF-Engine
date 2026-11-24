//! Calques de référence (`references.json` + `references/r_<id>.png`, spec §8) : une image (croquis, plan stylisé,
//! satmap) posée sous les yeux dans la vue Plan pour **décalquer** routes, relief et bâti. Un calque n'est jamais une
//! source de vérité : rien n'en est déduit automatiquement sans action explicite.
//!
//! Placement : le pixel `(px, py)` (origine en haut à gauche, `py` vers le bas) est posé en
//! `coin + R(rotation) · (px, py) · m_par_px`, dans le repère `(x, z)` de la carte (nord = −Z en haut du plan). Une
//! rotation positive tourne l'image dans le sens horaire vu du dessus.
//!
//! Images non fiables : dimensions lues dans l'en-tête et plafonnées **avant** décodage, poids plafonné, image
//! **réencodée** à l'import (aucun bloc annexe du fichier d'origine n'est conservé), nom de fichier dérivé de
//! l'identifiant (aucun chemin venu du JSON).

use std::collections::BTreeMap;
use std::io::Cursor;
use std::sync::Arc;

use mcf_geo::P2;
use serde::{Deserialize, Serialize};

use crate::ids::Id;
use crate::manifeste::Manifeste;

/// Valeur du champ `format`.
pub const FORMAT: &str = "mcfreferences";
/// Version du format.
pub const VERSION: u32 = 1;
/// Calques maximum sur une carte.
pub const MAX_REFERENCES: usize = 16;
/// Côté maximal d'une image (pixels).
pub const MAX_COTE_PX: u32 = 8192;
/// Pixels maximum pour l'ensemble des calques d'une carte (64 Mpx ≈ 256 Mo en RGBA) : sans ce budget, 16 images
/// de 8192² pixels réclameraient ~4 Go à l'ouverture d'une carte piégée.
pub const MAX_PIXELS_TOTAL: u64 = 64 * 1024 * 1024;
/// Poids maximal d'un fichier image (octets).
pub const MAX_OCTETS: u64 = 64 * 1024 * 1024;
/// Plage d'échelle autorisée (m par pixel).
pub const ECHELLE_M_PAR_PX: (f64, f64) = (0.001, 1000.0);
/// Longueur maximale d'un nom affiché.
pub const MAX_NOM: usize = 64;

/// Un calque de référence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reference {
    /// Nom affiché (texte libre, 64 caractères au plus, sans caractère de contrôle).
    pub nom: String,
    /// Largeur de l'image (pixels).
    pub largeur_px: u32,
    /// Hauteur de l'image (pixels).
    pub hauteur_px: u32,
    /// Position du coin haut gauche de l'image (m).
    pub coin: P2,
    /// Mètres par pixel.
    pub m_par_px: f64,
    /// Rotation (degrés, sens horaire vu du dessus).
    pub rotation_deg: f64,
    /// Opacité dans la vue Plan (0 à 1).
    pub opacite: f32,
    /// Affiché ?
    pub visible: bool,
}

impl Reference {
    fn axes(&self) -> ([f64; 2], [f64; 2]) {
        let (s, c) = self.rotation_deg.to_radians().sin_cos();
        // Axe des colonnes (px croissants) et des lignes (py croissants), en mètres par pixel.
        (
            [c * self.m_par_px, s * self.m_par_px],
            [-s * self.m_par_px, c * self.m_par_px],
        )
    }

    /// Position sur la carte du point `(px, py)` de l'image (pixels, réels).
    pub fn vers_carte(&self, px: f64, py: f64) -> P2 {
        let (u, v) = self.axes();
        [
            self.coin[0] + u[0] * px + v[0] * py,
            self.coin[1] + u[1] * px + v[1] * py,
        ]
    }

    /// Point de l'image (pixels, réels) sous la position `p` de la carte.
    pub fn vers_image(&self, p: P2) -> (f64, f64) {
        let (d0, d1) = (p[0] - self.coin[0], p[1] - self.coin[1]);
        let (s, c) = self.rotation_deg.to_radians().sin_cos();
        (
            (c * d0 + s * d1) / self.m_par_px,
            (-s * d0 + c * d1) / self.m_par_px,
        )
    }

    /// Coins de l'image sur la carte : haut gauche, haut droit, bas droit, bas gauche.
    pub fn coins(&self) -> [P2; 4] {
        let (l, h) = (f64::from(self.largeur_px), f64::from(self.hauteur_px));
        [
            self.vers_carte(0.0, 0.0),
            self.vers_carte(l, 0.0),
            self.vers_carte(l, h),
            self.vers_carte(0.0, h),
        ]
    }

    /// Le point `p` de la carte est-il sur l'image ?
    pub fn contient(&self, p: P2) -> bool {
        let (x, y) = self.vers_image(p);
        (0.0..=f64::from(self.largeur_px)).contains(&x) && (0.0..=f64::from(self.hauteur_px)).contains(&y)
    }

    /// Calage par une distance connue : les points `a` et `b` de la carte (posés sur deux repères de l'image) doivent
    /// être distants de `distance_m`. L'échelle change, `a` reste fixe sur l'image et la rotation est conservée.
    pub fn caler_distance(&mut self, a: P2, b: P2, distance_m: f64) -> Result<(), String> {
        if !(distance_m.is_finite() && distance_m > 0.0) {
            return Err("distance réelle invalide".into());
        }
        let (pa, pb) = (self.vers_image(a), self.vers_image(b));
        let px = (pb.0 - pa.0).hypot(pb.1 - pa.1);
        if px < 2.0 {
            return Err("les deux repères sont trop proches sur l'image (2 pixels au moins)".into());
        }
        let m = distance_m / px;
        if !(ECHELLE_M_PAR_PX.0..=ECHELLE_M_PAR_PX.1).contains(&m) {
            return Err(format!("échelle obtenue hors limites : {m} m/pixel"));
        }
        self.m_par_px = m;
        // `a` reste au même endroit de la carte : on recale le coin.
        let ici = self.vers_carte(pa.0, pa.1);
        self.coin = [self.coin[0] + a[0] - ici[0], self.coin[1] + a[1] - ici[1]];
        Ok(())
    }

    /// Vérifie les champs.
    pub fn valider(&self, m: &Manifeste) -> Result<(), String> {
        if self.nom.trim().is_empty()
            || self.nom.chars().count() > MAX_NOM
            || self.nom.chars().any(char::is_control)
        {
            return Err("nom vide, trop long ou avec caractère de contrôle".into());
        }
        for c in [self.largeur_px, self.hauteur_px] {
            if !(1..=MAX_COTE_PX).contains(&c) {
                return Err(format!("dimensions hors limites (1 à {MAX_COTE_PX} pixels)"));
            }
        }
        if !(ECHELLE_M_PAR_PX.0..=ECHELLE_M_PAR_PX.1).contains(&self.m_par_px) {
            return Err("échelle hors limites".into());
        }
        if !(self.rotation_deg.is_finite() && self.rotation_deg.abs() <= 360.0) {
            return Err("rotation hors limites (−360° à 360°)".into());
        }
        if !(0.0..=1.0).contains(&self.opacite) {
            return Err("opacité hors de 0 à 1".into());
        }
        // Le calque peut déborder de la carte, pas s'en éloigner sans limite.
        let (a, b) = m.bornes();
        let marge = (b[0] - a[0]).max(b[1] - a[1]) * 4.0;
        if !(self.coin[0].is_finite() && self.coin[1].is_finite())
            || self.coin[0] < a[0] - marge
            || self.coin[0] > b[0] + marge
            || self.coin[1] < a[1] - marge
            || self.coin[1] > b[1] + marge
        {
            return Err("position trop loin de la carte".into());
        }
        Ok(())
    }
}

/// Tous les calques de référence d'une carte.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct References {
    /// Toujours `mcfreferences`.
    pub format: String,
    /// Version.
    pub version: u32,
    /// Calques par identifiant, du dessous vers le dessus dans l'ordre des identifiants.
    pub references: BTreeMap<Id, Reference>,
}

impl Default for References {
    fn default() -> Self {
        Self {
            format: FORMAT.into(),
            version: VERSION,
            references: BTreeMap::new(),
        }
    }
}

impl References {
    /// Vérifie le fichier entier.
    pub fn valider(&self, m: &Manifeste) -> Result<(), String> {
        if self.format != FORMAT || self.version != VERSION {
            return Err(format!("format attendu : {FORMAT} v{VERSION}"));
        }
        if self.references.len() > MAX_REFERENCES {
            return Err(format!("{MAX_REFERENCES} calques de référence au plus"));
        }
        for (id, r) in &self.references {
            r.valider(m).map_err(|e| format!("calque {id} : {e}"))?;
        }
        // Vérifié sur les dimensions déclarées, avant tout décodage (voir `io::lire_references`).
        let pixels: u64 = self
            .references
            .values()
            .map(|r| u64::from(r.largeur_px) * u64::from(r.hauteur_px))
            .sum();
        if pixels > MAX_PIXELS_TOTAL {
            return Err(format!(
                "calques trop lourds : {pixels} pixels au total ({MAX_PIXELS_TOTAL} au plus)"
            ));
        }
        Ok(())
    }
}

/// Nom du fichier image d'un calque, dérivé de son identifiant (jamais lu dans un fichier).
pub fn nom_fichier(id: Id) -> String {
    format!("r_{:016x}.png", id.0)
}

/// Image RGBA 8 bits décodée.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageRgba {
    /// Largeur (pixels).
    pub largeur: u32,
    /// Hauteur (pixels).
    pub hauteur: u32,
    /// Pixels RGBA (sRGB), ligne par ligne depuis le haut.
    pub rgba: Arc<Vec<u8>>,
}

/// Décode un PNG non fiable en RGBA 8 bits. Les dimensions sont vérifiées dans l'en-tête avant tout décodage.
pub fn decoder_png(octets: &[u8]) -> Result<ImageRgba, String> {
    if octets.len() as u64 > MAX_OCTETS {
        return Err(format!("image trop lourde ({MAX_OCTETS} octets au plus)"));
    }
    let max = MAX_COTE_PX as usize;
    let mut dec = png::Decoder::new(Cursor::new(octets));
    // Plafond mémoire du décodeur : la plus grande image RGBA 16 bits autorisée.
    dec.set_limits(png::Limits { bytes: max * max * 8 });
    dec.set_transformations(png::Transformations::normalize_to_color8());
    let mut lecteur = dec.read_info().map_err(|e| format!("PNG refusé : {e}"))?;
    let (l, h) = lecteur.info().size();
    if !(1..=MAX_COTE_PX).contains(&l) || !(1..=MAX_COTE_PX).contains(&h) {
        return Err(format!("image {l}×{h} : {MAX_COTE_PX} pixels de côté au plus"));
    }
    let (couleur, profondeur) = lecteur.output_color_type();
    if profondeur != png::BitDepth::Eight {
        return Err("profondeur de couleur non gérée".into());
    }
    let taille = lecteur
        .output_buffer_size()
        .ok_or("image trop grande pour la mémoire")?;
    let mut tampon = vec![0u8; taille];
    let info = lecteur
        .next_frame(&mut tampon)
        .map_err(|e| format!("PNG refusé : {e}"))?;
    let n = (l as usize) * (h as usize);
    let ligne = info.line_size;
    let canaux = match couleur {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Indexed => return Err("palette non développée".into()),
    };
    let mut rgba = Vec::with_capacity(n * 4);
    for y in 0..h as usize {
        let debut = y * ligne;
        let pixels = tampon
            .get(debut..debut + l as usize * canaux)
            .ok_or("PNG tronqué")?;
        for p in pixels.chunks_exact(canaux) {
            let px = match canaux {
                1 => [p[0], p[0], p[0], 255],
                2 => [p[0], p[0], p[0], p[1]],
                3 => [p[0], p[1], p[2], 255],
                _ => [p[0], p[1], p[2], p[3]],
            };
            rgba.extend_from_slice(&px);
        }
    }
    Ok(ImageRgba {
        largeur: l,
        hauteur: h,
        rgba: Arc::new(rgba),
    })
}

/// Encode une image RGBA en PNG (réencodage propre : aucun bloc annexe).
pub fn encoder_png(image: &ImageRgba) -> Result<Vec<u8>, String> {
    let mut sortie = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut sortie, image.largeur, image.hauteur);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().map_err(|e| e.to_string())?;
        w.write_image_data(&image.rgba).map_err(|e| e.to_string())?;
        w.finish().map_err(|e| e.to_string())?;
    }
    Ok(sortie)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r() -> Reference {
        Reference {
            nom: "plan".into(),
            largeur_px: 1000,
            hauteur_px: 500,
            coin: [10.0, 20.0],
            m_par_px: 2.0,
            rotation_deg: 30.0,
            opacite: 0.6,
            visible: true,
        }
    }

    fn proche(a: P2, b: P2) -> bool {
        (a[0] - b[0]).abs() < 1e-6 && (a[1] - b[1]).abs() < 1e-6
    }

    #[test]
    fn aller_retour_image_carte() {
        let r = r();
        let p = r.vers_carte(123.0, 45.0);
        let (x, y) = r.vers_image(p);
        assert!((x - 123.0).abs() < 1e-9 && (y - 45.0).abs() < 1e-9);
        assert!(r.contient(p));
        assert!(!r.contient(r.vers_carte(-1.0, 10.0)));
    }

    #[test]
    fn sans_rotation_l_image_suit_les_axes_du_plan() {
        let mut r = r();
        r.rotation_deg = 0.0;
        assert_eq!(
            r.coins(),
            [[10.0, 20.0], [2010.0, 20.0], [2010.0, 1020.0], [10.0, 1020.0]]
        );
        // 90° horaire vu du dessus (+X à droite, +Z vers le bas de l'écran) : le haut de l'image part vers +Z.
        r.rotation_deg = 90.0;
        assert!(proche(r.vers_carte(1000.0, 0.0), [10.0, 2020.0]));
    }

    #[test]
    fn calage_par_distance_garde_le_premier_point() {
        let mut r = r();
        let (a, b) = (r.vers_carte(100.0, 100.0), r.vers_carte(400.0, 500.0));
        // 500 pixels entre les deux repères : 1 km → 2 m/pixel… on veut 4 m/pixel.
        r.caler_distance(a, b, 2000.0).expect("calage valide");
        assert!((r.m_par_px - 4.0).abs() < 1e-9);
        assert!(proche(r.vers_carte(100.0, 100.0), a));
        assert!((r.rotation_deg - 30.0).abs() < 1e-12);
        assert!(r.caler_distance(a, a, 10.0).is_err());
        assert!(r.caler_distance(a, b, -1.0).is_err());
    }

    #[test]
    fn validation_des_bornes() {
        let m = Manifeste::nouveau("t", 512.0);
        assert!(r().valider(&m).is_ok());
        for f in [
            |r: &mut Reference| r.m_par_px = 0.0,
            |r: &mut Reference| r.m_par_px = f64::NAN,
            |r: &mut Reference| r.opacite = 1.5,
            |r: &mut Reference| r.largeur_px = 0,
            |r: &mut Reference| r.hauteur_px = MAX_COTE_PX + 1,
            |r: &mut Reference| r.rotation_deg = f64::INFINITY,
            |r: &mut Reference| r.coin = [1e9, 0.0],
            |r: &mut Reference| r.nom = "a\nb".into(),
            |r: &mut Reference| r.nom = "x".repeat(MAX_NOM + 1),
        ] {
            let mut x = r();
            f(&mut x);
            assert!(x.valider(&m).is_err(), "{x:?}");
        }
        // Budget total : deux images de 8192² pixels dépassent 64 Mpx.
        let mut lourd = References::default();
        for _ in 0..2 {
            let mut x = r();
            x.largeur_px = MAX_COTE_PX;
            x.hauteur_px = MAX_COTE_PX;
            lourd.references.insert(Id::nouveau(), x);
        }
        assert!(lourd.valider(&m).is_err());
        let mut tout = References::default();
        for _ in 0..=MAX_REFERENCES {
            tout.references.insert(Id::nouveau(), r());
        }
        assert!(tout.valider(&m).is_err());
    }

    #[test]
    fn champ_inconnu_refuse() {
        let texte = r#"{"format":"mcfreferences","version":1,"references":{},"chemin":"../x"}"#;
        assert!(serde_json::from_str::<References>(texte).is_err());
    }

    #[test]
    fn png_aller_retour_et_conversion_rgba() {
        let image = ImageRgba {
            largeur: 3,
            hauteur: 2,
            rgba: Arc::new((0..24).map(|i| i as u8 * 10).collect()),
        };
        let octets = encoder_png(&image).expect("encodage");
        assert_eq!(decoder_png(&octets).expect("décodage"), image);
        // RVB sans alpha → alpha opaque.
        let mut rvb = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut rvb, 1, 1);
            enc.set_color(png::ColorType::Rgb);
            let mut w = enc.write_header().expect("en-tête");
            w.write_image_data(&[1, 2, 3]).expect("données");
        }
        assert_eq!(*decoder_png(&rvb).expect("rvb").rgba, vec![1, 2, 3, 255]);
    }

    #[test]
    fn png_geant_refuse_avant_decodage() {
        // En-tête annonçant 100 000 × 100 000 pixels, sans données : refusé sur l'en-tête.
        let mut octets = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut octets, 1, 1);
            enc.set_color(png::ColorType::Rgba);
            let mut w = enc.write_header().expect("en-tête");
            w.write_image_data(&[0, 0, 0, 0]).expect("données");
        }
        // IHDR : 8 (signature) + 4 (longueur) + 4 (type) → largeur, hauteur ; le CRC est recalculé.
        octets[16..20].copy_from_slice(&100_000u32.to_be_bytes());
        octets[20..24].copy_from_slice(&100_000u32.to_be_bytes());
        let crc = crc32(&octets[12..29]);
        octets[29..33].copy_from_slice(&crc.to_be_bytes());
        let e = decoder_png(&octets).expect_err("doit être refusé");
        assert!(e.contains("100000") || e.contains("refusé"), "{e}");
        assert!(decoder_png(b"pas un png").is_err());
    }

    fn crc32(octets: &[u8]) -> u32 {
        let mut crc = 0xFFFF_FFFFu32;
        for &b in octets {
            crc ^= u32::from(b);
            for _ in 0..8 {
                crc = if crc & 1 != 0 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }
}
