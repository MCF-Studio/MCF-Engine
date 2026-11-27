use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use bevy::diagnostic::{DiagnosticsStore, SystemInfo, SystemInformationDiagnosticsPlugin};
use bevy::prelude::*;
use bevy::render::diagnostic::RenderDiagnosticsPlugin;
use bevy::render::renderer::RenderAdapterInfo;
use bevy::window::PrimaryWindow;

use std::collections::{BTreeMap, HashMap};

use bevy::platform::time::Instant;

use crate::{
    CameraPath, FrameSample, FrameStats, SeriesStats, aggregate_render, frames_csv, gpu_par_passe, passes_csv,
};

/// Réglages d'une mesure.
#[derive(Resource, Debug, Clone)]
pub struct BenchSettings {
    /// Nom court de la mesure (dossier de résultats).
    pub name: String,
    /// Chauffe avant de mesurer (chargement des shaders, remplissage des caches), en secondes.
    pub warmup_secs: f32,
    /// Durée de mesure si aucune [`BenchCamera`] n'impose la sienne.
    pub duration_secs: f32,
    /// Dossier de résultats (un sous-dossier par mesure).
    pub out_dir: PathBuf,
    /// Paramètres de la scène, recopiés tels quels dans le résumé (nombre d'objets, options…).
    pub notes: Vec<(String, String)>,
}

impl BenchSettings {
    /// Réglages par défaut : 5 s de chauffe, résultats dans `mcf/bench/results` (ou `MCF_BENCH_OUT`).
    pub fn new(name: &str) -> Self {
        let out_dir = std::env::var_os("MCF_BENCH_OUT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../bench/results")));
        Self {
            name: name.to_owned(),
            warmup_secs: 5.0,
            duration_secs: 30.0,
            out_dir,
            notes: Vec::new(),
        }
    }

    /// Ajoute un paramètre au résumé.
    pub fn note(mut self, key: &str, value: impl ToString) -> Self {
        self.notes.push((key.to_owned(), value.to_string()));
        self
    }
}

/// Caméra pilotée par un parcours rejouable pendant la mesure.
#[derive(Component, Debug, Clone)]
pub struct BenchCamera(pub CameraPath);

/// Rejoue le parcours, enregistre le temps de chaque image, écrit `frames.csv` + `summary.txt`, ajoute une
/// ligne à `summary.csv`, puis quitte l'application.
pub struct BenchPlugin(pub BenchSettings);

impl Plugin for BenchPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<SystemInformationDiagnosticsPlugin>() {
            app.add_plugins(SystemInformationDiagnosticsPlugin);
        }
        // Temps CPU/GPU et statistiques de pipeline par passe de rendu. Bevy ne l'ajoute lui-même que
        // avec la feature `tracing-tracy` : on l'ajoute ici s'il manque (sans doublon).
        if !app.is_plugin_added::<RenderDiagnosticsPlugin>() {
            app.add_plugins(RenderDiagnosticsPlugin);
        }
        app.insert_resource(self.0.clone())
            .init_resource::<Recording>()
            .add_systems(Update, (drive_camera, record).chain());
    }
}

#[derive(Resource, Default)]
struct Recording {
    /// Instant réel (s) du début de la mesure, après la chauffe.
    start: Option<f64>,
    frames: Vec<FrameSample>,
    /// Temps GPU par passe de premier niveau, une valeur par image mesurée.
    passes: BTreeMap<String, Vec<Option<f64>>>,
    /// Instant de la dernière mesure lue pour chaque diagnostic de rendu : une passe qui ne tourne pas à chaque
    /// image (cuisson unique d'une texture…) laisse sa dernière valeur dans le `DiagnosticsStore` ; sans ce
    /// suivi, elle serait recomptée à chaque image.
    vus: HashMap<String, Instant>,
    done: bool,
}

/// Dernière valeur connue d'un diagnostic (chemin donné), s'il existe.
fn latest(store: Option<&DiagnosticsStore>, path: &bevy::diagnostic::DiagnosticPath) -> Option<f64> {
    store?.get(path)?.value()
}

/// Ajoute au résumé les moyenne / p99 / pic d'une série (`n/d` si la mesure est indisponible : les colonnes de
/// `summary.csv` doivent rester identiques d'une mesure à l'autre).
fn push_series(
    info: &mut Vec<(String, String)>,
    key: &str,
    unit: &str,
    stats: Option<SeriesStats>,
    prec: usize,
) {
    let fmt = |v: Option<f64>| v.map_or_else(|| "n/d".to_owned(), |x| format!("{x:.prec$}"));
    info.push((format!("{key}_moyen{unit}"), fmt(stats.map(|s| s.mean))));
    info.push((format!("{key}_p99{unit}"), fmt(stats.map(|s| s.p99))));
    info.push((format!("{key}_pic{unit}"), fmt(stats.map(|s| s.max))));
}

/// Diagnostics de rendu (`render/…`) mesurés depuis le dernier appel, d'après l'instant de leur dernière mesure.
fn mesures_fraiches(
    store: Option<&DiagnosticsStore>,
    vus: &mut HashMap<String, Instant>,
) -> Vec<(String, f64)> {
    let mut frais = Vec::new();
    for d in store.into_iter().flat_map(DiagnosticsStore::iter) {
        let chemin = d.path().as_str();
        if !chemin.starts_with("render/") {
            continue;
        }
        let Some(m) = d.measurement() else {
            continue;
        };
        let nouvelle = vus.get(chemin).is_none_or(|t| m.time > *t);
        if nouvelle {
            vus.insert(chemin.to_owned(), m.time);
            frais.push((chemin.to_owned(), m.value));
        }
    }
    frais
}

fn measure_duration(settings: &BenchSettings, cams: &Query<&BenchCamera>) -> f32 {
    cams.iter()
        .next()
        .map_or(settings.duration_secs, |c| c.0.duration())
}

fn drive_camera(time: Res<Time<Real>>, rec: Res<Recording>, mut cams: Query<(&BenchCamera, &mut Transform)>) {
    let t = rec.start.map_or(0.0, |s| (time.elapsed_secs_f64() - s) as f32);
    for (cam, mut tr) in &mut cams {
        *tr = cam.0.transform_at(t);
    }
}

fn record(
    time: Res<Time<Real>>,
    settings: Res<BenchSettings>,
    mut rec: ResMut<Recording>,
    cams: Query<&BenchCamera>,
    meshes: Query<(), With<Mesh3d>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    sys: Option<Res<SystemInfo>>,
    gpu: Option<Res<RenderAdapterInfo>>,
    store: Option<Res<DiagnosticsStore>>,
    mut exit: MessageWriter<AppExit>,
) {
    if rec.done {
        return;
    }
    let now = time.elapsed_secs_f64();
    // Mesures de rendu nouvelles depuis l'image précédente (lues aussi pendant la chauffe, pour que les valeurs
    // restées de la chauffe ne soient pas prises pour des nouvelles).
    let store = store.as_deref();
    let frais = mesures_fraiches(store, &mut rec.vus);
    let Some(start) = rec.start else {
        if now >= f64::from(settings.warmup_secs) {
            rec.start = Some(now);
            info!("mesure « {} » : début", settings.name);
        }
        return;
    };
    let t = (now - start) as f32;
    // Diagnostics : dernière valeur connue. Le GPU et le système sont en retard d'une ou deux images
    // (lecture asynchrone), et la mémoire système n'est rafraîchie qu'environ une fois par seconde.
    let render = aggregate_render(frais.iter().map(|(p, v)| (p.as_str(), *v)));
    let n = rec.frames.len();
    for (passe, v) in gpu_par_passe(frais.iter().map(|(p, v)| (p.as_str(), *v))) {
        // Une passe apparue en cours de mesure : images précédentes sans valeur.
        let serie = rec
            .passes
            .entry(passe.to_owned())
            .or_insert_with(|| vec![None; n]);
        serie.resize(n, None);
        serie.push(Some(v));
    }
    rec.frames.push(FrameSample {
        t_s: t,
        frame_ms: time.delta_secs_f64() * 1000.0,
        render,
        process_mem_gib: latest(store, &SystemInformationDiagnosticsPlugin::PROCESS_MEM_USAGE),
        system_mem_pct: latest(store, &SystemInformationDiagnosticsPlugin::SYSTEM_MEM_USAGE),
        process_cpu_pct: latest(store, &SystemInformationDiagnosticsPlugin::PROCESS_CPU_USAGE),
        system_cpu_pct: latest(store, &SystemInformationDiagnosticsPlugin::SYSTEM_CPU_USAGE),
    });
    if t < measure_duration(&settings, &cams) {
        return;
    }
    rec.done = true;
    let samples: Vec<f64> = rec.frames.iter().map(|f| f.frame_ms).collect();
    let Some(stats) = FrameStats::from_samples(&samples) else {
        error!("mesure « {} » : aucune image valide", settings.name);
        exit.write(AppExit::error());
        return;
    };
    let mut info: Vec<(String, String)> = vec![("mesure".into(), settings.name.clone())];
    let unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    info.push(("date_utc".into(), utc_date(unix)));
    info.push(("commit".into(), git_commit()));
    if let Some(s) = sys {
        info.push(("os".into(), s.os.clone()));
        info.push(("cpu".into(), s.cpu.clone()));
        info.push(("coeurs".into(), s.core_count.clone()));
        info.push(("ram".into(), s.memory.clone()));
    }
    if let Some(g) = gpu {
        info.push(("gpu".into(), g.name.clone()));
        info.push(("api".into(), format!("{:?}", g.backend)));
        info.push(("pilote".into(), format!("{} {}", g.driver, g.driver_info)));
    }
    if let Ok(w) = windows.single() {
        info.push((
            "resolution".into(),
            format!("{}x{}", w.physical_width(), w.physical_height()),
        ));
        info.push(("present_mode".into(), format!("{:?}", w.present_mode)));
    }
    info.push(("entites_maillage".into(), meshes.iter().count().to_string()));
    info.push((
        "profil".into(),
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }
        .into(),
    ));
    info.extend(settings.notes.iter().cloned());
    let s = stats;
    let results = [
        ("images", s.frames.to_string()),
        ("fps_moyen", format!("{:.1}", s.fps_mean)),
        ("fps_1pct_low", format!("{:.1}", s.fps_1pct_low)),
        ("ms_moyen", format!("{:.3}", s.mean_ms)),
        ("ms_p50", format!("{:.3}", s.p50_ms)),
        ("ms_p99", format!("{:.3}", s.p99_ms)),
        ("ms_pire", format!("{:.3}", s.worst_ms)),
        ("images_sup_33ms", s.over_33ms.to_string()),
        ("images_sup_50ms", s.over_50ms.to_string()),
    ];
    info.extend(results.iter().map(|(k, v)| ((*k).to_owned(), v.clone())));
    // Séries supplémentaires : moyenne, p99, pic.
    let series = |f: fn(&FrameSample) -> Option<f64>| {
        let v: Vec<Option<f64>> = rec.frames.iter().map(f).collect();
        SeriesStats::from_optional(&v)
    };
    push_series(&mut info, "rendu_cpu", "_ms", series(|f| f.render.cpu_ms), 3);
    push_series(&mut info, "rendu_gpu", "_ms", series(|f| f.render.gpu_ms), 3);
    push_series(&mut info, "triangles", "", series(|f| f.render.triangles), 0);
    push_series(
        &mut info,
        "mem_processus",
        "_gib",
        series(|f| f.process_mem_gib),
        3,
    );
    push_series(&mut info, "mem_systeme", "_pct", series(|f| f.system_mem_pct), 1);
    push_series(
        &mut info,
        "cpu_processus",
        "_pct",
        series(|f| f.process_cpu_pct),
        1,
    );
    push_series(&mut info, "cpu_systeme", "_pct", series(|f| f.system_cpu_pct), 1);
    // Bevy 0.20.1 n'expose aucun compteur d'appels de dessin : on l'écrit plutôt que de l'inventer.
    info.push(("appels_dessin".into(), "non exposé par Bevy 0.20.1".into()));

    let passes: Vec<(String, SeriesStats)> = rec
        .passes
        .iter()
        .filter_map(|(k, v)| Some((k.clone(), SeriesStats::from_optional(v)?)))
        .collect();
    match write_results(&settings, unix, &info, &rec.frames, &passes) {
        Ok(dir) => {
            info!(
                "mesure « {} » : {:.1} FPS moyen, {:.1} FPS 1 % low, pire image {:.1} ms → {}",
                settings.name,
                s.fps_mean,
                s.fps_1pct_low,
                s.worst_ms,
                dir.display()
            );
            exit.write(AppExit::Success);
        }
        Err(e) => {
            error!(
                "mesure « {} » : écriture des résultats impossible : {e}",
                settings.name
            );
            exit.write(AppExit::error());
        }
    }
}

fn write_results(
    settings: &BenchSettings,
    unix: u64,
    info: &[(String, String)],
    frames: &[FrameSample],
    passes: &[(String, SeriesStats)],
) -> std::io::Result<PathBuf> {
    let safe: String = settings
        .name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let dir = settings.out_dir.join(format!("{unix}_{safe}"));
    fs::create_dir_all(&dir)?;
    fs::write(dir.join("frames.csv"), frames_csv(frames))?;
    fs::write(dir.join("passes.csv"), passes_csv(passes))?;
    let mut summary = String::new();
    for (k, v) in info {
        let _ = writeln!(summary, "{k} = {v}");
    }
    fs::write(dir.join("summary.txt"), summary)?;
    // Une ligne par mesure dans summary.csv (en-tête écrit si le fichier est nouveau).
    let all = settings.out_dir.join("summary.csv");
    let cell = |v: &str| format!("\"{}\"", v.replace('"', "'"));
    let mut line = String::new();
    if !all.exists() {
        line += &info.iter().map(|(k, _)| cell(k)).collect::<Vec<_>>().join(";");
        line.push('\n');
    }
    line += &info.iter().map(|(_, v)| cell(v)).collect::<Vec<_>>().join(";");
    line.push('\n');
    use std::io::Write as _;
    fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(all)?
        .write_all(line.as_bytes())?;
    Ok(dir)
}

fn git_commit() -> String {
    std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map_or_else(
            || "?".into(),
            |o| String::from_utf8_lossy(&o.stdout).trim().to_owned(),
        )
}

/// Date UTC `AAAA-MM-JJ hh:mm:ss` (algorithme « civil from days » de H. Hinnant, sans dépendance).
fn utc_date(unix: u64) -> String {
    let days = (unix / 86_400) as i64;
    let secs = unix % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}",
        secs / 3600,
        secs % 3600 / 60,
        secs % 60
    )
}

#[cfg(test)]
mod tests {
    use super::utc_date;

    #[test]
    fn dates() {
        assert_eq!(utc_date(0), "1970-01-01 00:00:00");
        assert_eq!(utc_date(951_782_400), "2000-02-29 00:00:00");
        assert_eq!(utc_date(1_794_960_000), "2026-11-18 00:00:00");
    }
}
