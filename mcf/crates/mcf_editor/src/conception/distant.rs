//! Pilotage de l'éditeur par l'IA (spec `docs/SPEC_MCP_LSP_MOTEUR.md` §1), feature `remote`, option `--remote`.
//!
//! Le pont MCP (`mcf_mcp`) se connecte en TCP sur `127.0.0.1`, envoie **une** requête JSON sur une ligne et reçoit
//! **une** réponse sur une ligne, puis la connexion est fermée :
//!
//! ```text
//! → {"jeton": "…", "id": 1, "methode": "mcf.etat", "params": {}}
//! ← {"id": 1, "resultat": {…}}          ou   {"id": 1, "erreur": "…"}
//! ```
//!
//! Pourquoi pas le transport HTTP de `bevy_remote` : ses méthodes ne voient pas les en-têtes de la requête (aucun
//! jeton, aucun contrôle de `Host` possible), et il expose l'écriture brute des composants, contraire à « une seule
//! voie d'écriture ». Ici, pas de HTTP du tout : une page web qui viserait le port envoie `POST / HTTP/1.1`, ligne
//! refusée (JSON illisible), et ne lit jamais la réponse.
//!
//! Sécurité (modèle de permissions de la spec) :
//! - écoute sur `127.0.0.1` seulement, port choisi par le système ;
//! - jeton aléatoire de 256 bits par session, comparé en temps constant, écrit dans un fichier de session du dossier
//!   de l'utilisateur (`%LOCALAPPDATA%\MCF-Engine\remote\editeur.json`, droits 600 sous Unix), effacé à la fermeture ;
//! - liste fermée de méthodes ; ce lot ne fait que **lire** l'état et **déplacer les caméras** (aucune écriture dans
//!   la carte) ;
//! - plafonds : requête de 64 Kio, captures de 1280 px de côté au plus, une connexion traitée à la fois.

use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{Ipv4Addr, TcpListener, TcpStream},
    path::PathBuf,
    sync::{Mutex, mpsc},
    time::Duration,
};

use bevy::{
    diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin},
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured},
    window::PrimaryWindow,
};
use mcf_carte::Id;
use serde_json::{Value, json};

use super::{
    Disposition, Edition, OptionsDistant, Projet, Selection, ZoneVues,
    camera::{Orbite, Vue, VuePlan, rect_de},
};

/// Méthodes acceptées (liste fermée).
pub const METHODES: [&str; 3] = ["mcf.etat", "mcf.capture", "mcf.camera"];

/// Taille maximale d'une requête (octets).
const TAILLE_MAX_REQUETE: u64 = 64 * 1024;
/// Côté maximal d'une capture (pixels).
pub const CAPTURE_MAX_PX: u32 = 1280;
/// Côté par défaut d'une capture (pixels) : ~10 % des jetons d'une capture d'écran 2560 × 1440.
pub const CAPTURE_DEFAUT_PX: u32 = 768;
/// Attente maximale de la réponse de l'éditeur (une capture prend quelques images).
const ATTENTE_REPONSE: Duration = Duration::from_secs(15);

/// Une requête transmise au monde Bevy.
struct Requete {
    methode: String,
    params: Value,
    reponse: mpsc::Sender<Result<Value, String>>,
}

/// File des requêtes reçues par le fil d'écoute.
#[derive(Resource)]
struct Boite(Mutex<mpsc::Receiver<Requete>>);

/// Fichier de session : effacé quand l'application se ferme (le jeton ne survit pas à la session).
#[derive(Resource)]
struct FichierSession(PathBuf);

impl Drop for FichierSession {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Dossier des fichiers de session, propre à l'utilisateur. Même règle dans `mcf_mcp`.
pub fn dossier_session() -> Option<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("state")))?;
    Some(base.join("MCF-Engine").join("remote"))
}

/// Ajoute le pilotage à l'application : ouvre le port, écrit le fichier de session, installe le système qui répond.
/// En cas d'échec (port, fichier), l'éditeur démarre sans pilotage et le dit dans la console.
pub fn installer(app: &mut App, options: OptionsDistant) {
    match demarrer() {
        Ok((boite, fichier, port)) => {
            info!(
                "Pilotage par l'IA actif sur 127.0.0.1:{port} (session : {}){}",
                fichier.0.display(),
                if options.lecture_seule {
                    ", lecture seule"
                } else {
                    ""
                }
            );
            if !app.is_plugin_added::<FrameTimeDiagnosticsPlugin>() {
                app.add_plugins(FrameTimeDiagnosticsPlugin::default());
            }
            app.insert_resource(boite)
                .insert_resource(fichier)
                .add_systems(Update, repondre);
        }
        Err(e) => error!("Pilotage par l'IA impossible : {e}"),
    }
}

fn demarrer() -> Result<(Boite, FichierSession, u16), String> {
    let ecoute =
        TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).map_err(|e| format!("ouverture du port : {e}"))?;
    let port = ecoute.local_addr().map_err(|e| e.to_string())?.port();
    let mut octets = [0u8; 32];
    getrandom::fill(&mut octets).map_err(|e| format!("aléa du système indisponible : {e}"))?;
    let jeton: String = octets.iter().map(|o| format!("{o:02x}")).collect();

    let dossier = dossier_session().ok_or("dossier de l'utilisateur introuvable (LOCALAPPDATA / HOME)")?;
    std::fs::create_dir_all(&dossier).map_err(|e| format!("{} : {e}", dossier.display()))?;
    let chemin = dossier.join("editeur.json");
    let contenu = json!({
        "programme": "mcf_editor",
        "pid": std::process::id(),
        "port": port,
        "jeton": jeton,
    });
    ecrire_prive(&chemin, contenu.to_string().as_bytes())
        .map_err(|e| format!("{} : {e}", chemin.display()))?;

    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("mcf-remote".into())
        .spawn(move || ecouter(&ecoute, &jeton, &tx))
        .map_err(|e| e.to_string())?;
    Ok((Boite(Mutex::new(rx)), FichierSession(chemin), port))
}

/// Écrit un fichier lisible par l'utilisateur seul (Unix : 600 ; Windows : le dossier local de l'utilisateur n'est
/// lisible que par lui et les administrateurs).
fn ecrire_prive(chemin: &std::path::Path, octets: &[u8]) -> std::io::Result<()> {
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    o.open(chemin)?.write_all(octets)
}

/// Fil d'écoute : une connexion à la fois, une requête par connexion.
fn ecouter(ecoute: &TcpListener, jeton: &str, tx: &mpsc::Sender<Requete>) {
    for flux in ecoute.incoming() {
        let Ok(flux) = flux else { continue };
        if servir(flux, jeton, tx).is_err() {
            // Client parti ou trop lent : rien à faire, on passe au suivant.
        }
    }
}

fn servir(flux: TcpStream, jeton: &str, tx: &mpsc::Sender<Requete>) -> std::io::Result<()> {
    if !flux.peer_addr()?.ip().is_loopback() {
        return Ok(());
    }
    flux.set_read_timeout(Some(Duration::from_secs(5)))?;
    flux.set_write_timeout(Some(Duration::from_secs(5)))?;
    let mut ligne = String::new();
    BufReader::new((&flux).take(TAILLE_MAX_REQUETE)).read_line(&mut ligne)?;
    let reponse = traiter(&ligne, jeton, tx);
    let mut flux = flux;
    flux.write_all(reponse.to_string().as_bytes())?;
    flux.write_all(b"\n")?;
    flux.flush()
}

/// Comparaison en temps constant (longueurs publiques : le jeton fait toujours 64 caractères).
fn egal_temps_constant(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Vérifie la requête, la passe au monde Bevy et attend sa réponse.
fn traiter(ligne: &str, jeton: &str, tx: &mpsc::Sender<Requete>) -> Value {
    let Ok(v) = serde_json::from_str::<Value>(ligne.trim_end()) else {
        return json!({ "id": null, "erreur": "requête illisible (une ligne JSON attendue)" });
    };
    let id = v.get("id").cloned().unwrap_or(Value::Null);
    let erreur = |m: &str| json!({ "id": id, "erreur": m });
    let jeton_ok = v
        .get("jeton")
        .and_then(Value::as_str)
        .is_some_and(|j| egal_temps_constant(j.as_bytes(), jeton.as_bytes()));
    if !jeton_ok {
        // Freine les essais en série.
        std::thread::sleep(Duration::from_millis(250));
        return erreur("jeton refusé");
    }
    let Some(methode) = v.get("methode").and_then(Value::as_str) else {
        return erreur("champ « methode » manquant");
    };
    if !METHODES.contains(&methode) {
        return erreur(&format!(
            "méthode inconnue « {methode} » (connues : {})",
            METHODES.join(", ")
        ));
    }
    let (rtx, rrx) = mpsc::channel();
    let requete = Requete {
        methode: methode.to_owned(),
        params: v.get("params").cloned().unwrap_or(Value::Null),
        reponse: rtx,
    };
    if tx.send(requete).is_err() {
        return erreur("l'éditeur se ferme");
    }
    match rrx.recv_timeout(ATTENTE_REPONSE) {
        Ok(Ok(r)) => json!({ "id": id, "resultat": r }),
        Ok(Err(e)) => erreur(&e),
        Err(_) => erreur("l'éditeur n'a pas répondu à temps"),
    }
}

/// Répond aux requêtes en attente (au plus quelques-unes par image).
fn repondre(
    mut commands: Commands,
    boite: Res<Boite>,
    mut projet: ResMut<Projet>,
    mut edition: ResMut<Edition>,
    mut orbite: ResMut<Orbite>,
    mut plan: ResMut<VuePlan>,
    zone: Res<ZoneVues>,
    window: Single<&Window, With<PrimaryWindow>>,
    diagnostics: Res<DiagnosticsStore>,
) {
    let Ok(rx) = boite.0.lock() else { return };
    for _ in 0..8 {
        let Ok(r) = rx.try_recv() else { break };
        let resultat = match r.methode.as_str() {
            "mcf.etat" => Ok(etat(&mut projet, &edition, &orbite, &plan, &zone, &diagnostics)),
            "mcf.camera" => camera(&r.params, &mut projet, &mut edition, &mut orbite, &mut plan),
            "mcf.capture" => match preparer_capture(&r.params, &zone, &edition, window.scale_factor()) {
                Ok((rect, cote, vue)) => {
                    let reponse = r.reponse;
                    commands.spawn(Screenshot::primary_window()).observe(
                        move |ev: On<ScreenshotCaptured>| {
                            let _ = reponse.send(encoder_capture(&ev.image, rect, cote, vue));
                        },
                    );
                    continue;
                }
                Err(e) => Err(e),
            },
            m => Err(format!("méthode inconnue « {m} »")),
        };
        let _ = r.reponse.send(resultat);
    }
}

fn p2(p: [f64; 2]) -> Value {
    json!([arrondi(p[0]), arrondi(p[1])])
}

/// Arrondi au centimètre : des réponses courtes (jetons).
fn arrondi(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

fn nom_disposition(d: Disposition) -> &'static str {
    match d {
        Disposition::Plan => "plan",
        Disposition::Perspective => "perspective",
        Disposition::Partagee => "partagee",
    }
}

fn selection_json(s: &Selection) -> Value {
    match s {
        Selection::Rien => json!({ "type": "rien" }),
        Selection::Batiments(ids) => json!({
            "type": "batiments",
            "ids": ids.iter().take(50).map(Id::to_string).collect::<Vec<_>>(),
            "nombre": ids.len(),
        }),
        Selection::Troncon(id) => json!({ "type": "troncon", "id": id.to_string() }),
        Selection::Quartier(id) => json!({ "type": "quartier", "id": id.to_string() }),
    }
}

/// `mcf.etat` : la carte ouverte et l'état de l'éditeur, en JSON compact. Les noms (carte, voies) sont des données
/// de la carte, non fiables : le pont les marque comme telles.
fn etat(
    projet: &mut Projet,
    edition: &Edition,
    orbite: &Orbite,
    plan: &VuePlan,
    zone: &ZoneVues,
    diagnostics: &DiagnosticsStore,
) -> Value {
    let ilots = projet.carte.derive().ilots.len();
    let c = &projet.carte;
    let m = &c.manifeste;
    let (a, b) = m.bornes();
    let image_ms = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FRAME_TIME)
        .and_then(|d| d.smoothed());
    let vues: Vec<&str> = [Vue::Plan, Vue::Perspective]
        .into_iter()
        .filter(|v| rect_de(*v, zone.rect, edition.disposition).is_some())
        .map(|v| if v == Vue::Plan { "plan" } else { "3d" })
        .collect();
    json!({
        "carte": {
            "nom": m.nom,
            "dossier": c.dossier.as_ref().map(|d| d.display().to_string()),
            "taille_m": m.taille_m,
            "bornes_m": [p2(a), p2(b)],
            "altitudes_m": [m.altitude_min_m, m.altitude_max_m],
            "niveau_eau_m": m.niveau_eau_m,
            "modifications_non_enregistrees": !c.sale.propre(),
        },
        "compteurs": {
            "noeuds": c.reseau.noeuds.len(),
            "troncons": c.reseau.troncons.len(),
            "troncons_nommes": c.reseau.troncons.values().filter(|t| t.nom.is_some()).count(),
            "ilots": ilots,
            "affectations": c.quartiers.affectations.len(),
            "zones_libres": c.quartiers.zones.len(),
            "batiments": c.batiments.len(),
            "batiments_modelises": c.batiments.values().filter(|b| b.modele.is_some()).count(),
            "calques_reference": c.references.references.len(),
            "calques_objets": c.calques.len(),
        },
        "edition": {
            "etape": edition.etape.nom(),
            "outil": format!("{:?}", edition.outil),
            "selection": selection_json(&edition.selection),
            "points_en_cours": edition.points.len(),
            "disposition": nom_disposition(edition.disposition),
            "vues_affichees": vues,
            "promenade": edition.promenade,
            "conflits": edition.conflits.len(),
        },
        "message": { "texte": projet.message, "erreur": projet.erreur },
        "cameras": {
            "plan": { "centre": [arrondi(plan.centre.x.into()), arrondi(plan.centre.y.into())],
                      "m_par_px": arrondi(plan.m_par_px.into()) },
            "3d": {
                "cible": [arrondi(orbite.cible.x.into()), arrondi(orbite.cible.y.into()), arrondi(orbite.cible.z.into())],
                "distance_m": arrondi(orbite.distance.into()),
                "lacet_deg": arrondi(orbite.lacet.to_degrees().into()),
                "tangage_deg": arrondi(orbite.tangage.to_degrees().into()),
            },
        },
        "mesures": {
            "image_ms": image_ms.map(arrondi),
            "scene_ms": arrondi(edition.duree_scene_ms),
        },
    })
}

fn nombre(params: &Value, cle: &str) -> Result<Option<f64>, String> {
    match params.get(cle) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_f64()
            .filter(|x| x.is_finite())
            .map(Some)
            .ok_or_else(|| format!("« {cle} » doit être un nombre fini")),
    }
}

/// Centre d'un élément de la carte (bâtiment, tronçon, affectation, zone libre).
fn centre_element(projet: &Projet, id: &str) -> Result<[f64; 2], String> {
    let id: Id = id
        .to_owned()
        .try_into()
        .map_err(|_| format!("identifiant invalide « {id} »"))?;
    let c = &projet.carte;
    let moyenne = |pts: &[[f64; 2]]| {
        let n = pts.len().max(1) as f64;
        let s = pts.iter().fold([0.0, 0.0], |s, p| [s[0] + p[0], s[1] + p[1]]);
        [s[0] / n, s[1] / n]
    };
    if let Some(b) = c.batiments.get(&id) {
        return Ok(moyenne(&b.empreinte));
    }
    if let Some(t) = c.reseau.troncons.get(&id) {
        return t
            .points
            .get(t.points.len() / 2)
            .copied()
            .ok_or("tronçon sans point".into());
    }
    if let Some(a) = c.quartiers.affectations.get(&id) {
        return Ok(a.repere);
    }
    if let Some(z) = c.quartiers.zones.get(&id) {
        return Ok(moyenne(&z.polygone));
    }
    Err(format!("aucun élément d'identifiant {id}"))
}

/// `mcf.camera` : place les caméras (aucune écriture dans la carte). Paramètres, tous facultatifs :
/// `disposition` (`plan`, `perspective`, `partagee`), `cible` `[x, z]` ou `element` (identifiant), `distance_m`,
/// `lacet_deg`, `tangage_deg` (3D), `m_par_px` (Plan).
fn camera(
    params: &Value,
    projet: &mut Projet,
    edition: &mut Edition,
    orbite: &mut Orbite,
    plan: &mut VuePlan,
) -> Result<Value, String> {
    if !params.is_object() && !params.is_null() {
        return Err("paramètres : un objet JSON attendu".into());
    }
    if let Some(d) = params.get("disposition") {
        edition.disposition = match d.as_str() {
            Some("plan") => Disposition::Plan,
            Some("perspective" | "3d") => Disposition::Perspective,
            Some("partagee") => Disposition::Partagee,
            _ => return Err("« disposition » : plan, perspective ou partagee".into()),
        };
    }
    let cible = match (params.get("cible"), params.get("element").and_then(Value::as_str)) {
        (Some(_), Some(_)) => return Err("donner « cible » ou « element », pas les deux".into()),
        (Some(c), None) => {
            let xz = c
                .as_array()
                .filter(|a| a.len() == 2)
                .and_then(|a| Some([a[0].as_f64()?, a[1].as_f64()?]))
                .filter(|p| p.iter().all(|v| v.is_finite()))
                .ok_or("« cible » : [x, z] en mètres")?;
            Some(xz)
        }
        (None, Some(id)) => Some(centre_element(projet, id)?),
        (None, None) => None,
    };
    let (a, b) = projet.carte.manifeste.bornes();
    let cote = (b[0] - a[0]).max(b[1] - a[1]);
    if let Some(p) = cible {
        // Un peu de marge autour de la carte, pas plus : une cible lointaine ne montrerait que le vide.
        let marge = cote * 0.1;
        let p = [
            p[0].clamp(a[0] - marge, b[0] + marge),
            p[1].clamp(a[1] - marge, b[1] + marge),
        ];
        let h = if projet.carte.manifeste.contient(p) {
            projet.carte.derive().terrain.hauteur(p)
        } else {
            0.0
        };
        orbite.cible = Vec3::new(p[0] as f32, h as f32, p[1] as f32);
        orbite.elevation = 0.0;
        plan.centre = Vec2::new(p[0] as f32, p[1] as f32);
    }
    if let Some(d) = nombre(params, "distance_m")? {
        orbite.distance = d.clamp(5.0, 20_000.0) as f32;
    }
    if let Some(l) = nombre(params, "lacet_deg")? {
        orbite.lacet = (l as f32).to_radians();
    }
    if let Some(t) = nombre(params, "tangage_deg")? {
        orbite.tangage = (t as f32).to_radians().clamp(0.05, 1.55);
    }
    if let Some(m) = nombre(params, "m_par_px")? {
        plan.m_par_px = m.clamp(0.01, cote / 100.0) as f32;
    }
    Ok(json!({
        "disposition": nom_disposition(edition.disposition),
        "plan": { "centre": [arrondi(plan.centre.x.into()), arrondi(plan.centre.y.into())],
                  "m_par_px": arrondi(plan.m_par_px.into()) },
        "3d": {
            "cible": [arrondi(orbite.cible.x.into()), arrondi(orbite.cible.y.into()), arrondi(orbite.cible.z.into())],
            "distance_m": arrondi(orbite.distance.into()),
            "lacet_deg": arrondi(orbite.lacet.to_degrees().into()),
            "tangage_deg": arrondi(orbite.tangage.to_degrees().into()),
        },
    }))
}

/// Vue capturée.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VueCapture {
    Plan,
    Perspective,
    Fenetre,
}

/// Rectangle à découper (pixels physiques), côté maximal de l'image rendue.
fn preparer_capture(
    params: &Value,
    zone: &ZoneVues,
    edition: &Edition,
    echelle: f32,
) -> Result<(Option<URect>, u32, VueCapture), String> {
    let vue = match params.get("vue").and_then(Value::as_str).unwrap_or("3d") {
        "plan" => VueCapture::Plan,
        "3d" | "perspective" => VueCapture::Perspective,
        "fenetre" => VueCapture::Fenetre,
        v => return Err(format!("« vue » : plan, 3d ou fenetre (reçu « {v} »)")),
    };
    let cote = match nombre(params, "taille_max")? {
        None => CAPTURE_DEFAUT_PX,
        Some(t) => (t as u32).clamp(64, CAPTURE_MAX_PX),
    };
    let rect = match vue {
        VueCapture::Fenetre => None,
        VueCapture::Plan | VueCapture::Perspective => {
            let v = if vue == VueCapture::Plan {
                Vue::Plan
            } else {
                Vue::Perspective
            };
            let r = rect_de(v, zone.rect, edition.disposition).ok_or_else(|| {
                format!(
                    "la vue demandée n'est pas affichée (disposition « {} ») : changer la disposition avec mcf.camera",
                    nom_disposition(edition.disposition)
                )
            })?;
            let min = (r.min * echelle).max(Vec2::ZERO).as_uvec2();
            let max = (r.max * echelle).max(Vec2::ZERO).as_uvec2();
            Some(URect::from_corners(min, max))
        }
    };
    Ok((rect, cote, vue))
}

/// Découpe, réduit (moyenne par blocs) et encode en PNG (RVB, sans alpha).
fn encoder_capture(image: &Image, rect: Option<URect>, cote: u32, vue: VueCapture) -> Result<Value, String> {
    use bevy::render::render_resource::TextureFormat;
    let (w, h) = (image.width(), image.height());
    let donnees = image.data.as_deref().ok_or("capture vide")?;
    if donnees.len() != (w as usize) * (h as usize) * 4 {
        return Err(format!(
            "capture de taille inattendue ({} octets pour {w} × {h})",
            donnees.len()
        ));
    }
    let bgra = match image.texture_descriptor.format {
        TextureFormat::Bgra8Unorm | TextureFormat::Bgra8UnormSrgb => true,
        TextureFormat::Rgba8Unorm | TextureFormat::Rgba8UnormSrgb => false,
        f => return Err(format!("format de capture non pris en charge : {f:?}")),
    };
    let r = rect
        .unwrap_or(URect::new(0, 0, w, h))
        .intersect(URect::new(0, 0, w, h));
    let (rw, rh) = (r.width(), r.height());
    if rw == 0 || rh == 0 {
        return Err("zone de capture vide".into());
    }
    let (sw, sh, rgb) = reduire(donnees, w, r, cote, bgra);
    let png = encoder_png(&rgb, sw, sh)?;
    Ok(json!({
        "vue": match vue { VueCapture::Plan => "plan", VueCapture::Perspective => "3d", VueCapture::Fenetre => "fenetre" },
        "largeur": sw,
        "hauteur": sh,
        "source": [rw, rh],
        "png_base64": base64(&png),
    }))
}

/// Réduction par moyenne de blocs (facteur réel, chaque pixel de sortie moyenne sa zone source).
fn reduire(donnees: &[u8], largeur: u32, r: URect, cote: u32, bgra: bool) -> (u32, u32, Vec<u8>) {
    let (rw, rh) = (r.width(), r.height());
    let f = (rw.max(rh) as f32 / cote as f32).max(1.0);
    let sw = ((rw as f32 / f).round() as u32).max(1);
    let sh = ((rh as f32 / f).round() as u32).max(1);
    let mut sortie = Vec::with_capacity((sw * sh * 3) as usize);
    for y in 0..sh {
        let y0 = r.min.y + y * rh / sh;
        let y1 = (r.min.y + (y + 1) * rh / sh).max(y0 + 1);
        for x in 0..sw {
            let x0 = r.min.x + x * rw / sw;
            let x1 = (r.min.x + (x + 1) * rw / sw).max(x0 + 1);
            let mut s = [0u32; 3];
            for yy in y0..y1 {
                for xx in x0..x1 {
                    let i = ((yy * largeur + xx) * 4) as usize;
                    let p = &donnees[i..i + 3];
                    let (rr, g, b) = if bgra {
                        (p[2], p[1], p[0])
                    } else {
                        (p[0], p[1], p[2])
                    };
                    s[0] += u32::from(rr);
                    s[1] += u32::from(g);
                    s[2] += u32::from(b);
                }
            }
            let n = (y1 - y0) * (x1 - x0);
            sortie.extend(s.map(|c| ((c + n / 2) / n) as u8));
        }
    }
    (sw, sh, sortie)
}

fn encoder_png(rgb: &[u8], w: u32, h: u32) -> Result<Vec<u8>, String> {
    let mut sortie = Vec::new();
    let mut e = png::Encoder::new(&mut sortie, w, h);
    e.set_color(png::ColorType::Rgb);
    e.set_depth(png::BitDepth::Eight);
    let mut ecrivain = e.write_header().map_err(|e| e.to_string())?;
    ecrivain.write_image_data(rgb).map_err(|e| e.to_string())?;
    ecrivain.finish().map_err(|e| e.to_string())?;
    Ok(sortie)
}

/// Base64 standard (RFC 4648, avec remplissage).
fn base64(octets: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(octets.len().div_ceil(3) * 4);
    for bloc in octets.chunks(3) {
        let n = (u32::from(bloc[0]) << 16)
            | (u32::from(*bloc.get(1).unwrap_or(&0)) << 8)
            | u32::from(*bloc.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= bloc.len() {
                s.push(T[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                s.push('=');
            }
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_conforme_a_la_rfc() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn comparaison_du_jeton() {
        assert!(egal_temps_constant(b"abc", b"abc"));
        assert!(!egal_temps_constant(b"abc", b"abd"));
        assert!(!egal_temps_constant(b"abc", b"abcd"));
    }

    fn canal() -> (mpsc::Sender<Requete>, mpsc::Receiver<Requete>) {
        mpsc::channel()
    }

    #[test]
    fn requetes_refusees_avant_le_monde() {
        let (tx, rx) = canal();
        let j = "a".repeat(64);
        // Une requête HTTP (page web) n'est pas du JSON.
        let r = traiter("POST / HTTP/1.1", &j, &tx);
        assert!(
            r["erreur"].as_str().is_some_and(|e| e.contains("illisible")),
            "{r}"
        );
        // Mauvais jeton, jeton absent.
        let r = traiter(r#"{"id":1,"jeton":"b","methode":"mcf.etat"}"#, &j, &tx);
        assert_eq!(r["erreur"], "jeton refusé");
        let r = traiter(r#"{"id":2,"methode":"mcf.etat"}"#, &j, &tx);
        assert_eq!(r["erreur"], "jeton refusé");
        // Méthode hors liste (écriture brute de bevy_remote par exemple).
        let r = traiter(
            &format!(r#"{{"id":3,"jeton":"{j}","methode":"world.insert_components"}}"#),
            &j,
            &tx,
        );
        assert!(
            r["erreur"].as_str().is_some_and(|e| e.contains("inconnue")),
            "{r}"
        );
        assert_eq!(r["id"], 3);
        // Rien n'a atteint le monde Bevy.
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn requete_valide_transmise_et_reponse_relayee() {
        let (tx, rx) = canal();
        let j = "c".repeat(64);
        let fil = std::thread::spawn(move || {
            let r = rx.recv().expect("requête transmise");
            assert_eq!(r.methode, "mcf.etat");
            r.reponse.send(Ok(json!({"ok": true}))).expect("réponse");
        });
        let r = traiter(
            &format!(r#"{{"id":7,"jeton":"{j}","methode":"mcf.etat"}}"#),
            &j,
            &tx,
        );
        fil.join().expect("fil");
        assert_eq!(r, json!({"id": 7, "resultat": {"ok": true}}));
    }

    #[test]
    fn reduction_moyenne_et_ordre_des_canaux() {
        // 4 × 2 en BGRA : moitié gauche bleue, moitié droite rouge ; réduit à 2 × 1.
        let mut d = Vec::new();
        for _ in 0..2 {
            for x in 0..4 {
                d.extend(if x < 2 { [255, 0, 0, 255] } else { [0, 0, 255, 255] });
            }
        }
        let (w, h, rgb) = reduire(&d, 4, URect::new(0, 0, 4, 2), 2, true);
        assert_eq!((w, h), (2, 1));
        assert_eq!(rgb, vec![0, 0, 255, 255, 0, 0]);
    }
}
