//! Tests du format `.mcfmap` : aller-retour, compatibilité avec l'éditeur Godot, fichiers piégés refusés.
#![allow(clippy::unwrap_used, missing_docs)]

use std::collections::BTreeMap;

use mcf_map::*;
use serde_json::{Value, json};

fn catalog() -> SimpleCatalog {
    SimpleCatalog::new(7)
        .with("props/street_lamp", &["light_on_at_night", "color"])
        .with("props/sign", &["text", "variant"])
}

fn sample() -> McfMap {
    let mut m = McfMap::new("ville_mobilier", "Pseudo", "2026-11-20T18:00:00+01:00", 7);
    let mut props = BTreeMap::new();
    props.insert("light_on_at_night".into(), PropValue::Bool(true));
    props.insert("color".into(), PropValue::Color("#ffcc00".into()));
    m.items.push(MapItem {
        uid: "c1f4a2b3".into(),
        catalog: "props/street_lamp".into(),
        transform: MapTransform {
            origin: [12.5, 3.12, -40.0],
            ..MapTransform::IDENTITY
        },
        props,
    });
    let mut sign = BTreeMap::new();
    sign.insert("text".into(), PropValue::Text("Boulangerie Éloïse".into()));
    sign.insert("variant".into(), PropValue::Int(2));
    // Rotation de 90° autour de Y, échelle 2 sur X.
    m.items.push(MapItem {
        uid: "00ff00aa11".into(),
        catalog: "props/sign".into(),
        transform: MapTransform {
            origin: [-4999.0, 0.0, 4999.0],
            basis: [[0.0, 0.0, -2.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]],
        },
        props: sign,
    });
    m.roads.push(MapRoad {
        uid: "aaaabbbb".into(),
        template: "avenue".into(),
        points: vec![[0.0, 0.0, 0.0], [100.0, 0.5, 0.0], [100.0, 1.0, 350.0]],
    });
    m.junctions.push(MapJunction {
        uid: "ccccdddd".into(),
        kind: "rond_point".into(),
        origin: [50.0, 0.0, 50.0],
        yaw: Some(0.5),
        island: Some(6.0),
        arms: vec![
            JunctionArm {
                angle: 0.0,
                template: "route".into(),
            },
            JunctionArm {
                angle: 120.0,
                template: "chemin".into(),
            },
            JunctionArm {
                angle: 240.0,
                template: "route".into(),
            },
        ],
    });
    m
}

fn errors_of(v: &Value) -> Vec<String> {
    validate(v, Some(&catalog())).errors
}

fn assert_refused(v: &Value, contains: &str) {
    let errs = errors_of(v);
    assert!(
        errs.iter().any(|e| e.contains(contains)),
        "attendu « {contains} » dans {errs:?}"
    );
}

/// Relecture exacte des nombres : sans l'option `float_roundtrip` de serde_json, un nombre à 17 chiffres
/// significatifs (`-1806.6100000000001`) était relu `-1806.61`, et réenregistrer un calque le modifiait.
#[test]
fn nombres_relus_au_bit_pres() {
    let mut m = sample();
    m.items[0].transform.origin = [-1806.6100000000001, 0.07707599999999999, -1821.0900000000001];
    let text = m.to_json_pretty().unwrap();
    let back = load_str(&text, Some(&catalog())).unwrap();
    assert_eq!(back.map, m);
    assert_eq!(
        back.map.to_json_pretty().unwrap(),
        text,
        "réenregistrer ne doit rien changer"
    );
}

#[test]
fn aller_retour_identique() {
    let m = sample();
    let text = m.to_json_pretty().unwrap();
    let back = load_str(&text, Some(&catalog())).unwrap();
    assert_eq!(back.map, m);
    assert!(back.warnings.is_empty());
}

#[test]
fn fichier_de_l_editeur_godot_accepte() {
    // Forme produite par `McfMapFormat.layer_to_dict` + `JSON.stringify` : tout nombre est un flottant.
    let text = r#"{
	"author": "Pseudo", "catalog_version": 7.0, "created": "2026-11-20T18:00:00",
	"format": "mcfmap", "layer": "ville_mobilier", "version": 1.0,
	"items": [{ "uid": "c1f4a2b3", "catalog": "props/sign",
		"transform": { "origin": [12.5, 3.12, -40.0], "basis": [[1.0,0.0,0.0],[0.0,1.0,0.0],[0.0,0.0,1.0]] },
		"props": { "variant": 3.0 } }],
	"roads": [], "junctions": []
}"#;
    let l = load_str(text, Some(&catalog())).unwrap();
    assert_eq!(l.map.items[0].props["variant"], PropValue::Int(3));
    assert_eq!(l.map.catalog_version, 7);
}

#[test]
fn ile_de_base_aller_retour() {
    let m = McfMap::new_base(
        "Aroun",
        "2026-11-20T18:00:00",
        7,
        vec![
            BaseEdit {
                path: "City/Places/TownHall".into(),
                catalog: "props/sign".into(),
                from: MapTransform::at([1.0, 0.0, 1.0]),
                change: BaseChange::Moved(MapTransform::at([2.0, 0.0, 1.0])),
            },
            BaseEdit {
                path: "StreetLamps3D/Lamp_12".into(),
                catalog: "props/street_lamp".into(),
                from: MapTransform::at([5.0, 0.0, 5.0]),
                change: BaseChange::Removed,
            },
        ],
    );
    let back = load_str(&m.to_json_pretty().unwrap(), Some(&catalog())).unwrap();
    assert_eq!(back.map, m);
}

#[test]
fn catalogue_plus_recent_averti() {
    let mut m = sample();
    m.catalog_version = 9;
    let l = load_str(&m.to_json_pretty().unwrap(), Some(&catalog())).unwrap();
    assert_eq!(l.warnings.len(), 1);
}

#[test]
fn champs_inconnus_refuses() {
    let mut v = sample().to_value();
    v["script"] = json!("print(1)");
    assert_refused(&v, "champ inconnu « script »");
    let mut v = sample().to_value();
    v["items"][0]["scene"] = json!("res://x.tscn");
    assert_refused(&v, "élément 0 : champ inconnu « scene »");
    let mut v = sample().to_value();
    v["items"][0]["transform"]["scale"] = json!([1, 1, 1]);
    assert_refused(&v, "transform, champ inconnu « scale »");
}

#[test]
fn chemins_refuses_partout() {
    for (field, value) in [
        ("author", "../../etc"),
        ("layer", "user://x"),
        ("created", "C:\\x"),
    ] {
        let mut v = sample().to_value();
        v[field] = json!(value);
        assert_refused(&v, &format!("« {field} » invalide"));
    }
    let mut v = sample().to_value();
    v["items"][0]["catalog"] = json!("res://props/street_lamp.tscn");
    assert_refused(&v, "id de catalogue invalide");
    let mut v = sample().to_value();
    v["items"][1]["props"]["text"] = json!("voir http://piege");
    assert_refused(&v, "« text » invalide");
}

#[test]
fn texte_invisible_refuse() {
    let mut v = sample().to_value();
    v["items"][1]["props"]["text"] = json!("Banque\u{202E}euqnaB");
    assert_refused(&v, "« text » invalide");
}

#[test]
fn reglages_controles() {
    let mut v = sample().to_value();
    v["items"][0]["props"]["text"] = json!("x"); // autorisé globalement, pas pour un lampadaire
    assert_refused(&v, "réglage « text » interdit pour cet élément");
    let mut v = sample().to_value();
    v["items"][0]["props"]["on_load"] = json!("x");
    assert_refused(&v, "réglage non autorisé « on_load »");
    let mut v = sample().to_value();
    v["items"][1]["props"]["variant"] = json!(17);
    assert_refused(&v, "« variant » hors limites");
    let mut v = sample().to_value();
    v["items"][0]["props"]["color"] = json!("rouge");
    assert_refused(&v, "couleur");
    let mut v = sample().to_value();
    v["items"][0]["props"]["light_on_at_night"] = json!(1);
    assert_refused(&v, "vrai/faux");
}

#[test]
fn identifiants_controles() {
    let mut v = sample().to_value();
    v["items"][1]["uid"] = json!("c1f4a2b3");
    assert_refused(&v, "élément 1 : uid en double");
    let mut v = sample().to_value();
    v["roads"][0]["uid"] = json!("c1f4a2b3"); // un uid est unique dans tout le calque
    assert_refused(&v, "route 0 : uid en double");
    let mut v = sample().to_value();
    v["items"][0]["uid"] = json!("C1F4A2B3");
    assert_refused(&v, "uid invalide");
    let mut v = sample().to_value();
    v["items"][0]["catalog"] = json!("props/inconnu");
    assert_refused(&v, "id inconnu du catalogue");
}

#[test]
fn bornes_de_transform() {
    let mut v = sample().to_value();
    v["items"][0]["transform"]["origin"] = json!([5000.5, 0, 0]);
    assert_refused(&v, "position hors limites");
    let mut v = sample().to_value();
    v["items"][0]["transform"]["basis"] = json!([[20, 0, 0], [0, 1, 0], [0, 0, 1]]);
    assert_refused(&v, "échelle hors limites");
    let mut v = sample().to_value();
    v["items"][0]["transform"]["basis"] = json!([[1, 0, 0], [1, 0.01, 0], [0, 0, 1]]);
    assert_refused(&v, "axes alignés");
    let mut v = sample().to_value();
    v["items"][0]["transform"]["basis"] = json!([[1, 0, 0], [0, 1, 0]]);
    assert_refused(&v, "basis invalide");
    let mut v = sample().to_value();
    v["items"][0]["transform"]["origin"] = json!([1, "2", 3]);
    assert_refused(&v, "origin invalide");
}

#[test]
fn routes_et_carrefours_controles() {
    let mut v = sample().to_value();
    v["roads"][0]["points"][2] = json!([100.0, 1.0, 600.0]);
    assert_refused(&v, "segment trop long (> 400 m)");
    let mut v = sample().to_value();
    v["roads"][0]["template"] = json!("autoroute");
    assert_refused(&v, "route 0 : modèle inconnu");
    let mut v = sample().to_value();
    v["junctions"][0]["kind"] = json!("echangeur");
    assert_refused(&v, "nature inconnue");
    let mut v = sample().to_value();
    v["junctions"][0]["island"] = json!(50);
    assert_refused(&v, "îlot hors bornes");
    let mut v = sample().to_value();
    v["junctions"][0]["arms"] = json!([{ "angle": 0, "template": "route" }]);
    assert_refused(&v, "nombre de branches invalide");
}

#[test]
fn ile_de_base_controlee() {
    let edit = |path: &str| {
        json!({ "format": "mcfmap", "version": 1, "catalog_version": 7, "layer": BASE_LAYER,
            "author": "a", "created": "c", "items": [],
            "base_edits": [{ "path": path, "catalog": "props/sign",
                "from": MapTransform::IDENTITY_JSON(), "removed": true }] })
    };
    assert!(errors_of(&edit("City/Places/Mairie_1")).is_empty());
    assert_refused(&edit("Terrain/Heights"), "hors des groupes modifiables");
    assert_refused(&edit("City/Places/../Terrain"), "hors des groupes modifiables");
    assert_refused(&edit("City/Places/Mairie-1"), "hors des groupes modifiables");
    let mut v = edit("City/Places/Mairie_1");
    v["items"] = sample().to_value()["items"].clone();
    assert_refused(&v, "ne contient que des modifications de l'île");
    let mut v = edit("City/Places/Mairie_1");
    v["base_edits"][0]["transform"] = MapTransform::IDENTITY_JSON();
    assert_refused(&v, "supprimé ET déplacé");
}

#[test]
fn fichiers_enormes_ou_tordus_refuses() {
    // Trop imbriqué : refusé avant même l'analyse JSON.
    let deep = format!("{}{}", "[".repeat(10_000), "]".repeat(10_000));
    assert_eq!(load_str(&deep, None).unwrap_err().errors, ["JSON trop imbriqué"]);
    assert_eq!(
        load_str("pas du json", None).unwrap_err().errors,
        ["JSON invalide"]
    );
    assert_eq!(load_str("[1,2]", None).unwrap_err().errors, ["JSON invalide"]);
    // Trop d'éléments.
    let mut v = sample().to_value();
    let item = v["items"][0].clone();
    v["items"] = Value::Array(vec![item; MAX_ITEMS + 1]);
    assert_refused(&v, "trop d'éléments");
    // Trop d'erreurs : la validation s'arrête.
    let mut v = sample().to_value();
    v["items"] = Value::Array(vec![json!(1); 500]);
    let errs = errors_of(&v);
    assert!(errs.len() <= MAX_REPORTED_ERRORS + 2, "{}", errs.len());
}

#[test]
fn ecriture_refusee_si_invalide() {
    let mut m = sample();
    m.items[0].transform.origin[0] = f64::NAN;
    assert!(m.to_json_pretty().is_err());
    m.items[0].transform.origin[0] = 9_000.0;
    assert!(m.to_json_pretty().is_err());
}

#[test]
fn enregistrement_atomique() {
    let dir = std::env::temp_dir().join(format!("mcf_map_test_{}", std::process::id()));
    let path = dir.join("calque.mcfmap");
    save_file(&path, &sample()).unwrap();
    // Réécriture par-dessus un fichier existant.
    save_file(&path, &sample()).unwrap();
    assert!(!dir.join("calque.mcfmap.tmp").exists());
    assert_eq!(load_file(&path, Some(&catalog())).unwrap().map, sample());
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(
        load_file(&path, None).unwrap_err().errors,
        ["fichier introuvable"]
    );
}

/// Petit utilitaire de test : transform identité en JSON.
trait IdentityJson {
    #[allow(non_snake_case)]
    fn IDENTITY_JSON() -> Value;
}

impl IdentityJson for MapTransform {
    fn IDENTITY_JSON() -> Value {
        json!({ "origin": [0, 0, 0], "basis": [[1, 0, 0], [0, 1, 0], [0, 0, 1]] })
    }
}
