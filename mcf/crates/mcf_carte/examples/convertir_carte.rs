//! Convertit une image (vue satellite, plan illustré) vers les deux images de la convention des cartes sources
//! (`docs/CONVENTION_CARTES_SOURCE.md`) : `<nom>_couleurs.png` et `<nom>_relief.png`, à côté de l'image ou dans le
//! dossier donné. Relief **proposé** (déduit, pas mesuré), routes non extraites : voir `mcf_carte::conversion`.
//!
//! ```text
//! cargo run -p mcf_carte --example convertir_carte --release -- <image.png> [dossier de sortie]
//! ```

use std::path::{Path, PathBuf};
use std::time::Instant;

use mcf_carte::conversion::{self, Reglages};
use mcf_carte::references::{decoder_png, encoder_png};

fn main() -> Result<(), String> {
    let mut args = std::env::args_os().skip(1);
    let source = args
        .next()
        .map(PathBuf::from)
        .ok_or("usage : convertir_carte <image.png> [dossier de sortie]")?;
    let sortie = args
        .next()
        .map(PathBuf::from)
        .or_else(|| source.parent().map(Path::to_path_buf))
        .unwrap_or_default();
    let nom = source
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or("nom de fichier illisible")?;
    let octets = std::fs::read(&source).map_err(|e| format!("{} : {e}", source.display()))?;
    let image = decoder_png(&octets)?;
    let debut = Instant::now();
    let c = conversion::convertir(&image, &Reglages::default())?;
    println!(
        "Converti {} ({} × {} px) en {:?}.",
        source.display(),
        image.largeur,
        image.hauteur,
        debut.elapsed()
    );
    for (classe, part) in &c.parts {
        println!("  {:<14} {:5.1} %", classe.nom(), part * 100.0);
    }
    std::fs::create_dir_all(&sortie).map_err(|e| e.to_string())?;
    for (suffixe, img) in [("couleurs", &c.couleurs), ("relief", &c.relief)] {
        let chemin = sortie.join(format!("{nom}_{suffixe}.png"));
        std::fs::write(&chemin, encoder_png(img)?).map_err(|e| format!("{} : {e}", chemin.display()))?;
        println!("Écrit {}", chemin.display());
    }
    println!(
        "Relief proposé : noir = {} m, blanc = {} m (à reporter dans l'éditeur). Routes non extraites.",
        c.noir_m, c.blanc_m
    );
    Ok(())
}
