//! Contrôles de texte communs (chemins, caractères invisibles, hexadécimal, couleurs).

/// Le texte contient-il quelque chose qui ressemble à un chemin ou à une URL ?
pub(crate) fn has_path(s: &str) -> bool {
    let t = s.to_lowercase();
    t.contains("res://")
        || t.contains("user://")
        || t.contains("://")
        || t.contains("..")
        || t.contains('\\')
        || t.contains(":/")
}

/// Caractère de contrôle ou invisible ?
pub(crate) fn has_control(s: &str) -> bool {
    s.chars().any(|c| (c as u32) < 32 || is_invisible(c as u32))
}

/// Caractères invisibles ou qui changent le sens d'affichage (un texte de panneau pourrait tromper le relecteur).
pub(crate) fn is_invisible(c: u32) -> bool {
    (0x7F..=0x9F).contains(&c)
        || (0x200B..=0x200F).contains(&c)
        || (0x202A..=0x202E).contains(&c)
        || (0x2060..=0x206F).contains(&c)
        || c == 0xFEFF
        || (0xFFF9..=0xFFFB).contains(&c)
        || (0xE0000..=0xE007F).contains(&c)
}

/// Hexadécimal minuscule de `min..=max` caractères.
pub(crate) fn is_hex(s: &str, min: usize, max: usize) -> bool {
    let n = s.chars().count();
    (min..=max).contains(&n) && s.chars().all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
}

/// Couleur HTML valide au sens de Godot (`String.is_valid_html_color`) : `#` facultatif, 3, 4, 6 ou 8 chiffres
/// hexadécimaux.
pub(crate) fn is_html_color(s: &str) -> bool {
    let h = s.strip_prefix('#').unwrap_or(s);
    matches!(h.len(), 3 | 4 | 6 | 8) && h.chars().all(|c| c.is_ascii_hexdigit())
}

/// Les 40 premiers caractères (pour citer une valeur fautive sans recopier un texte énorme).
pub(crate) fn clip(s: &str) -> String {
    s.chars().take(40).collect()
}

/// Nombre de caractères (comme `String.length()` de Godot).
pub(crate) fn len(s: &str) -> usize {
    s.chars().count()
}

/// Profondeur maximale des crochets / accolades (hors chaînes), arrêtée dès que `stop` est atteint.
pub(crate) fn nesting_depth(text: &str, stop: usize) -> usize {
    let mut depth: usize = 0;
    let mut deepest = 0;
    let mut in_string = false;
    let mut escaped = false;
    for b in text.bytes() {
        if in_string {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_string = false;
            }
        } else if b == b'"' {
            in_string = true;
        } else if b == b'[' || b == b'{' {
            depth += 1;
            if depth > deepest {
                deepest = depth;
                if deepest >= stop {
                    return deepest;
                }
            }
        } else if b == b']' || b == b'}' {
            depth = depth.saturating_sub(1);
        }
    }
    deepest
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chemins_detectes() {
        for s in ["res://x", "USER://x", "http://x", "a/../b", "a\\b", "c:/x"] {
            assert!(has_path(s), "{s}");
        }
        assert!(!has_path("2026-11-20T18:00:00+01:00"));
        assert!(!has_path("Ville – mobilier"));
    }

    #[test]
    fn invisibles_detectes() {
        assert!(has_control("a\u{202E}b"));
        assert!(has_control("a\u{200B}b"));
        assert!(has_control("a\nb"));
        assert!(!has_control("Boulangerie Éloïse"));
    }

    #[test]
    fn couleurs() {
        for ok in ["#fff", "ffff", "#a1b2c3", "A1B2C3FF"] {
            assert!(is_html_color(ok), "{ok}");
        }
        for ko in ["#ff", "#ggg", "#12345", "rouge"] {
            assert!(!is_html_color(ko), "{ko}");
        }
    }

    #[test]
    fn hex_minuscule() {
        assert!(is_hex("c1f4a2b3", 8, 32));
        assert!(!is_hex("C1F4A2B3", 8, 32));
        assert!(!is_hex("c1f4", 8, 32));
    }

    #[test]
    fn profondeur() {
        assert_eq!(nesting_depth(r#"{"a":[[1]]}"#, 99), 3);
        assert_eq!(nesting_depth(r#"{"a":"[[[[["}"#, 99), 1);
        assert_eq!(nesting_depth(r#"{"a":"\"[[["}"#, 99), 1);
    }
}
