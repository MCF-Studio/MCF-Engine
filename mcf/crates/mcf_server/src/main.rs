//! Serveur dédié MCF (phase 1 : squelette). Headless : aucune fenêtre, aucun GPU.
//!
//! Configuration par variables d'environnement (conteneur) :
//! - `MCF_TICK_RATE` : ticks par seconde (1 à 240, défaut 30).

use std::time::Duration;

use bevy::app::ScheduleRunnerPlugin;
use bevy::log::LogPlugin;
use bevy::prelude::*;
use mcf_core::McfCorePlugin;

const DEFAULT_TICK_RATE: u32 = 30;

/// Ticks par seconde demandés.
#[derive(Resource, Debug, Clone, Copy)]
struct TickRate(u32);

fn tick_rate_from_env() -> Result<u32, String> {
    match std::env::var("MCF_TICK_RATE") {
        Err(_) => Ok(DEFAULT_TICK_RATE),
        Ok(s) => match s.trim().parse::<u32>() {
            Ok(n) if (1..=240).contains(&n) => Ok(n),
            _ => Err(format!(
                "MCF_TICK_RATE invalide : « {s} » (attendu : entier de 1 à 240)"
            )),
        },
    }
}

fn main() -> AppExit {
    let rate = match tick_rate_from_env() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{e}");
            return AppExit::error();
        }
    };
    App::new()
        .add_plugins(
            MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_secs_f64(
                1.0 / f64::from(rate),
            ))),
        )
        .add_plugins(LogPlugin::default())
        .add_plugins(McfCorePlugin)
        .insert_resource(TickRate(rate))
        .add_systems(Startup, |rate: Res<TickRate>| {
            info!("serveur MCF démarré : {} ticks/s visés", rate.0)
        })
        .add_systems(Update, report_tick_rate)
        .run()
}

/// Toutes les 10 s : ticks réellement effectués (premier indicateur de santé du serveur).
fn report_tick_rate(time: Res<Time<Real>>, mut ticks: Local<u32>, mut last: Local<f64>) {
    *ticks += 1;
    let now = time.elapsed_secs_f64();
    if now - *last >= 10.0 {
        info!("{:.1} ticks/s", f64::from(*ticks) / (now - *last));
        *ticks = 0;
        *last = now;
    }
}
