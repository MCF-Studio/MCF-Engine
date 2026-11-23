//! Tests de la carte de test de référence (`mcf_carte::carte_de_test`) : aller-retour fichiers, exactitude 2D = 3D
//! (spec §2.5), graphe routier, identifiants jamais réutilisés, fichiers piégés refusés.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use mcf_carte::carte_de_test::{self, AVENUE, ILOTS, IMPASSE, RUE_EN_L};
use mcf_carte::{Batiment, Carte, GrapheRoutier, Id, Origine, Pinceau};
use mcf_geo::{ClasseRoute, P2, polygone};

/// Tolérance d'aller-retour et d'exactitude (m).
const MM: f64 = 0.001;

/// Dossier temporaire neuf, supprimé à la fin du test (même en cas d'échec).
struct Temp(PathBuf);

impl Temp {
    fn new(nom: &str) -> Self {
        let d = std::env::temp_dir().join(format!(
            "mcf_carte_dt_{nom}_{}_{}",
            std::process::id(),
            Id::nouveau()
        ));
        let _ = fs::remove_dir_all(&d);
        Self(d)
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn carte() -> Carte {
    carte_de_test::construire().expect("carte de test")
}

fn proche(a: P2, b: P2) -> bool {
    (a[0] - b[0]).abs() <= MM && (a[1] - b[1]).abs() <= MM
}

fn polylignes_proches(a: &[P2], b: &[P2]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(p, q)| proche(*p, *q))
}

/// Tous les fichiers d'un dossier (chemin relatif → contenu), récursivement, ordre stable.
fn fichiers(racine: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut v = BTreeMap::new();
    let mut pile = vec![racine.to_path_buf()];
    while let Some(d) = pile.pop() {
        for e in fs::read_dir(&d).expect("lire dossier") {
            let p = e.expect("entrée").path();
            if p.is_dir() {
                pile.push(p);
            } else {
                let rel = p
                    .strip_prefix(racine)
                    .expect("relatif")
                    .to_string_lossy()
                    .replace('\\', "/");
                v.insert(rel, fs::read(&p).expect("lire fichier"));
            }
        }
    }
    v
}

/// Compare deux cartes à ±1 mm, identifiants exacts.
fn assert_cartes_identiques(a: &Carte, b: &Carte) {
    assert_eq!(a.manifeste, b.manifeste, "manifeste");
    // Réseau.
    assert_eq!(
        a.reseau.noeuds.keys().collect::<Vec<_>>(),
        b.reseau.noeuds.keys().collect::<Vec<_>>()
    );
    for (id, n) in &a.reseau.noeuds {
        let m = &b.reseau.noeuds[id];
        assert!(proche(n.position, m.position), "nœud {id}");
        assert_eq!(n.rond_point_m, m.rond_point_m);
    }
    assert_eq!(
        a.reseau.troncons.keys().collect::<Vec<_>>(),
        b.reseau.troncons.keys().collect::<Vec<_>>()
    );
    for (id, t) in &a.reseau.troncons {
        let u = &b.reseau.troncons[id];
        assert_eq!(
            (t.classe, t.de, t.vers, &t.nom),
            (u.classe, u.de, u.vers, &u.nom),
            "tronçon {id}"
        );
        assert!(polylignes_proches(&t.points, &u.points), "tronçon {id} : points");
        assert_eq!(t.rayons.len(), u.rayons.len());
        assert!(t.rayons.iter().zip(&u.rayons).all(|(r, s)| (r - s).abs() <= MM));
    }
    // Quartiers.
    assert_eq!(
        a.quartiers.zones.keys().collect::<Vec<_>>(),
        b.quartiers.zones.keys().collect::<Vec<_>>()
    );
    for (id, z) in &a.quartiers.zones {
        let y = &b.quartiers.zones[id];
        assert_eq!((z.famille, z.densite), (y.famille, y.densite));
        assert!(polylignes_proches(&z.polygone, &y.polygone), "zone {id}");
    }
    assert_eq!(a.quartiers.affectations, b.quartiers.affectations);
    // Bâti.
    assert_eq!(
        a.batiments.keys().collect::<Vec<_>>(),
        b.batiments.keys().collect::<Vec<_>>()
    );
    for (id, x) in &a.batiments {
        let y = &b.batiments[id];
        assert!(polylignes_proches(&x.empreinte, &y.empreinte), "bâtiment {id}");
        assert_eq!(
            (x.niveaux, x.famille, x.origine, x.verrou, x.affectation),
            (y.niveaux, y.famille, y.origine, y.verrou, y.affectation)
        );
        assert!((x.hauteur_niveau_m - y.hauteur_niveau_m).abs() <= MM);
    }
    // Contours.
    assert_eq!(
        a.contours.contours.keys().collect::<Vec<_>>(),
        b.contours.contours.keys().collect::<Vec<_>>()
    );
    for (id, c) in &a.contours.contours {
        let d = &b.contours.contours[id];
        assert_eq!((&c.nom, c.usage), (&d.nom, d.usage));
        assert!(polylignes_proches(&c.polygone, &d.polygone), "contour {id}");
    }
}

// ---------------------------------------------------------------------------------------------- contenu

#[test]
fn contenu_de_la_carte_de_test() {
    let mut c = carte();
    assert_eq!(c.valider(), Ok(()));
    assert_eq!(c.manifeste.taille_m, [256.0, 256.0]);
    // Rue 14 m, avenue 24 m : lues dans les profils de la carte.
    assert_eq!(c.manifeste.profils.get(ClasseRoute::Rue).emprise_m(), 14.0);
    assert_eq!(c.manifeste.profils.get(ClasseRoute::Avenue).emprise_m(), 24.0);
    // 4 îlots (zones), 2 à 4 blocs chacun, tous manuels, aucun sur une emprise.
    assert_eq!(c.quartiers.zones.len(), 4);
    for (zid, z) in &c.quartiers.zones {
        let n = c
            .batiments
            .values()
            .filter(|b| b.affectation == Some(*zid))
            .count();
        assert!((2..=4).contains(&n), "zone {zid} : {n} blocs");
        assert!(polygone::est_simple(&z.polygone));
    }
    assert!(c.batiments.values().all(|b| b.origine == Origine::Manuel));
    assert!(
        c.conflits().is_empty(),
        "un bloc empiète sur une route : {:?}",
        c.conflits()
    );
    // L'îlot B est concave : son aire est plus petite que celle de son enveloppe rectangulaire.
    let b = ILOTS[1].contour;
    let (mn, mx) = polygone::englobant(b);
    assert!(polygone::aire(b) < (mx[0] - mn[0]) * (mx[1] - mn[1]) - 100.0);
    // Contour de grotte : seul, simple, en bordure de carte.
    let (_, grotte) = c.contours.par_nom(carte_de_test::GROTTE).expect("grotte");
    assert_eq!(c.contours.contours.len(), 1);
    assert!(
        grotte.polygone.iter().any(|p| p[0] == 128.0),
        "la grotte touche le bord est"
    );
}

// ---------------------------------------------------------------------------------------------- 1. aller-retour

#[test]
fn aller_retour_identique_et_fichiers_stables() {
    let mut c = carte();
    // Un peu de relief : il doit survivre à l'aller-retour (précision 16 bits).
    c.pinceau(Pinceau::Lever, [-60.0, 60.0], 30.0, 6.0, 0.0);
    c.fin_trait();
    let (d1, d2, d3) = (Temp::new("ar1"), Temp::new("ar2"), Temp::new("ar3"));
    c.enregistrer(Some(&d1.0)).expect("enregistrer 1");
    let mut relue = Carte::ouvrir(&d1.0).expect("ouvrir");
    assert_cartes_identiques(&c, &relue);
    let ecart = relue
        .relief
        .h
        .iter()
        .zip(&c.relief.h)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f32, f32::max);
    assert!(ecart < 0.008, "écart de relief {ecart} m");
    // La carte relue, enregistrée ailleurs, produit exactement les mêmes octets (ordre stable, flottants exacts).
    relue.enregistrer(Some(&d2.0)).expect("enregistrer 2");
    // L'original enregistré une seconde fois aussi.
    c.enregistrer(Some(&d3.0)).expect("enregistrer 3");
    let (f1, f2, f3) = (fichiers(&d1.0), fichiers(&d2.0), fichiers(&d3.0));
    assert!(f1.contains_key("contours.json") && f1.contains_key("reseau.json"));
    assert_eq!(f1.keys().collect::<Vec<_>>(), f2.keys().collect::<Vec<_>>());
    for (nom, octets) in &f1 {
        assert!(octets == &f2[nom], "{nom} diffère après relecture");
        assert!(octets == &f3[nom], "{nom} diffère d'une sauvegarde à l'autre");
    }
    // Rien à écrire tant que rien ne change.
    assert!(relue.sale.propre());
}

// ---------------------------------------------------------------------------------------------- 2. exactitude

/// Distance entre deux segments du plan (0 s'ils se coupent ou se touchent).
fn distance_segments(a: P2, b: P2, c: P2, d: P2) -> f64 {
    if polygone::segments_se_coupent(a, b, c, d) {
        return 0.0;
    }
    [
        mcf_geo::distance_segment(a, c, d),
        mcf_geo::distance_segment(b, c, d),
        mcf_geo::distance_segment(c, a, b),
        mcf_geo::distance_segment(d, a, b),
    ]
    .into_iter()
    .fold(f64::INFINITY, f64::min)
}

/// Distance entre les contours de deux polygones.
fn distance_polygones(p: &[P2], q: &[P2]) -> f64 {
    let aretes = |v: &[P2]| {
        (0..v.len())
            .map(|i| (v[i], v[(i + 1) % v.len()]))
            .collect::<Vec<_>>()
    };
    let (ep, eq) = (aretes(p), aretes(q));
    ep.iter()
        .flat_map(|a| eq.iter().map(move |b| distance_segments(a.0, a.1, b.0, b.1)))
        .fold(f64::INFINITY, f64::min)
}

#[test]
fn exactitude_facade_bord_d_emprise_au_millimetre() {
    let mut c = carte();
    // Relief non plat : l'altitude ne doit déplacer ni x ni z (spec §2.3).
    c.pinceau(Pinceau::Lever, [0.0, 0.0], 120.0, 8.0, 0.0);
    c.fin_trait();
    let d = c.derive().clone();
    // Noyau : quads des bandes et polygones de carrefour.
    let noyau = d.geo.polygones_emprise();
    // Maillage 3D projeté : triangles des maillages réellement envoyés au rendu.
    let mut triangles: Vec<Vec<P2>> = Vec::new();
    let mut ajouter = |m: &mcf_geo::Maillage| {
        for t in m.indices.as_chunks::<3>().0 {
            triangles.push(
                t.iter()
                    .map(|&i| [m.positions[i as usize][0], m.positions[i as usize][2]])
                    .collect(),
            );
        }
    };
    let mut altitude_max: f64 = 0.0;
    for (id, gt) in &d.geo.troncons {
        let profil = d.altitudes.troncons.get(id).expect("profil en long");
        let alt: Vec<f64> = gt
            .abscisses_rognees()
            .iter()
            .map(|s| profil.altitude(*s))
            .collect();
        altitude_max = alt.iter().copied().fold(altitude_max, f64::max);
        for b in &gt.bandes {
            ajouter(&mcf_geo::maillage_bande(b, &alt, 0.15));
        }
    }
    for (id, n) in &d.geo.noeuds {
        if !n.carrefour.emprise.is_empty() {
            let h = d.altitudes.noeuds.get(id).copied().unwrap_or(0.0);
            ajouter(&mcf_geo::maillage_polygone(&n.carrefour.emprise, |_| h));
        }
    }
    assert!(
        altitude_max > 0.5,
        "le relief doit soulever les routes (max {altitude_max} m)"
    );
    let mut verifies = 0;
    for ilot in &ILOTS {
        for bloc in ilot.blocs {
            let attendue = bloc.empreinte();
            let b: &Batiment = c
                .batiments
                .values()
                .find(|b| polylignes_proches(&b.empreinte, &attendue))
                .expect("bloc de test présent");
            let d_noyau = noyau
                .iter()
                .map(|p| distance_polygones(&b.empreinte, p))
                .fold(f64::INFINITY, f64::min);
            let d_3d = triangles
                .iter()
                .map(|t| distance_polygones(&b.empreinte, t))
                .fold(f64::INFINITY, f64::min);
            assert!(
                (d_noyau - bloc.retrait_m).abs() <= MM,
                "îlot {} bloc {:?} : noyau {d_noyau:.4} m, attendu {} m",
                ilot.nom,
                bloc.rect,
                bloc.retrait_m
            );
            assert!(
                (d_3d - d_noyau).abs() <= MM,
                "îlot {} bloc {:?} : maillage 3D {d_3d:.4} m ≠ noyau {d_noyau:.4} m",
                ilot.nom,
                bloc.rect
            );
            verifies += 1;
        }
    }
    assert_eq!(verifies, 11);
    // Au moins un bloc par îlot touche le bord d'une route.
    assert!(ILOTS.iter().all(|i| i.blocs.iter().any(|b| b.retrait_m == 0.0)));
}

// ---------------------------------------------------------------------------------------------- 3. graphe

#[test]
fn graphe_routier() {
    let c = carte();
    let g = GrapheRoutier::construire(&c.reseau, &c.manifeste.profils);
    // Une arête par tronçon : L coupé par le T de l'avenue (2), avenue coupée par le T de l'impasse (2), impasse (1).
    assert_eq!(g.aretes.len(), c.reseau.troncons.len());
    assert_eq!(g.aretes.len(), 5);
    // Un nœud par carrefour ou extrémité : 2 T + 4 extrémités.
    assert_eq!(g.noeuds.len(), 6);
    let degres: Vec<usize> = g.noeuds.iter().map(|n| n.degre).collect();
    assert_eq!(degres.iter().filter(|d| **d == 3).count(), 2, "{degres:?}");
    assert_eq!(degres.iter().filter(|d| **d == 1).count(), 4, "{degres:?}");
    assert!(g.connexe());
    for a in &g.aretes {
        let t = &c.reseau.troncons[&a.troncon];
        assert_eq!(
            (a.classe, a.de, a.vers),
            (t.classe, t.de, t.vers),
            "classe lue dans les données"
        );
        let largeur = match a.classe {
            ClasseRoute::Rue => 14.0,
            ClasseRoute::Avenue => 24.0,
            autre => panic!("classe inattendue {autre:?}"),
        };
        assert_eq!(a.largeur_m, largeur);
        assert_eq!(
            a.longueur_m,
            mcf_geo::longueur(&c.reseau.axe(a.troncon).expect("axe"))
        );
        assert!(a.nom.is_some());
    }
    let n = |p: P2| g.noeud_proche(p, 0.01).expect("nœud");
    let (l_debut, l_fin) = (n(RUE_EN_L[0]), n(RUE_EN_L[2]));
    let t_est = n(AVENUE[1]);
    let t_ouest = n(IMPASSE[0]);
    let impasse = n(IMPASSE[1]);
    assert_eq!(
        g.noeuds.iter().find(|x| x.id == impasse).map(|x| x.degre),
        Some(1),
        "impasse : degré 1"
    );
    assert_eq!(
        g.noeuds.iter().find(|x| x.id == t_est).map(|x| x.degre),
        Some(3),
        "T : 3 branches"
    );
    // Longueurs : virage de rayon 10 m à 90° = 20 − 5π m de moins que l'angle vif (discrétisation ≤ 5 cm de corde).
    let virage = 20.0 - 5.0 * std::f64::consts::PI;
    let attendu_l = 140.0 + 150.0 - virage;
    let it = g.plus_court_chemin(l_debut, l_fin).expect("chemin le long du L");
    assert_eq!(it.noeuds, vec![l_debut, t_est, l_fin]);
    assert_eq!(it.troncons.len(), 2);
    assert!(
        (it.longueur_m - attendu_l).abs() < 0.05,
        "longueur du L {}",
        it.longueur_m
    );
    // Déterministe : même résultat recalculé, et dans l'autre sens les mêmes tronçons à l'envers.
    let g2 = GrapheRoutier::construire(&c.reseau, &c.manifeste.profils);
    assert_eq!(g2, g);
    assert_eq!(g2.plus_court_chemin(l_debut, l_fin).as_ref(), Some(&it));
    let retour = g.plus_court_chemin(l_fin, l_debut).expect("retour");
    assert_eq!(
        retour.troncons.iter().rev().collect::<Vec<_>>(),
        it.troncons.iter().collect::<Vec<_>>()
    );
    // Du début du L au fond de l'impasse : L (branche nord), avenue (partie est), impasse.
    let it = g.plus_court_chemin(l_debut, impasse).expect("vers l'impasse");
    assert_eq!(it.noeuds, vec![l_debut, t_est, t_ouest, impasse]);
    assert!((it.longueur_m - (140.0 + 80.0 - virage + 80.0 + 55.0)).abs() < 0.05);
}

// ---------------------------------------------------------------------------------------------- 4. identifiants

fn tous_les_ids(c: &Carte) -> BTreeSet<Id> {
    c.reseau
        .noeuds
        .keys()
        .chain(c.reseau.troncons.keys())
        .chain(c.quartiers.zones.keys())
        .chain(c.quartiers.affectations.keys())
        .chain(c.batiments.keys())
        .chain(c.contours.contours.keys())
        .copied()
        .collect()
}

#[test]
fn identifiants_jamais_reutilises_apres_suppression() {
    let mut c = carte();
    let mut vus = tous_les_ids(&c);
    let n0 = vus.len();
    assert_eq!(n0, 6 + 5 + 4 + 11 + 1, "chaque élément a son propre identifiant");
    for tour in 0..200 {
        // Supprimer l'impasse et un bloc, puis les recréer.
        let impasse = *c
            .reseau
            .troncons
            .iter()
            .find(|(_, t)| t.nom.as_deref() == Some(carte_de_test::NOM_IMPASSE))
            .expect("impasse")
            .0;
        let (bid, bloc) = c
            .batiments
            .iter()
            .next()
            .map(|(i, b)| (*i, b.clone()))
            .expect("bloc");
        c.modifier("supprimer", |tx| {
            tx.reseau().supprimer_troncon(impasse);
            tx.retirer_batiment(bid);
            Ok(())
        })
        .expect("supprimer");
        let nouveaux = c.tracer_route(ClasseRoute::Rue, &IMPASSE, 2.0).expect("retracer");
        let nouveau_bloc = Id::nouveau();
        c.modifier("reposer", |tx| {
            for id in &nouveaux {
                if let Some(t) = tx.reseau().troncons.get_mut(id) {
                    t.nom = Some(carte_de_test::NOM_IMPASSE.into());
                }
            }
            tx.poser_batiment(nouveau_bloc, bloc);
            Ok(())
        })
        .expect("reposer");
        // Les identifiants qui apparaissent n'ont jamais servi, même ceux des éléments supprimés.
        for id in tous_les_ids(&c) {
            if !vus.contains(&id) {
                vus.insert(id);
            } else {
                assert!(
                    id != impasse && id != bid,
                    "tour {tour} : identifiant supprimé réutilisé"
                );
            }
        }
        assert!(!c.reseau.troncons.contains_key(&impasse) && !c.batiments.contains_key(&bid));
    }
    // Annuler / refaire remet les mêmes objets (mêmes identifiants), sans en fabriquer de nouveaux.
    let avant = tous_les_ids(&c);
    c.annuler();
    c.refaire();
    assert_eq!(tous_les_ids(&c), avant);
    // 200 tours × (1 tronçon + 1 nœud d'impasse + 1 bloc) de plus, tous distincts.
    assert_eq!(vus.len(), n0 + 200 * 3);
}

// ---------------------------------------------------------------------------------------------- 5. pièges

fn ouvrir_refuse(d: &Path, motif: &str) {
    match Carte::ouvrir(d) {
        Ok(_) => panic!("{motif} : ouverture acceptée"),
        Err(e) => assert!(!e.is_empty(), "{motif}"),
    }
}

/// Remplace temporairement un fichier, vérifie le refus, puis le remet et vérifie que tout s'ouvre.
fn pieger(d: &Path, rel: &str, contenu: &[u8], motif: &str) {
    let chemin = d.join(rel);
    let original = fs::read(&chemin).ok();
    if let Some(parent) = chemin.parent() {
        fs::create_dir_all(parent).expect("dossier");
    }
    fs::write(&chemin, contenu).expect("écrire le piège");
    ouvrir_refuse(d, motif);
    match original {
        Some(o) => fs::write(&chemin, o).expect("remettre"),
        None => fs::remove_file(&chemin).expect("retirer"),
    }
    Carte::ouvrir(d).unwrap_or_else(|e| panic!("{motif} : la carte saine ne s'ouvre plus : {e}"));
}

fn texte(d: &Path, rel: &str) -> String {
    fs::read_to_string(d.join(rel)).expect("lire")
}

/// En-tête PNG valide (16 bits, niveaux de gris) annonçant `l × h` pixels, suivi de presque rien.
fn png_geant(l: u32, h: u32) -> Vec<u8> {
    fn crc(octets: &[u8]) -> u32 {
        let mut c = 0xFFFF_FFFFu32;
        for &o in octets {
            c ^= u32::from(o);
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    0xEDB8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
        }
        !c
    }
    fn bloc(sortie: &mut Vec<u8>, genre: &[u8; 4], donnees: &[u8]) {
        sortie.extend_from_slice(&(donnees.len() as u32).to_be_bytes());
        let mut corps = genre.to_vec();
        corps.extend_from_slice(donnees);
        sortie.extend_from_slice(&corps);
        sortie.extend_from_slice(&crc(&corps).to_be_bytes());
    }
    let mut v = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&l.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[16, 0, 0, 0, 0]);
    bloc(&mut v, b"IHDR", &ihdr);
    // Flux zlib minimal (bloc stocké vide) : tout décodeur qui ne vérifie pas les dimensions irait allouer ~8 Go.
    bloc(
        &mut v,
        b"IDAT",
        &[0x78, 0x01, 0x01, 0x00, 0x00, 0xFF, 0xFF, 0x00, 0x00, 0x00, 0x01],
    );
    bloc(&mut v, b"IEND", &[]);
    v
}

#[test]
fn fichiers_pieges_refuses() {
    let mut c = carte();
    let d = Temp::new("pieges");
    c.enregistrer(Some(&d.0)).expect("enregistrer");
    Carte::ouvrir(&d.0).expect("la carte saine s'ouvre");

    // Champ inconnu, dans chaque famille de fichiers.
    for rel in [
        "carte.mcfcarte.json",
        "reseau.json",
        "quartiers.json",
        "contours.json",
    ] {
        let t = texte(&d.0, rel);
        pieger(
            &d.0,
            rel,
            t.replacen('{', "{\"pirate\": 1,", 1).as_bytes(),
            &format!("champ inconnu dans {rel}"),
        );
    }
    let cellule = fichiers(&d.0)
        .into_keys()
        .find(|k| k.starts_with("batiments/c_"))
        .expect("cellule de bâti");
    let t = texte(&d.0, &cellule);
    pieger(
        &d.0,
        &cellule,
        t.replacen("\"niveaux\"", "\"script\": \"rm -rf\", \"niveaux\"", 1)
            .as_bytes(),
        "champ inconnu dans un bâtiment",
    );

    // Chemins avec « .. » : nom de carte, nom de contour, nom de fichier de calque.
    let t = texte(&d.0, "carte.mcfcarte.json");
    pieger(
        &d.0,
        "carte.mcfcarte.json",
        t.replace("\"carte_de_test\"", "\"../../evasion\"").as_bytes(),
        "nom de carte « .. »",
    );
    let t = texte(&d.0, "contours.json");
    pieger(
        &d.0,
        "contours.json",
        t.replace("Grotte_SOCK_Entree", "../Grotte_SOCK_Entree")
            .as_bytes(),
        "nom de contour « .. »",
    );
    pieger(&d.0, "calques/..mcfmap", b"{}", "fichier de calque « ..mcfmap »");
    pieger(&d.0, "calques/a..b.mcfmap", b"{}", "fichier de calque « a..b »");
    // Et à l'écriture : un calque nommé « ../sortie » est refusé et rien n'est écrit hors du projet.
    let mut evasion = Carte::ouvrir(&d.0).expect("ouvrir");
    evasion.calques.insert(
        "../sortie".into(),
        mcf_map::McfMap::new("sortie", "test", "2026-11-23", 1),
    );
    evasion.sale.calques.insert("../sortie".into());
    assert!(evasion.enregistrer(None).is_err());
    assert!(!d.0.join("sortie.mcfmap").exists() && !d.0.join("calques").join("../sortie.mcfmap").exists());

    // PNG géant : l'en-tête annonce 65 535 × 65 535 pixels de 16 bits (8 Go) → refusé avant décodage.
    let tuile = "relief/t_000_000.png";
    let debut = std::time::Instant::now();
    pieger(&d.0, tuile, &png_geant(65_535, 65_535), "PNG géant (en-tête)");
    assert!(
        debut.elapsed().as_secs() < 10,
        "le refus doit être immédiat, pas après décodage"
    );
    // Fichier de tuile plus gros que le plafond (16 Mio) → refusé sans être lu.
    let gros = vec![0u8; mcf_carte::io::MAX_TUILE as usize + 1];
    pieger(&d.0, tuile, &gros, "tuile trop lourde");
}
