//! Validation stricte d'un calque, sur le JSON brut (avant toute conversion en types Rust) : on peut ainsi
//! signaler **toutes** les fautes d'un fichier, avec les mêmes messages que l'éditeur Godot.

use std::collections::HashSet;

use serde_json::{Map, Value};

use crate::text::{clip, has_control, has_path, is_hex, is_html_color, len};
use crate::*;

/// Résultat d'une validation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// Fautes : le fichier est refusé s'il y en a au moins une.
    pub errors: Vec<String>,
    /// Avertissements : le fichier est accepté.
    pub warnings: Vec<String>,
}

impl Report {
    /// Aucune faute ?
    pub fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }
}

/// Valide un calque. `catalog` : identifiants connus et réglages autorisés (sans catalogue, seuls la forme et les
/// bornes sont vérifiées).
pub fn validate(data: &Value, catalog: Option<&dyn Catalog>) -> Report {
    let mut r = Report::default();
    let Some(obj) = data.as_object() else {
        r.errors.push("le fichier doit être un objet JSON".into());
        return r;
    };
    let errors = &mut r.errors;
    for k in obj.keys() {
        if !TOP_KEYS.contains(&k.as_str()) {
            errors.push(format!("champ inconnu « {} »", clip(k)));
        }
    }
    if obj.get("format").and_then(Value::as_str) != Some(FORMAT) {
        errors.push("format attendu « mcfmap »".into());
    }
    if as_int(obj.get("version")) != Some(VERSION) {
        errors.push("version de format non gérée".into());
    }
    match as_int(obj.get("catalog_version")) {
        Some(v) if (0..=MAX_CATALOG_VERSION).contains(&v) => {
            if let Some(cat) = catalog
                && v > cat.version()
            {
                r.warnings.push(format!(
                    "fait avec un catalogue plus récent (v{v} > v{}) : mettez à jour votre catalogue",
                    cat.version()
                ));
            }
        }
        _ => errors.push("catalog_version manquant ou hors limites".into()),
    }
    for key in ["layer", "author", "created"] {
        let ok = obj.get(key).and_then(Value::as_str).is_some_and(is_safe_name);
        if !ok {
            errors.push(format!("« {key} » invalide"));
        }
    }
    let Some(items) = obj.get("items").and_then(Value::as_array) else {
        errors.push("« items » doit être une liste".into());
        return r;
    };
    if items.len() > MAX_ITEMS {
        errors.push(format!("trop d'éléments ({}, max {MAX_ITEMS})", items.len()));
        return r;
    }
    let mut uids = HashSet::new();
    for (i, it) in items.iter().enumerate() {
        validate_item(it, i, catalog, &mut uids, errors);
        if errors.len() > MAX_REPORTED_ERRORS {
            errors.push("… trop d'erreurs, arrêt de la validation".into());
            break;
        }
    }
    if let Some(roads) = obj.get("roads") {
        validate_roads(roads, &mut uids, errors);
    }
    if let Some(junctions) = obj.get("junctions") {
        validate_junctions(junctions, &mut uids, errors);
    }
    if let Some(edits) = obj.get("base_edits") {
        // Fichier de l'île de base : rien d'autre dedans (un calque de contributeur ne touche jamais l'île).
        let empty_or_absent = |k: &str| {
            obj.get(k)
                .is_none_or(|v| v.as_array().is_some_and(|a| a.is_empty()))
        };
        if !items.is_empty() || !empty_or_absent("roads") || !empty_or_absent("junctions") {
            errors.push("un fichier de l'île de base ne contient que des modifications de l'île".into());
        }
        validate_base_edits(edits, catalog, errors);
    }
    r
}

/// Nom sûr (calque, auteur, date) : court, sans chemin ni caractère invisible.
fn is_safe_name(s: &str) -> bool {
    !has_path(s) && len(s) <= MAX_NAME && !has_control(s)
}

/// Entier au sens de Godot (le JSON de Godot ne donne que des flottants) : nombre fini, entier, |v| < 1e9.
fn as_int(v: Option<&Value>) -> Option<i64> {
    let f = v?.as_f64()?;
    if !f.is_finite() || f.abs() >= 1.0e9 {
        return None;
    }
    // `is_equal_approx` de Godot : tolérance relative 1e-5 (minimum 1e-5).
    let tolerance = (1.0e-5 * f.abs()).max(1.0e-5);
    ((f - f.round()).abs() < tolerance).then_some(f.round() as i64)
}

fn number(v: Option<&Value>) -> Option<f64> {
    v.filter(|v| v.is_number()).and_then(Value::as_f64)
}

/// Tableau de 3 nombres.
fn vec3(v: Option<&Value>) -> Option<[f64; 3]> {
    let a = v?.as_array()?;
    if a.len() != 3 || !a.iter().all(Value::is_number) {
        return None;
    }
    Some([a[0].as_f64()?, a[1].as_f64()?, a[2].as_f64()?])
}

fn in_bounds(p: &[f64; 3]) -> bool {
    p.iter().all(|c| c.is_finite() && c.abs() <= MAX_COORD)
}

fn unknown_keys(o: &Map<String, Value>, allowed: &[&str], tag: &str, suffix: &str, errors: &mut Vec<String>) {
    for k in o.keys() {
        if !allowed.contains(&k.as_str()) {
            errors.push(format!("{tag} : {suffix}champ inconnu « {} »", clip(k)));
        }
    }
}

fn check_uid(v: Option<&Value>, tag: &str, uids: &mut HashSet<String>, errors: &mut Vec<String>) {
    match v.and_then(Value::as_str) {
        Some(uid) if is_hex(uid, 8, 32) => {
            if !uids.insert(uid.to_owned()) {
                errors.push(format!("{tag} : uid en double"));
            }
        }
        _ => errors.push(format!("{tag} : uid invalide")),
    }
}

fn validate_item(
    it: &Value,
    i: usize,
    catalog: Option<&dyn Catalog>,
    uids: &mut HashSet<String>,
    errors: &mut Vec<String>,
) {
    let tag = format!("élément {i}");
    let Some(o) = it.as_object() else {
        errors.push(format!("{tag} : doit être un objet"));
        return;
    };
    unknown_keys(o, ITEM_KEYS, &tag, "", errors);
    check_uid(o.get("uid"), &tag, uids, errors);
    let mut editable: Option<&[String]> = None;
    match o.get("catalog").and_then(Value::as_str) {
        Some(cid) if !has_path(cid) => {
            if let Some(cat) = catalog {
                editable = cat.editable_props(cid);
                if editable.is_none() {
                    errors.push(format!("{tag} : id inconnu du catalogue « {} »", clip(cid)));
                }
            }
        }
        _ => errors.push(format!("{tag} : id de catalogue invalide")),
    }
    validate_transform(o.get("transform"), &tag, errors);
    let empty = Map::new();
    let props = match o.get("props") {
        None => &empty,
        Some(Value::Object(p)) => p,
        Some(_) => {
            errors.push(format!("{tag} : props doit être un objet"));
            return;
        }
    };
    for (pk, pv) in props {
        let Some(kind) = prop_kind(pk) else {
            errors.push(format!("{tag} : réglage non autorisé « {} »", clip(pk)));
            continue;
        };
        if let Some(allowed) = editable
            && !allowed.iter().any(|a| a == pk)
        {
            errors.push(format!("{tag} : réglage « {pk} » interdit pour cet élément"));
            continue;
        }
        if let Some(err) = check_prop(pk, kind, pv) {
            errors.push(format!("{tag} : {err}"));
        }
    }
}

fn check_prop(key: &str, kind: PropKind, v: &Value) -> Option<String> {
    let ok = match kind {
        PropKind::Bool => v.is_boolean(),
        PropKind::Int { min, max } => as_int(Some(v)).is_some_and(|n| (min..=max).contains(&n)),
        PropKind::Text { max } => v
            .as_str()
            .is_some_and(|s| len(s) <= max && !has_path(s) && !has_control(s)),
        PropKind::Color => v.as_str().is_some_and(|s| len(s) <= 9 && is_html_color(s)),
    };
    if ok {
        return None;
    }
    Some(match kind {
        PropKind::Bool => format!("« {key} » doit être vrai/faux"),
        PropKind::Int { .. } => format!("« {key} » hors limites"),
        PropKind::Text { .. } => {
            format!("« {key} » invalide (longueur, chemin ou caractère de contrôle)")
        }
        PropKind::Color => format!("« {key} » doit être une couleur #rrggbb"),
    })
}

fn validate_transform(t: Option<&Value>, tag: &str, errors: &mut Vec<String>) {
    let Some(o) = t.and_then(Value::as_object) else {
        errors.push(format!("{tag} : transform manquant"));
        return;
    };
    unknown_keys(o, TRANSFORM_KEYS, tag, "transform, ", errors);
    match vec3(o.get("origin")) {
        None => errors.push(format!("{tag} : origin invalide")),
        Some(p) if !in_bounds(&p) => errors.push(format!("{tag} : position hors limites")),
        Some(_) => {}
    }
    let Some(b) = o.get("basis").and_then(Value::as_array).filter(|b| b.len() == 3) else {
        errors.push(format!("{tag} : basis invalide"));
        return;
    };
    let mut cols = [[0.0; 3]; 3];
    for (i, col) in b.iter().enumerate() {
        let Some(c) = vec3(Some(col)) else {
            errors.push(format!("{tag} : basis invalide"));
            return;
        };
        cols[i] = c;
    }
    let mut volume = 1.0;
    for c in &cols {
        let l = (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt();
        if !l.is_finite() || l < SCALE_MIN * 0.99 || l > SCALE_MAX * 1.01 {
            errors.push(format!("{tag} : échelle hors limites ({l:.3})"));
            return;
        }
        volume *= l;
    }
    // Base « écrasée » (axes presque alignés) : sans inverse, les calculs de sélection partiraient en NaN.
    // L'éditeur n'écrit que des rotations × échelles (déterminant = produit des longueurs).
    if determinant(&cols).abs() < volume * 0.05 {
        errors.push(format!("{tag} : orientation invalide (axes alignés)"));
    }
}

/// Déterminant d'une base donnée par ses colonnes.
pub(crate) fn determinant(c: &[[f64; 3]; 3]) -> f64 {
    let [x, y, z] = c;
    x[0] * (y[1] * z[2] - y[2] * z[1]) - y[0] * (x[1] * z[2] - x[2] * z[1])
        + z[0] * (x[1] * y[2] - x[2] * y[1])
}

fn validate_roads(roads: &Value, uids: &mut HashSet<String>, errors: &mut Vec<String>) {
    let Some(roads) = roads.as_array() else {
        errors.push("« roads » doit être une liste".into());
        return;
    };
    if roads.len() > MAX_ROADS {
        errors.push(format!("trop de routes ({}, max {MAX_ROADS})", roads.len()));
        return;
    }
    for (i, r) in roads.iter().enumerate() {
        let tag = format!("route {i}");
        let Some(o) = r.as_object() else {
            errors.push(format!("{tag} : doit être un objet"));
            continue;
        };
        unknown_keys(o, ROAD_KEYS, &tag, "", errors);
        check_uid(o.get("uid"), &tag, uids, errors);
        if !o
            .get("template")
            .and_then(Value::as_str)
            .is_some_and(|t| ROAD_TEMPLATES.contains(&t))
        {
            errors.push(format!("{tag} : modèle inconnu"));
        }
        let Some(pts) = o
            .get("points")
            .and_then(Value::as_array)
            .filter(|p| (ROAD_MIN_POINTS..=ROAD_MAX_POINTS).contains(&p.len()))
        else {
            errors.push(format!(
                "{tag} : nombre de points invalide ({ROAD_MIN_POINTS} à {ROAD_MAX_POINTS})"
            ));
            continue;
        };
        let mut prev: Option<[f64; 3]> = None;
        for p in pts {
            let Some(v) = vec3(Some(p)) else {
                errors.push(format!("{tag} : point invalide"));
                break;
            };
            if !in_bounds(&v) {
                errors.push(format!("{tag} : point hors limites"));
                break;
            }
            if let Some(q) = prev {
                let d = ((v[0] - q[0]).powi(2) + (v[1] - q[1]).powi(2) + (v[2] - q[2]).powi(2)).sqrt();
                if d > ROAD_MAX_SEGMENT {
                    errors.push(format!(
                        "{tag} : segment trop long (> {} m)",
                        ROAD_MAX_SEGMENT as i64
                    ));
                    break;
                }
            }
            prev = Some(v);
        }
    }
}

fn validate_junctions(list: &Value, uids: &mut HashSet<String>, errors: &mut Vec<String>) {
    let Some(list) = list.as_array() else {
        errors.push("« junctions » doit être une liste".into());
        return;
    };
    if list.len() > MAX_JUNCTIONS {
        errors.push(format!(
            "trop de carrefours ({}, max {MAX_JUNCTIONS})",
            list.len()
        ));
        return;
    }
    for (i, j) in list.iter().enumerate() {
        let tag = format!("carrefour {i}");
        let Some(o) = j.as_object() else {
            errors.push(format!("{tag} : doit être un objet"));
            continue;
        };
        unknown_keys(o, JUNCTION_KEYS, &tag, "", errors);
        check_uid(o.get("uid"), &tag, uids, errors);
        if !o
            .get("kind")
            .and_then(Value::as_str)
            .is_some_and(|k| JUNCTION_KINDS.contains(&k))
        {
            errors.push(format!("{tag} : nature inconnue"));
        }
        match vec3(o.get("origin")) {
            None => errors.push(format!("{tag} : position invalide")),
            Some(p) if !in_bounds(&p) => errors.push(format!("{tag} : position hors limites")),
            Some(_) => {}
        }
        for key in ["yaw", "island"] {
            if let Some(v) = o.get(key)
                && !number(Some(v)).is_some_and(|f| f.is_finite() && f.abs() <= MAX_YAW)
            {
                errors.push(format!("{tag} : « {key} » invalide"));
            }
        }
        if let Some(island) = number(o.get("island"))
            && !(ISLAND_MIN..=ISLAND_MAX).contains(&island)
        {
            errors.push(format!("{tag} : taille de l'îlot hors bornes"));
        }
        let Some(arms) = o
            .get("arms")
            .and_then(Value::as_array)
            .filter(|a| (JUNCTION_MIN_ARMS..=JUNCTION_MAX_ARMS).contains(&a.len()))
        else {
            errors.push(format!(
                "{tag} : nombre de branches invalide ({JUNCTION_MIN_ARMS} à {JUNCTION_MAX_ARMS})"
            ));
            continue;
        };
        for a in arms {
            let Some(ao) = a.as_object() else {
                errors.push(format!("{tag} : branche invalide"));
                break;
            };
            for k in ao.keys() {
                if !ARM_KEYS.contains(&k.as_str()) {
                    errors.push(format!("{tag} : champ inconnu « {} » dans une branche", clip(k)));
                }
            }
            if !number(ao.get("angle")).is_some_and(|f| f.is_finite() && f.abs() <= 720.0) {
                errors.push(format!("{tag} : angle de branche invalide"));
            }
            if !ao
                .get("template")
                .and_then(Value::as_str)
                .is_some_and(|t| ROAD_TEMPLATES.contains(&t))
            {
                errors.push(format!("{tag} : modèle de branche inconnu"));
            }
        }
    }
}

/// Chemin de nœud de l'île acceptable : sous un des groupes modifiables, noms simples (lettres, chiffres, `_`).
pub fn is_base_path(path: &str) -> bool {
    if len(path) > MAX_BASE_PATH {
        return false;
    }
    if !BASE_ROOTS.iter().any(|r| path.starts_with(&format!("{r}/"))) {
        return false;
    }
    path.split('/').all(|part| {
        !part.is_empty()
            && len(part) <= MAX_NAME
            && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

fn validate_base_edits(list: &Value, catalog: Option<&dyn Catalog>, errors: &mut Vec<String>) {
    let Some(list) = list.as_array() else {
        errors.push("« base_edits » doit être une liste".into());
        return;
    };
    if list.len() > MAX_BASE_EDITS {
        errors.push(format!(
            "trop de modifications de l'île ({}, max {MAX_BASE_EDITS})",
            list.len()
        ));
        return;
    }
    let mut seen = HashSet::new();
    for (i, e) in list.iter().enumerate() {
        let tag = format!("modification de l'île {i}");
        let Some(o) = e.as_object() else {
            errors.push(format!("{tag} : doit être un objet"));
            continue;
        };
        unknown_keys(o, BASE_EDIT_KEYS, &tag, "", errors);
        match o.get("path").and_then(Value::as_str) {
            Some(p) if is_base_path(p) => {
                if !seen.insert(p.to_owned()) {
                    errors.push(format!("{tag} : chemin en double"));
                }
            }
            _ => errors.push(format!("{tag} : chemin invalide ou hors des groupes modifiables")),
        }
        match o.get("catalog").and_then(Value::as_str) {
            Some(cid) if !has_path(cid) => {
                if let Some(cat) = catalog
                    && cat.editable_props(cid).is_none()
                {
                    errors.push(format!("{tag} : id inconnu du catalogue « {} »", clip(cid)));
                }
            }
            _ => errors.push(format!("{tag} : id de catalogue invalide")),
        }
        validate_transform(o.get("from"), &format!("{tag} (from)"), errors);
        match o.get("removed").unwrap_or(&Value::Bool(false)) {
            Value::Bool(true) => {
                if o.contains_key("transform") {
                    errors.push(format!("{tag} : supprimé ET déplacé"));
                }
            }
            Value::Bool(false) => validate_transform(o.get("transform"), &tag, errors),
            _ => errors.push(format!("{tag} : « removed » doit être vrai/faux")),
        }
        if errors.len() > MAX_REPORTED_ERRORS {
            return;
        }
    }
}
