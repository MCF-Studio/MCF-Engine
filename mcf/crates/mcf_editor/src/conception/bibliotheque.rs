//! Bibliothèque d'assets 3D (bâtiments, arbres, mobilier urbain…) : fenêtre de navigation par section et famille,
//! recherche, vignettes. Catalogue versionné `assets/bibliotheque.json` (écrit par
//! `mcf/tools/bibliotheque/importer.py`, validé par `mcf_carte::bibliotheque`) ; modèles et vignettes copiés dans
//! `assets/bibliotheque/` (non versionnés). Vignettes fabriquées par `cargo editor -- --vignettes` (module
//! [`vignettes`]).
//!
//! Une seule famille est affichée à la fois (jamais tout le catalogue d'un coup) : les vignettes ne sont chargées
//! qu'à l'affichage, au plus quelques-unes par image.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use bevy_egui::{EguiTextureHandle, EguiUserTextures, egui};
use mcf_carte::bibliotheque::{self, Bibliotheque, Place};

/// Dossier des modèles, relatif au dossier `assets` de l'éditeur.
pub const DOSSIER: &str = "bibliotheque";
/// Catalogue, relatif au dossier `assets`.
const FICHIER: &str = "bibliotheque.json";
/// Vignettes, relatives au dossier des modèles.
pub const VIGNETTES: &str = "_vignettes";
/// Résultats de recherche affichés au plus.
const MAX_RESULTATS: usize = 240;
/// Vignettes demandées au chargement par image au plus.
const CHARGEMENTS_PAR_IMAGE: usize = 24;
/// Côté d'une carte d'objet dans la grille (points).
const CARTE: f32 = 118.0;

/// Catalogue chargé au démarrage.
#[derive(Resource, Default)]
pub struct Catalogue {
    /// Catalogue valide (`None` s'il manque ou s'il est refusé).
    pub biblio: Option<Bibliotheque>,
    /// Erreur de lecture.
    pub erreur: Option<String>,
}

/// État de la fenêtre.
#[derive(Resource, Default)]
pub struct Etat {
    /// Fenêtre ouverte.
    pub ouverte: bool,
    section: usize,
    famille: usize,
    recherche: String,
    selection: Option<Place>,
    /// Vignettes connues : identifiant d'objet → texture egui (`None` : pas de vignette sur disque).
    textures: HashMap<String, Option<egui::TextureId>>,
    /// Vignettes demandées par la dernière image de l'interface.
    demandes: Vec<String>,
    /// Section à afficher à la prochaine ouverture (identifiant).
    aller_section: Option<String>,
    /// Replier la fenêtre à la prochaine image (une pose vient d'être armée).
    replier: bool,
}

pub struct BibliothequePlugin;

impl Plugin for BibliothequePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(charger(&racine_assets()))
            .init_resource::<Etat>()
            .add_systems(Update, (basculer, charger_vignettes));
    }
}

/// Dossier `assets` de l'éditeur.
pub fn racine_assets() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("assets")
}

/// Lit le catalogue (borné, fichier ordinaire uniquement).
pub fn charger(assets: &Path) -> Catalogue {
    let chemin = assets.join(FICHIER);
    let lu = (|| {
        let meta = std::fs::symlink_metadata(&chemin).map_err(|e| format!("{} : {e}", chemin.display()))?;
        if !meta.is_file() || meta.len() > bibliotheque::MAX_OCTETS {
            return Err(format!(
                "{} : fichier ordinaire de 8 Mo au plus attendu",
                chemin.display()
            ));
        }
        let texte = std::fs::read_to_string(&chemin).map_err(|e| format!("{} : {e}", chemin.display()))?;
        Bibliotheque::depuis_json(&texte)
    })();
    match lu {
        Ok(b) => Catalogue {
            biblio: Some(b),
            erreur: None,
        },
        Err(e) => {
            warn!("bibliothèque : {e}");
            Catalogue {
                biblio: None,
                erreur: Some(e),
            }
        }
    }
}

/// Chemin d'asset d'une vignette.
pub fn vignette(id: &str) -> String {
    format!("{DOSSIER}/{VIGNETTES}/{id}.png")
}

impl Etat {
    /// Ouvre la fenêtre sur la section d'identifiant `section` (si elle existe ; sinon telle quelle).
    pub fn ouvrir_sur(&mut self, section: &str) {
        self.ouverte = true;
        self.recherche.clear();
        self.aller_section = Some(section.to_owned());
    }
}

/// Bâtiment modélisé (fiche du catalogue des bâtiments) qui correspond à un objet de la bibliothèque : même modèle,
/// d'après son chemin (`props/` en tête et extension `.gltf` / `.glb` mis à part). Un tel objet se pose comme un
/// bâtiment (empreinte, conflits, lotissement), les autres comme du décor.
pub fn fiche_de(o: &bibliotheque::Objet, fiches: &super::modeles::Catalogue) -> Option<usize> {
    let sans = |c: &str| {
        let c = c.strip_prefix("props/").unwrap_or(c);
        c.rsplit_once('.').map_or(c, |(a, _)| a).to_ascii_lowercase()
    };
    let cle = sans(&o.fichier);
    fiches.fiches.iter().position(|f| sans(&f.fichier) == cle)
}

/// Arme la pose de l'objet `p` : bâtiment modélisé s'il a une fiche, décor sinon ; la fenêtre se replie pour
/// laisser voir la carte.
fn armer(
    p: Place,
    b: &Bibliotheque,
    etat: &mut Etat,
    edition: &mut super::Edition,
    fiches: &super::modeles::Catalogue,
) {
    let Some(o) = b.objet(p) else { return };
    edition.selection = super::Selection::Rien;
    edition.outil = match fiche_de(o, fiches) {
        Some(i) => super::Outil::Poser(i),
        None => super::Outil::PoserObjet(p),
    };
    etat.selection = Some(p);
    etat.replier = true;
}

/// Touche B : ouvrir / fermer (sauf pendant une saisie).
fn basculer(touches: Res<ButtonInput<KeyCode>>, zone: Res<super::ZoneVues>, mut etat: ResMut<Etat>) {
    if touches.just_pressed(KeyCode::KeyB) && !zone.clavier_ui {
        etat.ouverte = !etat.ouverte;
    }
}

/// Charge les vignettes demandées par l'interface (quelques-unes par image) et les enregistre auprès d'egui.
fn charger_vignettes(
    assets: Res<AssetServer>,
    mut textures: ResMut<EguiUserTextures>,
    mut etat: ResMut<Etat>,
) {
    if etat.demandes.is_empty() {
        return;
    }
    let racine = racine_assets();
    let demandes = std::mem::take(&mut etat.demandes);
    for id in demandes.into_iter().take(CHARGEMENTS_PAR_IMAGE) {
        if etat.textures.contains_key(&id) {
            continue;
        }
        let chemin = vignette(&id);
        let tex = racine.join(&chemin).is_file().then(|| {
            let h: Handle<Image> = assets.load(chemin);
            textures.add_image(EguiTextureHandle::Strong(h))
        });
        etat.textures.insert(id, tex);
    }
}

/// Texture d'une vignette si elle est prête ; sinon la demande pour une prochaine image.
fn texture(etat: &mut Etat, id: &str) -> Option<egui::TextureId> {
    match etat.textures.get(id) {
        Some(t) => *t,
        None => {
            if !etat.demandes.iter().any(|d| d == id) {
                etat.demandes.push(id.to_owned());
            }
            None
        }
    }
}

/// Bouton de la barre du haut.
pub fn bouton(ui: &mut egui::Ui, etat: &mut Etat) {
    if ui
        .selectable_label(etat.ouverte, "📚 Bibliothèque (B)")
        .on_hover_text("Assets 3D du jeu : bâtiments, arbres, mobilier urbain… par section, avec recherche.")
        .clicked()
    {
        etat.ouverte = !etat.ouverte;
    }
}

/// Fenêtre de la bibliothèque.
pub fn fenetre(
    ctx: &egui::Context,
    catalogue: &Catalogue,
    etat: &mut Etat,
    edition: &mut super::Edition,
    fiches: &super::modeles::Catalogue,
) {
    if !etat.ouverte {
        return;
    }
    if let (Some(id), Some(b)) = (etat.aller_section.take(), &catalogue.biblio)
        && let Some(i) = b.sections.iter().position(|s| s.id == id)
    {
        etat.section = i;
        etat.famille = 0;
    }
    let mut ouverte = true;
    let replier = std::mem::take(&mut etat.replier);
    let mut fenetre = egui::Window::new("📚 Bibliothèque d'assets");
    if replier {
        fenetre = fenetre.open(&mut ouverte).collapsible(true);
        // Repliée : la carte reste visible pendant la pose ; un clic sur la barre la rouvre.
        let id = egui::Id::new("📚 Bibliothèque d'assets");
        if let Some(mut etat_egui) =
            egui::collapsing_header::CollapsingState::load(ctx, id.with("collapsing"))
        {
            etat_egui.set_open(false);
            etat_egui.store(ctx);
        }
    } else {
        fenetre = fenetre.open(&mut ouverte);
    }
    fenetre
        .default_size([1040.0, 660.0])
        .min_size([640.0, 360.0])
        .resizable(true)
        .collapsible(true)
        .show(ctx, |ui| {
            let Some(b) = &catalogue.biblio else {
                ui.colored_label(
                    egui::Color32::from_rgb(230, 80, 70),
                    catalogue.erreur.as_deref().unwrap_or("catalogue absent"),
                );
                ui.label("Importer : python mcf/tools/bibliotheque/importer.py");
                return;
            };
            contenu(ui, b, etat, edition, fiches);
        });
    etat.ouverte = ouverte;
}

fn contenu(
    ui: &mut egui::Ui,
    b: &Bibliotheque,
    etat: &mut Etat,
    edition: &mut super::Edition,
    fiches: &super::modeles::Catalogue,
) {
    // Les panneaux intérieurs se rétrécissent à leur contenu : on garde une fenêtre confortable.
    ui.set_min_size(egui::vec2(980.0, 600.0));
    // Barre de recherche.
    ui.horizontal(|ui| {
        let champ = egui::TextEdit::singleline(&mut etat.recherche)
            .hint_text("🔍 Rechercher : banc, lampadaire, conteneur rouge, chêne…")
            .desired_width(ui.available_width() - 190.0);
        ui.add(champ);
        if !etat.recherche.is_empty() && ui.button("✖ Effacer").clicked() {
            etat.recherche.clear();
        }
        ui.label(egui::RichText::new(format!("{} objets", b.nombre())).weak());
    });
    ui.separator();

    let recherche = etat.recherche.trim().to_owned();
    egui::Panel::left("biblio_sections")
        .resizable(false)
        .default_size(230.0)
        .show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("sections").show(ui, |ui| {
                for (i, s) in b.sections.iter().enumerate() {
                    let n: usize = s.familles.iter().map(|f| f.objets.len()).sum();
                    let actif = recherche.is_empty() && etat.section == i;
                    let texte = format!("{}  {}", s.icone, s.nom);
                    let rep = ui.add_sized(
                        [ui.available_width(), 26.0],
                        egui::Button::selectable(actif, texte)
                            .right_text(egui::RichText::new(n.to_string()).weak()),
                    );
                    if rep.clicked() {
                        etat.section = i;
                        etat.famille = 0;
                        etat.recherche.clear();
                    }
                }
            });
        });
    egui::Panel::right("biblio_fiche")
        .resizable(true)
        .default_size(280.0)
        .size_range(220.0..=420.0)
        .show(ui, |ui| fiche(ui, b, etat, edition, fiches));

    egui::CentralPanel::default().show(ui, |ui| {
        let arme = if recherche.is_empty() {
            famille_courante(ui, b, etat)
        } else {
            resultats(ui, b, etat, &recherche)
        };
        if let Some(p) = arme {
            armer(p, b, etat, edition, fiches);
        }
    });
}

/// Familles de la section courante (une ligne de choix) puis la grille de la famille choisie. Renvoie l'objet
/// double-cliqué (à poser).
fn famille_courante(ui: &mut egui::Ui, b: &Bibliotheque, etat: &mut Etat) -> Option<Place> {
    let s = b.sections.get(etat.section)?;
    ui.heading(format!("{} {}", s.icone, s.nom));
    ui.horizontal_wrapped(|ui| {
        for (j, f) in s.familles.iter().enumerate() {
            let rep = ui.selectable_label(etat.famille == j, format!("{} ({})", f.nom, f.objets.len()));
            if rep.clicked() {
                etat.famille = j;
            }
        }
    });
    ui.separator();
    let f = s.familles.get(etat.famille)?;
    let places: Vec<Place> = (0..f.objets.len())
        .map(|k| (etat.section, etat.famille, k))
        .collect();
    egui::ScrollArea::vertical()
        .id_salt(("famille", etat.section, etat.famille))
        .auto_shrink([false, false])
        .show(ui, |ui| grille(ui, b, etat, &places))
        .inner
}

fn resultats(ui: &mut egui::Ui, b: &Bibliotheque, etat: &mut Etat, recherche: &str) -> Option<Place> {
    let places = b.chercher(recherche, MAX_RESULTATS);
    ui.horizontal(|ui| {
        ui.heading("Résultats");
        let n = places.len();
        ui.label(
            egui::RichText::new(if n >= MAX_RESULTATS {
                format!("{n} premiers : précisez la recherche")
            } else {
                format!("{n} objet(s)")
            })
            .weak(),
        );
    });
    ui.separator();
    if places.is_empty() {
        ui.label("Aucun objet ne correspond. Essayez un autre mot (en français ou en anglais).");
        return None;
    }
    egui::ScrollArea::vertical()
        .id_salt("resultats")
        .auto_shrink([false, false])
        .show(ui, |ui| grille(ui, b, etat, &places))
        .inner
}

/// Grille de cartes (vignette + nom). Renvoie l'objet double-cliqué.
fn grille(ui: &mut egui::Ui, b: &Bibliotheque, etat: &mut Etat, places: &[Place]) -> Option<Place> {
    let largeur = ui.available_width();
    let colonnes = ((largeur / (CARTE + 8.0)).floor() as usize).max(1);
    let mut arme = None;
    egui::Grid::new(("grille", places.first().copied()))
        .spacing([8.0, 8.0])
        .show(ui, |ui| {
            for (n, p) in places.iter().enumerate() {
                let Some(o) = b.objet(*p) else { continue };
                let choisi = etat.selection == Some(*p);
                let tex = texture(etat, &o.id);
                let rep = carte(ui, &o.nom, tex, choisi);
                if rep.clicked() {
                    etat.selection = Some(*p);
                }
                if rep.double_clicked() {
                    arme = Some(*p);
                }
                let famille = &b.sections[p.0].familles[p.1].nom;
                rep.on_hover_text(format!("{}\n{famille}\nDouble-clic : poser sur la carte", o.nom));
                if (n + 1) % colonnes == 0 {
                    ui.end_row();
                }
            }
        });
    arme
}

fn carte(ui: &mut egui::Ui, nom: &str, tex: Option<egui::TextureId>, choisi: bool) -> egui::Response {
    let (rect, rep) = ui.allocate_exact_size(egui::vec2(CARTE, CARTE + 34.0), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let v = ui.visuals();
        let fond = if choisi {
            v.selection.bg_fill
        } else if rep.hovered() {
            v.widgets.hovered.bg_fill
        } else {
            v.extreme_bg_color
        };
        let p = ui.painter();
        p.rect_filled(rect, 6.0, fond);
        let image = egui::Rect::from_min_size(
            rect.min + egui::vec2(4.0, 4.0),
            egui::vec2(CARTE - 8.0, CARTE - 8.0),
        );
        match tex {
            Some(t) => {
                p.image(
                    t,
                    image,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }
            None => {
                p.rect_filled(image, 4.0, v.faint_bg_color);
                p.text(
                    image.center(),
                    egui::Align2::CENTER_CENTER,
                    "📦",
                    egui::FontId::proportional(30.0),
                    v.weak_text_color(),
                );
            }
        }
        let texte = egui::Rect::from_min_max(
            egui::pos2(rect.min.x + 4.0, rect.min.y + CARTE),
            rect.max - egui::vec2(4.0, 2.0),
        );
        let couleur = if choisi {
            v.selection.stroke.color
        } else {
            v.text_color()
        };
        let galerie = p.layout(
            nom.to_owned(),
            egui::FontId::proportional(11.5),
            couleur,
            texte.width(),
        );
        p.with_clip_rect(texte).galley(texte.min, galerie, couleur);
    }
    rep
}

/// Fiche de l'objet choisi.
fn fiche(
    ui: &mut egui::Ui,
    b: &Bibliotheque,
    etat: &mut Etat,
    edition: &mut super::Edition,
    fiches: &super::modeles::Catalogue,
) {
    let Some(p) = etat.selection else {
        ui.add_space(12.0);
        ui.label(egui::RichText::new("Choisissez un objet pour voir sa fiche.").weak());
        return;
    };
    let Some(o) = b.objet(p) else { return };
    let (s, f) = (&b.sections[p.0], &b.sections[p.0].familles[p.1]);
    let cote = ui.available_width().min(300.0);
    let tex = texture(etat, &o.id);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(cote, cote), egui::Sense::hover());
    match tex {
        Some(t) => {
            ui.painter().image(
                t,
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
        None => {
            ui.painter().rect_filled(rect, 6.0, ui.visuals().extreme_bg_color);
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "pas encore de vignette\ncargo editor -- --vignettes",
                egui::FontId::proportional(12.0),
                ui.visuals().weak_text_color(),
            );
        }
    }
    ui.add_space(6.0);
    ui.heading(&o.nom);
    ui.label(format!("{} {} › {}", s.icone, s.nom, f.nom));
    ui.add_space(6.0);
    egui::Grid::new("fiche_details").num_columns(2).show(ui, |ui| {
        ui.label(egui::RichText::new("Identifiant").weak());
        ui.monospace(&o.id);
        ui.end_row();
        ui.label(egui::RichText::new("Fichier").weak());
        ui.add(egui::Label::new(egui::RichText::new(&o.fichier).monospace().small()).wrap());
        ui.end_row();
    });
    if !o.mots.is_empty() {
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(format!("Mots-clés : {}", o.mots.join(", ")))
                .small()
                .weak(),
        );
    }
    ui.add_space(8.0);
    let batiment = fiche_de(o, fiches).and_then(|i| fiches.fiches.get(i));
    let present = racine_assets().join(DOSSIER).join(&o.fichier).is_file();
    let texte = match batiment {
        Some(_) => "✋ Poser ce bâtiment",
        None => "✋ Poser sur la carte",
    };
    let bouton = ui.add_enabled(
        present,
        egui::Button::new(egui::RichText::new(texte).strong())
            .min_size(egui::vec2(ui.available_width(), 32.0)),
    );
    let bouton = match batiment {
        Some(f) => bouton.on_hover_text(format!(
            "Bâtiment modélisé « {} » : posé avec son empreinte ({:.0} × {:.0} m), vu par les routes et le lotissement.",
            f.nom, f.largeur_m, f.profondeur_m
        )),
        None => bouton.on_hover_text("Décor : clic sur la carte pour poser, R pour tourner, Échap pour finir."),
    };
    let bouton = if present {
        bouton
    } else {
        bouton.on_disabled_hover_text("Modèle absent du dossier assets/bibliotheque (importer.py).")
    };
    if bouton.clicked() {
        armer(p, b, etat, edition, fiches);
    }
    if ui.small_button("📋 Copier l'identifiant").clicked() {
        ui.ctx().copy_text(o.id.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Chaque bâtiment modélisé du catalogue se retrouve dans la bibliothèque (il s'y pose avec son empreinte), et
    /// un objet de décor n'est pas pris pour un bâtiment.
    #[test]
    fn batiments_du_catalogue_reconnus_dans_la_bibliotheque() {
        let b = charger(&racine_assets()).biblio.expect("bibliothèque");
        let fiches = super::super::modeles::charger(&super::super::modeles::dossier());
        let tous: Vec<&bibliotheque::Objet> = b
            .sections
            .iter()
            .flat_map(|s| &s.familles)
            .flat_map(|f| &f.objets)
            .collect();
        for (i, f) in fiches.fiches.iter().enumerate() {
            assert!(
                tous.iter().any(|o| fiche_de(o, &fiches) == Some(i)),
                "{} introuvable dans la bibliothèque",
                f.id
            );
        }
        let banc = b.chercher("banc", 1);
        let o = b.objet(banc[0]).expect("banc");
        assert_eq!(fiche_de(o, &fiches), None);
    }
}

/// Fabrique les vignettes manquantes : chaque modèle est chargé seul, cadré de trois quarts face (façade = +Z),
/// photographié, puis retiré. `cargo editor -- --vignettes [--forcer]`.
pub mod vignettes {
    use std::collections::VecDeque;

    use bevy::{
        camera::primitives::Aabb,
        math::ops,
        prelude::*,
        render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk},
        window::WindowResolution,
        world_serialization::WorldInstanceReady,
    };

    use super::{DOSSIER, charger, racine_assets, vignette};

    /// Côté des vignettes (pixels).
    const COTE: u32 = 256;
    /// Images d'attente au plus pour un modèle (chargement, textures).
    const ATTENTE_MAX: u32 = 600;

    #[derive(Resource)]
    struct File {
        restants: VecDeque<(String, String)>,
        total: usize,
        faites: usize,
        echecs: Vec<String>,
        courant: Option<Courant>,
    }

    struct Courant {
        id: String,
        racine: Entity,
        scene: bevy::asset::UntypedHandle,
        images: u32,
        pret: bool,
        cadre: Option<u32>,
        capture: bool,
        essais: u32,
    }

    #[derive(Component)]
    struct Pret;

    #[derive(Component)]
    struct Fini;

    /// Photo uniforme (matériaux ou textures pas encore prêts) : à reprendre.
    #[derive(Component)]
    struct Reessayer;

    /// Photos uniformes au plus avant d'abandonner un modèle.
    const ESSAIS_MAX: u32 = 40;

    /// Lance la fabrication et rend la main à la fin.
    pub fn lancer(forcer: bool) -> AppExit {
        let racine = racine_assets();
        let cat = charger(&racine);
        let Some(b) = cat.biblio else {
            eprintln!("bibliothèque : {}", cat.erreur.unwrap_or_default());
            return AppExit::error();
        };
        let _ = std::fs::create_dir_all(racine.join(DOSSIER).join(super::VIGNETTES));
        let restants: VecDeque<(String, String)> = b
            .sections
            .iter()
            .flat_map(|s| &s.familles)
            .flat_map(|f| &f.objets)
            .filter(|o| forcer || !racine.join(vignette(&o.id)).is_file())
            .filter(|o| racine.join(DOSSIER).join(&o.fichier).is_file())
            .map(|o| (o.id.clone(), format!("{DOSSIER}/{}", o.fichier)))
            .collect();
        println!("{} vignette(s) à fabriquer", restants.len());
        if restants.is_empty() {
            return AppExit::Success;
        }
        App::new()
            .add_plugins(DefaultPlugins.set(WindowPlugin {
                primary_window: Some(Window {
                    title: "MCF Editor — vignettes de la bibliothèque".into(),
                    resolution: WindowResolution::new(COTE, COTE).with_scale_factor_override(1.0),
                    resizable: false,
                    ..default()
                }),
                ..default()
            }))
            .insert_resource(ClearColor(Color::srgb(0.17, 0.19, 0.22)))
            .insert_resource(File {
                total: restants.len(),
                restants,
                faites: 0,
                echecs: Vec::new(),
                courant: None,
            })
            .add_systems(Startup, scene)
            .add_systems(Update, avancer)
            .run()
    }

    fn scene(mut commands: Commands) {
        commands.spawn((
            Camera3d::default(),
            Projection::Perspective(PerspectiveProjection {
                fov: 30f32.to_radians(),
                ..default()
            }),
            Transform::from_xyz(4.0, 3.0, 6.0).looking_at(Vec3::ZERO, Vec3::Y),
        ));
        commands.spawn((
            DirectionalLight {
                illuminance: 9_000.0,
                shadow_maps_enabled: true,
                ..default()
            },
            Transform::from_xyz(3.0, 6.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y),
        ));
        commands.spawn((
            DirectionalLight {
                illuminance: 2_500.0,
                ..default()
            },
            Transform::from_xyz(-4.0, 2.0, -3.0).looking_at(Vec3::ZERO, Vec3::Y),
        ));
        commands.insert_resource(GlobalAmbientLight {
            brightness: 600.0,
            ..default()
        });
    }

    fn marquer_pret(
        pret: On<WorldInstanceReady>,
        mut commands: Commands,
        enfants: Query<&Children>,
        noms: Query<&Name>,
    ) {
        let racine = pret.event_target();
        for e in enfants.iter_descendants(racine) {
            if noms.get(e).is_ok_and(|n| n.as_str().contains("-col")) {
                commands.entity(e).insert(Visibility::Hidden);
            }
        }
        commands.entity(racine).insert(Pret);
    }

    #[allow(clippy::too_many_arguments)]
    fn avancer(
        mut commands: Commands,
        assets: Res<AssetServer>,
        mut file: ResMut<File>,
        prets: Query<(), With<Pret>>,
        finis: Query<(), With<Fini>>,
        a_reprendre: Query<(), With<Reessayer>>,
        enfants: Query<&Children>,
        boites: Query<(&GlobalTransform, &Aabb, &InheritedVisibility)>,
        mut camera: Query<&mut Transform, With<Camera3d>>,
        mut sortie: MessageWriter<AppExit>,
    ) {
        let racine_disque = racine_assets();
        let file = &mut *file;
        let Some(c) = &mut file.courant else {
            let Some((id, chemin)) = file.restants.pop_front() else {
                println!(
                    "terminé : {} vignette(s), {} échec(s){}",
                    file.faites,
                    file.echecs.len(),
                    if file.echecs.is_empty() {
                        String::new()
                    } else {
                        format!(" : {}", file.echecs.join(", "))
                    }
                );
                sortie.write(AppExit::Success);
                return;
            };
            let scene = assets.load(GltfAssetLabel::Scene(0).from_asset(chemin));
            let racine = commands
                .spawn(WorldAssetRoot(scene.clone()))
                .observe(marquer_pret)
                .id();
            file.courant = Some(Courant {
                id,
                racine,
                scene: scene.untyped(),
                images: 0,
                pret: false,
                cadre: None,
                capture: false,
                essais: 0,
            });
            return;
        };
        c.images += 1;
        c.pret |= prets.contains(c.racine);
        let abandon = c.images > ATTENTE_MAX;
        if c.capture && a_reprendre.contains(c.racine) {
            commands.entity(c.racine).remove::<Reessayer>();
            c.essais += 1;
            if c.essais > ESSAIS_MAX {
                file.echecs.push(c.id.clone());
                commands.entity(c.racine).despawn();
                file.courant = None;
            } else {
                c.capture = false;
                c.cadre = Some(c.images);
            }
            return;
        }
        if c.capture {
            if finis.contains(c.racine) || abandon {
                if abandon {
                    file.echecs.push(c.id.clone());
                } else {
                    file.faites += 1;
                }
                commands.entity(c.racine).despawn();
                file.courant = None;
                if file.faites.is_multiple_of(50) {
                    println!("{} / {}", file.faites, file.total);
                }
            }
            return;
        }
        if !c.pret {
            if abandon {
                file.echecs.push(c.id.clone());
                commands.entity(c.racine).despawn();
                file.courant = None;
            }
            return;
        }
        // Cadrage : boîte englobante des maillages visibles (quelques images après l'instanciation, le temps que
        // les boîtes soient calculées et les textures chargées).
        if c.cadre.is_none() {
            let (mut min, mut max) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
            for e in enfants.iter_descendants(c.racine) {
                let Ok((gt, aabb, vis)) = boites.get(e) else {
                    continue;
                };
                if !vis.get() {
                    continue;
                }
                let (centre, demi) = (Vec3::from(aabb.center), Vec3::from(aabb.half_extents));
                for sx in [-1.0, 1.0] {
                    for sy in [-1.0, 1.0] {
                        for sz in [-1.0, 1.0] {
                            let p = gt.transform_point(centre + demi * Vec3::new(sx, sy, sz));
                            min = min.min(p);
                            max = max.max(p);
                        }
                    }
                }
            }
            if min.x > max.x {
                if c.images > 120 {
                    file.echecs.push(c.id.clone());
                    commands.entity(c.racine).despawn();
                    file.courant = None;
                }
                return;
            }
            let centre = (min + max) / 2.0;
            debug!("vignette {} : boîte {min} → {max}", c.id);
            let rayon = ((max - min).length() / 2.0).max(0.05);
            let distance = rayon / ops::sin(15f32.to_radians()) * 1.05;
            let direction = Vec3::new(0.75, 0.55, 1.0).normalize();
            if let Ok(mut t) = camera.single_mut() {
                *t = Transform::from_translation(centre + direction * distance).looking_at(centre, Vec3::Y);
            }
            c.cadre = Some(c.images);
            return;
        }
        // Quelques images pour les textures, puis la photo.
        if c.images >= c.cadre.unwrap_or(0) + 8 && assets.is_loaded_with_dependencies(c.scene.id()) {
            let racine = c.racine;
            let chemin = racine_disque.join(vignette(&c.id));
            commands.spawn(Screenshot::primary_window()).observe(
                move |photo: On<ScreenshotCaptured>, mut commands: Commands| {
                    // Image uniforme : rien n'est encore dessiné (matériau en attente de ses textures).
                    let vide =
                        photo.image.data.as_ref().is_none_or(|d| {
                            d.len() < 4 || d.as_chunks::<4>().0.iter().all(|p| p[..] == d[..4])
                        });
                    if vide {
                        commands.entity(racine).insert(Reessayer);
                    } else {
                        save_to_disk(&chemin)(photo);
                        commands.entity(racine).insert(Fini);
                    }
                },
            );
            c.capture = true;
        }
    }
}
