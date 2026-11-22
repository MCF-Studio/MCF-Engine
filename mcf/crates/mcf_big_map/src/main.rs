//! Génère la grosse carte du test A.
//!
//! ```text
//! cargo big-map                                  # palier de départ, graine par défaut
//! cargo big-map -- --palier stress --graine 42 --sortie D:/cartes/stress_42
//! ```
//!
//! Sortie par défaut : `mcf/bench/maps/<palier>_<graine>/` (non versionné). Le dossier doit être absent ou vide.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use mcf_big_map::{Params, generate, write_dir};

fn parse() -> Result<(Params, Option<PathBuf>), String> {
    // Le palier est appliqué d'abord, les autres options ensuite : l'ordre des options ne compte pas.
    let (mut tier, mut seed, mut cell, mut out) = (None, None, None, None);
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut val = || it.next().ok_or_else(|| format!("valeur manquante après {flag}"));
        match flag.as_str() {
            "--palier" => tier = Some(val()?),
            "--graine" => {
                let v = val()?;
                seed = Some(v.parse::<u64>().map_err(|_| format!("graine invalide : {v}"))?);
            }
            "--cellule" => {
                let v = val()?;
                cell = Some(v.parse::<f64>().map_err(|_| format!("cellule invalide : {v}"))?);
            }
            "--sortie" => out = Some(PathBuf::from(val()?)),
            other => return Err(format!("option inconnue : {other}")),
        }
    }
    let mut p = match tier.as_deref() {
        None | Some("depart") => Params::start(),
        Some("stress") => Params::stress(),
        Some(other) => return Err(format!("palier inconnu : {other} (depart ou stress)")),
    };
    p.seed = seed.unwrap_or(p.seed);
    p.cell = cell.unwrap_or(p.cell);
    p.check()?;
    Ok((p, out))
}

fn main() -> ExitCode {
    let (params, out) = match parse() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    let dir = out.unwrap_or_else(|| {
        PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../bench/maps"))
            .join(format!("{}_{:016x}", params.tier, params.seed))
    });
    let t0 = Instant::now();
    let g = match generate(&params) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("génération impossible : {e}");
            return ExitCode::FAILURE;
        }
    };
    let t_gen = t0.elapsed();
    match write_dir(&g, &dir) {
        Ok(w) => {
            println!(
                "palier « {} » : {} éléments, {} fichiers ({:.1} Mio, plus gros {:.2} Mio), génération {:.2} s, total {:.2} s\n→ {}",
                params.tier,
                params.quantities.total(),
                w.files,
                w.bytes as f64 / 1_048_576.0,
                w.largest as f64 / 1_048_576.0,
                t_gen.as_secs_f64(),
                t0.elapsed().as_secs_f64(),
                dir.display()
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
