//! Lecture d'un dossier produit par `mcf_big_map` : `manifest.json` + calques `.mcfmap`. Sans Bevy (hors types
//! mathématiques), testable seul.
//!
//! Tout ce qui vient du disque est **non fiable** (règle du projet) :
//! - le manifeste est borné en taille avant analyse, ses champs sont en liste blanche, les noms de fichiers sont
//!   des noms simples (`[a-z0-9_]`, extension `.mcfmap`) : aucun chemin, aucune sortie du dossier ;
//! - chaque calque passe la **validation stricte** de `mcf_map` avec le catalogue du banc **compilé** dans le
//!   programme (le `catalogue.json` du dossier n'est jamais lu : il ne peut rien autoriser de plus) ;
//! - le nombre d'éléments annoncé par le manifeste doit correspondre exactement à ce qui est lu.
//!
//! Les calques sont lus **en parallèle** (un fil par cœur, 8 au plus), puis remis dans l'ordre du manifeste : le
//! résultat est identique quel que soit le nombre de fils.

use std::path::Path;
use std::time::{Duration, Instant};

use bevy::color::{ColorToPacked, Srgba};
use bevy::transform::components::Transform;
use mcf_big_map::{Kind, MANIFEST_VERSION};
use mcf_map::{McfMap, PropValue, SimpleCatalog, load_file};
use serde_json::Value;

use crate::catalog::{ModelRef, model_of};

/// Taille maximale du manifeste (le palier « stress » en fait ~30 Kio).
pub const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
/// Nombre maximal de calques dans un dossier.
pub const MAX_FILES: usize = 4_096;
/// Nombre maximal d'éléments sur toute la carte (borne du générateur).
pub const MAX_ELEMENTS: usize = 2_000_000;

const MANIFEST_KEYS: &[&str] = &[
    "format",
    "version",
    "palier",
    "graine",
    "taille_m",
    "cellule_m",
    "cellules_par_cote",
    "ville_m",
    "catalog_version",
    "quantites",
    "elements",
    "fichiers",
];
const FILE_KEYS: &[&str] = &["fichier", "cellule", "elements"];

/// Un calque annoncé par le manifeste.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestFile {
    /// Nom du fichier (sans chemin), `bench_c003_004_1.mcfmap`.
    pub name: String,
    /// Cellule (colonne selon X, ligne selon Z).
    pub cell: (u16, u16),
    /// Nombre d'éléments annoncé.
    pub elements: usize,
}

/// Manifeste validé.
#[derive(Debug, Clone, PartialEq)]
pub struct Manifest {
    /// Palier (`depart`, `stress`…).
    pub tier: String,
    /// Graine, en hexadécimal (telle qu'écrite).
    pub seed: String,
    /// Côté de l'île, en mètres.
    pub size_m: f64,
    /// Côté d'une cellule, en mètres.
    pub cell_m: f64,
    /// Côté de la ville, en mètres.
    pub city_m: f64,
    /// Nombre total d'éléments annoncé.
    pub elements: usize,
    /// Calques, dans l'ordre du manifeste.
    pub files: Vec<ManifestFile>,
}

fn is_plain_file_name(name: &str) -> bool {
    name.strip_suffix(".mcfmap").is_some_and(|stem| {
        (1..=64).contains(&stem.len())
            && stem
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    })
}

fn only_keys(o: &serde_json::Map<String, Value>, allowed: &[&str], what: &str) -> Result<(), String> {
    match o.keys().find(|k| !allowed.contains(&k.as_str())) {
        Some(k) => Err(format!("{what} : champ inconnu « {k} »")),
        None => Ok(()),
    }
}

fn count(v: Option<&Value>, max: usize, what: &str) -> Result<usize, String> {
    v.and_then(Value::as_u64)
        .and_then(|n| usize::try_from(n).ok())
        .filter(|n| *n <= max)
        .ok_or_else(|| format!("{what} : entier de 0 à {max} attendu"))
}

/// Analyse et valide `manifest.json`.
pub fn parse_manifest(text: &str) -> Result<Manifest, String> {
    if text.len() as u64 > MAX_MANIFEST_BYTES {
        return Err("manifeste trop gros".into());
    }
    let v: Value = serde_json::from_str(text).map_err(|e| format!("manifeste : JSON invalide ({e})"))?;
    let o = v.as_object().ok_or("manifeste : objet attendu")?;
    only_keys(o, MANIFEST_KEYS, "manifeste")?;
    if o.get("format").and_then(Value::as_str) != Some("mcf_big_map") {
        return Err("manifeste : format « mcf_big_map » attendu".into());
    }
    if o.get("version").and_then(Value::as_i64) != Some(MANIFEST_VERSION) {
        return Err(format!("manifeste : version {MANIFEST_VERSION} attendue"));
    }
    if o.get("catalog_version").and_then(Value::as_i64) != Some(mcf_big_map::CATALOG_VERSION) {
        return Err(format!(
            "manifeste : catalogue version {} attendu",
            mcf_big_map::CATALOG_VERSION
        ));
    }
    let text_field = |k: &str, max: usize| {
        o.get(k)
            .and_then(Value::as_str)
            .filter(|s| {
                !s.is_empty() && s.len() <= max && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
            })
            .map(str::to_owned)
            .ok_or_else(|| format!("manifeste : « {k} » invalide"))
    };
    let meters = |k: &str, lo: f64, hi: f64| {
        o.get(k)
            .and_then(Value::as_f64)
            .filter(|x| (lo..=hi).contains(x))
            .ok_or_else(|| format!("manifeste : « {k} » hors limites ({lo} à {hi} m)"))
    };
    let size_m = meters("taille_m", 1_000.0, 2.0 * mcf_map::MAX_COORD)?;
    let cell_m = meters("cellule_m", 100.0, 2_000.0)?;
    let city_m = meters("ville_m", 100.0, size_m)?;
    let cells = count(o.get("cellules_par_cote"), 100, "manifeste : cellules_par_cote")?;
    if cells != (size_m / cell_m).ceil() as usize {
        return Err("manifeste : cellules_par_cote incohérent avec taille_m / cellule_m".into());
    }
    let elements = count(o.get("elements"), MAX_ELEMENTS, "manifeste : elements")?;
    let list = o
        .get("fichiers")
        .and_then(Value::as_array)
        .ok_or("manifeste : « fichiers » doit être une liste")?;
    if list.len() > MAX_FILES {
        return Err(format!("manifeste : plus de {MAX_FILES} calques"));
    }
    let mut files = Vec::with_capacity(list.len());
    let mut total = 0usize;
    for (n, f) in list.iter().enumerate() {
        let what = format!("manifeste : fichier n° {n}");
        let fo = f.as_object().ok_or_else(|| format!("{what} : objet attendu"))?;
        only_keys(fo, FILE_KEYS, &what)?;
        let name = fo
            .get("fichier")
            .and_then(Value::as_str)
            .filter(|s| is_plain_file_name(s))
            .ok_or_else(|| format!("{what} : nom de fichier refusé (nom simple en .mcfmap attendu)"))?;
        let cell = fo
            .get("cellule")
            .and_then(Value::as_array)
            .filter(|a| a.len() == 2)
            .and_then(|a| {
                let c = |i: usize| a[i].as_u64().filter(|v| (*v as usize) < cells).map(|v| v as u16);
                Some((c(0)?, c(1)?))
            })
            .ok_or_else(|| format!("{what} : cellule invalide"))?;
        let n_el = count(fo.get("elements"), mcf_map::MAX_ITEMS, &what)?;
        total += n_el;
        if files.iter().any(|x: &ManifestFile| x.name == name) {
            return Err(format!("{what} : « {name} » en double"));
        }
        files.push(ManifestFile {
            name: name.to_owned(),
            cell,
            elements: n_el,
        });
    }
    if total != elements {
        return Err(format!(
            "manifeste : {elements} éléments annoncés, {total} dans la liste des fichiers"
        ));
    }
    Ok(Manifest {
        tier: text_field("palier", 32)?,
        seed: text_field("graine", 16)?,
        size_m,
        cell_m,
        city_m,
        elements,
        files,
    })
}

/// Un élément prêt à afficher : 64 octets environ, sans chaîne ni allocation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    /// Modèle.
    pub model: ModelRef,
    /// Placement monde.
    pub transform: Transform,
    /// Teinte (réglage `color`, véhicules), en sRGB 8 bits.
    pub tint: Option<[u8; 4]>,
    /// Numéro du calque (ordre du manifeste).
    pub layer: u16,
}

/// Convertit un calque validé en éléments à afficher.
pub fn placements_of(map: &McfMap, layer: u16) -> Result<Vec<Placement>, String> {
    map.items
        .iter()
        .map(|it| {
            let model = model_of(&it.catalog)
                .ok_or_else(|| format!("{} : modèle inconnu « {} »", map.layer, it.catalog))?;
            let tint = match it.props.get("color") {
                Some(PropValue::Color(c)) => Some(
                    Srgba::hex(c)
                        .map_err(|_| format!("{} : couleur « {c} » illisible", map.layer))?
                        .to_u8_array(),
                ),
                _ => None,
            };
            Ok(Placement {
                model,
                transform: mcf_core::coords::to_bevy(&it.transform),
                tint,
                layer,
            })
        })
        .collect()
}

/// Carte chargée.
#[derive(Debug, Clone)]
pub struct LoadedMap {
    /// Manifeste.
    pub manifest: Manifest,
    /// Éléments, calque après calque dans l'ordre du manifeste, puis dans l'ordre de chaque fichier.
    pub placements: Vec<Placement>,
    /// Avertissements de validation (fichiers acceptés quand même).
    pub warnings: usize,
    /// Durée de lecture + validation + conversion.
    pub read_time: Duration,
}

impl LoadedMap {
    /// Nombre d'éléments d'une nature.
    pub fn count(&self, kind: Kind) -> usize {
        self.placements.iter().filter(|p| p.model.kind == kind).count()
    }
}

fn load_layer(
    dir: &Path,
    f: &ManifestFile,
    layer: u16,
    cat: &SimpleCatalog,
) -> Result<(Vec<Placement>, usize), String> {
    let loaded = load_file(&dir.join(&f.name), Some(cat)).map_err(|e| format!("{} : {e}", f.name))?;
    if loaded.map.items.len() != f.elements {
        return Err(format!(
            "{} : {} éléments lus, {} annoncés par le manifeste",
            f.name,
            loaded.map.items.len(),
            f.elements
        ));
    }
    if !loaded.map.roads.is_empty() || !loaded.map.junctions.is_empty() || loaded.map.base_edits.is_some() {
        return Err(format!(
            "{} : routes et île de base pas encore gérées par le banc",
            f.name
        ));
    }
    Ok((placements_of(&loaded.map, layer)?, loaded.warnings.len()))
}

/// Lit le dossier d'une carte générée. Toute faute arrête le chargement (rien n'est affiché à moitié).
pub fn load_map_dir(dir: &Path) -> Result<LoadedMap, String> {
    let t0 = Instant::now();
    let mpath = dir.join("manifest.json");
    let size = std::fs::metadata(&mpath)
        .map_err(|e| format!("{} : {e}", mpath.display()))?
        .len();
    if size > MAX_MANIFEST_BYTES {
        return Err(format!(
            "{} : manifeste trop gros ({size} octets)",
            mpath.display()
        ));
    }
    let text = std::fs::read_to_string(&mpath).map_err(|e| format!("{} : {e}", mpath.display()))?;
    let manifest = parse_manifest(&text)?;
    let cat = mcf_big_map::catalog();
    let files = &manifest.files;
    let threads = std::thread::available_parallelism()
        .map_or(1, std::num::NonZeroUsize::get)
        .clamp(1, 8)
        .min(files.len().max(1));
    // Répartition fixe et entrelacée (fil k : calques k, k + n, …) : équilibre sans file partagée.
    let results: Vec<Vec<(usize, Result<(Vec<Placement>, usize), String>)>> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..threads)
            .map(|k| {
                let cat = &cat;
                s.spawn(move || {
                    (k..files.len())
                        .step_by(threads)
                        .map(|i| (i, load_layer(dir, &files[i], i as u16, cat)))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| {
                h.join()
                    .unwrap_or_else(|_| vec![(0, Err("lecture interrompue (panique)".into()))])
            })
            .collect()
    });
    let mut per_file: Vec<Option<(Vec<Placement>, usize)>> = vec![None; files.len()];
    for (i, r) in results.into_iter().flatten() {
        per_file[i] = Some(r?);
    }
    let mut placements = Vec::with_capacity(manifest.elements);
    let mut warnings = 0;
    for (i, r) in per_file.into_iter().enumerate() {
        let (p, w) = r.ok_or_else(|| format!("{} : non lu", files[i].name))?;
        placements.extend(p);
        warnings += w;
    }
    Ok(LoadedMap {
        manifest,
        placements,
        warnings,
        read_time: t0.elapsed(),
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use std::path::PathBuf;

    use mcf_big_map::{Params, Quantities, generate, manifest, write_dir};

    use super::*;

    pub(crate) fn small_params() -> Params {
        Params {
            tier: "test".into(),
            quantities: Quantities {
                buildings: 60,
                interiors: 6,
                objects: 900,
                trees: 500,
                lamps: 80,
                shadow_lamps: 3,
                characters: 10,
                vehicles: 15,
            },
            max_per_file: 150,
            ..Params::start()
        }
    }

    /// Dossier temporaire propre à un test (effacé à la fin).
    pub(crate) struct TempMap(pub PathBuf);

    impl TempMap {
        pub(crate) fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("mcf_test_a_{tag}_{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            let g = generate(&small_params()).expect("génération");
            write_dir(&g, &dir).expect("écriture");
            Self(dir)
        }
    }

    impl Drop for TempMap {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn charge_toute_la_carte_dans_l_ordre() {
        let tmp = TempMap::new("ordre");
        let p = small_params();
        let a = load_map_dir(&tmp.0).expect("chargement");
        assert_eq!(a.placements.len(), p.quantities.total());
        for k in Kind::ALL {
            assert_eq!(a.count(k), p.quantities.of(k), "{k:?}");
        }
        // Même ordre que le générateur : calque après calque, élément après élément.
        let g = generate(&p).expect("génération");
        let expected: Vec<ModelRef> = g
            .layers
            .iter()
            .flat_map(|l| &l.map.items)
            .map(|it| model_of(&it.catalog).expect("modèle"))
            .collect();
        let got: Vec<ModelRef> = a.placements.iter().map(|p| p.model).collect();
        assert_eq!(got, expected);
        assert!(a.placements.windows(2).all(|w| w[0].layer <= w[1].layer));
        // Deux chargements donnent exactement la même chose.
        let b = load_map_dir(&tmp.0).expect("chargement");
        assert_eq!(a.placements, b.placements);
        assert_eq!(a.manifest, b.manifest);
    }

    #[test]
    fn teintes_des_vehicules() {
        let tmp = TempMap::new("teintes");
        let m = load_map_dir(&tmp.0).expect("chargement");
        for p in &m.placements {
            assert_eq!(p.tint.is_some(), p.model.kind == Kind::Vehicle, "{:?}", p.model);
        }
    }

    fn manifest_text() -> String {
        manifest(&generate(&small_params()).expect("génération")).to_string()
    }

    #[test]
    fn manifeste_du_generateur_accepte() {
        let m = parse_manifest(&manifest_text()).expect("manifeste");
        assert_eq!(m.tier, "test");
        assert_eq!(m.elements, small_params().quantities.total());
    }

    #[test]
    fn manifestes_refuses() {
        let base: Value = serde_json::from_str(&manifest_text()).expect("JSON");
        let cases: &[(&str, fn(&mut Value))] = &[
            ("chemin", |v| v["fichiers"][0]["fichier"] = "../x.mcfmap".into()),
            ("absolu", |v| v["fichiers"][0]["fichier"] = "C:/x.mcfmap".into()),
            ("extension", |v| v["fichiers"][0]["fichier"] = "x.json".into()),
            ("champ inconnu", |v| v["script"] = "rm -rf".into()),
            ("champ inconnu fichier", |v| {
                v["fichiers"][0]["exec"] = true.into()
            }),
            ("format", |v| v["format"] = "autre".into()),
            ("version", |v| v["version"] = 2.into()),
            ("total", |v| v["elements"] = 1.into()),
            ("taille", |v| v["taille_m"] = 1e9.into()),
            ("cellule hors carte", |v| {
                v["fichiers"][0]["cellule"] = serde_json::json!([99, 0])
            }),
            ("palier", |v| v["palier"] = "a/b".into()),
            ("doublon", |v| {
                let first = v["fichiers"][0]["fichier"].clone();
                v["fichiers"][1]["fichier"] = first;
            }),
        ];
        for (name, f) in cases {
            let mut v = base.clone();
            f(&mut v);
            assert!(parse_manifest(&v.to_string()).is_err(), "cas « {name} » accepté");
        }
        assert!(parse_manifest("[]").is_err());
        assert!(parse_manifest(&" ".repeat(MAX_MANIFEST_BYTES as usize + 1)).is_err());
    }

    #[test]
    fn calque_modifie_refuse() {
        let tmp = TempMap::new("modifie");
        let m = parse_manifest(&std::fs::read_to_string(tmp.0.join("manifest.json")).expect("lecture"))
            .expect("manifeste");
        let path = tmp.0.join(&m.files[0].name);
        let text = std::fs::read_to_string(&path).expect("lecture");
        // Identifiant hors catalogue du banc : la validation stricte doit refuser tout le chargement.
        let bad = text.replacen("\"bench_", "\"props/inconnu_", 1);
        std::fs::write(&path, bad).expect("écriture");
        assert!(load_map_dir(&tmp.0).is_err());
    }

    #[test]
    fn noms_de_fichiers() {
        assert!(is_plain_file_name("bench_c000_001_0.mcfmap"));
        for bad in [
            "",
            ".mcfmap",
            "a/b.mcfmap",
            "a\\b.mcfmap",
            "..mcfmap",
            "A.mcfmap",
            "a.mcfmap.tmp",
        ] {
            assert!(!is_plain_file_name(bad), "{bad}");
        }
    }
}
