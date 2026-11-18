//! Pont MCF Engine ↔ langage MCF : affiche une interface `.uimcf` avec `bevy_ui`.
//!
//! ```text
//! dossier .mcf / .uimcf ──► vérification (compilateur MCF) ──► ViewSession (runtime MCF)
//!                                                                   │ arbre de vue
//!                                         mcf_view::Renderer ◄──────┘
//!                                                │ liste de changements
//!                            entités bevy_ui ◄───┘            ▲
//!                                  │ clic, saisie             │
//!                                  └──────── événements ──────┘
//! ```
//!
//! Règles (voir `mcf/lang/README.md` et `docs/cibles/affichage.md` du dépôt MyLangage) :
//! - les `.mcf` / `.uimcf` sont **non fiables** : ils n'affichent que ce que leur arbre décrit et ne
//!   déclenchent que les fonctions de leur propre projet ;
//! - les valeurs d'affichage arrivent déjà typées et validées : rien n'est interprété ici ;
//! - un événement pour un élément retiré est ignoré (identifiants jamais réutilisés).
//!
//! **Expérimental.** Modèle d'exécution provisoire (D69 ouverte) : chaque événement s'exécute
//! jusqu'au bout, dans le thread de rendu. Licence du compilateur (D67) à trancher avant toute
//! distribution.

mod project;
mod style;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;

use bevy::input_focus::InputFocus;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::text::{EditableText, TextCursorStyle, TextEditChange};
use bevy::ui_widgets::{Activate, Button, TextInput};
use mcf_runtime::ViewSession;
use mcf_view::{Change, Event, Kind, NodeId, Prop, Renderer, Style, StyleValue};

/// Affiche le composant `component` du projet `project` (dossier de `.mcf` / `.uimcf`).
pub struct McfLangPlugin {
    /// Dossier du projet.
    pub project: PathBuf,
    /// Nom du composant `ui` à afficher.
    pub component: String,
    /// Police (chemin d'asset) ; `None` : police par défaut de Bevy (sans accents).
    pub font: Option<String>,
}

impl Plugin for McfLangPlugin {
    fn build(&self, app: &mut App) {
        match project::open(&self.project, &self.component) {
            Ok(session) => {
                app.insert_resource(McfUi::new(session, self.font.clone()));
            }
            Err(message) => {
                error!("interface MCF non chargée :\n{message}");
                return;
            }
        }
        app.add_systems(Startup, load_font)
            .add_systems(Update, (submit_on_enter, process, apply_input_values).chain())
            .add_observer(on_activate)
            .add_observer(on_text_change);
    }
}

/// Numéro de l'élément `mcf_view` porté par une entité.
#[derive(Component, Clone, Copy)]
struct McfNode(NodeId);

struct Elem {
    entity: Entity,
    kind: Kind,
    props: BTreeMap<Prop, String>,
    style: BTreeMap<Style, StyleValue>,
    /// `Button` : l'entité du texte.
    label: Option<Entity>,
}

#[derive(Resource)]
struct McfUi {
    session: ViewSession<'static>,
    renderer: Renderer,
    elems: HashMap<NodeId, Elem>,
    pending: Vec<(NodeId, Event, Option<String>)>,
    input_updates: Vec<(Entity, String, u8)>,
    dirty: bool,
    font_path: Option<String>,
    font: Option<Handle<Font>>,
}

impl McfUi {
    fn new(session: ViewSession<'static>, font_path: Option<String>) -> Self {
        McfUi {
            session,
            renderer: Renderer::new(),
            elems: HashMap::new(),
            pending: Vec::new(),
            input_updates: Vec::new(),
            dirty: true,
            font_path,
            font: None,
        }
    }

    fn text_font(&self, size: f32) -> TextFont {
        let mut f = TextFont {
            font_size: FontSize::Px(size),
            ..default()
        };
        if let Some(h) = &self.font {
            f.font = h.clone().into();
        }
        f
    }

    /// Applique la liste de changements, dans l'ordre, puis rafraîchit les éléments touchés.
    fn apply(&mut self, commands: &mut Commands, changes: Vec<Change>) {
        let mut touched: Vec<NodeId> = Vec::new();
        let mut seen: HashSet<NodeId> = HashSet::new();
        let mut touch = |id: NodeId, touched: &mut Vec<NodeId>| {
            if seen.insert(id) {
                touched.push(id);
            }
        };
        for change in changes {
            match change {
                Change::Create {
                    id,
                    parent,
                    index,
                    kind,
                } => {
                    let entity = commands.spawn((Node::default(), McfNode(id))).id();
                    let mut label = None;
                    match kind {
                        Kind::Button => {
                            commands.entity(entity).insert((Button, Hovered::default()));
                            label = Some(commands.spawn(Text::new("")).id());
                            if let Some(l) = label {
                                commands.entity(entity).add_child(l);
                            }
                        }
                        Kind::Text | Kind::Image => {
                            commands.entity(entity).insert(Text::new(""));
                        }
                        Kind::Input => {
                            commands.entity(entity).insert((
                                TextInput,
                                EditableText::new(""),
                                TextLayout::no_wrap(),
                                TextCursorStyle::default(),
                            ));
                        }
                        Kind::Window | Kind::Box => {}
                    }
                    if let Some(p) = parent
                        && let Some(pe) = self.elems.get(&p).map(|e| e.entity)
                    {
                        commands.entity(pe).insert_children(index, &[entity]);
                    }
                    self.elems.insert(
                        id,
                        Elem {
                            entity,
                            kind,
                            props: BTreeMap::new(),
                            style: BTreeMap::new(),
                            label,
                        },
                    );
                    touch(id, &mut touched);
                }
                Change::Remove { id, subtree } => {
                    if let Some(e) = self.elems.get(&id) {
                        commands.entity(e.entity).despawn();
                    }
                    for gone in subtree {
                        self.elems.remove(&gone);
                    }
                }
                Change::SetProp { id, prop, value } => {
                    if let Some(e) = self.elems.get_mut(&id) {
                        if prop == Prop::Value && e.kind == Kind::Input {
                            self.input_updates.push((e.entity, value.clone(), 0));
                        }
                        e.props.insert(prop, value);
                        touch(id, &mut touched);
                    }
                }
                Change::RemoveProp { id, prop } => {
                    if let Some(e) = self.elems.get_mut(&id) {
                        e.props.remove(&prop);
                        touch(id, &mut touched);
                    }
                }
                Change::SetStyle { id, style, value } => {
                    if let Some(e) = self.elems.get_mut(&id) {
                        e.style.insert(style, value);
                        touch(id, &mut touched);
                    }
                }
                Change::RemoveStyle { id, style } => {
                    if let Some(e) = self.elems.get_mut(&id) {
                        e.style.remove(&style);
                        touch(id, &mut touched);
                    }
                }
                Change::Listen { id, event } => {
                    if let Some(e) = self.elems.get(&id) {
                        // Un élément cliquable devient un `Button` (focus, état pressé, `Activate`).
                        if event == Event::Click && e.kind != Kind::Button {
                            commands.entity(e.entity).insert((Button, Hovered::default()));
                        }
                    }
                }
                Change::Unlisten { id, event } => {
                    if let Some(e) = self.elems.get(&id)
                        && event == Event::Click
                        && e.kind != Kind::Button
                    {
                        commands.entity(e.entity).remove::<(Button, Hovered)>();
                    }
                }
            }
        }
        for id in touched {
            if let Some(e) = self.elems.get(&id) {
                self.refresh(commands, e);
            }
        }
    }

    /// Recalcule les composants d'affichage d'un élément à partir de ses propriétés et styles.
    fn refresh(&self, commands: &mut Commands, e: &Elem) {
        let c = style::compute(e.kind, &e.style);
        let mut entity = commands.entity(e.entity);
        entity.insert((c.node, BackgroundColor(c.background), BorderColor::all(c.border)));
        let text = |p: Prop| e.props.get(&p).cloned().unwrap_or_default();
        match e.kind {
            Kind::Text => {
                entity.insert((
                    Text::new(text(Prop::Text)),
                    TextColor(c.text_color),
                    self.text_font(c.font_size),
                ));
            }
            Kind::Image => {
                // Les images ne sont pas encore chargées : un texte tient lieu de cadre.
                let alt = text(Prop::Alt);
                let shown = if alt.is_empty() { text(Prop::Source) } else { alt };
                entity.insert((
                    Text::new(format!("[{shown}]")),
                    TextColor(c.text_color),
                    self.text_font(c.font_size),
                ));
            }
            Kind::Input => {
                entity.insert((TextColor(c.text_color), self.text_font(c.font_size)));
            }
            Kind::Button => {
                if let Some(label) = e.label {
                    commands.entity(label).insert((
                        Text::new(text(Prop::Label)),
                        TextColor(c.text_color),
                        self.text_font(c.font_size),
                    ));
                }
            }
            Kind::Window | Kind::Box => {}
        }
    }
}

fn load_font(mut ui: ResMut<McfUi>, assets: Res<AssetServer>) {
    if let Some(path) = ui.font_path.clone() {
        ui.font = Some(assets.load(path));
    }
}

/// Clic sur un bouton : l'événement est transmis au runtime à la prochaine mise à jour.
fn on_activate(ev: On<Activate>, nodes: Query<&McfNode>, mut ui: ResMut<McfUi>) {
    if let Ok(node) = nodes.get(ev.entity) {
        ui.pending.push((node.0, Event::Click, None));
    }
}

/// Saisie dans un champ : le texte est transmis s'il a changé (le curseur seul ne compte pas).
fn on_text_change(ev: On<TextEditChange>, inputs: Query<(&McfNode, &EditableText)>, mut ui: ResMut<McfUi>) {
    let Ok((node, text)) = inputs.get(ev.event_target()) else {
        return;
    };
    let value = text.value().to_string();
    let Some(elem) = ui.elems.get_mut(&node.0) else {
        return;
    };
    if elem.props.get(&Prop::Value) != Some(&value) {
        elem.props.insert(Prop::Value, value.clone());
        ui.pending.push((node.0, Event::Change, Some(value)));
    }
}

/// Touche Entrée dans un champ : `onSubmit`.
fn submit_on_enter(
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<InputFocus>,
    inputs: Query<(&McfNode, &EditableText)>,
    mut ui: ResMut<McfUi>,
) {
    if !keys.just_pressed(KeyCode::Enter) {
        return;
    }
    if let Some(e) = focus.get()
        && let Ok((node, text)) = inputs.get(e)
    {
        ui.pending
            .push((node.0, Event::Submit, Some(text.value().to_string())));
    }
}

/// Traite les événements reçus, puis recalcule la vue et applique les changements.
fn process(mut ui: ResMut<McfUi>, mut commands: Commands) {
    let ui = &mut *ui;
    for (id, event, text) in std::mem::take(&mut ui.pending) {
        // Élément déjà retiré : l'événement est ignoré.
        let Some(path) = ui.renderer.path(id).map(<[usize]>::to_vec) else {
            continue;
        };
        match ui.session.dispatch(&path, event, text.as_deref()) {
            Ok(handled) => ui.dirty |= handled,
            Err(e) => error!("{} : {}", e.diagnostic.code.id, e.diagnostic.message),
        }
    }
    let printed = ui.session.take_output();
    if !printed.is_empty() {
        info!("{}", printed.trim_end());
    }
    if !ui.dirty {
        return;
    }
    ui.dirty = false;
    match ui.session.view() {
        Ok(tree) => match ui.renderer.update(Some(tree)) {
            Ok(changes) => ui.apply(&mut commands, changes),
            Err(e) => error!("vue refusée : {} éléments (MCF0416)", e.count),
        },
        Err(e) => error!("{} : {}", e.diagnostic.code.id, e.diagnostic.message),
    }
}

/// Reporte dans les champs la valeur calculée par la vue, si elle diffère de ce qui est saisi
/// (sinon le curseur sauterait pendant la frappe).
fn apply_input_values(mut ui: ResMut<McfUi>, mut inputs: Query<&mut EditableText>) {
    let updates = std::mem::take(&mut ui.input_updates);
    for (entity, value, tries) in updates {
        match inputs.get_mut(entity) {
            Ok(mut text) => {
                if text.value().to_string() != value {
                    text.editor_mut().set_text(&value);
                }
            }
            // L'entité vient d'être créée : le composant arrive à la fin de l'image.
            Err(_) if tries < 5 => ui.input_updates.push((entity, value, tries + 1)),
            Err(_) => {}
        }
    }
}
