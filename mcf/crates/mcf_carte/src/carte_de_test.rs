//! Carte de test de référence : petite, entièrement décrite par les constantes ci-dessous, pour les tests
//! d'aller-retour, d'exactitude 2D = 3D (spec §2.5), de graphe routier et pour l'éditeur (« Ouvrir carte de test »).
//!
//! Emprise 256 × 256 m centrée sur l'origine (le contenu tient dans ~200 × 200 m), mètres, repère MCF (`x`, `z`),
//! nord = −Z. Bords d'emprise utiles (axe ± demi-emprise) :
//!
//! ```text
//!   rue en L      : z = −60 de x = −90 à 50 (virage arrondi en (50, −60)), puis x = 50 jusqu'à z = 90
//!                   bords : z = −53 (sud), x = 43 (ouest)
//!   avenue        : z = 20 de x = −90 à 50, finit en T sur le L      bords : z = 8 (nord), z = 32 (sud)
//!   impasse (rue) : x = −30 de z = 20 (T sur l'avenue) à z = 75       bords : x = −37 et x = −23
//!   grotte        : contour « Grotte_SOCK_Entree » collé au bord est de la carte (x = 128)
//! ```
//!
//! Les blocs sont posés à la main (`manuel`) avec un retrait connu par rapport au bord d'emprise le plus proche :
//! c'est la valeur que le test d'exactitude compare à la géométrie calculée.

use mcf_geo::{ClasseRoute, P2, polygone};

use crate::batiments::{Batiment, Origine};
use crate::carte::Carte;
use crate::contours::UsageContour;
use crate::gabarits::{Densite, Famille};
use crate::ids::Id;
use crate::manifeste::Manifeste;

/// Nom de la carte.
pub const NOM: &str = "carte_de_test";
/// Côté de la carte (m) : multiple de la tuile, juste au-dessus des ~200 m utiles.
pub const TAILLE_M: f64 = 256.0;
/// Tuile de relief (m).
pub const TUILE_M: u32 = 128;
/// Graine fixe : la carte est la même à chaque construction (hors identifiants).
pub const GRAINE: u64 = 0x4D43_465F_5445_5354;
/// Accroche des tracés (m).
pub const ACCROCHE_M: f64 = 2.0;

/// Rue de 14 m en L (virage à 90°, arrondi au rayon par défaut de la classe).
pub const RUE_EN_L: [P2; 3] = [[-90.0, -60.0], [50.0, -60.0], [50.0, 90.0]];
/// Nom de la rue en L.
pub const NOM_RUE_EN_L: &str = "Rue du Coude";
/// Avenue de 24 m, qui finit en T sur la branche verticale du L.
pub const AVENUE: [P2; 2] = [[-90.0, 20.0], [50.0, 20.0]];
/// Nom de l'avenue.
pub const NOM_AVENUE: &str = "Avenue Centrale";
/// Impasse de 14 m (classe rue), branchée en T sur l'avenue.
pub const IMPASSE: [P2; 2] = [[-30.0, 20.0], [-30.0, 75.0]];
/// Nom de l'impasse.
pub const NOM_IMPASSE: &str = "Impasse des Tilleuls";

/// Nom du contour de l'entrée de grotte (ADR-008 du jeu).
pub const GROTTE: &str = "Grotte_SOCK_Entree";
/// Contour de la grotte : polygone simple, deux sommets sur le bord est de la carte.
pub const GROTTE_POLYGONE: [P2; 5] = [
    [100.0, -104.0],
    [104.0, -118.0],
    [128.0, -122.0],
    [128.0, -92.0],
    [110.0, -90.0],
];

/// Un bloc de test : rectangle `[x0, z0, x1, z1]`, niveaux, retrait attendu jusqu'au bord d'emprise le plus proche.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlocDeTest {
    /// `[x0, z0, x1, z1]` (m).
    pub rect: [f64; 4],
    /// Niveaux.
    pub niveaux: u32,
    /// Distance façade → bord d'emprise de route attendue (m) ; 0 = façade sur l'alignement.
    pub retrait_m: f64,
}

impl BlocDeTest {
    /// Empreinte, sens trigonométrique dans `(x, z)`.
    pub fn empreinte(&self) -> Vec<P2> {
        let [x0, z0, x1, z1] = self.rect;
        polygone::oriente_positif(&[[x0, z0], [x1, z0], [x1, z1], [x0, z1]])
    }
}

/// Un îlot loti (zone libre) et ses blocs.
#[derive(Debug, Clone, Copy)]
pub struct IlotDeTest {
    /// Nom (documentation et messages de test).
    pub nom: &'static str,
    /// Famille.
    pub famille: Famille,
    /// Densité.
    pub densite: Densite,
    /// Hauteur d'un niveau (m).
    pub hauteur_niveau_m: f64,
    /// Contour de l'îlot (bords d'emprise, coins dégagés des arrondis de carrefour).
    pub contour: &'static [P2],
    /// Blocs.
    pub blocs: &'static [BlocDeTest],
}

const fn bloc(rect: [f64; 4], niveaux: u32, retrait_m: f64) -> BlocDeTest {
    BlocDeTest {
        rect,
        niveaux,
        retrait_m,
    }
}

/// Les quatre îlots lotis. B est concave (en L).
pub const ILOTS: [IlotDeTest; 4] = [
    IlotDeTest {
        nom: "A (rectangle, entre le L et l'avenue)",
        famille: Famille::Residentiel,
        densite: Densite::Moyenne,
        hauteur_niveau_m: 3.0,
        contour: &[[-88.0, -53.0], [-38.0, -53.0], [-38.0, 8.0], [-88.0, 8.0]],
        blocs: &[
            bloc([-80.0, -53.0, -62.0, -41.0], 4, 0.0),
            bloc([-56.0, -49.0, -42.0, -35.0], 3, 4.0),
            bloc([-80.0, -8.0, -64.0, 6.0], 5, 2.0),
        ],
    },
    IlotDeTest {
        nom: "B (concave en L, dans le coude)",
        famille: Famille::Commerce,
        densite: Densite::Moyenne,
        hauteur_niveau_m: 4.0,
        contour: &[
            [-28.0, -53.0],
            [30.0, -53.0],
            [30.0, -20.0],
            [43.0, -20.0],
            [43.0, 2.0],
            [37.0, 8.0],
            [-28.0, 8.0],
        ],
        blocs: &[
            bloc([0.0, -6.0, 20.0, 8.0], 3, 0.0),
            bloc([31.0, -16.0, 43.0, -4.0], 2, 0.0),
            bloc([-20.0, -50.0, -6.0, -38.0], 4, 3.0),
        ],
    },
    IlotDeTest {
        nom: "C (à l'ouest de l'impasse)",
        famille: Famille::Residentiel,
        densite: Densite::Faible,
        hauteur_niveau_m: 3.0,
        contour: &[
            [-88.0, 32.0],
            [-43.0, 32.0],
            [-37.0, 38.0],
            [-37.0, 100.0],
            [-88.0, 100.0],
        ],
        blocs: &[
            bloc([-49.0, 44.0, -37.0, 58.0], 2, 0.0),
            bloc([-82.0, 34.5, -62.0, 46.0], 1, 2.5),
        ],
    },
    IlotDeTest {
        nom: "D (entre l'impasse et le L)",
        famille: Famille::Services,
        densite: Densite::Moyenne,
        hauteur_niveau_m: 3.0,
        contour: &[
            [-17.0, 32.0],
            [37.0, 32.0],
            [43.0, 38.0],
            [43.0, 100.0],
            [-23.0, 100.0],
            [-23.0, 38.0],
        ],
        blocs: &[
            bloc([31.0, 60.0, 43.0, 74.0], 3, 0.0),
            bloc([-12.0, 32.0, 6.0, 46.0], 2, 0.0),
            bloc([-8.0, 70.0, 14.0, 88.0], 3, 15.0),
        ],
    },
];

/// Trace une route et nomme les tronçons créés (une étape d'historique chacun).
fn tracer(c: &mut Carte, classe: ClasseRoute, chemin: &[P2], nom: &str) -> Result<Vec<Id>, String> {
    let ids = c.tracer_route(classe, chemin, ACCROCHE_M)?;
    c.modifier(&format!("Nommer : {nom}"), |tx| {
        for id in &ids {
            if let Some(t) = tx.reseau().troncons.get_mut(id) {
                t.nom = Some(nom.to_owned());
            }
        }
        Ok(())
    })?;
    Ok(ids)
}

/// Construit la carte de test (non enregistrée). Identifiants neufs à chaque appel, géométrie identique.
pub fn construire() -> Result<Carte, String> {
    let mut m = Manifeste::nouveau(NOM, TAILLE_M);
    m.tuile_m = TUILE_M;
    m.graine = GRAINE;
    let mut c = Carte::nouvelle(m)?;
    tracer(&mut c, ClasseRoute::Rue, &RUE_EN_L, NOM_RUE_EN_L)?;
    tracer(&mut c, ClasseRoute::Avenue, &AVENUE, NOM_AVENUE)?;
    tracer(&mut c, ClasseRoute::Rue, &IMPASSE, NOM_IMPASSE)?;
    for ilot in &ILOTS {
        let zone = c.ajouter_zone(ilot.contour.to_vec(), ilot.famille, ilot.densite)?;
        c.modifier(&format!("Blocs de l'îlot {}", ilot.nom), |tx| {
            for b in ilot.blocs {
                tx.poser_batiment(
                    Id::nouveau(),
                    Batiment {
                        empreinte: b.empreinte(),
                        niveaux: b.niveaux,
                        hauteur_niveau_m: ilot.hauteur_niveau_m,
                        famille: ilot.famille,
                        origine: Origine::Manuel,
                        verrou: false,
                        affectation: Some(zone),
                    },
                );
            }
            Ok(())
        })?;
    }
    c.ajouter_contour(GROTTE, UsageContour::EntreeSouterrain, GROTTE_POLYGONE.to_vec())?;
    Ok(c)
}
