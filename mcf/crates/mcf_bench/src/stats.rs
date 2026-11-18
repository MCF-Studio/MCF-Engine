/// Statistiques sur une série de temps par image (en millisecondes).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameStats {
    /// Nombre d'images mesurées.
    pub frames: usize,
    /// Temps moyen par image.
    pub mean_ms: f64,
    /// Médiane.
    pub p50_ms: f64,
    /// 99e centile.
    pub p99_ms: f64,
    /// Pire image.
    pub worst_ms: f64,
    /// FPS moyen (images / temps total).
    pub fps_mean: f64,
    /// **1 % low** : FPS moyen sur le 1 % des images les plus lentes (définition courante des tests matériels).
    pub fps_1pct_low: f64,
    /// Images de plus de 33,3 ms (sous 30 FPS).
    pub over_33ms: usize,
    /// Images de plus de 50 ms (saccade nettement visible).
    pub over_50ms: usize,
}

impl FrameStats {
    /// Calcule les statistiques. `None` si la série est vide ou contient une valeur non finie / négative.
    pub fn from_samples(samples_ms: &[f64]) -> Option<Self> {
        if samples_ms.is_empty() || samples_ms.iter().any(|v| !v.is_finite() || *v < 0.0) {
            return None;
        }
        let mut sorted = samples_ms.to_vec();
        sorted.sort_by(f64::total_cmp);
        let n = sorted.len();
        let total: f64 = sorted.iter().sum();
        // Centile par rang le plus proche.
        let pct = |p: f64| sorted[((p * n as f64).ceil() as usize).clamp(1, n) - 1];
        let slowest = (n / 100).max(1);
        let slow_mean = sorted[n - slowest..].iter().sum::<f64>() / slowest as f64;
        let fps = |ms: f64| if ms > 0.0 { 1000.0 / ms } else { f64::INFINITY };
        Some(Self {
            frames: n,
            mean_ms: total / n as f64,
            p50_ms: pct(0.50),
            p99_ms: pct(0.99),
            worst_ms: sorted[n - 1],
            fps_mean: fps(total / n as f64),
            fps_1pct_low: fps(slow_mean),
            over_33ms: sorted.iter().filter(|v| **v > 1000.0 / 30.0).count(),
            over_50ms: sorted.iter().filter(|v| **v > 50.0).count(),
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn serie_reguliere() {
        let s = FrameStats::from_samples(&[10.0; 500]).unwrap();
        assert_eq!(s.frames, 500);
        assert!((s.fps_mean - 100.0).abs() < 1e-9);
        assert!((s.fps_1pct_low - 100.0).abs() < 1e-9);
        assert_eq!(s.over_33ms, 0);
    }

    #[test]
    fn saccades_comptees_et_1pct_low() {
        // 990 images à 10 ms + 10 images à 100 ms : le 1 % le plus lent = les 10 saccades.
        let mut v = vec![10.0; 990];
        v.extend([100.0; 10]);
        let s = FrameStats::from_samples(&v).unwrap();
        assert!((s.fps_1pct_low - 10.0).abs() < 1e-9);
        assert_eq!(s.over_50ms, 10);
        assert_eq!(s.worst_ms, 100.0);
        assert_eq!(s.p50_ms, 10.0);
        assert_eq!(s.p99_ms, 10.0);
    }

    #[test]
    fn series_invalides() {
        assert!(FrameStats::from_samples(&[]).is_none());
        assert!(FrameStats::from_samples(&[1.0, f64::NAN]).is_none());
        assert!(FrameStats::from_samples(&[-1.0]).is_none());
    }

    #[test]
    fn petite_serie() {
        let s = FrameStats::from_samples(&[5.0, 20.0]).unwrap();
        assert_eq!(s.worst_ms, 20.0);
        assert!((s.fps_1pct_low - 50.0).abs() < 1e-9);
    }
}
