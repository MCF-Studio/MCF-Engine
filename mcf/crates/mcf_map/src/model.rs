//! Types Rust d'un calque. On ne les construit à partir du JSON **qu'après** validation (voir `io::load_str`).

use std::collections::BTreeMap;

use serde_json::{Map, Number, Value, json};

use crate::{BASE_LAYER, FORMAT, PropKind, VERSION, prop_kind};

/// Position + base (rotation × échelle), en mètres, repère Godot / Bevy.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MapTransform {
    /// Position.
    pub origin: [f64; 3],
    /// Axes X, Y, Z de la base (colonnes de la matrice).
    pub basis: [[f64; 3]; 3],
}

impl MapTransform {
    /// Aucune rotation, échelle 1, à l'origine.
    pub const IDENTITY: Self = Self {
        origin: [0.0; 3],
        basis: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
    };

    /// Identité placée en `origin`.
    pub fn at(origin: [f64; 3]) -> Self {
        Self {
            origin,
            ..Self::IDENTITY
        }
    }

    fn to_value(self) -> Value {
        json!({ "origin": self.origin, "basis": self.basis })
    }

    fn from_value(v: &Value) -> Option<Self> {
        let o = v.as_object()?;
        let b = o.get("basis")?.as_array()?;
        Some(Self {
            origin: vec3(o.get("origin")?)?,
            basis: [vec3(b.first()?)?, vec3(b.get(1)?)?, vec3(b.get(2)?)?],
        })
    }
}

/// Valeur d'un réglage autorisé.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PropValue {
    /// Vrai / faux (`light_on_at_night`).
    Bool(bool),
    /// Entier (`variant`).
    Int(i64),
    /// Texte court (`text`).
    Text(String),
    /// Couleur HTML (`color`).
    Color(String),
}

impl PropValue {
    fn to_value(&self) -> Value {
        match self {
            Self::Bool(b) => Value::Bool(*b),
            Self::Int(n) => Value::Number(Number::from(*n)),
            Self::Text(s) | Self::Color(s) => Value::String(s.clone()),
        }
    }

    fn from_value(key: &str, v: &Value) -> Option<Self> {
        Some(match prop_kind(key)? {
            PropKind::Bool => Self::Bool(v.as_bool()?),
            PropKind::Int { .. } => Self::Int(v.as_f64()?.round() as i64),
            PropKind::Text { .. } => Self::Text(v.as_str()?.to_owned()),
            PropKind::Color => Self::Color(v.as_str()?.to_owned()),
        })
    }
}

/// Un élément du catalogue posé sur la carte.
#[derive(Debug, Clone, PartialEq)]
pub struct MapItem {
    /// Identifiant unique stable (hexadécimal minuscule, 8 à 32 caractères).
    pub uid: String,
    /// Identifiant de la fiche du catalogue (`props/street_lamp`).
    pub catalog: String,
    /// Placement.
    pub transform: MapTransform,
    /// Réglages (liste blanche de la fiche).
    pub props: BTreeMap<String, PropValue>,
}

/// Une route tracée sur spline.
#[derive(Debug, Clone, PartialEq)]
pub struct MapRoad {
    /// Identifiant unique stable.
    pub uid: String,
    /// Modèle (`chemin`, `route`, `avenue`).
    pub template: String,
    /// Points au sol, en coordonnées monde.
    pub points: Vec<[f64; 3]>,
}

/// Une branche de carrefour.
#[derive(Debug, Clone, PartialEq)]
pub struct JunctionArm {
    /// Angle en degrés (0 = +Z, sens de `yaw`).
    pub angle: f64,
    /// Modèle de route de la branche.
    pub template: String,
}

/// Un carrefour ou un rond-point.
#[derive(Debug, Clone, PartialEq)]
pub struct MapJunction {
    /// Identifiant unique stable.
    pub uid: String,
    /// Nature (`carrefour`, `rond_point`).
    pub kind: String,
    /// Centre.
    pub origin: [f64; 3],
    /// Orientation (radians), si précisée.
    pub yaw: Option<f64>,
    /// Rayon de l'îlot central (rond-point), si précisé.
    pub island: Option<f64>,
    /// Branches.
    pub arms: Vec<JunctionArm>,
}

/// Ce que devient un élément de l'île de base.
#[derive(Debug, Clone, PartialEq)]
pub enum BaseChange {
    /// Déplacé / tourné / mis à l'échelle.
    Moved(MapTransform),
    /// Supprimé.
    Removed,
}

/// Modification d'un élément de l'île de base (`Island.tscn`).
#[derive(Debug, Clone, PartialEq)]
pub struct BaseEdit {
    /// Chemin du nœud dans l'île (sous un groupe modifiable).
    pub path: String,
    /// Fiche du catalogue de l'élément.
    pub catalog: String,
    /// Placement d'origine (refus si l'île a changé depuis).
    pub from: MapTransform,
    /// Nouveau placement ou suppression.
    pub change: BaseChange,
}

/// Un calque complet.
#[derive(Debug, Clone, PartialEq)]
pub struct McfMap {
    /// Version du catalogue utilisé pour le créer.
    pub catalog_version: i64,
    /// Nom du calque.
    pub layer: String,
    /// Auteur.
    pub author: String,
    /// Date de création (ISO 8601).
    pub created: String,
    /// Éléments posés.
    pub items: Vec<MapItem>,
    /// Routes.
    pub roads: Vec<MapRoad>,
    /// Carrefours.
    pub junctions: Vec<MapJunction>,
    /// `Some` : fichier des modifications de l'île de base (et alors rien d'autre).
    pub base_edits: Option<Vec<BaseEdit>>,
}

impl McfMap {
    /// Calque vide.
    pub fn new(layer: &str, author: &str, created: &str, catalog_version: i64) -> Self {
        Self {
            catalog_version,
            layer: layer.to_owned(),
            author: author.to_owned(),
            created: created.to_owned(),
            items: Vec::new(),
            roads: Vec::new(),
            junctions: Vec::new(),
            base_edits: None,
        }
    }

    /// Fichier des modifications de l'île de base.
    pub fn new_base(author: &str, created: &str, catalog_version: i64, edits: Vec<BaseEdit>) -> Self {
        Self {
            base_edits: Some(edits),
            ..Self::new(BASE_LAYER, author, created, catalog_version)
        }
    }

    /// JSON du calque (clés triées, comme `JSON.stringify` de Godot). Les nombres non finis deviennent `null`
    /// et seront refusés par la validation.
    pub fn to_value(&self) -> Value {
        let mut m = Map::new();
        m.insert("format".into(), FORMAT.into());
        m.insert("version".into(), VERSION.into());
        m.insert("catalog_version".into(), self.catalog_version.into());
        m.insert("layer".into(), self.layer.clone().into());
        m.insert("author".into(), self.author.clone().into());
        m.insert("created".into(), self.created.clone().into());
        let items: Vec<Value> = self
            .items
            .iter()
            .map(|it| {
                let props: Map<String, Value> =
                    it.props.iter().map(|(k, v)| (k.clone(), v.to_value())).collect();
                json!({
                    "uid": it.uid, "catalog": it.catalog,
                    "transform": it.transform.to_value(), "props": props,
                })
            })
            .collect();
        m.insert("items".into(), items.into());
        let roads: Vec<Value> = self
            .roads
            .iter()
            .map(|r| json!({ "uid": r.uid, "template": r.template, "points": r.points }))
            .collect();
        let junctions: Vec<Value> = self
            .junctions
            .iter()
            .map(|j| {
                let mut o = Map::new();
                o.insert("uid".into(), j.uid.clone().into());
                o.insert("kind".into(), j.kind.clone().into());
                o.insert("origin".into(), json!(j.origin));
                if let Some(y) = j.yaw {
                    o.insert("yaw".into(), json!(y));
                }
                if let Some(i) = j.island {
                    o.insert("island".into(), json!(i));
                }
                let arms: Vec<Value> = j
                    .arms
                    .iter()
                    .map(|a| json!({ "angle": a.angle, "template": a.template }))
                    .collect();
                o.insert("arms".into(), arms.into());
                Value::Object(o)
            })
            .collect();
        match &self.base_edits {
            Some(edits) => {
                // Même forme que `McfMapFormat.base_to_dict` : routes et carrefours seulement s'il y en a
                // (la validation refusera alors le fichier).
                if !roads.is_empty() {
                    m.insert("roads".into(), roads.into());
                }
                if !junctions.is_empty() {
                    m.insert("junctions".into(), junctions.into());
                }
                let edits: Vec<Value> = edits
                    .iter()
                    .map(|e| {
                        let mut o = Map::new();
                        o.insert("path".into(), e.path.clone().into());
                        o.insert("catalog".into(), e.catalog.clone().into());
                        o.insert("from".into(), e.from.to_value());
                        match e.change {
                            BaseChange::Moved(t) => {
                                o.insert("transform".into(), t.to_value());
                                o.insert("removed".into(), false.into());
                            }
                            BaseChange::Removed => {
                                o.insert("removed".into(), true.into());
                            }
                        }
                        Value::Object(o)
                    })
                    .collect();
                m.insert("base_edits".into(), edits.into());
            }
            None => {
                m.insert("roads".into(), roads.into());
                m.insert("junctions".into(), junctions.into());
            }
        }
        Value::Object(m)
    }

    /// Conversion d'un JSON **déjà validé**. `None` seulement si la validation a été contournée.
    pub(crate) fn from_validated(v: &Value) -> Option<Self> {
        let o = v.as_object()?;
        let s = |k: &str| o.get(k).and_then(Value::as_str).map(str::to_owned);
        let list = |k: &str| {
            o.get(k)
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default()
        };
        let items = list("items")
            .iter()
            .map(|it| {
                let io = it.as_object()?;
                let mut props = BTreeMap::new();
                if let Some(p) = io.get("props").and_then(Value::as_object) {
                    for (k, pv) in p {
                        props.insert(k.clone(), PropValue::from_value(k, pv)?);
                    }
                }
                Some(MapItem {
                    uid: io.get("uid")?.as_str()?.to_owned(),
                    catalog: io.get("catalog")?.as_str()?.to_owned(),
                    transform: MapTransform::from_value(io.get("transform")?)?,
                    props,
                })
            })
            .collect::<Option<Vec<_>>>()?;
        let roads = list("roads")
            .iter()
            .map(|r| {
                let ro = r.as_object()?;
                Some(MapRoad {
                    uid: ro.get("uid")?.as_str()?.to_owned(),
                    template: ro.get("template")?.as_str()?.to_owned(),
                    points: ro
                        .get("points")?
                        .as_array()?
                        .iter()
                        .map(vec3)
                        .collect::<Option<_>>()?,
                })
            })
            .collect::<Option<Vec<_>>>()?;
        let junctions = list("junctions")
            .iter()
            .map(|j| {
                let jo = j.as_object()?;
                Some(MapJunction {
                    uid: jo.get("uid")?.as_str()?.to_owned(),
                    kind: jo.get("kind")?.as_str()?.to_owned(),
                    origin: vec3(jo.get("origin")?)?,
                    yaw: jo.get("yaw").and_then(Value::as_f64),
                    island: jo.get("island").and_then(Value::as_f64),
                    arms: jo
                        .get("arms")?
                        .as_array()?
                        .iter()
                        .map(|a| {
                            Some(JunctionArm {
                                angle: a.get("angle")?.as_f64()?,
                                template: a.get("template")?.as_str()?.to_owned(),
                            })
                        })
                        .collect::<Option<_>>()?,
                })
            })
            .collect::<Option<Vec<_>>>()?;
        let base_edits = match o.get("base_edits") {
            None => None,
            Some(list) => Some(
                list.as_array()?
                    .iter()
                    .map(|e| {
                        let eo = e.as_object()?;
                        let removed = eo.get("removed").and_then(Value::as_bool).unwrap_or(false);
                        Some(BaseEdit {
                            path: eo.get("path")?.as_str()?.to_owned(),
                            catalog: eo.get("catalog")?.as_str()?.to_owned(),
                            from: MapTransform::from_value(eo.get("from")?)?,
                            change: if removed {
                                BaseChange::Removed
                            } else {
                                BaseChange::Moved(MapTransform::from_value(eo.get("transform")?)?)
                            },
                        })
                    })
                    .collect::<Option<Vec<_>>>()?,
            ),
        };
        Some(Self {
            catalog_version: o.get("catalog_version")?.as_f64()?.round() as i64,
            layer: s("layer")?,
            author: s("author")?,
            created: s("created")?,
            items,
            roads,
            junctions,
            base_edits,
        })
    }
}

fn vec3(v: &Value) -> Option<[f64; 3]> {
    let a = v.as_array()?;
    if a.len() != 3 {
        return None;
    }
    Some([a[0].as_f64()?, a[1].as_f64()?, a[2].as_f64()?])
}
