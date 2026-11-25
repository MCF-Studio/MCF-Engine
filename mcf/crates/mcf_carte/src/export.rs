//! Rendus du plan stylisé (spec §8) : tuiles PNG du plan complet et pyramide de tuiles « minimap » pour le jeu.
//! Rendu logiciel depuis les données (pas de capture d'écran) : déterministe, faisable sans carte graphique.

use std::fs;
use std::path::PathBuf;

use mcf_geo::{ClasseRoute, NatureBande, P2};
use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Stroke, Transform};

use crate::carte::Carte;
use crate::relief::{self, Relief};

/// Style de rendu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StylePlan {
    /// Plan de conception : relief estompé, courbes de niveau, quartiers.
    Complet,
    /// Minimap / GPS du jeu : lisible en petit, sans courbes de niveau.
    Jeu,
}

/// Côté d'une tuile exportée (pixels).
pub const TUILE_PX: u32 = 1024;
/// Pixels maximum d'un rendu (garde-fou mémoire).
pub const MAX_PIXELS: u64 = 8192 * 8192;

fn couleur(c: [u8; 3], a: f32) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(Color::from_rgba8(c[0], c[1], c[2], (a * 255.0) as u8));
    p.anti_alias = true;
    p
}

fn chemin(poly: &[P2], ferme: bool) -> Option<tiny_skia::Path> {
    let mut pb = PathBuilder::new();
    let (first, rest) = poly.split_first()?;
    pb.move_to(first[0] as f32, first[1] as f32);
    for p in rest {
        pb.line_to(p[0] as f32, p[1] as f32);
    }
    if ferme {
        pb.close();
    }
    pb.finish()
}

fn rang(c: ClasseRoute) -> usize {
    ClasseRoute::TOUTES.iter().position(|x| *x == c).unwrap_or(0)
}

/// Rend une zone de la carte. `coin` : coin `(x, z)` minimal (nord-ouest), image orientée nord en haut.
pub fn rendre(
    carte: &mut Carte,
    coin: P2,
    largeur: u32,
    hauteur: u32,
    m_par_px: f64,
    style: StylePlan,
) -> Result<Pixmap, String> {
    if !(m_par_px.is_finite() && m_par_px > 0.0) || u64::from(largeur) * u64::from(hauteur) > MAX_PIXELS {
        return Err("rendu hors bornes".into());
    }
    let mut pix = Pixmap::new(largeur, hauteur).ok_or("image vide")?;
    let eau = carte.manifeste.niveau_eau_m;
    let profils = carte.manifeste.profils;
    let quartiers = carte.quartiers.clone();
    let batiments: Vec<_> = carte.batiments.values().cloned().collect();
    let reseau = carte.reseau.clone();
    let d = carte.derive();
    // 1. Terrain estompé, pixel par pixel.
    fond(&mut pix, &d.terrain, coin, m_par_px, eau, style);
    let t = Transform::from_row(
        (1.0 / m_par_px) as f32,
        0.0,
        0.0,
        (1.0 / m_par_px) as f32,
        (-coin[0] / m_par_px) as f32,
        (-coin[1] / m_par_px) as f32,
    );
    let remplir = |pix: &mut Pixmap, poly: &[P2], c: [u8; 3], a: f32| {
        if let Some(p) = chemin(poly, true) {
            pix.fill_path(&p, &couleur(c, a), FillRule::Winding, t, None);
        }
    };
    let trait_ = |pix: &mut Pixmap, poly: &[P2], ferme: bool, c: [u8; 3], a: f32, largeur_m: f64| {
        if let Some(p) = chemin(poly, ferme) {
            let s = Stroke {
                width: largeur_m as f32,
                ..Stroke::default()
            };
            pix.stroke_path(&p, &couleur(c, a), &s, t, None);
        }
    };
    // 2. Courbes de niveau.
    if style == StylePlan::Complet {
        let pas = ((m_par_px * 2.0) / d.terrain.resolution).round().max(1.0) as usize;
        for (a, b, h) in relief::courbes_de_niveau(&d.terrain, 5.0, pas) {
            let maitresse = (h / 25.0).fract().abs() < 1e-6;
            trait_(
                &mut pix,
                &[a, b],
                false,
                [140, 110, 80],
                if maitresse { 0.55 } else { 0.3 },
                m_par_px * if maitresse { 1.4 } else { 0.8 },
            );
        }
    }
    // 3. Quartiers.
    let alpha_q = if style == StylePlan::Complet { 0.55 } else { 0.8 };
    for a in quartiers.affectations.values() {
        if let Some(i) = crate::ilots::ilot_en(&d.ilots, a.repere) {
            let poly = i.interieur.as_ref().unwrap_or(&i.contour);
            remplir(&mut pix, poly, a.famille.couleur_plan(), alpha_q);
        }
    }
    for z in quartiers.zones.values() {
        remplir(&mut pix, &z.polygone, z.famille.couleur_plan(), alpha_q);
    }
    // 4. Routes : emprises (contour sombre), puis chaussées, des plus modestes aux plus larges.
    let mut ordre: Vec<_> = d.geo.troncons.iter().collect();
    ordre.sort_by_key(|(_, g)| rang(g.classe));
    let classe_noeud = |id: &crate::ids::Id| {
        reseau
            .troncons
            .values()
            .filter(|t| t.de == *id || t.vers == *id)
            .map(|t| t.classe)
            .max_by_key(|c| rang(*c))
            .unwrap_or(ClasseRoute::Rue)
    };
    let bord = [96, 96, 96];
    let trottoir = if style == StylePlan::Complet {
        [226, 222, 214]
    } else {
        [240, 238, 232]
    };
    for (_, g) in &ordre {
        for b in &g.bandes {
            let mut poly = b.gauche.clone();
            poly.extend(b.droite.iter().rev());
            trait_(&mut pix, &poly, true, bord, 0.8, m_par_px * 1.2);
        }
    }
    for n in d.geo.noeuds.values() {
        if !n.carrefour.emprise.is_empty() {
            trait_(&mut pix, &n.carrefour.emprise, true, bord, 0.8, m_par_px * 1.2);
        }
    }
    for (_, g) in &ordre {
        for b in &g.bandes {
            let mut poly = b.gauche.clone();
            poly.extend(b.droite.iter().rev());
            let c = match b.nature {
                NatureBande::Chaussee => profils.get(g.classe).couleur_plan,
                NatureBande::TerrePlein => mcf_geo::COULEUR_TERRE_PLEIN,
                NatureBande::CoteGauche | NatureBande::CoteDroit => trottoir,
            };
            remplir(&mut pix, &poly, c, 1.0);
        }
    }
    for (id, n) in &d.geo.noeuds {
        if n.carrefour.emprise.is_empty() {
            continue;
        }
        remplir(&mut pix, &n.carrefour.emprise, trottoir, 1.0);
        remplir(
            &mut pix,
            &n.carrefour.chaussee,
            profils.get(classe_noeud(id)).couleur_plan,
            1.0,
        );
        if !n.ilot.is_empty() {
            remplir(&mut pix, &n.ilot, [170, 214, 150], 1.0);
        }
    }
    // 5. Réseau électrique : un trait fin, par-dessus les routes.
    for l in reseau.lignes_electriques.values() {
        trait_(
            &mut pix,
            &l.points,
            false,
            crate::reseau::COULEUR_ELECTRIQUE,
            0.9,
            m_par_px * 1.6,
        );
    }
    // 6. Bâti : ombre portée vers le sud-est, puis toit et contour.
    for b in &batiments {
        let decal = (b.hauteur() * 0.15).clamp(0.5, 12.0);
        let ombre: Vec<P2> = b.empreinte.iter().map(|p| [p[0] + decal, p[1] + decal]).collect();
        remplir(&mut pix, &ombre, [40, 40, 40], 0.18);
    }
    for b in &batiments {
        let toit = if style == StylePlan::Complet {
            [205, 198, 190]
        } else {
            [215, 210, 200]
        };
        remplir(&mut pix, &b.empreinte, toit, 1.0);
        trait_(&mut pix, &b.empreinte, true, [110, 104, 98], 1.0, m_par_px);
    }
    Ok(pix)
}

fn fond(pix: &mut Pixmap, terrain: &Relief, coin: P2, mpp: f64, eau: Option<f64>, style: StylePlan) {
    let w = pix.width() as usize;
    let donnees = pix.data_mut();
    for (k, px) in donnees.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let (u, v) = (k % w, k / w);
        let p = [coin[0] + (u as f64 + 0.5) * mpp, coin[1] + (v as f64 + 0.5) * mpp];
        let h = terrain.hauteur(p);
        let mut c = relief::couleur_plan(h, terrain.normale(p), eau);
        if style == StylePlan::Jeu {
            c = c.map(|x| 0.35 + x * 0.6);
        }
        px.copy_from_slice(&[
            (c[0] * 255.0) as u8,
            (c[1] * 255.0) as u8,
            (c[2] * 255.0) as u8,
            255,
        ]);
    }
}

/// PNG RGBA d'une image (pixels démultipliés).
pub fn png_rgba(pix: &Pixmap) -> Result<Vec<u8>, String> {
    let mut rgba = Vec::with_capacity(pix.data().len());
    for p in pix.pixels() {
        let c = p.demultiply();
        rgba.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
    }
    let mut sortie = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut sortie, pix.width(), pix.height());
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().map_err(|e| e.to_string())?;
        w.write_image_data(&rgba).map_err(|e| e.to_string())?;
    }
    Ok(sortie)
}

/// Exporte le plan en tuiles de 1024 px dans `exports/plan` (complet) ou en pyramide `exports/minimap/<niveau>`
/// (jeu : niveau 0 = `m_par_px`, chaque niveau suivant deux fois moins détaillé, jusqu'à une seule tuile).
/// Refuse d'écraser un export existant sans `ecraser`.
pub fn exporter(
    carte: &mut Carte,
    m_par_px: f64,
    style: StylePlan,
    ecraser: bool,
) -> Result<Vec<PathBuf>, String> {
    let Some(projet) = carte.dossier.clone() else {
        return Err("enregistrez la carte avant d'exporter".into());
    };
    let racine = projet.join("exports").join(match style {
        StylePlan::Complet => "plan",
        StylePlan::Jeu => "minimap",
    });
    if racine.exists() && !ecraser {
        return Err(format!(
            "{} existe déjà : confirmez le remplacement",
            racine.display()
        ));
    }
    if racine.exists() {
        fs::remove_dir_all(&racine).map_err(|e| e.to_string())?;
    }
    let (coin, fin) = carte.manifeste.bornes();
    let mut fichiers = Vec::new();
    let mut mpp = m_par_px;
    let mut niveau = 0;
    loop {
        let (wx, wz) = (
            ((fin[0] - coin[0]) / mpp).ceil() as u32,
            ((fin[1] - coin[1]) / mpp).ceil() as u32,
        );
        let dossier = match style {
            StylePlan::Complet => racine.clone(),
            StylePlan::Jeu => racine.join(niveau.to_string()),
        };
        fs::create_dir_all(&dossier).map_err(|e| e.to_string())?;
        for tz in 0..wz.div_ceil(TUILE_PX) {
            for tx in 0..wx.div_ceil(TUILE_PX) {
                let c = [
                    coin[0] + f64::from(tx * TUILE_PX) * mpp,
                    coin[1] + f64::from(tz * TUILE_PX) * mpp,
                ];
                let pix = rendre(carte, c, TUILE_PX, TUILE_PX, mpp, style)?;
                let chemin = dossier.join(format!("{tx}_{tz}.png"));
                mcf_map::write_atomic(&chemin, &png_rgba(&pix)?).map_err(|e| e.to_string())?;
                fichiers.push(chemin);
            }
        }
        if style == StylePlan::Complet || (wx <= TUILE_PX && wz <= TUILE_PX) || niveau >= 12 {
            break;
        }
        mpp *= 2.0;
        niveau += 1;
    }
    Ok(fichiers)
}

/// Aperçu d'ensemble (≤ 2048 px de côté).
pub fn apercu(carte: &mut Carte, style: StylePlan) -> Result<Vec<u8>, String> {
    let (coin, fin) = carte.manifeste.bornes();
    let cote = (fin[0] - coin[0]).max(fin[1] - coin[1]);
    let mpp = (cote / 2048.0).max(0.25);
    let (w, h) = (
        ((fin[0] - coin[0]) / mpp).ceil() as u32,
        ((fin[1] - coin[1]) / mpp).ceil() as u32,
    );
    png_rgba(&rendre(carte, coin, w, h, mpp, style)?)
}

/// Écrit l'aperçu dans `exports/apercu_<style>.png`.
pub fn exporter_apercu(carte: &mut Carte, style: StylePlan) -> Result<PathBuf, String> {
    let Some(projet) = carte.dossier.clone() else {
        return Err("enregistrez la carte avant d'exporter".into());
    };
    let octets = apercu(carte, style)?;
    let chemin: PathBuf = projet.join("exports").join(match style {
        StylePlan::Complet => "apercu_plan.png",
        StylePlan::Jeu => "apercu_minimap.png",
    });
    mcf_map::write_atomic(&chemin, &octets).map_err(|e| e.to_string())?;
    Ok(chemin)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gabarits::{Densite, Famille};
    use crate::manifeste::Manifeste;

    #[test]
    fn le_plan_montre_routes_quartiers_et_bati() {
        let mut c = Carte::nouvelle(Manifeste::nouveau("rendu", 512.0)).expect("carte");
        for k in [-100.0, 0.0, 100.0] {
            c.tracer_route(ClasseRoute::Rue, &[[-100.0, k], [100.0, k]], 2.0)
                .expect("h");
            c.tracer_route(ClasseRoute::Avenue, &[[k, -100.0], [k, 100.0]], 2.0)
                .expect("v");
        }
        let a = c
            .affecter([50.0, 50.0], Famille::Residentiel, Densite::Faible)
            .expect("affecter");
        assert!(c.lotir(a).expect("lotir") > 0);
        let pix = rendre(&mut c, [-128.0, -128.0], 256, 256, 1.0, StylePlan::Complet).expect("rendu");
        let px = |x: f64, z: f64| {
            let (u, v) = ((x + 128.0) as u32, (z + 128.0) as u32);
            let p = pix.pixel(u, v).expect("pixel").demultiply();
            [p.red(), p.green(), p.blue()]
        };
        // Chaussée de rue (blanc) à mi-chemin entre deux carrefours, chaussée d'avenue (orange).
        assert_eq!(px(50.0, -100.0), [255, 255, 255]);
        assert_eq!(px(0.0, 50.0), [245, 160, 70]);
        // Un bâtiment généré se voit sur le plan.
        let b = c.batiments.values().next().expect("bâtiment").centre();
        assert_eq!(px(b[0], b[1]), [205, 198, 190]);
        assert!(png_rgba(&pix).expect("png").starts_with(b"\x89PNG"));
    }
}
