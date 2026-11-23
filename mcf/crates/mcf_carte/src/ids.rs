//! Identifiants stables (64 bits, écrits en 16 caractères hexadécimaux) et générateur pseudo-aléatoire déterministe.

use std::fmt;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Identifiant d'un élément de carte. Aléatoire à la création (deux auteurs qui travaillent sur des branches Git
/// différentes n'entrent pas en collision), stable ensuite.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Id(pub u64);

/// Pas du compteur d'identifiants (impair : `n ↦ n × PAS` est une bijection sur 64 bits).
const PAS: u64 = 0x9E37_79B9_7F4A_7C15;

/// Finaliseur de SplitMix64 : bijection sur 64 bits.
fn finaliser(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

impl Id {
    /// Nouvel identifiant. Une graine tirée une fois par processus (horloge + adresse) est combinée à un compteur,
    /// puis passée dans une bijection : **deux appels d'un même processus ne rendent jamais le même identifiant**,
    /// même après suppression de l'élément (aucune réutilisation). Entre deux sessions ou deux branches Git, la
    /// graine diffère : collision possible mais improbable (64 bits aléatoires).
    pub fn nouveau() -> Self {
        static GRAINE: OnceLock<u64> = OnceLock::new();
        static COMPTEUR: AtomicU64 = AtomicU64::new(0);
        let graine = *GRAINE.get_or_init(|| {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0);
            let pile = &nanos as *const u64 as u64;
            finaliser(nanos ^ pile.rotate_left(32))
        });
        loop {
            let n = COMPTEUR.fetch_add(1, Ordering::Relaxed);
            let v = finaliser(graine.wrapping_add(n.wrapping_mul(PAS)));
            // 0 est réservé (identifiant « nul ») : on passe au suivant plutôt que de le remplacer par une valeur
            // qui pourrait déjà être sortie.
            if v != 0 {
                return Self(v);
            }
        }
    }
}

impl fmt::Debug for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:016x}", self.0)
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:016x}", self.0)
    }
}

impl From<Id> for String {
    fn from(id: Id) -> Self {
        id.to_string()
    }
}

impl TryFrom<String> for Id {
    type Error = String;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        if s.len() != 16
            || !s
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err("identifiant attendu : 16 caractères hexadécimaux minuscules".into());
        }
        u64::from_str_radix(&s, 16)
            .map(Id)
            .map_err(|_| "identifiant illisible".into())
    }
}

/// Pas de SplitMix64.
pub fn melanger(etat: &mut u64) -> u64 {
    *etat = etat.wrapping_add(PAS);
    finaliser(*etat)
}

/// Générateur déterministe : même graine, même suite (lotissement, relief naturel).
#[derive(Debug, Clone)]
pub struct Alea(u64);

impl Alea {
    /// Générateur initialisé.
    pub fn new(graine: u64) -> Self {
        Self(graine)
    }

    /// Entier suivant.
    pub fn suivant(&mut self) -> u64 {
        melanger(&mut self.0)
    }

    /// Réel dans `[0, 1)`.
    pub fn unite(&mut self) -> f64 {
        (self.suivant() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Réel dans `[a, b]`.
    pub fn entre(&mut self, a: f64, b: f64) -> f64 {
        a + (b - a) * self.unite()
    }

    /// Entier dans `[a, b]`.
    pub fn entier(&mut self, a: u32, b: u32) -> u32 {
        if b <= a {
            return a;
        }
        a + (self.suivant() % u64::from(b - a + 1)) as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiant_texte() {
        let id = Id(0xabc);
        let s: String = id.into();
        assert_eq!(s, "0000000000000abc");
        assert_eq!(Id::try_from(s), Ok(id));
        assert!(Id::try_from("ABC".to_string()).is_err());
        assert!(Id::try_from("000000000000000G".to_string()).is_err());
        assert_ne!(Id::nouveau(), Id::nouveau());
    }

    #[test]
    fn alea_deterministe() {
        let mut a = Alea::new(7);
        let mut b = Alea::new(7);
        for _ in 0..100 {
            let x = a.entre(2.0, 5.0);
            assert_eq!(x, b.entre(2.0, 5.0));
            assert!((2.0..=5.0).contains(&x));
        }
    }
}
