//! Key names and chords ("ctrl+alt+del", "win+r", "f12") to the core's [`Key`] and to X11 keysyms, the values RFB key
//! events carry.

use sighter_core::ports::{FunctionKey, Key};

/// Why a key name was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KeyError {
    /// An unknown key name.
    #[error("unknown key {0:?} (use names like ctrl, alt, shift, win, enter, tab, esc, del, f1..f24, or one character)")]
    Unknown(String),
    /// A chord with an empty part, such as `ctrl++s`.
    #[error("bad key combo {0:?}: write it like ctrl+s or ctrl+alt+del (use 'plus' for the + key)")]
    BadCombo(String),
}

/// The X11 keysym for a key.
pub fn keysym(k: Key) -> u32 {
    match k {
        Key::Char(c) => char_keysym(c),
        Key::Enter => 0xff0d,
        Key::Escape => 0xff1b,
        Key::Tab => 0xff09,
        Key::Backspace => 0xff08,
        Key::Ctrl => 0xffe3,
        Key::Alt => 0xffe9,
        Key::Shift => 0xffe1,
        Key::Delete => 0xffff,
        Key::Super => 0xffeb,
        Key::Insert => 0xff63,
        Key::Home => 0xff50,
        Key::End => 0xff57,
        Key::PageUp => 0xff55,
        Key::PageDown => 0xff56,
        Key::Left => 0xff51,
        Key::Up => 0xff52,
        Key::Right => 0xff53,
        Key::Down => 0xff54,
        Key::Menu => 0xff67,
        Key::Function(n) => 0xffbe + u32::from(n.number()) - 1,
    }
}

/// The keysym that types `c`: Latin-1 maps directly, other characters use the Unicode keysym range.
pub fn char_keysym(c: char) -> u32 {
    match c {
        '\n' | '\r' => 0xff0d,
        '\t' => 0xff09,
        '\x1b' => 0xff1b,
        '\x08' => 0xff08,
        '\x7f' => 0xffff,
        // Other control codes have no Latin-1 keysym: use the Unicode range, as X11 does for characters without one.
        c if (c as u32) < 0x20 || (0x80..0xa0).contains(&(c as u32)) => 0x0100_0000 | c as u32,
        c if (c as u32) < 0x100 => c as u32,
        c => 0x0100_0000 | c as u32,
    }
}

/// One named key or a single character.
pub fn parse_key(name: &str) -> Result<Key, KeyError> {
    let lower = name.to_ascii_lowercase();
    let named = match lower.as_str() {
        "ctrl" | "control" => Some(Key::Ctrl),
        "shift" => Some(Key::Shift),
        "alt" => Some(Key::Alt),
        "win" | "super" | "meta" | "cmd" => Some(Key::Super),
        "enter" | "return" => Some(Key::Enter),
        "tab" => Some(Key::Tab),
        "esc" | "escape" => Some(Key::Escape),
        "backspace" => Some(Key::Backspace),
        "del" | "delete" => Some(Key::Delete),
        "ins" | "insert" => Some(Key::Insert),
        "home" => Some(Key::Home),
        "end" => Some(Key::End),
        "pgup" | "pageup" => Some(Key::PageUp),
        "pgdn" | "pagedown" => Some(Key::PageDown),
        "left" => Some(Key::Left),
        "up" => Some(Key::Up),
        "right" => Some(Key::Right),
        "down" => Some(Key::Down),
        "space" => Some(Key::Char(' ')),
        "menu" | "apps" => Some(Key::Menu),
        "plus" => Some(Key::Char('+')),
        _ => None,
    };
    if let Some(k) = named {
        return Ok(k);
    }
    if let Some(n) = lower
        .strip_prefix('f')
        .and_then(|n| n.parse::<u8>().ok())
        .and_then(FunctionKey::new)
    {
        return Ok(Key::Function(n));
    }
    let mut chars = name.chars();
    match (chars.next(), chars.next()) {
        // Letters in combos are sent lower-case, as a keyboard would.
        (Some(c), None) => Ok(Key::Char(c.to_ascii_lowercase())),
        _ => Err(KeyError::Unknown(name.to_string())),
    }
}

/// Parses "ctrl+alt+del", "win+r", "f12", "ctrl+plus" into keys pressed in order and released in reverse.
pub fn parse_chord(s: &str) -> Result<Vec<Key>, KeyError> {
    let parts: Vec<&str> = s.split('+').map(str::trim).collect();
    if parts.iter().any(|p| p.is_empty()) {
        return Err(KeyError::BadCombo(s.to_string()));
    }
    parts.into_iter().map(parse_key).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn f(n: u8) -> FunctionKey {
        FunctionKey::new(n).unwrap()
    }

    #[test]
    fn the_secure_attention_sequence_parses() {
        assert_eq!(parse_chord("ctrl+alt+del").unwrap(), vec![Key::Ctrl, Key::Alt, Key::Delete]);
        assert_eq!(parse_chord("Win+R").unwrap(), vec![Key::Super, Key::Char('r')]);
        assert_eq!(parse_chord("ctrl+plus").unwrap(), vec![Key::Ctrl, Key::Char('+')]);
        assert_eq!(parse_chord("f12").unwrap(), vec![Key::Function(f(12))]);
    }

    #[test]
    fn bad_names_are_typed_errors() {
        assert!(matches!(parse_chord("ctrl++s"), Err(KeyError::BadCombo(_))));
        assert!(matches!(parse_key("hyperdrive"), Err(KeyError::Unknown(_))));
        assert!(matches!(parse_key("f25"), Err(KeyError::Unknown(_))));
        assert!(parse_key("hyperdrive").unwrap_err().to_string().contains("hyperdrive"));
        assert!(parse_chord("a+").unwrap_err().to_string().contains("ctrl+s"));
    }

    #[test]
    fn every_named_key_has_the_x11_keysym() {
        let names = [
            "ctrl",
            "control",
            "shift",
            "alt",
            "win",
            "super",
            "meta",
            "cmd",
            "enter",
            "return",
            "tab",
            "esc",
            "escape",
            "backspace",
            "del",
            "delete",
            "ins",
            "insert",
            "home",
            "end",
            "pgup",
            "pageup",
            "pgdn",
            "pagedown",
            "left",
            "up",
            "right",
            "down",
            "space",
            "menu",
            "apps",
            "plus",
        ];
        for n in names {
            assert!(parse_key(n).is_ok(), "{n}");
        }
        let expect = [
            (Key::Enter, 0xff0d),
            (Key::Escape, 0xff1b),
            (Key::Tab, 0xff09),
            (Key::Backspace, 0xff08),
            (Key::Ctrl, 0xffe3),
            (Key::Alt, 0xffe9),
            (Key::Shift, 0xffe1),
            (Key::Delete, 0xffff),
            (Key::Super, 0xffeb),
            (Key::Insert, 0xff63),
            (Key::Home, 0xff50),
            (Key::End, 0xff57),
            (Key::PageUp, 0xff55),
            (Key::PageDown, 0xff56),
            (Key::Left, 0xff51),
            (Key::Up, 0xff52),
            (Key::Right, 0xff53),
            (Key::Down, 0xff54),
            (Key::Menu, 0xff67),
            (Key::Function(f(1)), 0xffbe),
            (Key::Function(f(12)), 0xffc9),
            (Key::Function(f(24)), 0xffbe + 23),
            (Key::Char('a'), 0x61),
            (Key::Char('\n'), 0xff0d),
            (Key::Char('\t'), 0xff09),
            (Key::Char('\r'), 0xff0d),
            (Key::Char('\x1b'), 0xff1b),
            (Key::Char('\x08'), 0xff08),
            (Key::Char('\x7f'), 0xffff),
            (Key::Char('\x01'), 0x0100_0001),
            (Key::Char('\u{85}'), 0x0100_0085),
            (Key::Char('é'), 0xe9),
            (Key::Char('€'), 0x0100_20ac),
        ];
        for (k, sym) in expect {
            assert_eq!(keysym(k), sym, "{k:?}");
        }
    }

    proptest! {
        /// Arbitrary text never panics the key parsers.
        #[test]
        fn parsers_never_panic(s in ".{0,40}") {
            let _ = parse_chord(&s);
            let _ = parse_key(&s);
        }
    }
}
