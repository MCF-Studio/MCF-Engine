//! Mesures par image et calculs purs associés (sans dépendance à Bevy, donc testables).

use std::fmt::Write as _;

/// Totaux de rendu d'une image, lus dans les diagnostics de Bevy (`RenderDiagnosticsPlugin`).
/// `None` = non disponible (ex. temps GPU et triangles : Vulkan / DX12 seulement).
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct RenderTotals {
    /// Somme des temps CPU des spans de rendu de premier niveau (`render/<span>/elapsed_cpu`), en ms.
    pub cpu_ms: Option<f64>,
    /// Somme des temps GPU des spans de premier niveau (`render/<span>/elapsed_gpu`), en ms.
    pub gpu_ms: Option<f64>,
    /// Somme des primitives sorties du clipper de toutes les passes (`…/clipper_primitives_out`) : triangles.
    pub triangles: Option<f64>,
}

/// Additionne les diagnostics de rendu. `entries` = (chemin, valeur), ex. `render/fxaa/elapsed_gpu`.
///
/// Les temps ne comptent que les spans de premier niveau (un seul composant entre `render` et le champ) pour ne pas
/// compter deux fois un span imbriqué dans un autre. Les triangles se somment sur toutes les passes (une passe de
/// rendu n'est jamais imbriquée dans une autre passe).
pub fn aggregate_render<'a>(entries: impl IntoIterator<Item = (&'a str, f64)>) -> RenderTotals {
    let mut totals = RenderTotals::default();
    let add = |slot: &mut Option<f64>, v: f64| *slot = Some(slot.unwrap_or(0.0) + v);
    for (path, value) in entries {
        if !value.is_finite() {
            continue;
        }
        let mut parts = path.split('/');
        if parts.next() != Some("render") {
            continue;
        }
        let rest: Vec<&str> = parts.collect();
        let Some((field, spans)) = rest.split_last() else {
            continue;
        };
        match *field {
            "elapsed_cpu" if spans.len() == 1 => add(&mut totals.cpu_ms, value),
            "elapsed_gpu" if spans.len() == 1 => add(&mut totals.gpu_ms, value),
            "clipper_primitives_out" if !spans.is_empty() => add(&mut totals.triangles, value),
            _ => {}
        }
    }
    totals
}

/// Une image mesurée.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct FrameSample {
    /// Temps écoulé depuis le début de la mesure (s).
    pub t_s: f32,
    /// Durée de l'image (ms).
    pub frame_ms: f64,
    /// Rendu : CPU / GPU / triangles (dernière valeur connue, décalée d'une ou deux images côté GPU).
    pub render: RenderTotals,
    /// Mémoire du processus (Gio).
    pub process_mem_gib: Option<f64>,
    /// Mémoire du système (% utilisé).
    pub system_mem_pct: Option<f64>,
    /// CPU du processus (%).
    pub process_cpu_pct: Option<f64>,
    /// CPU du système (%).
    pub system_cpu_pct: Option<f64>,
}

/// En-tête de `frames.csv`.
pub const FRAMES_CSV_HEADER: &str = "t_s,frame_ms,render_cpu_ms,render_gpu_ms,triangles,process_mem_gib,system_mem_pct,process_cpu_pct,system_cpu_pct";

/// Contenu de `frames.csv` : une ligne par image, cellule vide si la mesure n'est pas disponible.
pub fn frames_csv(frames: &[FrameSample]) -> String {
    let cell = |v: Option<f64>, prec: usize| v.map_or_else(String::new, |x| format!("{x:.prec$}"));
    let mut csv = String::from(FRAMES_CSV_HEADER);
    csv.push('\n');
    for f in frames {
        let _ = writeln!(
            csv,
            "{:.4},{:.4},{},{},{},{},{},{},{}",
            f.t_s,
            f.frame_ms,
            cell(f.render.cpu_ms, 4),
            cell(f.render.gpu_ms, 4),
            cell(f.render.triangles, 0),
            cell(f.process_mem_gib, 4),
            cell(f.system_mem_pct, 2),
            cell(f.process_cpu_pct, 2),
            cell(f.system_cpu_pct, 2),
        );
    }
    csv
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn totaux_premier_niveau_seulement() {
        let t = aggregate_render([
            ("render/main_opaque_pass_3d/elapsed_gpu", 2.0),
            ("render/fxaa/elapsed_gpu", 0.5),
            // Span imbriqué : ne doit pas être recompté.
            ("render/fxaa/fxaa/elapsed_gpu", 0.4),
            ("render/main_opaque_pass_3d/elapsed_cpu", 1.0),
            ("render/main_opaque_pass_3d/clipper_primitives_out", 1000.0),
            ("render/fxaa/fxaa/clipper_primitives_out", 2.0),
            // Hors rendu : ignoré.
            ("process/mem_usage", 3.0),
        ]);
        assert_eq!(t.gpu_ms, Some(2.5));
        assert_eq!(t.cpu_ms, Some(1.0));
        assert_eq!(t.triangles, Some(1002.0));
    }

    #[test]
    fn totaux_absents() {
        assert_eq!(aggregate_render([]), RenderTotals::default());
        // Valeurs non finies et valeurs « brutes » (render/<nom>) ignorées.
        let t = aggregate_render([("render/x/elapsed_gpu", f64::NAN), ("render/elapsed_gpu", 1.0)]);
        assert_eq!(t, RenderTotals::default());
    }

    #[test]
    fn csv_cellules_vides_si_absent() {
        let f = FrameSample {
            t_s: 1.0,
            frame_ms: 16.5,
            process_mem_gib: Some(1.25),
            ..Default::default()
        };
        let csv = frames_csv(&[f]);
        let mut lines = csv.lines();
        assert_eq!(lines.next(), Some(FRAMES_CSV_HEADER));
        assert_eq!(lines.next(), Some("1.0000,16.5000,,,,1.2500,,,"));
        assert_eq!(lines.next(), None);
        // Autant de colonnes dans l'en-tête que dans les lignes.
        assert_eq!(FRAMES_CSV_HEADER.split(',').count(), 9);
    }
}
