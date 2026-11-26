//! Bibliothèque d'assets 3D de l'éditeur : sections → familles → objets, noms en français, mots-clés de recherche.
//!
//! Le catalogue (`bibliotheque.json`, écrit par `mcf/tools/bibliotheque/importer.py`) est **non fiable** comme toute
//! donnée lue sur disque : champs connus seulement, identifiants sûrs et uniques, chemins relatifs confinés au dossier
//! de la bibliothèque (pas de `..`, pas de chemin absolu, extension `.glb` / `.gltf`), textes et quantités bornés.
//! Aucun code n'est lu depuis le catalogue.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::manifeste::nom_sur;

/// Valeur du champ `format`.
pub const FORMAT: &str = "mcfbibliotheque";
/// Version du format.
pub const VERSION: u32 = 1;
/// Taille maximale du fichier (octets).
pub const MAX_OCTETS: u64 = 8 * 1024 * 1024;
/// Sections au plus.
pub const MAX_SECTIONS: usize = 64;
/// Familles au plus (toutes sections).
pub const MAX_FAMILLES: usize = 4_000;
/// Objets au plus (toutes familles).
pub const MAX_OBJETS: usize = 50_000;
/// Mots-clés par objet au plus.
pub const MAX_MOTS: usize = 32;
/// Longueur maximale d'un nom affiché (caractères).
pub const MAX_NOM: usize = 80;
/// Longueur maximale d'un chemin de modèle (octets).
pub const MAX_CHEMIN: usize = 256;

/// Catalogue complet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bibliotheque {
    /// Toujours [`FORMAT`].
    pub format: String,
    /// Toujours [`VERSION`].
    pub version: u32,
    /// Sections, dans l'ordre d'affichage.
    pub sections: Vec<Section>,
}

/// Une section (onglet) : « Mobilier urbain », « Campagne et nature »…
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Section {
    /// Identifiant sûr.
    pub id: String,
    /// Nom affiché.
    pub nom: String,
    /// Pictogramme (quelques caractères).
    #[serde(default)]
    pub icone: String,
    /// Familles, dans l'ordre d'affichage.
    pub familles: Vec<FamilleObjets>,
}

/// Une famille d'objets : « Bancs », « Lampadaires »…
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FamilleObjets {
    /// Identifiant sûr.
    pub id: String,
    /// Nom affiché.
    pub nom: String,
    /// Objets, dans l'ordre d'affichage.
    pub objets: Vec<Objet>,
}

/// Un objet posable : un modèle glTF.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Objet {
    /// Identifiant sûr et unique dans la bibliothèque.
    pub id: String,
    /// Nom affiché.
    pub nom: String,
    /// Modèle `.glb` / `.gltf`, relatif au dossier de la bibliothèque (`/` comme séparateur).
    pub fichier: String,
    /// Mots-clés de recherche (en plus du nom).
    #[serde(default)]
    pub mots: Vec<String>,
}

/// Position d'un objet : (section, famille, objet).
pub type Place = (usize, usize, usize);

fn texte_ok(t: &str, max: usize) -> bool {
    !t.trim().is_empty() && t.chars().count() <= max && !t.chars().any(char::is_control)
}

/// Chemin relatif sûr : segments non vides, sans `.` ni `..`, sans `\`, `:` ni caractère de contrôle, extension de
/// modèle glTF.
pub fn chemin_sur(c: &str) -> bool {
    let ext_ok = {
        let bas = c.to_ascii_lowercase();
        bas.ends_with(".glb") || bas.ends_with(".gltf")
    };
    ext_ok
        && c.len() <= MAX_CHEMIN
        && !c.starts_with('/')
        && !c.chars().any(|ch| ch == '\\' || ch == ':' || ch.is_control())
        && c.split('/').all(|s| !s.is_empty() && s != "." && s != "..")
}

impl Bibliotheque {
    /// Lit et valide un catalogue.
    pub fn depuis_json(texte: &str) -> Result<Self, String> {
        if texte.len() as u64 > MAX_OCTETS {
            return Err("catalogue trop gros".into());
        }
        let b: Self = serde_json::from_str(texte).map_err(|e| format!("catalogue illisible : {e}"))?;
        b.valider()?;
        Ok(b)
    }

    /// Vérifie toutes les bornes.
    pub fn valider(&self) -> Result<(), String> {
        if self.format != FORMAT || self.version != VERSION {
            return Err(format!("format {} v{} inattendu", self.format, self.version));
        }
        if self.sections.len() > MAX_SECTIONS {
            return Err("trop de sections".into());
        }
        let (mut familles, mut objets) = (0usize, 0usize);
        let mut ids = HashSet::new();
        for s in &self.sections {
            if !nom_sur(&s.id) || !texte_ok(&s.nom, MAX_NOM) || s.icone.chars().count() > 8 {
                return Err(format!("section invalide : {:?}", s.id));
            }
            for f in &s.familles {
                familles += 1;
                if !nom_sur(&f.id) || !texte_ok(&f.nom, MAX_NOM) {
                    return Err(format!("famille invalide : {:?}", f.id));
                }
                for o in &f.objets {
                    objets += 1;
                    if !nom_sur(&o.id) || !ids.insert(o.id.as_str()) {
                        return Err(format!("identifiant d'objet invalide ou en double : {:?}", o.id));
                    }
                    if !texte_ok(&o.nom, MAX_NOM) {
                        return Err(format!("nom invalide pour {}", o.id));
                    }
                    if !chemin_sur(&o.fichier) {
                        return Err(format!("chemin refusé pour {} : {:?}", o.id, o.fichier));
                    }
                    if o.mots.len() > MAX_MOTS || !o.mots.iter().all(|m| texte_ok(m, 32)) {
                        return Err(format!("mots-clés invalides pour {}", o.id));
                    }
                }
            }
        }
        if familles > MAX_FAMILLES || objets > MAX_OBJETS {
            return Err("trop de familles ou d'objets".into());
        }
        Ok(())
    }

    /// Nombre total d'objets.
    pub fn nombre(&self) -> usize {
        self.sections
            .iter()
            .flat_map(|s| &s.familles)
            .map(|f| f.objets.len())
            .sum()
    }

    /// Objet à une place.
    pub fn objet(&self, (s, f, o): Place) -> Option<&Objet> {
        self.sections.get(s)?.familles.get(f)?.objets.get(o)
    }

    /// Objets dont le nom, la famille, la section ou les mots-clés contiennent **tous** les mots de la requête (sans
    /// tenir compte des accents ni de la casse), au plus `max`, dans l'ordre du catalogue. Requête vide : rien.
    pub fn chercher(&self, requete: &str, max: usize) -> Vec<Place> {
        let termes: Vec<String> = normaliser(requete)
            .split_whitespace()
            .map(str::to_owned)
            .collect();
        if termes.is_empty() {
            return Vec::new();
        }
        let mut out = Vec::new();
        for (is, s) in self.sections.iter().enumerate() {
            for (jf, f) in s.familles.iter().enumerate() {
                let contexte = normaliser(&format!("{} {}", s.nom, f.nom));
                for (ko, o) in f.objets.iter().enumerate() {
                    let texte = format!(
                        "{contexte} {} {}",
                        normaliser(&o.nom),
                        normaliser(&o.mots.join(" "))
                    );
                    if termes.iter().all(|t| texte.contains(t.as_str())) {
                        out.push((is, jf, ko));
                        if out.len() >= max {
                            return out;
                        }
                    }
                }
            }
        }
        out
    }
}

/// Minuscules sans accents (é → e, ç → c, œ → oe…), pour la recherche.
pub fn normaliser(t: &str) -> String {
    let mut s = String::with_capacity(t.len());
    for c in t.chars().flat_map(char::to_lowercase) {
        match c {
            'à' | 'â' | 'ä' | 'á' | 'ã' => s.push('a'),
            'é' | 'è' | 'ê' | 'ë' => s.push('e'),
            'î' | 'ï' | 'í' => s.push('i'),
            'ô' | 'ö' | 'ó' => s.push('o'),
            'ù' | 'û' | 'ü' | 'ú' => s.push('u'),
            'ÿ' => s.push('y'),
            'ç' => s.push('c'),
            'œ' => s.push_str("oe"),
            'æ' => s.push_str("ae"),
            '’' | '\'' | '-' | '«' | '»' | '(' | ')' | ',' | '—' => s.push(' '),
            c => s.push(c),
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exemple() -> Bibliotheque {
        Bibliotheque::depuis_json(
            r#"{"format":"mcfbibliotheque","version":1,"sections":[
              {"id":"mobilier","nom":"Mobilier urbain","icone":"🪑","familles":[
                {"id":"mobilier_00","nom":"Bancs","objets":[
                  {"id":"bench_concrete","nom":"Banc en béton","fichier":"props/03/Benches/BenchConcrete.glb","mots":["bench","concrete"]},
                  {"id":"bench_wood","nom":"Banc en bois","fichier":"props/03/Benches/BenchWood.glb"}]}]},
              {"id":"nature","nom":"Campagne et nature","familles":[
                {"id":"nature_00","nom":"Arbres","objets":[
                  {"id":"tree_oak","nom":"Chêne","fichier":"props/07/Trees/TreeOak.glb","mots":["oak"]}]}]}]}"#,
        )
        .expect("catalogue valide")
    }

    #[test]
    fn recherche_sans_accents_et_sur_tous_les_mots() {
        let b = exemple();
        assert_eq!(b.nombre(), 3);
        assert_eq!(b.chercher("beton", 10), vec![(0, 0, 0)]);
        assert_eq!(b.chercher("BANC bois", 10), vec![(0, 0, 1)]);
        assert_eq!(b.chercher("chene", 10), vec![(1, 0, 0)]);
        assert_eq!(b.chercher("oak", 10), vec![(1, 0, 0)]);
        assert_eq!(b.chercher("nature", 10), vec![(1, 0, 0)], "la section compte");
        assert_eq!(b.chercher("banc", 1).len(), 1, "borné");
        assert!(b.chercher("  ", 10).is_empty());
    }

    #[test]
    fn chemins_confines() {
        for ok in ["a/b.glb", "props/x/Y.gltf"] {
            assert!(chemin_sur(ok), "{ok}");
        }
        for ko in [
            "../a.glb",
            "a/../b.glb",
            "/a.glb",
            "C:/a.glb",
            "a\\b.glb",
            "a//b.glb",
            "a/b.png",
            "./a.glb",
        ] {
            assert!(!chemin_sur(ko), "{ko}");
        }
    }

    #[test]
    fn catalogue_refuse() {
        let base = |objet: &str| {
            format!(
                r#"{{"format":"mcfbibliotheque","version":1,"sections":[{{"id":"s","nom":"S","familles":[{{"id":"f","nom":"F","objets":[{objet}]}}]}}]}}"#
            )
        };
        let bon = r#"{"id":"a","nom":"A","fichier":"a.glb"}"#;
        assert!(Bibliotheque::depuis_json(&base(bon)).is_ok());
        for mauvais in [
            r#"{"id":"A majuscule","nom":"A","fichier":"a.glb"}"#,
            r#"{"id":"a","nom":"","fichier":"a.glb"}"#,
            r#"{"id":"a","nom":"A","fichier":"../a.glb"}"#,
            r#"{"id":"a","nom":"A","fichier":"a.glb","script":"x"}"#,
            &format!("{bon},{bon}"),
        ] {
            assert!(Bibliotheque::depuis_json(&base(mauvais)).is_err(), "{mauvais}");
        }
    }

    #[test]
    fn catalogue_du_depot_valide() {
        let chemin =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../mcf_editor/assets/bibliotheque.json");
        let texte = std::fs::read_to_string(&chemin).expect("catalogue du dépôt");
        let b = Bibliotheque::depuis_json(&texte).expect("catalogue du dépôt valide");
        assert!(b.nombre() > 500);
        assert!(!b.chercher("lampadaire", 50).is_empty());
    }
}
