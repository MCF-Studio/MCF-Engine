//! Le document de carte : sources, historique (annuler / refaire illimité dans une limite mémoire) et dérivés
//! recalculés à la demande. Toute modification passe par une **transaction** : elle est validée en entier ou
//! annulée en entier (invariant 5 de `ARCHITECTURE_DONNEES_CARTE.md`).

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use mcf_geo::{ClasseRoute, P2, polygone};

use crate::batiments::{Batiment, Origine};
use crate::gabarits::{Densite, Famille};
use crate::geometrie::{self, GeoReseau};
use crate::ids::Id;
use crate::ilots::{self, Ilot};
use crate::lotissement;
use crate::manifeste::Manifeste;
use crate::quartiers::{Affectation, Quartiers, ZoneLibre};
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
    batiments: BTreeMap<Id, Option<Batiment>>,
    relief: BTreeMap<(usize, usize), Vec<f32>>,
    calques: BTreeMap<String, Option<mcf_map::McfMap>>,
}

impl Etape {
    fn vide(&self) -> bool {
        self.manifeste.is_none()
            && self.reseau.is_none()
            && self.quartiers.is_none()
            && self.batiments.is_empty()
            && self.relief.is_empty()
            && self.calques.is_empty()
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
    /// Cellules de bâti.
    pub batiments: bool,
    /// Tuiles de relief `(x, z)`.
    pub relief: BTreeSet<(usize, usize)>,
    /// Calques d'objets.
    pub calques: BTreeSet<String>,
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
    /// Bâti.
    pub batiments: u64,
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
    /// Bâti.
    pub batiments: BTreeMap<Id, Batiment>,
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

    fn noter_batiment(&mut self, id: Id) {
        self.etape.batiments.entry(id).or_insert_with(|| self.carte.batiments.get(&id).cloned());
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

    /// Calque d'objets modifiable.
    pub fn calque(&mut self, nom: &str) -> Option<&mut mcf_map::McfMap> {
        self.etape.calques.entry(nom.to_owned()).or_insert_with(|| self.carte.calques.get(nom).cloned());
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
        let relief = Relief::plat(&manifeste, 0.0);
        let (tx, tz) = manifeste.tuiles();
        Ok(Self {
            dossier: None,
            relief,
            reseau: Reseau::default(),
            quartiers: Quartiers::default(),
            batiments: BTreeMap::new(),
            calques: BTreeMap::new(),
            sale: Sale {
                manifeste: true,
                reseau: true,
                quartiers: true,
                batiments: true,
                relief: (0..tz).flat_map(|z| (0..tx).map(move |x| (x, z))).collect(),
                calques: BTreeSet::new(),
            },
            revisions: Revisions::default(),
            manifeste,
            passe: Vec::new(),
            futur: Vec::new(),
            trait_en_cours: None,
            derive: None,
        })
    }

    pub(crate) fn assembler(
        dossier: PathBuf,
        manifeste: Manifeste,
        reseau: Reseau,
        quartiers: Quartiers,
        batiments: BTreeMap<Id, Batiment>,
        relief: Relief,
        calques: BTreeMap<String, mcf_map::McfMap>,
    ) -> Self {
        Self {
            dossier: Some(dossier),
            manifeste,
            reseau,
            quartiers,
            batiments,
            relief,
            calques,
            sale: Sale::default(),
            revisions: Revisions::default(),
            passe: Vec::new(),
            futur: Vec::new(),
            trait_en_cours: None,
            derive: None,
        }
    }

    /// Vérifie toutes les sources.
    pub fn valider(&self) -> Result<(), String> {
        self.manifeste.valider().map_err(|e| format!("manifeste : {e}"))?;
        self.reseau.valider(&self.manifeste).map_err(|e| format!("réseau : {e}"))?;
        self.quartiers.valider(&self.manifeste).map_err(|e| format!("quartiers : {e}"))?;
        if self.batiments.len() > crate::batiments::MAX_BATIMENTS {
            return Err("trop de bâtiments".into());
        }
        for (id, b) in &self.batiments {
            b.valider(&self.manifeste).map_err(|e| format!("bâtiment {id} : {e}"))?;
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
                c.reseau.valider(&c.manifeste).map_err(|m| format!("réseau : {m}"))?;
            }
            if e.quartiers.is_some() {
                c.quartiers.valider(&c.manifeste).map_err(|m| format!("quartiers : {m}"))?;
            }
            for id in e.batiments.keys() {
                if let Some(b) = c.batiments.get(id) {
                    b.valider(&c.manifeste).map_err(|m| format!("bâtiment : {m}"))?;
                }
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
        if !e.batiments.is_empty() {
            self.sale.batiments = true;
            self.revisions.batiments += 1;
        }
        if !e.relief.is_empty() {
            self.revisions.relief += 1;
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
        for (id, avant) in e.batiments {
            let courant = match avant {
                Some(b) => self.batiments.insert(id, b),
                None => self.batiments.remove(&id),
            };
            inverse.batiments.insert(id, courant);
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
            etape.relief.entry((bx, bz)).or_insert_with(|| self.relief.copier(self.zone_bloc(bx, bz)));
        }
        let m = &self.manifeste;
        let graine = m.graine;
        let bornes = (m.altitude_min_m, m.altitude_max_m);
        self.relief.pinceau(outil, centre, rayon, force, cible, graine, bornes);
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

    /// Dérivés à jour (recalcule ce qui a changé).
    pub fn derive(&mut self) -> &Derive {
        let r = self.revisions;
        let a_jour = self.derive.as_ref().is_some_and(|d| {
            d.revisions.reseau == r.reseau && d.revisions.relief == r.relief && d.revisions.manifeste == r.manifeste
        });
        if !a_jour {
            let profils = &self.manifeste.profils;
            let geo = geometrie::calculer(&self.reseau, profils);
            let altitudes = relief::altitudes_reseau(&self.reseau, &geo, &self.relief, profils);
            let terrain = relief::terrain_final(&self.relief, &geo, &altitudes, profils);
            let ilots = ilots::calculer(&self.reseau, &geo, profils);
            self.derive = Some(Derive {
                geo,
                altitudes,
                ilots,
                terrain,
                revisions: r,
            });
        }
        let d = self.derive.get_or_insert_with(Derive::default);
        d.revisions = r;
        d
    }

    // ---------------------------------------------------------------- commandes

    /// Trace une route.
    pub fn tracer_route(&mut self, classe: ClasseRoute, chemin: &[P2], accroche: f64) -> Result<Vec<Id>, String> {
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
            q.affectations.retain(|_, a| !polygone::contient(&contour, a.repere));
            let id = Id::nouveau();
            q.affectations.insert(id, Affectation { repere, famille, densite });
            Ok(id)
        })
    }

    /// Ajoute une zone libre.
    pub fn ajouter_zone(&mut self, polygone_: Vec<P2>, famille: Famille, densite: Densite) -> Result<Id, String> {
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
            .chain(self.quartiers.zones.iter().filter(|(_, z)| z.famille.constructible()).map(|(i, _)| *i))
            .collect();
        let mut total = 0;
        for id in ids {
            total += self.lotir(id).unwrap_or(0);
        }
        Ok(total)
    }

    /// Déplace des bâtiments (ils deviennent manuels). Les verrouillés ne bougent pas.
    pub fn deplacer_batiments(&mut self, ids: &[Id], delta: P2) -> Result<usize, String> {
        self.modifier("Déplacer des bâtiments", |tx| {
            let mut n = 0;
            for id in ids {
                if let Some(b) = tx.batiment(*id)
                    && !b.verrou
                {
                    for p in &mut b.empreinte {
                        p[0] += delta[0];
                        p[1] += delta[1];
                    }
                    b.origine = Origine::Manuel;
                    n += 1;
                }
            }
            Ok(n)
        })
    }

    /// Bâtiments qui empiètent sur une route ou un carrefour.
    pub fn conflits(&mut self) -> Vec<Id> {
        let emprises = self.derive().geo.polygones_emprise();
        self.batiments
            .iter()
            .filter(|(_, b)| {
                let r = polygone::reduit(&b.empreinte, 0.05);
                emprises.iter().any(|e| polygone::se_chevauchent(&r, e))
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
            c.tracer_route(ClasseRoute::Rue, &[[-100.0, k], [100.0, k]], 2.0).expect("h");
            c.tracer_route(ClasseRoute::Rue, &[[k, -100.0], [k, 100.0]], 2.0).expect("v");
        }
        c
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
    fn quartier_lotissement_et_travail_manuel() {
        let mut c = quartier();
        let a = c.affecter([50.0, 50.0], Famille::Residentiel, Densite::Moyenne).expect("affecter");
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
