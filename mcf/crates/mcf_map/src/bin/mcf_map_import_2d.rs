//! Convertisseur strict de la convention `.mcf2d.json` vers les sorties MCF.
//!
//! Usage : `cargo run -p mcf_map --bin mcf_map_import_2d -- <source> <dossier-sortie> [--ecraser]`.

use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
};

use mcf_map::{JunctionArm, MapItem, MapJunction, MapRoad, MapTransform, McfMap, PropValue, write_atomic};
use serde::Deserialize;
use serde_json::{Value, json};

const MAX_SOURCE_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceMap {
    format: String,
    version: u64,
    name: String,
    metres_per_pixel: f64,
    origin_px: [f64; 2],
    background: String,
    roads: Vec<SourceRoad>,
    #[serde(default)]
    junctions: Vec<SourceJunction>,
    #[serde(default)]
    zones: Vec<SourceZone>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceRoad {
    id: String,
    class: String,
    points: Vec<[f64; 2]>,
    #[serde(default)]
    altitude_m: Option<f64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceJunction {
    id: String,
    kind: String,
    origin: [f64; 2],
    #[serde(default)]
    altitude_m: Option<f64>,
    #[serde(default)]
    yaw_deg: Option<f64>,
    #[serde(default)]
    island_m: Option<f64>,
    arms: Vec<SourceArm>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceArm {
    angle: f64,
    class: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceZone {
    id: String,
    kind: String,
    polygon: Vec<[f64; 2]>,
    #[serde(default)]
    altitude_m: Option<f64>,
}

fn usage() -> ! {
    eprintln!("Usage : mcf_map_import_2d <source.mcf2d.json> <dossier-sortie> [--ecraser]");
    std::process::exit(2);
}

fn main() {
    let mut args = env::args_os().skip(1);
    let Some(source) = args.next().map(PathBuf::from) else {
        usage()
    };
    let Some(output_dir) = args.next().map(PathBuf::from) else {
        usage()
    };
    let flag = args.next();
    if args.next().is_some() || flag.as_deref().is_some_and(|value| value != "--ecraser") {
        usage();
    }
    let overwrite = flag.is_some();
    if source.extension().and_then(|s| s.to_str()) != Some("json") {
        fail("la source doit être un fichier JSON");
    }

    let source_text = read_source(&source).unwrap_or_else(|message| fail(&message));
    let input: SourceMap = serde_json::from_str(&source_text)
        .unwrap_or_else(|error| fail(&format!("source .mcf2d.json invalide : {error}")));
    let converted = convert(input).unwrap_or_else(|message| fail(&message));

    let base = output_dir.join(&converted.name);
    let map_path = base.with_extension("mcfmap");
    let zones_path = base.with_extension("mcfzones.json");
    let import_path = base.with_extension("import.json");
    for path in [&map_path, &zones_path, &import_path] {
        if path.exists() && !overwrite {
            fail(&format!(
                "sortie déjà présente : {} (ajoutez --ecraser)",
                path.display()
            ));
        }
    }

    mcf_map::save_file(&map_path, &converted.map)
        .unwrap_or_else(|error| fail(&format!(".mcfmap non écrit : {error}")));
    write_json(&zones_path, &converted.zones).unwrap_or_else(|message| fail(&message));
    write_json(&import_path, &converted.import).unwrap_or_else(|message| fail(&message));
    println!("Carte importée : {}", map_path.display());
    println!("Zones : {}", zones_path.display());
    println!("Manifeste : {}", import_path.display());
}

struct Converted {
    name: String,
    map: McfMap,
    zones: Value,
    import: Value,
}

fn convert(source: SourceMap) -> Result<Converted, String> {
    if source.format != "mcf2d" || source.version != 1 {
        return Err("format attendu « mcf2d » en version 1".into());
    }
    check_id(&source.name, "name")?;
    check_finite(source.metres_per_pixel, "metres_per_pixel")?;
    if source.metres_per_pixel <= 0.0 || source.metres_per_pixel > 100.0 {
        return Err("metres_per_pixel doit être compris entre 0 et 100".into());
    }
    if source.background.is_empty() || source.background.contains(['/', '\\']) {
        return Err("background doit être un nom de fichier simple non vide".into());
    }
    for point in source.origin_px {
        check_finite(point, "origin_px")?;
    }

    let mut seen = BTreeMap::new();
    let mut map = McfMap::new(&source.name, "mcf2d_import", "import_mcf2d", 1);
    for (index, road) in source.roads.iter().enumerate() {
        check_id(&road.id, "id de route")?;
        unique(&mut seen, &road.id)?;
        check_template(&road.class)?;
        if !(2..=200).contains(&road.points.len()) {
            return Err(format!("route « {} » : 2 à 200 points requis", road.id));
        }
        let altitude = road.altitude_m.unwrap_or(0.0);
        check_finite(altitude, "altitude_m")?;
        let points = road
            .points
            .iter()
            .map(|p| point_world(*p, altitude, &source))
            .collect::<Result<Vec<_>, _>>()?;
        map.roads.push(MapRoad {
            uid: uid("road", &road.id, index),
            template: road.class.clone(),
            points,
        });
    }
    for (index, junction) in source.junctions.iter().enumerate() {
        check_id(&junction.id, "id de carrefour")?;
        unique(&mut seen, &junction.id)?;
        if !matches!(junction.kind.as_str(), "carrefour" | "rond_point") {
            return Err(format!("carrefour « {} » : kind inconnu", junction.id));
        }
        if !(2..=6).contains(&junction.arms.len()) {
            return Err(format!("carrefour « {} » : 2 à 6 bras requis", junction.id));
        }
        let altitude = junction.altitude_m.unwrap_or(0.0);
        let origin = point_world(junction.origin, altitude, &source)?;
        let arms = junction
            .arms
            .iter()
            .map(|arm| {
                check_template(&arm.class)?;
                check_finite(arm.angle, "angle")?;
                // Dans le plan 2D, 0=nord et le sens est horaire. Dans MCF, 0=+Z.
                Ok(JunctionArm {
                    angle: -arm.angle,
                    template: arm.class.clone(),
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let yaw = junction.yaw_deg.map(|degrees| -degrees.to_radians());
        if let Some(value) = yaw {
            check_finite(value, "yaw_deg")?;
        }
        if let Some(value) = junction.island_m {
            check_finite(value, "island_m")?;
        }
        map.junctions.push(MapJunction {
            uid: uid("junction", &junction.id, index),
            kind: junction.kind.clone(),
            origin,
            yaw,
            island: junction.island_m,
            arms,
        });
    }
    let mut zone_values = Vec::new();
    for (index, zone) in source.zones.iter().enumerate() {
        check_id(&zone.id, "id de zone")?;
        check_id(&zone.kind, "kind de zone")?;
        unique(&mut seen, &zone.id)?;
        if !(3..=200).contains(&zone.polygon.len()) || polygon_self_intersects(&zone.polygon) {
            return Err(format!(
                "zone « {} » : polygone invalide ou auto-intersecté",
                zone.id
            ));
        }
        let altitude = zone.altitude_m.unwrap_or(0.0);
        let polygon = zone
            .polygon
            .iter()
            .map(|p| point_world(*p, altitude, &source))
            .collect::<Result<Vec<_>, _>>()?;
        let centroid = polygon
            .iter()
            .fold([0.0; 3], |mut total, point| {
                for i in 0..3 {
                    total[i] += point[i];
                }
                total
            })
            .map(|value| value / polygon.len() as f64);
        let mut props = BTreeMap::new();
        props.insert("text".to_owned(), PropValue::Text(zone.id.clone()));
        map.items.push(MapItem {
            uid: uid("zone", &zone.id, index),
            catalog: format!("markers/zone/{}", zone.kind),
            transform: MapTransform::at(centroid),
            props,
        });
        zone_values.push(json!({ "id": zone.id, "kind": zone.kind, "polygon": polygon }));
    }
    map.to_json_pretty()
        .map_err(|error| format!("conversion refusée par mcf_map : {error}"))?;
    Ok(Converted {
        name: source.name.clone(),
        map,
        zones: json!({ "format": "mcfzones", "version": 1, "source": format!("{}.mcf2d.json", source.name), "background": source.background, "zones": zone_values }),
        import: json!({ "format": "mcf2d-import", "version": 1, "source": source.name, "metres_per_pixel": source.metres_per_pixel, "warnings": [] }),
    })
}

fn point_world(point: [f64; 2], altitude: f64, source: &SourceMap) -> Result<[f64; 3], String> {
    check_finite(point[0], "coordonnée de point")?;
    check_finite(point[1], "coordonnée de point")?;
    check_finite(altitude, "altitude_m")?;
    Ok([
        (point[0] - source.origin_px[0]) * source.metres_per_pixel,
        altitude,
        -(point[1] - source.origin_px[1]) * source.metres_per_pixel,
    ])
}

fn check_id(value: &str, field: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-'))
    {
        return Err(format!("{field} : identifiant invalide"));
    }
    Ok(())
}
fn check_template(value: &str) -> Result<(), String> {
    if matches!(value, "chemin" | "route" | "avenue") {
        Ok(())
    } else {
        Err(format!("classe de route inconnue : {value}"))
    }
}
fn check_finite(value: f64, field: &str) -> Result<(), String> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(format!("{field} doit être un nombre fini"))
    }
}
fn unique(seen: &mut BTreeMap<String, ()>, id: &str) -> Result<(), String> {
    if seen.insert(id.to_owned(), ()).is_some() {
        Err(format!("identifiant dupliqué : {id}"))
    } else {
        Ok(())
    }
}

fn uid(kind: &str, id: &str, index: usize) -> String {
    // FNV-1a 64 bits : stable entre machines, puis représenté dans le format hexadécimal MCF.
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in format!("{kind}:{index}:{id}").bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

fn polygon_self_intersects(points: &[[f64; 2]]) -> bool {
    let len = points.len();
    (0..len).any(|a| {
        ((a + 1)..len).any(|b| {
            let next_a = (a + 1) % len;
            let next_b = (b + 1) % len;
            a != b
                && next_a != b
                && next_b != a
                && segments_intersect(points[a], points[next_a], points[b], points[next_b])
        })
    })
}
fn segments_intersect(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> bool {
    let cross =
        |p: [f64; 2], q: [f64; 2], r: [f64; 2]| (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0]);
    let (ab_c, ab_d, cd_a, cd_b) = (cross(a, b, c), cross(a, b, d), cross(c, d, a), cross(c, d, b));
    ab_c * ab_d <= 0.0 && cd_a * cd_b <= 0.0
}

fn read_source(path: &Path) -> Result<String, String> {
    let metadata = fs::metadata(path).map_err(|_| format!("source illisible : {}", path.display()))?;
    if metadata.len() > MAX_SOURCE_BYTES {
        return Err("source trop volumineuse".into());
    }
    fs::read_to_string(path).map_err(|_| format!("source illisible ou UTF-8 invalide : {}", path.display()))
}
fn write_json(path: &Path, value: &Value) -> Result<(), String> {
    let text = serde_json::to_vec_pretty(value).map_err(|_| "JSON de sortie impossible".to_owned())?;
    write_atomic(path, &text).map_err(|error| format!("sortie non écrite ({}): {error}", path.display()))
}
fn fail(message: &str) -> ! {
    eprintln!("Erreur : {message}");
    std::process::exit(1);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn un_point_image_devient_un_point_monde() {
        let source = SourceMap {
            format: "mcf2d".into(),
            version: 1,
            name: "test".into(),
            metres_per_pixel: 0.5,
            origin_px: [10.0, 20.0],
            background: "a.png".into(),
            roads: vec![],
            junctions: vec![],
            zones: vec![],
        };
        assert_eq!(
            point_world([14.0, 26.0], 3.0, &source).expect("point"),
            [2.0, 3.0, -3.0]
        );
    }
    #[test]
    fn le_convertisseur_produit_un_mcfmap_valide() {
        let source = SourceMap {
            format: "mcf2d".into(),
            version: 1,
            name: "test".into(),
            metres_per_pixel: 1.0,
            origin_px: [0.0, 0.0],
            background: "a.png".into(),
            roads: vec![SourceRoad {
                id: "rue".into(),
                class: "route".into(),
                points: vec![[0.0, 0.0], [5.0, 0.0]],
                altitude_m: None,
            }],
            junctions: vec![],
            zones: vec![],
        };
        let converted = convert(source).expect("conversion");
        assert!(mcf_map::load_str(&converted.map.to_json_pretty().expect("json"), None).is_ok());
    }
}
