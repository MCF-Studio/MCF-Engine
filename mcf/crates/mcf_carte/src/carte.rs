//! Le document de carte : sources, historique (annuler / refaire illimité dans une limite mémoire) et dérivés
//! recalculés à la demande. Toute modification passe par une **transaction** : elle est validée en entier ou
//! annulée en entier (invariant 5 de `ARCHITECTURE_DONNEES_CARTE.md`).

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use mcf_geo::{ClasseRoute, P2, polygone};

use crate::batiments::{Batiment, Origine, Transformation};
use crate::contours::{Contour, Contours, UsageContour};
use crate::decor::ObjetDecor;
use crate::gabarits::{Densite, Famille};
use crate::geometrie::{self, GeoReseau};
use crate::ids::Id;
use crate::ilots::{self, Ilot};
use crate::lotissement;
use crate::manifeste::Manifeste;
use crate::quartiers::{Affectation, Quartiers, ZoneLibre};
use crate::references::{self, ImageRgba, Reference, References};
use crate::releve;
use crate::relief::{self, AltitudesReseau, Pinceau, Relief, Zone};
use crate::reseau::Reseau;

/// Côté des blocs de relief mémorisés pour l'annulation (échantillons).
const BLOC: usize = 32;
/// Étapes d'historique conservées.
pub const HISTORIQUE_MAX: usize = 300;

/// Image « avant » d'une étape : ce qu'il faut remettre pour l'annuler.
#[derive(Debug, Clone, Default)]
struct Etape {
    libelle: String,
    manifeste: Option<Manifeste>,
    reseau: Option<Reseau>,
    quartiers: Option<Quartiers>,
    contours: Option<Contours>,
    references: Option<References>,
    batiments: BTreeMap<Id, Option<Batiment>>,
    decor: BTreeMap<Id, Option<ObjetDecor>>,
    relief: BTreeMap<(usize, usize), Vec<f32>>,
    calques: BTreeMap<String, Option<mcf_map::McfMap>>,
    /// Pixels des calques de référence peints (image « avant »).
    images: BTreeMap<Id, ImageRgba>,
}

impl Etape {
    fn vide(&self) -> bool {
        self.manifeste.is_none()
            && self.reseau.is_none()
            && self.quartiers.is_none()
            && self.contours.is_none()
            && self.references.is_none()
            && self.batiments.is_empty()
            && self.decor.is_empty()
            && self.relief.is_empty()
            && self.calques.is_empty()
            && self.images.is_empty()
    }
}

/// Ce qui a changé depuis le dernier enregistrement.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Sale {
    /// Manifeste.
    pub manifeste: bool,
    /// Réseau.
    pub reseau: bool,
    /// Quartiers.
    pub quartiers: bool,
    /// Contours nommés.
    pub contours: bool,
    /// Calques de référence (fichier et images).
    pub references: bool,
    /// Cellules de bâti.
    pub batiments: bool,
    /// Cellules de décor.
    pub decor: bool,
    /// Tuiles de relief `(x, z)`.
    pub relief: BTreeSet<(usize, usize)>,
    /// Calques d'objets.
    pub calques: BTreeSet<String>,
    /// Images de calques de référence peintes (à réécrire).
    pub images: BTreeSet<Id>,
}

impl Sale {
    /// Rien à enregistrer ?
    pub fn propre(&self) -> bool {
        *self == Self::default()
    }
}

/// Compteurs de révision : l'éditeur ne reconstruit que ce qui a changé.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Revisions {
    /// Réseau.
    pub reseau: u64,
    /// Relief source.
    pub relief: u64,
    /// Quartiers.
    pub quartiers: u64,
    /// Contours nommés.
    pub contours: u64,
    /// Calques de référence.
    pub references: u64,
    /// Bâti.
    pub batiments: u64,
    /// Décor.
    pub decor: u64,
    /// Calques d'objets.
    pub calques: u64,
    /// Manifeste.
    pub manifeste: u64,
}

/// Données dérivées.
#[derive(Debug, Clone, Default)]
pub struct Derive {
    /// Géométrie du réseau.
    pub geo: GeoReseau,
    /// Altitudes du réseau.
    pub altitudes: AltitudesReseau,
    /// Îlots.
    pub ilots: Vec<Ilot>,
    /// Terrain final (terrassé).
    pub terrain: Relief,
    /// Révisions des sources ayant servi.
    pub revisions: Revisions,
    /// Dernière mise à jour du terrain final (ce que l'affichage doit refaire).
    pub maj_terrain: MajTerrain,
}

/// Mise à jour du terrain final : il passe de la version `de` à la version `a`. `blocs` : blocs de terrain
/// ([`relief::BLOC_TERRAIN`]) refaits, `None` si tout a été refait. Un affichage resté à la version `de` ne refait
/// que ces blocs ; sinon il refait tout.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MajTerrain {
    /// Version d'avant.
    pub de: u64,
    /// Version actuelle.
    pub a: u64,
    /// Blocs refaits (`None` : tout).
    pub blocs: Option<BTreeSet<(usize, usize)>>,
}

impl MajTerrain {
    /// Rectangles `(min, max)` (m) refaits, vus depuis la version `vu` d'un affichage ; `None` : tout refaire.
    pub fn rectangles(&self, vu: Option<u64>, terrain: &Relief) -> Option<Vec<[P2; 2]>> {
        if vu == Some(self.a) {
            return Some(Vec::new());
        }
        if vu != Some(self.de) {
            return None;
        }
        let blocs = self.blocs.as_ref()?;
        Some(
            blocs
                .iter()
                .map(|b| terrain.rectangle_zone(terrain.zone_bloc_terrain(*b)))
                .collect(),
        )
    }
}

/// Un projet de carte ouvert.
#[derive(Debug, Clone)]
pub struct Carte {
    /// Dossier du projet (`None` tant qu'il n'est pas enregistré).
    pub dossier: Option<PathBuf>,
    /// Manifeste.
    pub manifeste: Manifeste,
    /// Réseau routier.
    pub reseau: Reseau,
    /// Quartiers.
    pub quartiers: Quartiers,
    /// Contours nommés (entrées de souterrain…).
    pub contours: Contours,
    /// Calques de référence (placement ; les pixels sont dans `images_references`).
    pub references: References,
    /// Pixels des calques de référence, par identifiant. Annuler un import retire seulement le calque (l'image reste
    /// en mémoire pour « refaire ») ; les coups de pinceau sur un calque (`peindre_reference`) sont dans l'historique.
    pub images_references: BTreeMap<Id, ImageRgba>,
    /// Version des pixels de chaque calque peint (l'éditeur ne renvoie au GPU que les images qui ont changé).
    pub versions_images: BTreeMap<Id, u64>,
    /// Bâti.
    pub batiments: BTreeMap<Id, Batiment>,
    /// Décor posé depuis la bibliothèque.
    pub decor: BTreeMap<Id, ObjetDecor>,
    /// Relief source.
    pub relief: Relief,
    /// Calques d'objets posés (`.mcfmap`), par nom de fichier.
    pub calques: BTreeMap<String, mcf_map::McfMap>,
    /// Modifications non enregistrées.
    pub sale: Sale,
    /// Révisions courantes.
    pub revisions: Revisions,
    passe: Vec<Etape>,
    futur: Vec<Etape>,
    trait_en_cours: Option<Etape>,
    derive: Option<Derive>,
    /// Échantillons du relief source modifiés depuis le dernier calcul des dérivés.
    relief_touche: Option<Zone>,
}

/// Accès aux sources pendant une transaction ; chaque accès mémorise l'état d'avant.
pub struct Transaction<'a> {
    carte: &'a mut Carte,
    etape: Etape,
}

impl Transaction<'_> {
    /// Manifeste (lecture).
    pub fn manifeste(&self) -> &Manifeste {
        &self.carte.manifeste
    }

    /// Manifeste modifiable.
    pub fn manifeste_mut(&mut self) -> &mut Manifeste {
        if self.etape.manifeste.is_none() {
            self.etape.manifeste = Some(self.carte.manifeste.clone());
        }
        &mut self.carte.manifeste
    }

    /// Réseau modifiable.
    pub fn reseau(&mut self) -> &mut Reseau {
        if self.etape.reseau.is_none() {
            self.etape.reseau = Some(self.carte.reseau.clone());
        }
        &mut self.carte.reseau
    }

    /// Réseau et manifeste ensemble (le tracé a besoin des bornes).
    pub fn reseau_et_manifeste(&mut self) -> (&mut Reseau, &Manifeste) {
        if self.etape.reseau.is_none() {
            self.etape.reseau = Some(self.carte.reseau.clone());
        }
        (&mut self.carte.reseau, &self.carte.manifeste)
    }

    /// Quartiers modifiables.
    pub fn quartiers(&mut self) -> &mut Quartiers {
        if self.etape.quartiers.is_none() {
            self.etape.quartiers = Some(self.carte.quartiers.clone());
        }
        &mut self.carte.quartiers
    }

    /// Contours modifiables.
    pub fn contours(&mut self) -> &mut Contours {
        if self.etape.contours.is_none() {
            self.etape.contours = Some(self.carte.contours.clone());
        }
        &mut self.carte.contours
    }

    /// Calques de référence modifiables.
    pub fn references(&mut self) -> &mut References {
        if self.etape.references.is_none() {
            self.etape.references = Some(self.carte.references.clone());
        }
        &mut self.carte.references
    }

    fn noter_batiment(&mut self, id: Id) {
        self.etape
            .batiments
            .entry(id)
            .or_insert_with(|| self.carte.batiments.get(&id).cloned());
    }

    /// Bâtiment modifiable.
    pub fn batiment(&mut self, id: Id) -> Option<&mut Batiment> {
        self.noter_batiment(id);
        self.carte.batiments.get_mut(&id)
    }

    /// Ajoute ou remplace un bâtiment.
    pub fn poser_batiment(&mut self, id: Id, b: Batiment) {
        self.noter_batiment(id);
        self.carte.batiments.insert(id, b);
    }

    /// Retire un bâtiment.
    pub fn retirer_batiment(&mut self, id: Id) -> Option<Batiment> {
        self.noter_batiment(id);
        self.carte.batiments.remove(&id)
    }

    fn noter_objet(&mut self, id: Id) {
        self.etape
            .decor
            .entry(id)
            .or_insert_with(|| self.carte.decor.get(&id).cloned());
    }

    /// Objet de décor modifiable.
    pub fn objet(&mut self, id: Id) -> Option<&mut ObjetDecor> {
        self.noter_objet(id);
        self.carte.decor.get_mut(&id)
    }

    /// Ajoute ou remplace un objet de décor.
    pub fn poser_objet(&mut self, id: Id, o: ObjetDecor) {
        self.noter_objet(id);
        self.carte.decor.insert(id, o);
    }

    /// Retire un objet de décor.
    pub fn retirer_objet(&mut self, id: Id) -> Option<ObjetDecor> {
        self.noter_objet(id);
        self.carte.decor.remove(&id)
    }

    /// Calque d'objets modifiable.
    pub fn calque(&mut self, nom: &str) -> Option<&mut mcf_map::McfMap> {
        self.etape
            .calques
            .entry(nom.to_owned())
            .or_insert_with(|| self.carte.calques.get(nom).cloned());
        self.carte.calques.get_mut(nom)
    }

    /// Lecture de la carte pendant la transaction.
    pub fn carte(&self) -> &Carte {
        self.carte
    }
}

fn blocs_de(z: Zone) -> impl Iterator<Item = (usize, usize)> {
    let (bx0, bz0, bx1, bz1) = (z.x0 / BLOC, z.z0 / BLOC, z.x1.div_ceil(BLOC), z.z1.div_ceil(BLOC));
    (bz0..bz1).flat_map(move |bz| (bx0..bx1).map(move |bx| (bx, bz)))
}

impl Carte {
    /// Nouvelle carte vide (relief plat à 0 m), non encore enregistrée.
    pub fn nouvelle(manifeste: Manifeste) -> Result<Self, String> {
        manifeste.valider()?;
        // Plat à 0 m, ou au bord le plus proche si la plage d'altitude ne contient pas 0.
        let relief = Relief::plat(
            &manifeste,
            0.0f64.clamp(manifeste.altitude_min_m, manifeste.altitude_max_m),
        );
        let (tx, tz) = manifeste.tuiles();
        Ok(Self {
            dossier: None,
            relief,
            reseau: Reseau::default(),
            quartiers: Quartiers::default(),
            contours: Contours::default(),
            references: References::default(),
            images_references: BTreeMap::new(),
            versions_images: BTreeMap::new(),
            batiments: BTreeMap::new(),
            decor: BTreeMap::new(),
            calques: BTreeMap::new(),
            sale: Sale {
                manifeste: true,
                reseau: true,
                quartiers: true,
                contours: true,
                references: true,
                batiments: true,
                decor: true,
                relief: (0..tz).flat_map(|z| (0..tx).map(move |x| (x, z))).collect(),
                calques: BTreeSet::new(),
                images: BTreeSet::new(),
            },
            revisions: Revisions::default(),
            manifeste,
            passe: Vec::new(),
            futur: Vec::new(),
            trait_en_cours: None,
            derive: None,
            relief_touche: None,
        })
    }

    pub(crate) fn assembler(
        dossier: PathBuf,
        manifeste: Manifeste,
        reseau: Reseau,
        quartiers: Quartiers,
        contours: Contours,
        (references, images_references): (References, BTreeMap<Id, ImageRgba>),
        batiments: BTreeMap<Id, Batiment>,
        relief: Relief,
        calques: BTreeMap<String, mcf_map::McfMap>,
    ) -> Self {
        Self {
            dossier: Some(dossier),
            manifeste,
            reseau,
            quartiers,
            contours,
            references,
            images_references,
            versions_images: BTreeMap::new(),
            batiments,
            decor: BTreeMap::new(),
            relief,
            calques,
            sale: Sale::default(),
            revisions: Revisions::default(),
            passe: Vec::new(),
            futur: Vec::new(),
            trait_en_cours: None,
            derive: None,
            relief_touche: None,
        }
    }

    /// Vérifie toutes les sources.
    pub fn valider(&self) -> Result<(), String> {
        self.manifeste.valider().map_err(|e| format!("manifeste : {e}"))?;
        self.reseau
            .valider(&self.manifeste)
            .map_err(|e| format!("réseau : {e}"))?;
        self.quartiers
            .valider(&self.manifeste)
            .map_err(|e| format!("quartiers : {e}"))?;
        self.contours
            .valider(&self.manifeste)
            .map_err(|e| format!("contours : {e}"))?;
        self.valider_references()?;
        if self.batiments.len() > crate::batiments::MAX_BATIMENTS {
            return Err("trop de bâtiments".into());
        }
        for (id, b) in &self.batiments {
            b.valider(&self.manifeste)
                .map_err(|e| format!("bâtiment {id} : {e}"))?;
        }
        if self.decor.len() > crate::decor::MAX_OBJETS {
            return Err("trop d'objets de décor".into());
        }
        for (id, o) in &self.decor {
            o.valider(&self.manifeste)
                .map_err(|e| format!("objet de décor {id} : {e}"))?;
        }
        Ok(())
    }

    /// Exécute une modification. En cas d'erreur (ou de résultat invalide), tout est remis en l'état.
    pub fn modifier<R>(
        &mut self,
        libelle: &str,
        f: impl FnOnce(&mut Transaction<'_>) -> Result<R, String>,
    ) -> Result<R, String> {
        self.fin_trait();
        let mut tx = Transaction {
            carte: self,
            etape: Etape {
                libelle: libelle.to_owned(),
                ..Default::default()
            },
        };
        let resultat = f(&mut tx).and_then(|r| {
            let c = &tx.carte;
            let e = &tx.etape;
            if e.manifeste.is_some() {
                c.manifeste.valider().map_err(|m| format!("manifeste : {m}"))?;
            }
            if e.reseau.is_some() || e.manifeste.is_some() {
                c.reseau
                    .valider(&c.manifeste)
                    .map_err(|m| format!("réseau : {m}"))?;
            }
            if e.quartiers.is_some() {
                c.quartiers
                    .valider(&c.manifeste)
                    .map_err(|m| format!("quartiers : {m}"))?;
            }
            if e.contours.is_some() || e.manifeste.is_some() {
                c.contours
                    .valider(&c.manifeste)
                    .map_err(|m| format!("contours : {m}"))?;
            }
            if e.references.is_some() || e.manifeste.is_some() {
                c.valider_references()?;
            }
            for id in e.batiments.keys() {
                if let Some(b) = c.batiments.get(id) {
                    b.valider(&c.manifeste).map_err(|m| format!("bâtiment : {m}"))?;
                }
            }
            for id in e.decor.keys() {
                if let Some(o) = c.decor.get(id) {
                    o.valider(&c.manifeste).map_err(|m| format!("décor : {m}"))?;
                }
            }
            if c.decor.len() > crate::decor::MAX_OBJETS {
                return Err("trop d'objets de décor".into());
            }
            Ok(r)
        });
        let etape = tx.etape;
        match resultat {
            Ok(r) => {
                if !etape.vide() {
                    self.marquer(&etape);
                    self.passe.push(etape);
                    if self.passe.len() > HISTORIQUE_MAX {
                        self.passe.remove(0);
                    }
                    self.futur.clear();
                }
                Ok(r)
            }
            Err(e) => {
                self.restaurer(etape);
                Err(e)
            }
        }
    }

    /// Note les familles touchées par une étape.
    fn marquer(&mut self, e: &Etape) {
        if e.manifeste.is_some() {
            self.sale.manifeste = true;
            self.revisions.manifeste += 1;
        }
        if e.reseau.is_some() {
            self.sale.reseau = true;
            self.revisions.reseau += 1;
        }
        if e.quartiers.is_some() {
            self.sale.quartiers = true;
            self.revisions.quartiers += 1;
        }
        if e.contours.is_some() {
            self.sale.contours = true;
            self.revisions.contours += 1;
        }
        if e.references.is_some() {
            self.sale.references = true;
            self.revisions.references += 1;
        }
        if !e.batiments.is_empty() {
            self.sale.batiments = true;
            self.revisions.batiments += 1;
        }
        if !e.decor.is_empty() {
            self.sale.decor = true;
            self.revisions.decor += 1;
        }
        if !e.relief.is_empty() {
            self.revisions.relief += 1;
            for (bx, bz) in e.relief.keys() {
                let z = self.zone_bloc(*bx, *bz);
                self.toucher_relief(z);
            }
            let par_tuile = (self.manifeste.tuile_m as f64 / self.manifeste.resolution_relief_m) as usize;
            for (bx, bz) in e.relief.keys() {
                let (x0, z0) = (bx * BLOC, bz * BLOC);
                let (x1, z1) = (x0 + BLOC, z0 + BLOC);
                // Les tuiles partagent leurs bords : un échantillon de bord appartient à deux tuiles.
                let (tx, tz) = self.manifeste.tuiles();
                for z in z0.saturating_sub(1) / par_tuile..=(z1 / par_tuile).min(tz - 1) {
                    for x in x0.saturating_sub(1) / par_tuile..=(x1 / par_tuile).min(tx - 1) {
                        self.sale.relief.insert((x, z));
                    }
                }
            }
        }
        if !e.calques.is_empty() {
            self.revisions.calques += 1;
            self.sale.calques.extend(e.calques.keys().cloned());
        }
        if !e.images.is_empty() {
            self.sale.references = true;
            self.revisions.references += 1;
            for id in e.images.keys() {
                self.sale.images.insert(*id);
                *self.versions_images.entry(*id).or_default() += 1;
            }
        }
    }

    /// Remet l'image « avant » d'une étape et renvoie l'image inverse (pour refaire).
    fn restaurer(&mut self, e: Etape) -> Etape {
        let mut inverse = Etape {
            libelle: e.libelle.clone(),
            ..Default::default()
        };
        if let Some(m) = e.manifeste {
            inverse.manifeste = Some(std::mem::replace(&mut self.manifeste, m));
        }
        if let Some(r) = e.reseau {
            inverse.reseau = Some(std::mem::replace(&mut self.reseau, r));
        }
        if let Some(q) = e.quartiers {
            inverse.quartiers = Some(std::mem::replace(&mut self.quartiers, q));
        }
        if let Some(c) = e.contours {
            inverse.contours = Some(std::mem::replace(&mut self.contours, c));
        }
        if let Some(r) = e.references {
            inverse.references = Some(std::mem::replace(&mut self.references, r));
        }
        for (id, avant) in e.batiments {
            let courant = match avant {
                Some(b) => self.batiments.insert(id, b),
                None => self.batiments.remove(&id),
            };
            inverse.batiments.insert(id, courant);
        }
        for (id, avant) in e.decor {
            let courant = match avant {
                Some(o) => self.decor.insert(id, o),
                None => self.decor.remove(&id),
            };
            inverse.decor.insert(id, courant);
        }
        for ((bx, bz), valeurs) in e.relief {
            let z = self.zone_bloc(bx, bz);
            inverse.relief.insert((bx, bz), self.relief.copier(z));
            self.relief.coller(z, &valeurs);
        }
        for (nom, avant) in e.calques {
            let courant = match avant {
                Some(c) => self.calques.insert(nom.clone(), c),
                None => self.calques.remove(&nom),
            };
            inverse.calques.insert(nom, courant);
        }
        for (id, avant) in e.images {
            if let Some(courant) = self.images_references.insert(id, avant) {
                inverse.images.insert(id, courant);
            }
        }
        inverse
    }

    fn zone_bloc(&self, bx: usize, bz: usize) -> Zone {
        Zone {
            x0: bx * BLOC,
            z0: bz * BLOC,
            x1: ((bx + 1) * BLOC).min(self.relief.nx),
            z1: ((bz + 1) * BLOC).min(self.relief.nz),
        }
    }

    /// Annule la dernière étape. Renvoie son libellé.
    pub fn annuler(&mut self) -> Option<String> {
        self.fin_trait();
        let e = self.passe.pop()?;
        let libelle = e.libelle.clone();
        let inverse = self.restaurer(e);
        self.marquer(&inverse);
        self.futur.push(inverse);
        Some(libelle)
    }

    /// Refait la dernière étape annulée.
    pub fn refaire(&mut self) -> Option<String> {
        self.fin_trait();
        let e = self.futur.pop()?;
        let libelle = e.libelle.clone();
        let inverse = self.restaurer(e);
        self.marquer(&inverse);
        self.passe.push(inverse);
        Some(libelle)
    }

    /// Libellés de l'historique (passé, futur).
    pub fn historique(&self) -> (Vec<&str>, Vec<&str>) {
        (
            self.passe.iter().map(|e| e.libelle.as_str()).collect(),
            self.futur.iter().rev().map(|e| e.libelle.as_str()).collect(),
        )
    }

    // ---------------------------------------------------------------- relief

    /// Un coup de pinceau. Les coups successifs d'un même geste forment une seule étape (voir `fin_trait`).
    pub fn pinceau(&mut self, outil: Pinceau, centre: P2, rayon: f64, force: f64, cible: f64) {
        let rayon = rayon.clamp(0.5, 2000.0);
        let zone = self.relief.zone_disque(centre, rayon);
        if zone.vide() {
            return;
        }
        let mut etape = self.trait_en_cours.take().unwrap_or_else(|| Etape {
            libelle: format!("Relief : {}", outil.nom()),
            ..Default::default()
        });
        for (bx, bz) in blocs_de(zone) {
            etape
                .relief
                .entry((bx, bz))
                .or_insert_with(|| self.relief.copier(self.zone_bloc(bx, bz)));
        }
        let m = &self.manifeste;
        let graine = m.graine;
        let bornes = (m.altitude_min_m, m.altitude_max_m);
        self.relief
            .pinceau(outil, centre, rayon, force, cible, graine, bornes);
        self.toucher_relief(zone);
        self.revisions.relief += 1;
        self.trait_en_cours = Some(etape);
    }

    /// Termine le geste de sculpture en cours (une seule annulation pour tout le geste).
    pub fn fin_trait(&mut self) {
        if let Some(e) = self.trait_en_cours.take()
            && !e.vide()
        {
            self.marquer(&e);
            self.passe.push(e);
            if self.passe.len() > HISTORIQUE_MAX {
                self.passe.remove(0);
            }
            self.futur.clear();
        }
    }

    // ---------------------------------------------------------------- dérivés

    /// Note des échantillons du relief source modifiés (dérivés à refaire autour).
    fn toucher_relief(&mut self, z: Zone) {
        let z = Zone {
            x0: z.x0.saturating_sub(1),
            z0: z.z0.saturating_sub(1),
            x1: (z.x1 + 1).min(self.relief.nx),
            z1: (z.z1 + 1).min(self.relief.nz),
        };
        self.relief_touche = Some(self.relief_touche.map_or(z, |t| t.union(z)));
    }

    /// Dérivés à jour (recalcule ce qui a changé). Après une retouche du réseau ou du relief, seuls les profils en
    /// long et les blocs de terrain concernés sont refaits (voir [`MajTerrain`]) ; le résultat est identique à un
    /// calcul complet (test `derives_incrementaux_identiques`).
    pub fn derive(&mut self) -> &Derive {
        let r = self.revisions;
        let a_jour = self.derive.as_ref().is_some_and(|d| {
            d.revisions.reseau == r.reseau
                && d.revisions.relief == r.relief
                && d.revisions.manifeste == r.manifeste
        });
        if !a_jour {
            let relief_touche = self.relief_touche.take();
            let ancien = self.derive.take();
            let reprise = ancien.filter(|d| {
                d.revisions.manifeste == r.manifeste
                    && d.terrain.nx == self.relief.nx
                    && d.terrain.nz == self.relief.nz
                    && (d.revisions.relief == r.relief || relief_touche.is_some())
            });
            self.derive = Some(match reprise {
                Some(d) => self.derive_partiel(d, relief_touche),
                None => self.derive_complet(),
            });
        }
        let d = self.derive.get_or_insert_with(Derive::default);
        d.revisions = r;
        d
    }

    fn derive_complet(&self) -> Derive {
        let profils = &self.manifeste.profils;
        let geo = geometrie::calculer(&self.reseau, profils);
        let altitudes = relief::altitudes_reseau(&self.reseau, &geo, &self.relief, profils);
        let terrain = relief::terrain_final(&self.relief, &geo, &altitudes, profils);
        let ilots = ilots::calculer(&self.reseau, &geo, profils);
        let version = self.derive.as_ref().map_or(0, |d| d.maj_terrain.a) + 1;
        Derive {
            geo,
            altitudes,
            ilots,
            terrain,
            revisions: self.revisions,
            maj_terrain: MajTerrain {
                de: version - 1,
                a: version,
                blocs: None,
            },
        }
    }

    /// Reprend les dérivés `d` : profils inchangés gardés, blocs de terrain refaits autour de ce qui a bougé.
    fn derive_partiel(&self, d: Derive, relief_touche: Option<Zone>) -> Derive {
        let profils = &self.manifeste.profils;
        let reseau_change = d.revisions.reseau != self.revisions.reseau;
        let geo = if reseau_change {
            geometrie::calculer(&self.reseau, profils)
        } else {
            d.geo.clone()
        };
        let rect_relief = relief_touche
            .filter(|z| !z.vide())
            .map(|z| self.relief.rectangle_zone(z));
        let altitudes = relief::altitudes_reseau_depuis(
            &self.reseau,
            &geo,
            &self.relief,
            profils,
            Some(&relief::Reprise {
                geo: &d.geo,
                altitudes: &d.altitudes,
                relief_touche: rect_relief,
            }),
        );
        // Blocs à refaire : autour de chaque tronçon ou carrefour dont la géométrie ou les altitudes ont changé (dans
        // son ancien et son nouvel état), et sous le relief retouché.
        let mut blocs = BTreeSet::new();
        let terrain_src = &self.relief;
        let autour_troncon = |g: &geometrie::GeoTroncon, blocs: &mut BTreeSet<(usize, usize)>| {
            let portee = relief::portee_terrassement(profils, g.classe);
            for s in g.axe.windows(2) {
                let [a, b] = relief::englobant(s);
                blocs.extend(
                    terrain_src
                        .blocs_rectangle([a[0] - portee, a[1] - portee], [b[0] + portee, b[1] + portee]),
                );
            }
        };
        let ids: BTreeSet<Id> = d
            .geo
            .troncons
            .keys()
            .chain(geo.troncons.keys())
            .copied()
            .collect();
        for id in ids {
            let (avant, apres) = (d.geo.troncons.get(&id), geo.troncons.get(&id));
            let meme = avant == apres && d.altitudes.troncons.get(&id) == altitudes.troncons.get(&id);
            if meme {
                continue;
            }
            for g in [avant, apres].into_iter().flatten() {
                autour_troncon(g, &mut blocs);
            }
        }
        let ids: BTreeSet<Id> = d.geo.noeuds.keys().chain(geo.noeuds.keys()).copied().collect();
        for id in ids {
            let (avant, apres) = (d.geo.noeuds.get(&id), geo.noeuds.get(&id));
            if avant == apres && d.altitudes.noeuds.get(&id) == altitudes.noeuds.get(&id) {
                continue;
            }
            for n in [avant, apres].into_iter().flatten() {
                if !n.carrefour.emprise.is_empty() {
                    let [a, b] = relief::englobant(&n.carrefour.emprise);
                    blocs.extend(terrain_src.blocs_rectangle(a, b));
                }
            }
        }
        if let Some([a, b]) = rect_relief {
            blocs.extend(terrain_src.blocs_rectangle(a, b));
        }
        let mut terrain = d.terrain;
        relief::terrain_final_partiel(&self.relief, &geo, &altitudes, profils, &mut terrain, &blocs);
        let ilots = if reseau_change {
            ilots::calculer(&self.reseau, &geo, profils)
        } else {
            d.ilots
        };
        let version = d.maj_terrain.a;
        let maj_terrain = if blocs.is_empty() {
            MajTerrain {
                de: version,
                a: version,
                blocs: Some(blocs),
            }
        } else {
            MajTerrain {
                de: version,
                a: version + 1,
                blocs: Some(blocs),
            }
        };
        Derive {
            geo,
            altitudes,
            ilots,
            terrain,
            revisions: self.revisions,
            maj_terrain,
        }
    }

    // ---------------------------------------------------------------- commandes

    /// Trace une route.
    pub fn tracer_route(
        &mut self,
        classe: ClasseRoute,
        chemin: &[P2],
        accroche: f64,
    ) -> Result<Vec<Id>, String> {
        self.modifier(&format!("Tracer : {}", classe.nom()), |tx| {
            let (r, m) = tx.reseau_et_manifeste();
            r.tracer(classe, chemin, accroche, m)
        })
    }

    /// Affecte l'îlot qui contient `repere` (remplace une affectation existante du même îlot).
    pub fn affecter(&mut self, repere: P2, famille: Famille, densite: Densite) -> Result<Id, String> {
        let contour = {
            let d = self.derive();
            ilots::ilot_en(&d.ilots, repere).map(|i| i.contour.clone())
        };
        let Some(contour) = contour else {
            return Err("aucun îlot ici : fermez-le par des routes, ou dessinez une zone libre".into());
        };
        self.modifier(&format!("Quartier : {} {}", famille.nom(), densite.nom()), |tx| {
            let q = tx.quartiers();
            q.affectations
                .retain(|_, a| !polygone::contient(&contour, a.repere));
            let id = Id::nouveau();
            q.affectations.insert(
                id,
                Affectation {
                    repere,
                    famille,
                    densite,
                },
            );
            Ok(id)
        })
    }

    /// Ajoute une zone libre.
    pub fn ajouter_zone(
        &mut self,
        polygone_: Vec<P2>,
        famille: Famille,
        densite: Densite,
    ) -> Result<Id, String> {
        self.modifier("Zone libre", |tx| {
            let id = Id::nouveau();
            tx.quartiers().zones.insert(
                id,
                ZoneLibre {
                    polygone: polygone::oriente_positif(&polygone_),
                    famille,
                    densite,
                },
            );
            Ok(id)
        })
    }

    /// Redessine le contour d'une zone libre (sommets déplacés, ajoutés ou retirés). Les bâtiments déjà posés ne
    /// bougent pas : relotir la zone pour les refaire.
    pub fn remodeler_zone(&mut self, id: Id, polygone_: Vec<P2>) -> Result<(), String> {
        if polygone_.len() < 3 {
            return Err("une zone a au moins 3 sommets".into());
        }
        if !polygone::est_simple(&polygone_) {
            return Err("ses bords se croiseraient".into());
        }
        if !polygone_.iter().all(|p| self.manifeste.contient(*p)) {
            return Err("un sommet sortirait de la carte".into());
        }
        self.modifier("Modifier une zone libre", |tx| {
            let z = tx
                .quartiers()
                .zones
                .get_mut(&id)
                .ok_or_else(|| "zone introuvable".to_string())?;
            z.polygone = polygone::oriente_positif(&polygone_);
            Ok(())
        })
    }

    /// Surface constructible et réglages d'une affectation ou d'une zone libre.
    fn cible_lotissement(&mut self, id: Id) -> Option<(Vec<P2>, Famille, Densite, u64)> {
        let graine = self.manifeste.graine;
        if let Some(a) = self.quartiers.affectations.get(&id).cloned() {
            let d = self.derive();
            let i = ilots::ilot_en(&d.ilots, a.repere)?;
            return Some((i.interieur.clone()?, a.famille, a.densite, graine ^ i.cle));
        }
        let z = self.quartiers.zones.get(&id)?;
        Some((z.polygone.clone(), z.famille, z.densite, graine ^ id.0))
    }

    /// Ajoute un contour nommé (polygone simple, nom unique).
    pub fn ajouter_contour(
        &mut self,
        nom: &str,
        usage: UsageContour,
        polygone_: Vec<P2>,
    ) -> Result<Id, String> {
        self.modifier(&format!("Contour : {nom}"), |tx| {
            if tx.carte().contours.par_nom(nom).is_some() {
                return Err(format!("un contour s'appelle déjà « {nom} »"));
            }
            let id = Id::nouveau();
            tx.contours().contours.insert(
                id,
                Contour {
                    nom: nom.to_owned(),
                    usage,
                    polygone: polygone::oriente_positif(&polygone_),
                },
            );
            Ok(id)
        })
    }

    /// Creuse un plan d'eau relevé sur un calque (`releve::Eau`) : à l'intérieur, le relief descend sous `niveau`
    /// jusqu'à `profondeur` mètres, en pente douce sur `rive` mètres depuis le bord ; il n'est jamais relevé. Le
    /// niveau d'eau global de la carte devient `niveau`. Une seule étape d'historique. Renvoie le nombre
    /// d'échantillons abaissés.
    pub fn creuser_eau(
        &mut self,
        eau: &releve::Eau,
        niveau: f64,
        profondeur: f64,
        rive: f64,
    ) -> Result<usize, String> {
        self.fin_trait();
        let m = &self.manifeste;
        if !(niveau.is_finite() && (m.altitude_min_m..=m.altitude_max_m).contains(&niveau)) {
            return Err("niveau d'eau hors de la plage d'altitude de la carte".into());
        }
        if !(profondeur.is_finite() && (0.5..=500.0).contains(&profondeur)) {
            return Err("profondeur hors limites (0,5 à 500 m)".into());
        }
        if !(rive.is_finite() && (0.0..=2000.0).contains(&rive)) {
            return Err("largeur de rive hors limites (0 à 2 000 m)".into());
        }
        let fond = (niveau - profondeur).max(m.altitude_min_m);
        let bord = (niveau + 0.2).min(m.altitude_max_m);
        // Échantillons couvrant le plan d'eau, plus un rang autour (les échantillons secs servent de rivage).
        let (a, b) = eau.englobant();
        let r = &self.relief;
        let indice = |v: f64, o: f64| ((v - o) / r.resolution).floor();
        let borne = |v: f64, n: usize| v.clamp(0.0, n as f64) as usize;
        let zone = Zone {
            x0: borne(indice(a[0], r.origine[0]) - 1.0, r.nx),
            z0: borne(indice(a[1], r.origine[1]) - 1.0, r.nz),
            x1: borne(indice(b[0], r.origine[0]) + 3.0, r.nx),
            z1: borne(indice(b[1], r.origine[1]) + 3.0, r.nz),
        };
        if zone.vide() {
            return Err("le plan d'eau est hors de la carte".into());
        }
        let (w, h) = (zone.x1 - zone.x0, zone.z1 - zone.z0);
        let mut dedans = releve::Masque::vide(w, h);
        for iz in 0..h {
            for ix in 0..w {
                if eau.contient(r.position(zone.x0 + ix, zone.z0 + iz)) {
                    dedans.v[iz * w + ix] = true;
                }
            }
        }
        if dedans.compte() == 0 {
            return Err("aucun échantillon de relief dans ce plan d'eau (trop petit ?)".into());
        }
        // Distance au rivage (échantillon sec le plus proche), en mètres.
        let distance = releve::distance_a(&dedans, false);
        let pas = r.resolution / f64::from(releve::PAS_DROIT);
        let mut etape = Etape {
            libelle: "Relevé : plan d'eau".into(),
            ..Default::default()
        };
        if self.manifeste.niveau_eau_m != Some(niveau) {
            etape.manifeste = Some(self.manifeste.clone());
            self.manifeste.niveau_eau_m = Some(niveau);
        }
        for (bx, bz) in blocs_de(zone) {
            etape
                .relief
                .entry((bx, bz))
                .or_insert_with(|| self.relief.copier(self.zone_bloc(bx, bz)));
        }
        let mut abaisses = 0;
        for iz in 0..h {
            for ix in 0..w {
                let k = iz * w + ix;
                if !dedans.v[k] {
                    continue;
                }
                let d = f64::from(distance[k]) * pas;
                let t = if rive <= 0.0 { 1.0 } else { (d / rive).min(1.0) };
                let lisse = t * t * (3.0 - 2.0 * t);
                let cible = (bord + (fond - bord) * lisse) as f32;
                let i = self.relief.index(zone.x0 + ix, zone.z0 + iz);
                if self.relief.h[i] > cible {
                    self.relief.h[i] = cible;
                    abaisses += 1;
                }
            }
        }
        self.marquer(&etape);
        self.passe.push(etape);
        if self.passe.len() > HISTORIQUE_MAX {
            self.passe.remove(0);
        }
        self.futur.clear();
        Ok(abaisses)
    }

    /// Relief tiré d'un calque de référence en niveaux de gris (convention `docs/CONVENTION_CARTES_SOURCE.md`) :
    /// sous l'image, chaque échantillon prend l'altitude `bas + niveaux(gris) × (haut − bas)` (noir = `bas`, blanc =
    /// `haut` ; `niveaux` rééquilibre clair et foncé). Un calque **en couleurs** est refusé (c'est la carte couleurs, pas la
    /// carte de relief : la lire en gris inverserait montagnes et plaines).
    /// Un pixel transparent ne porte pas de donnée : le relief y reste tel quel. `lissage_m` adoucit les marches des
    /// 256 niveaux de gris (moyenne glissante, 0 = aucune). Une seule étape d'historique ; renvoie le nombre
    /// d'échantillons modifiés.
    pub fn relief_depuis_reference(
        &mut self,
        id: Id,
        bas: f64,
        haut: f64,
        lissage_m: f64,
        niveaux: &crate::relief_image::Niveaux,
    ) -> Result<usize, String> {
        use crate::relief_image::{LISSAGE_MAX_M, altitude_sous, lisser, part_coloree};
        self.fin_trait();
        let (min, max) = (self.manifeste.altitude_min_m, self.manifeste.altitude_max_m);
        let plage = min..=max;
        if !(plage.contains(&bas) && plage.contains(&haut) && bas < haut) {
            return Err(format!(
                "altitudes du noir et du blanc : dans la plage de la carte ({min} à {max} m), noir plus bas que blanc"
            ));
        }
        if !(lissage_m.is_finite() && (0.0..=LISSAGE_MAX_M).contains(&lissage_m)) {
            return Err(format!("lissage hors limites (0 à {LISSAGE_MAX_M} m)"));
        }
        niveaux.valider()?;
        let reference = self
            .references
            .references
            .get(&id)
            .ok_or("calque de référence introuvable")?
            .clone();
        let image = self
            .images_references
            .get(&id)
            .ok_or("image du calque absente")?
            .clone();
        let couleur = part_coloree(&image);
        if couleur > 0.05 {
            return Err(format!(
                "« {} » est en couleurs ({:.0} % des pixels) : choisissez le calque de relief en niveaux de gris",
                reference.nom,
                couleur * 100.0
            ));
        }
        // Échantillons sous l'emprise du calque.
        let coins = reference.coins();
        let (a, b) = coins
            .iter()
            .fold(([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]), |(a, b), c| {
                ([a[0].min(c[0]), a[1].min(c[1])], [b[0].max(c[0]), b[1].max(c[1])])
            });
        let r = &self.relief;
        let indice = |v: f64, o: f64| ((v - o) / r.resolution).floor();
        let borne = |v: f64, n: usize| v.clamp(0.0, n as f64) as usize;
        let zone = Zone {
            x0: borne(indice(a[0], r.origine[0]), r.nx),
            z0: borne(indice(a[1], r.origine[1]), r.nz),
            x1: borne(indice(b[0], r.origine[0]) + 2.0, r.nx),
            z1: borne(indice(b[1], r.origine[1]) + 2.0, r.nz),
        };
        if zone.vide() {
            return Err("le calque est hors de la carte".into());
        }
        let (w, h) = (zone.x1 - zone.x0, zone.z1 - zone.z0);
        let mut cible = vec![f32::NAN; w * h];
        for iz in 0..h {
            for ix in 0..w {
                let p = r.position(zone.x0 + ix, zone.z0 + iz);
                if let Some(alt) = altitude_sous(&reference, &image, p, bas, haut, niveaux) {
                    cible[iz * w + ix] = alt as f32;
                }
            }
        }
        let rayon = (lissage_m / r.resolution).round() as usize;
        lisser(&mut cible, w, h, rayon);
        let mut etape = Etape {
            libelle: "Relief depuis un calque".into(),
            ..Default::default()
        };
        for (bx, bz) in blocs_de(zone) {
            etape
                .relief
                .entry((bx, bz))
                .or_insert_with(|| self.relief.copier(self.zone_bloc(bx, bz)));
        }
        let mut modifies = 0;
        for iz in 0..h {
            for ix in 0..w {
                let v = cible[iz * w + ix];
                if v.is_nan() {
                    continue;
                }
                let v = v.clamp(min as f32, max as f32);
                let i = self.relief.index(zone.x0 + ix, zone.z0 + iz);
                if self.relief.h[i] != v {
                    self.relief.h[i] = v;
                    modifies += 1;
                }
            }
        }
        if modifies == 0 {
            return Err("aucun échantillon de relief modifié (calque transparent, hors de la carte, ou déjà appliqué)".into());
        }
        self.marquer(&etape);
        self.passe.push(etape);
        if self.passe.len() > HISTORIQUE_MAX {
            self.passe.remove(0);
        }
        self.futur.clear();
        Ok(modifies)
    }

    /// Un coup de pinceau sur un calque de relief en niveaux de gris (`centre` et `rayon_m` en mètres de carte). Les
    /// coups d'un même geste forment une seule étape d'historique (voir `fin_trait`). Un calque en couleurs est refusé.
    pub fn peindre_reference(
        &mut self,
        id: Id,
        mode: crate::relief_image::PinceauCalque,
        centre: P2,
        rayon_m: f64,
        force: f64,
        cible: f64,
    ) -> Result<(), String> {
        use crate::relief_image::{part_coloree, peindre};
        if !(rayon_m.is_finite() && (0.5..=5000.0).contains(&rayon_m))
            || !(0.0..=1.0).contains(&force)
            || !(0.0..=1.0).contains(&cible)
            || !(centre[0].is_finite() && centre[1].is_finite())
        {
            return Err("pinceau de calque : rayon (0,5 à 5 000 m), force et gris (0 à 1) invalides".into());
        }
        let reference = self
            .references
            .references
            .get(&id)
            .ok_or("calque de référence introuvable")?
            .clone();
        let avant = self
            .images_references
            .get(&id)
            .ok_or("image du calque absente")?
            .clone();
        let mut etape = self.trait_en_cours.take().unwrap_or_else(|| Etape {
            libelle: "Peindre un calque de relief".into(),
            ..Default::default()
        });
        if !etape.images.contains_key(&id) {
            // Début du geste sur ce calque : on vérifie une fois qu'il est bien en niveaux de gris.
            if part_coloree(&avant) > 0.05 {
                self.trait_en_cours = Some(etape);
                return Err(format!(
                    "« {} » est en couleurs : le pinceau ne retouche qu'une carte de relief en niveaux de gris",
                    reference.nom
                ));
            }
            etape.images.insert(id, avant);
        }
        let image = self
            .images_references
            .get_mut(&id)
            .ok_or("image du calque absente")?;
        let (l, h) = (image.largeur as usize, image.hauteur as usize);
        peindre(
            std::sync::Arc::make_mut(&mut image.rgba).as_mut_slice(),
            l,
            h,
            reference.vers_image(centre),
            rayon_m / reference.m_par_px,
            mode,
            force,
            cible,
        );
        self.revisions.references += 1;
        *self.versions_images.entry(id).or_default() += 1;
        self.trait_en_cours = Some(etape);
        Ok(())
    }

    /// Calques valides et images présentes, aux bonnes dimensions.
    fn valider_references(&self) -> Result<(), String> {
        self.references
            .valider(&self.manifeste)
            .map_err(|e| format!("calques de référence : {e}"))?;
        for (id, r) in &self.references.references {
            match self.images_references.get(id) {
                Some(i) if i.largeur == r.largeur_px && i.hauteur == r.hauteur_px => {}
                _ => return Err(format!("calque {id} : image absente ou de mauvaise taille")),
            }
        }
        Ok(())
    }

    /// Importe une image PNG (non fiable) comme calque de référence. Elle couvre d'abord la largeur de la carte, coin
    /// haut gauche sur celui de la carte, à moitié transparente ; on la cale ensuite (`modifier_reference`).
    pub fn importer_reference(&mut self, nom: &str, octets_png: &[u8]) -> Result<Id, String> {
        let image = references::decoder_png(octets_png)?;
        let (a, b) = self.manifeste.bornes();
        let id = Id::nouveau();
        let reference = Reference {
            nom: nom.trim().to_owned(),
            largeur_px: image.largeur,
            hauteur_px: image.hauteur,
            coin: a,
            m_par_px: ((b[0] - a[0]) / f64::from(image.largeur))
                .clamp(references::ECHELLE_M_PAR_PX.0, references::ECHELLE_M_PAR_PX.1),
            rotation_deg: 0.0,
            opacite: 0.5,
            visible: true,
        };
        self.images_references.insert(id, image);
        let r = self.modifier(&format!("Calque de référence : {}", nom.trim()), |tx| {
            tx.references().references.insert(id, reference);
            Ok(id)
        });
        if r.is_err() {
            self.images_references.remove(&id);
        }
        r
    }

    /// Modifie le placement ou l'affichage d'un calque (une étape d'historique).
    pub fn modifier_reference(
        &mut self,
        id: Id,
        libelle: &str,
        f: impl FnOnce(&mut Reference) -> Result<(), String>,
    ) -> Result<(), String> {
        self.modifier(libelle, |tx| {
            let r = tx
                .references()
                .references
                .get_mut(&id)
                .ok_or("calque de référence introuvable")?;
            f(r)
        })
    }

    /// Retire un calque (annulable : l'image reste en mémoire).
    pub fn retirer_reference(&mut self, id: Id) -> Result<(), String> {
        self.modifier("Retirer un calque de référence", |tx| {
            tx.references()
                .references
                .remove(&id)
                .map(|_| ())
                .ok_or_else(|| "calque de référence introuvable".to_owned())
        })
    }

    /// Fait (re)pousser les bâtiments d'un quartier. Les bâtiments manuels ou verrouillés ne sont jamais touchés.
    pub fn lotir(&mut self, id: Id) -> Result<usize, String> {
        let Some((surface, famille, densite, graine)) = self.cible_lotissement(id) else {
            return Err("quartier introuvable ou îlot trop petit".into());
        };
        if !famille.constructible() {
            return Err(format!("{} : rien ne pousse ici", famille.nom()));
        }
        let Some(gabarit) = self.manifeste.gabarits.get(famille, densite).copied() else {
            return Err("gabarit manquant".into());
        };
        let mut obstacles = self.derive().geo.polygones_emprise();
        let a_remplacer: Vec<Id> = self
            .batiments
            .iter()
            .filter(|(_, b)| b.affectation == Some(id) && b.origine == Origine::Genere && !b.verrou)
            .map(|(i, _)| *i)
            .collect();
        obstacles.extend(
            self.batiments
                .iter()
                .filter(|(i, _)| !a_remplacer.contains(i))
                .map(|(_, b)| b.empreinte.clone()),
        );
        let blocs = lotissement::lotir(&surface, &gabarit, graine, &obstacles);
        let n = blocs.len();
        self.modifier(&format!("Lotir : {} {}", famille.nom(), densite.nom()), |tx| {
            for i in &a_remplacer {
                tx.retirer_batiment(*i);
            }
            for (empreinte, niveaux) in blocs {
                tx.poser_batiment(
                    Id::nouveau(),
                    Batiment {
                        empreinte,
                        niveaux,
                        hauteur_niveau_m: gabarit.hauteur_niveau_m,
                        famille,
                        origine: Origine::Genere,
                        verrou: false,
                        affectation: Some(id),
                        modele: None,
                        decalage_y_m: 0.0,
                    },
                );
            }
            Ok(n)
        })
    }

    /// Lotit tous les quartiers constructibles.
    pub fn lotir_tout(&mut self) -> Result<usize, String> {
        let ids: Vec<Id> = self
            .quartiers
            .affectations
            .iter()
            .filter(|(_, a)| a.famille.constructible())
            .map(|(i, _)| *i)
            .chain(
                self.quartiers
                    .zones
                    .iter()
                    .filter(|(_, z)| z.famille.constructible())
                    .map(|(i, _)| *i),
            )
            .collect();
        let mut total = 0;
        for id in ids {
            total += self.lotir(id).unwrap_or(0);
        }
        Ok(total)
    }

    /// Déplace des bâtiments (ils deviennent manuels). Les verrouillés ne bougent pas.
    pub fn deplacer_batiments(&mut self, ids: &[Id], delta: P2) -> Result<usize, String> {
        self.transformer_batiments(ids, &Transformation::deplacement(delta))
    }

    /// Applique une transformation (gizmo, inspecteur) à des bâtiments, en une étape d'historique. Ils deviennent
    /// manuels ; les verrouillés ne bougent pas ; une inclinaison seule ne touche que les bâtiments modélisés (un
    /// bloc ne s'incline pas). Refusé en entier si un bâtiment sortirait de la carte. Rend le nombre de bâtiments
    /// modifiés.
    pub fn transformer_batiments(&mut self, ids: &[Id], t: &Transformation) -> Result<usize, String> {
        let libelle = if t.cap_deg != 0.0 {
            "Tourner des bâtiments"
        } else if t.tangage_deg != 0.0 || t.roulis_deg != 0.0 {
            "Incliner des bâtiments"
        } else if t.dy_m != 0.0 && t.delta == [0.0, 0.0] {
            "Monter / descendre des bâtiments"
        } else {
            "Déplacer des bâtiments"
        };
        self.modifier(libelle, |tx| {
            let mut n = 0;
            for id in ids {
                if let Some(b) = tx.batiment(*id)
                    && !b.verrou
                    && !(t.inclinaison_seule() && b.modele.is_none())
                {
                    b.transformer(t);
                    b.origine = Origine::Manuel;
                    n += 1;
                }
            }
            Ok(n)
        })
    }

    /// Pose un objet de décor de la bibliothèque. Renvoie son identifiant.
    pub fn poser_objet(&mut self, modele: &str, position: P2, cap_deg: f64) -> Result<Id, String> {
        self.modifier("Poser un objet", |tx| {
            let id = Id::nouveau();
            tx.poser_objet(id, ObjetDecor::nouveau(modele, position, cap_deg));
            Ok(id)
        })
    }

    /// Transforme des objets de décor (les verrouillés ne bougent pas). Renvoie le nombre d'objets transformés.
    pub fn transformer_objets(&mut self, ids: &[Id], t: &Transformation) -> Result<usize, String> {
        let libelle = if t.cap_deg != 0.0 {
            "Tourner des objets"
        } else if t.dy_m != 0.0 && t.delta == [0.0, 0.0] {
            "Monter / descendre des objets"
        } else {
            "Déplacer des objets"
        };
        self.modifier(libelle, |tx| {
            let mut n = 0;
            for id in ids {
                if let Some(o) = tx.objet(*id)
                    && !o.verrou
                {
                    o.transformer(t);
                    n += 1;
                }
            }
            Ok(n)
        })
    }

    /// Règle l'échelle d'objets de décor (bornée).
    pub fn mettre_a_l_echelle(&mut self, ids: &[Id], echelle: f64) -> Result<usize, String> {
        let e = echelle.clamp(crate::decor::ECHELLE.0, crate::decor::ECHELLE.1);
        self.modifier("Échelle d'objets", |tx| {
            let mut n = 0;
            for id in ids {
                if let Some(o) = tx.objet(*id)
                    && !o.verrou
                {
                    o.echelle = e;
                    n += 1;
                }
            }
            Ok(n)
        })
    }

    /// Supprime des objets de décor (pas les verrouillés). Renvoie le nombre d'objets supprimés.
    pub fn supprimer_objets(&mut self, ids: &[Id]) -> Result<usize, String> {
        self.modifier("Supprimer des objets", |tx| {
            let mut n = 0;
            for id in ids {
                if tx.carte().decor.get(id).is_some_and(|o| !o.verrou) {
                    tx.retirer_objet(*id);
                    n += 1;
                }
            }
            Ok(n)
        })
    }

    /// Verrouille ou déverrouille des objets de décor.
    pub fn verrouiller_objets(&mut self, ids: &[Id], verrou: bool) -> Result<usize, String> {
        self.modifier(if verrou { "Verrouiller" } else { "Déverrouiller" }, |tx| {
            let mut n = 0;
            for id in ids {
                if let Some(o) = tx.objet(*id) {
                    o.verrou = verrou;
                    n += 1;
                }
            }
            Ok(n)
        })
    }

    /// Pose un bâtiment modélisé de la fiche `fiche`, centré sur `centre`, orienté de `cap_deg`. Il est manuel.
    pub fn poser_modele(
        &mut self,
        fiche: &crate::catalogue::Fiche,
        centre: P2,
        cap_deg: f64,
    ) -> Result<Id, String> {
        let b = fiche.batiment(centre, cap_deg);
        let id = Id::nouveau();
        self.modifier(&format!("Poser « {} »", fiche.nom), |tx| {
            tx.poser_batiment(id, b);
            Ok(id)
        })
    }

    /// Bâtiments qui empiètent sur une route ou un carrefour.
    pub fn conflits(&mut self) -> Vec<Id> {
        let emprises = self.derive().geo.polygones_emprise();
        let index = crate::grille::Grille::de_polygones(&emprises, 64.0);
        self.batiments
            .iter()
            .filter(|(_, b)| {
                let r = polygone::reduit(&b.empreinte, 0.05);
                index
                    .proches(&crate::grille::boite(&r))
                    .into_iter()
                    .any(|i| polygone::se_chevauchent(&r, &emprises[i as usize]))
            })
            .map(|(i, _)| *i)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quartier() -> Carte {
        let mut c = Carte::nouvelle(Manifeste::nouveau("t", 1024.0)).expect("carte");
        for k in [-100.0, 0.0, 100.0] {
            c.tracer_route(ClasseRoute::Rue, &[[-100.0, k], [100.0, k]], 2.0)
                .expect("h");
            c.tracer_route(ClasseRoute::Rue, &[[k, -100.0], [k, 100.0]], 2.0)
                .expect("v");
        }
        c
    }

    /// Calque synthétique de 256 × 256 pixels à 2 m : mer à gauche (x < 64), ville rose, route jaune horizontale.
    fn carte_avec_calque() -> (Carte, Id) {
        let mut c = Carte::nouvelle(Manifeste::nouveau("t", 512.0)).expect("carte");
        let n = 256u32;
        let mut rgba = Vec::new();
        for y in 0..n {
            for x in 0..n {
                let p: [u8; 3] = if x < 64 {
                    [60, 130, 200]
                } else if (150..220).contains(&x) && (30..100).contains(&y) {
                    [235, 150, 140]
                } else if (160..=163).contains(&y) {
                    [240, 200, 40]
                } else {
                    [150, 200, 110]
                };
                rgba.extend_from_slice(&[p[0], p[1], p[2], 255]);
            }
        }
        let png = references::encoder_png(&ImageRgba {
            largeur: n,
            hauteur: n,
            rgba: std::sync::Arc::new(rgba),
        })
        .expect("png");
        let id = c.importer_reference("plan", &png).expect("import");
        (c, id)
    }

    fn analyser(c: &Carte, id: Id, sorte: releve::Sorte, px: f64, py: f64) -> Vec<releve::Proposition> {
        let r = &c.references.references[&id];
        let g = releve::Grille::preparer(&c.images_references[&id], r, &c.manifeste).expect("grille");
        let mut reglages = sorte.reglages();
        reglages.aire_min_m2 = 100.0;
        g.analyser(sorte, r.vers_carte(px, py), &reglages)
            .expect("analyse")
    }

    #[test]
    fn releve_eau_creuse_le_relief_et_s_annule() {
        let (mut c, id) = carte_avec_calque();
        assert_eq!(c.references.references[&id].m_par_px, 2.0);
        let props = analyser(&c, id, releve::Sorte::Eau, 10.0, 10.0);
        let releve::Proposition::Eau(eau) = &props[0] else {
            panic!("{props:?}")
        };
        let (dans, loin_dans, sec) = ([-200.0, 0.0], [-250.0, 0.0], [0.0, 0.0]);
        let n = c.creuser_eau(eau, -1.0, 6.0, 20.0).expect("creuser");
        assert!(n > 0);
        assert_eq!(c.manifeste.niveau_eau_m, Some(-1.0));
        assert!(c.relief.hauteur(dans) < -1.0);
        assert!(
            (c.relief.hauteur(loin_dans) + 7.0).abs() < 0.01,
            "{}",
            c.relief.hauteur(loin_dans)
        );
        assert_eq!(c.relief.hauteur(sec), 0.0, "la terre n'est pas touchée");
        assert_eq!(c.annuler().as_deref(), Some("Relevé : plan d'eau"));
        assert_eq!(c.manifeste.niveau_eau_m, None);
        assert_eq!(c.relief.hauteur(dans), 0.0);
        assert!(c.creuser_eau(eau, 1e6, 6.0, 20.0).is_err());
    }

    #[test]
    fn relief_depuis_un_calque_en_niveaux_de_gris() {
        // Carte de 512 m (−64 à 448 m) ; image 4 × 1 étirée sur toute la largeur : noir, gris, blanc, transparent.
        let mut c = Carte::nouvelle(Manifeste::nouveau("relief", 512.0)).expect("carte");
        let px = |g: u8, a: u8| [g, g, g, a];
        let image = ImageRgba {
            largeur: 4,
            hauteur: 1,
            rgba: std::sync::Arc::new(
                [px(0, 255), px(128, 255), px(255, 255), px(0, 0)]
                    .into_iter()
                    .flatten()
                    .collect(),
            ),
        };
        let id = c
            .importer_reference("relief", &references::encoder_png(&image).expect("png"))
            .expect("import");
        // 128 m par pixel : centres des pixels à x = −192, −64, 64 et 192 m, image haute de 128 m (z de −256 à −128).
        let (noir, gris, blanc, trou) = ([-192.0, -192.0], [-64.0, -192.0], [64.0, -192.0], [192.0, -192.0]);
        let n = c
            .relief_depuis_reference(id, 0.0, 400.0, 0.0, &Default::default())
            .expect("relief");
        assert!(n > 0);
        assert_eq!(c.relief.hauteur(noir), 0.0);
        assert!((c.relief.hauteur(gris) - 400.0 * 128.0 / 255.0).abs() < 0.01);
        assert_eq!(c.relief.hauteur(blanc), 400.0);
        assert_eq!(c.relief.hauteur(trou), 0.0, "pixel transparent : relief intact");
        assert_eq!(
            c.relief.hauteur([0.0, 100.0]),
            0.0,
            "hors de l'image : relief intact"
        );
        assert_eq!(c.annuler().as_deref(), Some("Relief depuis un calque"));
        assert_eq!(c.relief.hauteur(blanc), 0.0);
        // Bornes : hors de la plage de la carte, ou noir au-dessus du blanc.
        assert!(
            c.relief_depuis_reference(id, 0.0, 1000.0, 0.0, &Default::default())
                .is_err()
        );
        assert!(
            c.relief_depuis_reference(id, 300.0, 100.0, 0.0, &Default::default())
                .is_err()
        );
        assert!(
            c.relief_depuis_reference(id, 0.0, 400.0, f64::NAN, &Default::default())
                .is_err()
        );
    }

    #[test]
    fn peindre_un_calque_de_relief_s_annule_d_un_geste() {
        use crate::relief_image::PinceauCalque;
        let mut c = Carte::nouvelle(Manifeste::nouveau("peinture", 512.0)).expect("carte");
        let gris = ImageRgba {
            largeur: 8,
            hauteur: 8,
            rgba: std::sync::Arc::new(vec![100; 8 * 8 * 4]),
        };
        let id = c
            .importer_reference("relief", &references::encoder_png(&gris).expect("png"))
            .expect("import");
        let avant = c.images_references[&id].clone();
        // 64 m par pixel ; deux coups du même geste au centre de la carte.
        for _ in 0..2 {
            c.peindre_reference(id, PinceauCalque::Eclaircir, [0.0, 0.0], 100.0, 0.5, 0.0)
                .expect("coup");
        }
        c.fin_trait();
        assert_ne!(c.images_references[&id], avant);
        assert!(c.sale.images.contains(&id), "image à réécrire à l'enregistrement");
        assert_eq!(c.annuler().as_deref(), Some("Peindre un calque de relief"));
        assert_eq!(
            c.images_references[&id], avant,
            "un seul geste, une seule annulation"
        );
        assert_eq!(c.refaire().as_deref(), Some("Peindre un calque de relief"));
        assert_ne!(c.images_references[&id], avant);
        // Un calque en couleurs est refusé.
        let couleurs = ImageRgba {
            largeur: 2,
            hauteur: 1,
            rgba: std::sync::Arc::new(vec![184, 217, 155, 255, 64, 150, 220, 255]),
        };
        let id2 = c
            .importer_reference("couleurs", &references::encoder_png(&couleurs).expect("png"))
            .expect("import");
        assert!(
            c.peindre_reference(id2, PinceauCalque::Eclaircir, [0.0, 0.0], 100.0, 0.5, 0.0)
                .is_err()
        );
        assert!(
            c.peindre_reference(id, PinceauCalque::Eclaircir, [0.0, 0.0], f64::NAN, 0.5, 0.0)
                .is_err()
        );
    }

    #[test]
    fn releve_zone_et_route_acceptees_par_la_carte() {
        let (mut c, id) = carte_avec_calque();
        let zones = analyser(&c, id, releve::Sorte::Zone, 180.0, 60.0);
        let releve::Proposition::Zone { polygone, .. } = &zones[0] else {
            panic!("{zones:?}")
        };
        c.ajouter_zone(
            releve::borner(polygone, &c.manifeste),
            Famille::Residentiel,
            Densite::Moyenne,
        )
        .expect("zone acceptée");
        let routes = analyser(&c, id, releve::Sorte::Routes, 128.0, 161.5);
        assert_eq!(routes.len(), 1, "{routes:?}");
        let releve::Proposition::Route { trace, .. } = &routes[0] else {
            panic!()
        };
        let ids = c
            .tracer_route(ClasseRoute::Route, &releve::borner(trace, &c.manifeste), 4.0)
            .expect("route acceptée");
        assert!(!ids.is_empty());
    }

    /// Les dérivés repris après une retouche (réseau, relief, annulation) sont exactement ceux d'un calcul complet,
    /// et seule une partie du terrain est refaite.
    #[test]
    fn derives_incrementaux_identiques() {
        let mut c = quartier();
        c.pinceau(Pinceau::Naturel, [0.0, 0.0], 500.0, 25.0, 0.0);
        c.pinceau(Pinceau::Lever, [60.0, 40.0], 90.0, 18.0, 0.0);
        c.fin_trait();
        c.derive();
        let total = {
            let t = &c.derive().terrain;
            t.nx.div_ceil(relief::BLOC_TERRAIN) * t.nz.div_ceil(relief::BLOC_TERRAIN)
        };
        let verifier = |c: &mut Carte, quoi: &str, partiel_attendu: bool| {
            let partiel = c.derive().clone();
            let complet = c.derive_complet();
            assert_eq!(partiel.geo, complet.geo, "{quoi} : géométrie");
            assert_eq!(partiel.altitudes, complet.altitudes, "{quoi} : altitudes");
            assert_eq!(partiel.ilots, complet.ilots, "{quoi} : îlots");
            let ecart = partiel
                .terrain
                .h
                .iter()
                .zip(&complet.terrain.h)
                .filter(|(a, b)| a.to_bits() != b.to_bits())
                .count();
            assert_eq!(
                ecart, 0,
                "{quoi} : {ecart} échantillon(s) de terrain différent(s)"
            );
            if partiel_attendu {
                let n = partiel.maj_terrain.blocs.as_ref().map(|b| b.len());
                assert!(
                    n.is_some_and(|n| n > 0 && n < total),
                    "{quoi} : {n:?} bloc(s) refait(s) sur {total}"
                );
            }
        };
        let (id, p) = c
            .reseau
            .noeuds
            .iter()
            .map(|(i, n)| (*i, n.position))
            .next()
            .expect("nœud");
        c.modifier("déplacer", |tx| {
            let (r, m) = tx.reseau_et_manifeste();
            r.deplacer_noeud(id, [p[0] + 7.0, p[1] - 3.0], m);
            Ok(())
        })
        .expect("déplacement");
        verifier(&mut c, "nœud déplacé", true);
        c.pinceau(Pinceau::Lever, [0.0, 0.0], 30.0, 6.0, 0.0);
        verifier(&mut c, "coup de pinceau sur un carrefour", true);
        c.pinceau(Pinceau::Abaisser, [300.0, 300.0], 20.0, 4.0, 0.0);
        verifier(&mut c, "coup de pinceau loin des routes", true);
        c.fin_trait();
        verifier(&mut c, "fin du trait", false);
        c.annuler();
        verifier(&mut c, "trait annulé", true);
        c.annuler();
        verifier(&mut c, "déplacement annulé", true);
        c.refaire();
        verifier(&mut c, "déplacement refait", true);
        c.tracer_route(ClasseRoute::Route, &[[-300.0, 250.0], [250.0, 180.0]], 2.0)
            .expect("route");
        verifier(&mut c, "route tracée", true);
        c.annuler();
        verifier(&mut c, "route annulée", true);
    }

    #[test]
    fn annuler_refaire_le_reseau() {
        let mut c = quartier();
        let n = c.reseau.troncons.len();
        assert_eq!(c.annuler().as_deref(), Some("Tracer : Rue"));
        assert!(c.reseau.troncons.len() < n);
        c.refaire();
        assert_eq!(c.reseau.troncons.len(), n);
    }

    #[test]
    fn transaction_invalide_annulee() {
        let mut c = quartier();
        let avant = c.reseau.clone();
        let r = c.modifier("piège", |tx| {
            if let Some(n) = tx.reseau().noeuds.values_mut().next() {
                n.position = [1e9, 0.0];
            }
            Ok(())
        });
        assert!(r.is_err());
        assert_eq!(c.reseau, avant);
    }

    #[test]
    fn zone_libre_remodelee_annulable_et_bords_croises_refuses() {
        let mut c = quartier();
        let carre = vec![[10.0, 10.0], [60.0, 10.0], [60.0, 60.0], [10.0, 60.0]];
        let id = c
            .ajouter_zone(carre.clone(), Famille::Residentiel, Densite::Moyenne)
            .expect("zone");
        let mut plus_grand = carre.clone();
        plus_grand[2] = [80.0, 80.0];
        plus_grand.insert(1, [35.0, 0.0]);
        c.remodeler_zone(id, plus_grand).expect("remodeler");
        assert_eq!(c.quartiers.zones[&id].polygone.len(), 5);
        // Bords croisés, trop peu de sommets, hors carte : refusés, la zone ne change pas.
        let croise = vec![[10.0, 10.0], [60.0, 60.0], [60.0, 10.0], [10.0, 60.0]];
        assert!(c.remodeler_zone(id, croise).is_err());
        assert!(c.remodeler_zone(id, carre[..2].to_vec()).is_err());
        let mut dehors = carre.clone();
        dehors[0] = [-1e6, 0.0];
        assert!(c.remodeler_zone(id, dehors).is_err());
        assert_eq!(c.quartiers.zones[&id].polygone.len(), 5);
        c.annuler();
        assert_eq!(c.quartiers.zones[&id].polygone.len(), 4);
    }

    #[test]
    fn quartier_lotissement_et_travail_manuel() {
        let mut c = quartier();
        let a = c
            .affecter([50.0, 50.0], Famille::Residentiel, Densite::Moyenne)
            .expect("affecter");
        let n = c.lotir(a).expect("lotir");
        assert!(n >= 4);
        assert!(c.conflits().is_empty(), "un bâtiment généré empiète sur une rue");
        // Un bâtiment déplacé devient manuel et survit à une régénération.
        let un = *c.batiments.keys().next().expect("un bâtiment");
        c.deplacer_batiments(&[un], [0.5, 0.0]).expect("déplacer");
        c.lotir(a).expect("relotir");
        assert_eq!(c.batiments[&un].origine, Origine::Manuel);
        // Annuler la régénération remet l'état précédent.
        c.annuler();
        assert!(c.batiments.contains_key(&un));
    }

    #[test]
    fn trait_de_pinceau_une_seule_annulation() {
        let mut c = quartier();
        for k in 0..10 {
            c.pinceau(Pinceau::Lever, [k as f64 * 5.0, 0.0], 20.0, 1.0, 0.0);
        }
        c.fin_trait();
        assert!(c.relief.hauteur([20.0, 0.0]) > 1.0);
        c.annuler();
        assert_eq!(c.relief.hauteur([20.0, 0.0]), 0.0);
        assert!(!c.sale.relief.is_empty());
    }

    #[test]
    fn la_route_suit_le_relief() {
        let mut c = quartier();
        c.pinceau(Pinceau::Lever, [0.0, 50.0], 80.0, 10.0, 0.0);
        c.fin_trait();
        let d = c.derive();
        // Le terrain final est plat en travers de la rue verticale x = 0.
        let (a, b) = (d.terrain.hauteur([-5.0, 50.0]), d.terrain.hauteur([5.0, 50.0]));
        assert!((a - b).abs() < 0.05);
    }
}
