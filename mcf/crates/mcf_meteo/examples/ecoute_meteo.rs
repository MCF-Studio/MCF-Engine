//! Écoute de l'ambiance sonore sans lancer le jeu : écrit un fichier WAV de 20 s par préréglage météo (sur la plage,
//! à 15 m du bord), plus un orage avec tonnerre, et mesure le coût de la synthèse.
//!
//! `cargo run -p mcf_meteo --example ecoute_meteo -- dossier_de_sortie`

use std::io::Write;
use std::time::Instant;

use mcf_meteo::houle::PERIODE_HOULE_S;
use mcf_meteo::orage::Eclair;
use mcf_meteo::rivage::params_bord;
use mcf_meteo::son::{EntreesSon, FREQUENCE, SynthAmbiance};
use mcf_meteo::{ParametresRivage, PresetMeteo, ReglagesSon};

fn ecrire_wav(chemin: &std::path::Path, echantillons: &[f32]) -> std::io::Result<()> {
    let donnees = (echantillons.len() * 2) as u32;
    let mut f = std::io::BufWriter::new(std::fs::File::create(chemin)?);
    f.write_all(b"RIFF")?;
    f.write_all(&(36 + donnees).to_le_bytes())?;
    f.write_all(b"WAVEfmt ")?;
    f.write_all(&16u32.to_le_bytes())?;
    f.write_all(&1u16.to_le_bytes())?; // PCM
    f.write_all(&2u16.to_le_bytes())?; // stéréo
    f.write_all(&FREQUENCE.to_le_bytes())?;
    f.write_all(&(FREQUENCE * 4).to_le_bytes())?;
    f.write_all(&4u16.to_le_bytes())?;
    f.write_all(&16u16.to_le_bytes())?;
    f.write_all(b"data")?;
    f.write_all(&donnees.to_le_bytes())?;
    for v in echantillons {
        f.write_all(&((v.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes())?;
    }
    f.flush()
}

fn main() -> std::io::Result<()> {
    let dossier = std::path::PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| ".".into()));
    std::fs::create_dir_all(&dossier)?;
    let reglages = ReglagesSon::default();
    let secondes = 20.0;
    let images = (secondes * FREQUENCE as f32) as usize;
    let mut total_s = 0.0;
    let mut total_audio = 0.0;
    for (nom, preset, orage) in [
        ("clair", PresetMeteo::Clair, false),
        ("nuageux", PresetMeteo::Nuageux, false),
        ("pluie", PresetMeteo::Pluie, false),
        ("tempete", PresetMeteo::Tempete, true),
        ("neige", PresetMeteo::Neige, false),
    ] {
        let etat = preset.etat();
        let b = params_bord(&etat, &ParametresRivage::default());
        let mut e = EntreesSon {
            vent_vitesse: etat.vent_vitesse,
            rafales: etat.rafales,
            precipitations: etat.precipitations,
            facteur_neige: etat.facteur_neige(),
            amplitude_bord: b.amplitude,
            pulsation_bord: b.omega,
            distance_rivage: 15.0,
            hauteur: 1.7,
            ..Default::default()
        };
        let mut synth = SynthAmbiance::default();
        if orage {
            for (instant, proximite) in [(3.0, 0.3), (11.0, 0.9)] {
                let eclair = Eclair::new(proximite);
                synth.tonnerre(instant + eclair.delai_tonnerre_s, proximite);
            }
        }
        let mut sortie = vec![0.0f32; images * 2];
        let debut = Instant::now();
        for (i, bloc) in sortie.chunks_mut(960).enumerate() {
            e.temps_ocean = (i as f32 * 0.01) % PERIODE_HOULE_S;
            synth.rendre(&e, &reglages, bloc);
        }
        total_s += debut.elapsed().as_secs_f32();
        total_audio += secondes;
        let chemin = dossier.join(format!("ambiance_{nom}.wav"));
        ecrire_wav(&chemin, &sortie)?;
        println!("{}", chemin.display());
    }
    println!(
        "Coût de la synthèse : {:.2} % d'un cœur ({:.1} ms par seconde de son)",
        total_s / total_audio * 100.0,
        total_s / total_audio * 1000.0
    );
    Ok(())
}
