//! How many threads may do a kind of work at once: "Auto" (the engine decides from what it knows, ER-20) or a number the person
//! set. A setting, validated here in the engine (rule 51); the UI only offers the choices.

use serde::{Deserialize, Serialize};

/// Auto, or exactly this many threads. Externally tagged on the wire (`"auto"`, `{"fixed": 4}`), which postcard can carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Threads {
    /// The engine decides.
    #[default]
    Auto,
    /// This many (at least 1, at most the setting's own most).
    Fixed(u8),
}

impl Threads {
    /// The value kept: a fixed count is brought into `1..=most`.
    pub fn clamped(self, most: u8) -> Threads {
        match self {
            Threads::Auto => Threads::Auto,
            Threads::Fixed(n) => Threads::Fixed(n.clamp(1, most.max(1))),
        }
    }

    /// The number of threads to use: the fixed count (clamped into `1..=most`), or `auto` when the setting is Auto.
    pub fn resolve(self, auto: usize, most: u8) -> usize {
        match self.clamped(most) {
            Threads::Auto => auto.max(1),
            Threads::Fixed(n) => usize::from(n),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_uses_the_engines_number_and_a_fixed_count_is_clamped() {
        assert_eq!(Threads::Auto.resolve(6, 16), 6);
        assert_eq!(Threads::Auto.resolve(0, 16), 1);
        assert_eq!(Threads::Fixed(3).resolve(6, 16), 3);
        assert_eq!(Threads::Fixed(0).resolve(6, 16), 1);
        assert_eq!(Threads::Fixed(200).resolve(6, 16), 16);
        assert_eq!(Threads::Fixed(200).clamped(8), Threads::Fixed(8));
        assert_eq!(Threads::Auto.clamped(8), Threads::Auto);
    }

    #[test]
    fn the_wire_shape_is_a_word_or_a_tagged_count_and_survives_postcard() {
        assert_eq!(serde_json::to_value(Threads::Auto).unwrap(), "auto");
        assert_eq!(serde_json::to_value(Threads::Fixed(4)).unwrap(), serde_json::json!({"fixed": 4}));
        let back: Threads = serde_json::from_value(serde_json::json!({"fixed": 9})).unwrap();
        assert_eq!(back, Threads::Fixed(9));
        for t in [Threads::Auto, Threads::Fixed(7)] {
            assert_eq!(postcard::from_bytes::<Threads>(&postcard::to_stdvec(&t).unwrap()).unwrap(), t);
        }
        assert!(
            serde_json::from_value::<Threads>(serde_json::json!(4)).is_err(),
            "a bare number is not a setting"
        );
    }
}
