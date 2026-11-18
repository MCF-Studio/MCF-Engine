//! Style final d'un élément `.uimcf` (déjà validé par le runtime) → composants `bevy_ui`.
//!
//! Aucune chaîne n'est interprétée ici : couleurs, longueurs et mots viennent de types fermés
//! (`mcf_view::StyleValue`).

use std::collections::BTreeMap;

use bevy::prelude::*;
use mcf_view::{Kind, Length, Style, StyleValue};

/// Ce que le style d'un élément change dans `bevy_ui`.
pub struct Computed {
    pub node: Node,
    pub background: Color,
    pub border: Color,
    pub text_color: Color,
    pub font_size: f32,
}

const TEXT: Color = Color::srgb(0.80, 0.84, 0.96);
const BUTTON_BG: Color = Color::srgb(0.19, 0.20, 0.27);
const FIELD_BG: Color = Color::srgb(0.12, 0.12, 0.18);
const BORDER: Color = Color::srgb(0.27, 0.28, 0.35);

fn val(l: Length) -> Val {
    match l {
        Length::Px(n) => Val::Px(n as f32),
        Length::Percent(p) => Val::Percent(p as f32),
    }
}

fn color(c: mcf_view::Color) -> Color {
    Color::srgba_u8(c.r, c.g, c.b, c.a)
}

/// Valeurs par défaut de chaque sorte d'élément (celles de la page d'aperçu HTML).
fn defaults(kind: Kind) -> Computed {
    let mut node = Node {
        flex_direction: FlexDirection::Column,
        ..default()
    };
    let (mut background, mut border) = (Color::NONE, Color::NONE);
    match kind {
        Kind::Window => {
            node.width = Val::Percent(100.0);
            node.height = Val::Percent(100.0);
        }
        Kind::Button => {
            node.padding = UiRect::axes(Val::Px(14.0), Val::Px(6.0));
            node.border = UiRect::all(Val::Px(1.0));
            node.border_radius = BorderRadius::all(Val::Px(6.0));
            node.justify_content = JustifyContent::Center;
            node.align_items = AlignItems::Center;
            background = BUTTON_BG;
            border = BORDER;
        }
        Kind::Input => {
            node.padding = UiRect::axes(Val::Px(10.0), Val::Px(6.0));
            node.border = UiRect::all(Val::Px(1.0));
            node.border_radius = BorderRadius::all(Val::Px(6.0));
            node.min_width = Val::Px(120.0);
            background = FIELD_BG;
            border = BORDER;
        }
        Kind::Box | Kind::Text | Kind::Image => {}
    }
    Computed {
        node,
        background,
        border,
        text_color: TEXT,
        font_size: 16.0,
    }
}

pub fn compute(kind: Kind, styles: &BTreeMap<Style, StyleValue>) -> Computed {
    let mut c = defaults(kind);
    let n = &mut c.node;
    for (style, value) in styles {
        match (style, value) {
            (Style::Width, StyleValue::Length(l)) => n.width = val(*l),
            (Style::Height, StyleValue::Length(l)) => n.height = val(*l),
            (Style::MinWidth, StyleValue::Length(l)) => n.min_width = val(*l),
            (Style::MaxWidth, StyleValue::Length(l)) => n.max_width = val(*l),
            (Style::MinHeight, StyleValue::Length(l)) => n.min_height = val(*l),
            (Style::MaxHeight, StyleValue::Length(l)) => n.max_height = val(*l),
            (Style::Padding, StyleValue::Length(l)) => n.padding = UiRect::all(val(*l)),
            (Style::PaddingTop, StyleValue::Length(l)) => n.padding.top = val(*l),
            (Style::PaddingRight, StyleValue::Length(l)) => n.padding.right = val(*l),
            (Style::PaddingBottom, StyleValue::Length(l)) => n.padding.bottom = val(*l),
            (Style::PaddingLeft, StyleValue::Length(l)) => n.padding.left = val(*l),
            (Style::Margin, StyleValue::Length(l)) => n.margin = UiRect::all(val(*l)),
            (Style::MarginTop, StyleValue::Length(l)) => n.margin.top = val(*l),
            (Style::MarginRight, StyleValue::Length(l)) => n.margin.right = val(*l),
            (Style::MarginBottom, StyleValue::Length(l)) => n.margin.bottom = val(*l),
            (Style::MarginLeft, StyleValue::Length(l)) => n.margin.left = val(*l),
            (Style::Gap, StyleValue::Length(l)) => {
                n.row_gap = val(*l);
                n.column_gap = val(*l);
            }
            (Style::BorderWidth, StyleValue::Length(l)) => {
                n.border = UiRect::all(val(*l));
                if c.border == Color::NONE {
                    c.border = BORDER;
                }
            }
            (Style::Radius, StyleValue::Length(l)) => n.border_radius = BorderRadius::all(val(*l)),
            (Style::FontSize, StyleValue::Length(Length::Px(px))) => c.font_size = *px as f32,
            (Style::X, StyleValue::Length(l)) => {
                n.position_type = PositionType::Absolute;
                n.left = val(*l);
            }
            (Style::Y, StyleValue::Length(l)) => {
                n.position_type = PositionType::Absolute;
                n.top = val(*l);
            }
            (Style::Background, StyleValue::Color(col)) => c.background = color(*col),
            (Style::Color, StyleValue::Color(col)) => c.text_color = color(*col),
            (Style::BorderColor, StyleValue::Color(col)) => c.border = color(*col),
            (Style::Grow, StyleValue::Grow(g)) => n.flex_grow = *g as f32,
            (Style::Visible, StyleValue::Bool(false)) => n.display = Display::None,
            (Style::Layout, StyleValue::Word(w)) => {
                n.flex_direction = if w == "row" {
                    FlexDirection::Row
                } else {
                    // `stack` (enfants superposés) n'est pas encore pris en charge : colonne.
                    FlexDirection::Column
                }
            }
            (Style::Align, StyleValue::Word(w)) => {
                n.align_items = match w.as_str() {
                    "start" => AlignItems::FlexStart,
                    "center" => AlignItems::Center,
                    "end" => AlignItems::FlexEnd,
                    _ => AlignItems::Stretch,
                }
            }
            (Style::Justify, StyleValue::Word(w)) => {
                n.justify_content = match w.as_str() {
                    "center" => JustifyContent::Center,
                    "end" => JustifyContent::FlexEnd,
                    "between" => JustifyContent::SpaceBetween,
                    "around" => JustifyContent::SpaceAround,
                    _ => JustifyContent::FlexStart,
                }
            }
            (Style::Overflow, StyleValue::Word(w)) if w == "hidden" => n.overflow = Overflow::clip(),
            // `fontWeight`, `opacity`, `textAlign` : pas encore pris en charge côté Bevy.
            _ => {}
        }
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_layout_and_gap() {
        let mut s = BTreeMap::new();
        s.insert(Style::Layout, StyleValue::Word("row".into()));
        s.insert(Style::Gap, StyleValue::Length(Length::Px(8)));
        let c = compute(Kind::Box, &s);
        assert_eq!(c.node.flex_direction, FlexDirection::Row);
        assert_eq!(c.node.row_gap, Val::Px(8.0));
    }

    #[test]
    fn named_colors_override_defaults() {
        let mut s = BTreeMap::new();
        let red = mcf_view::Color::parse("#ff0000").expect("couleur valide");
        s.insert(Style::Background, StyleValue::Color(red));
        let c = compute(Kind::Button, &s);
        assert_eq!(c.background, Color::srgba_u8(255, 0, 0, 255));
    }
}
