//! Lecture / écriture de fichiers `.mcfmap`.

use std::fmt;
use std::fs;
use std::io::{self, Read, Write};
use std::path::Path;

use serde_json::Value;

use crate::text::nesting_depth;
use crate::{Catalog, MAX_DEPTH, MAX_FILE_BYTES, McfMap, validate};

/// Calque chargé et validé.
#[derive(Debug, Clone, PartialEq)]
pub struct Loaded {
    /// Le calque.
    pub map: McfMap,
    /// Avertissements de la validation (le fichier est accepté).
    pub warnings: Vec<String>,
}

/// Fichier refusé : toutes les fautes trouvées.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadError {
    /// Fautes, dans l'ordre du fichier.
    pub errors: Vec<String>,
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "fichier .mcfmap refusé : {}", self.errors.join(" ; "))
    }
}

impl std::error::Error for LoadError {}

impl From<Vec<String>> for LoadError {
    fn from(errors: Vec<String>) -> Self {
        Self { errors }
    }
}

fn refuse(msg: impl Into<String>) -> LoadError {
    LoadError {
        errors: vec![msg.into()],
    }
}

/// Analyse le JSON d'un calque sans le valider. Refuse ce qui est trop gros ou trop imbriqué **avant** l'analyse.
pub fn parse_text(text: &str) -> Result<Value, LoadError> {
    if text.len() > MAX_FILE_BYTES {
        return Err(refuse("fichier trop gros"));
    }
    let text = text.strip_prefix('\u{FEFF}').unwrap_or(text);
    if nesting_depth(text, MAX_DEPTH + 1) > MAX_DEPTH {
        return Err(refuse("JSON trop imbriqué"));
    }
    match serde_json::from_str::<Value>(text) {
        Ok(v) if v.is_object() => Ok(v),
        _ => Err(refuse("JSON invalide")),
    }
}

/// Lit un fichier sans le valider. La taille est contrôlée avant la lecture, et la lecture est bornée.
pub fn read_file(path: &Path) -> Result<Value, LoadError> {
    let file = fs::File::open(path).map_err(|e| match e.kind() {
        io::ErrorKind::NotFound => refuse("fichier introuvable"),
        _ => refuse("fichier illisible"),
    })?;
    let size = file.metadata().map_err(|_| refuse("fichier illisible"))?.len();
    if size > MAX_FILE_BYTES as u64 {
        return Err(refuse(format!(
            "fichier trop gros ({size} octets, max {MAX_FILE_BYTES})"
        )));
    }
    let mut bytes = Vec::with_capacity(size as usize);
    // +1 : détecte un fichier qui aurait grossi entre le contrôle et la lecture.
    file.take(MAX_FILE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| refuse("fichier illisible"))?;
    let text = String::from_utf8(bytes).map_err(|_| refuse("fichier illisible (UTF-8 invalide)"))?;
    parse_text(&text)
}

/// Analyse + validation stricte + conversion.
pub fn load_str(text: &str, catalog: Option<&dyn Catalog>) -> Result<Loaded, LoadError> {
    load_value(&parse_text(text)?, catalog)
}

/// Lecture + validation stricte + conversion.
pub fn load_file(path: &Path, catalog: Option<&dyn Catalog>) -> Result<Loaded, LoadError> {
    load_value(&read_file(path)?, catalog)
}

fn load_value(v: &Value, catalog: Option<&dyn Catalog>) -> Result<Loaded, LoadError> {
    let report = validate(v, catalog);
    if !report.is_ok() {
        return Err(report.errors.into());
    }
    let map = McfMap::from_validated(v).ok_or_else(|| refuse("incohérence interne après validation"))?;
    Ok(Loaded {
        map,
        warnings: report.warnings,
    })
}

impl McfMap {
    /// JSON indenté (tabulations, comme l'éditeur Godot). Refuse d'écrire un calque que la relecture refuserait.
    pub fn to_json_pretty(&self) -> Result<String, LoadError> {
        let v = self.to_value();
        let report = validate(&v, None);
        if !report.is_ok() {
            return Err(report.errors.into());
        }
        let mut out = Vec::new();
        let fmt = serde_json::ser::PrettyFormatter::with_indent(b"\t");
        let mut ser = serde_json::Serializer::with_formatter(&mut out, fmt);
        serde::Serialize::serialize(&v, &mut ser).map_err(|_| refuse("sérialisation impossible"))?;
        String::from_utf8(out).map_err(|_| refuse("sérialisation impossible"))
    }
}

/// Erreur d'enregistrement.
#[derive(Debug)]
pub enum SaveError {
    /// Le calque est invalide : il n'a pas été écrit.
    Invalid(LoadError),
    /// Erreur du disque : le fichier d'origine n'a pas été touché.
    Io(io::Error),
}

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(e) => write!(f, "{e}"),
            Self::Io(e) => write!(f, "écriture impossible : {e}"),
        }
    }
}

impl std::error::Error for SaveError {}

/// Valide puis écrit le calque de façon atomique.
pub fn save_file(path: &Path, map: &McfMap) -> Result<(), SaveError> {
    let text = map.to_json_pretty().map_err(SaveError::Invalid)?;
    write_atomic(path, text.as_bytes()).map_err(SaveError::Io)
}

/// Écriture atomique : `<fichier>.tmp` écrit, synchronisé et relu, puis renommé à la place du fichier. À tout
/// instant il existe sur le disque une version complète (un plantage pendant l'enregistrement n'abîme rien).
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        fs::create_dir_all(dir)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = std::path::PathBuf::from(tmp);
    let result = (|| {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        if fs::metadata(&tmp)?.len() != bytes.len() as u64 {
            return Err(io::Error::other("taille relue différente (disque plein ?)"));
        }
        // Remplace le fichier existant (MoveFileEx avec remplacement sous Windows, rename(2) ailleurs).
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}
