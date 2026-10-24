//! Key bindings (the shell's syntax: `Mod+Shift+P`, `Alt+Left`, `F5`; see `parseKey` in `@decryp7/paddock-ui/shell`) as
//! native menu accelerators (`CmdOrCtrl+Shift+P`), in the key names Tauri's menus (muda) accept.

/// The longest binding converted; longer text is not a binding.
pub const MAX_BINDING: usize = 64;

/// The native accelerator of a binding (`Mod+Shift+O` gives `CmdOrCtrl+Shift+O`), or `None` for a binding a menu cannot
/// show (unknown modifier or key, a repeated modifier, no key). Pure.
pub fn accelerator(binding: &str) -> Option<String> {
    if binding.is_empty() || binding.len() > MAX_BINDING {
        return None;
    }
    let parts: Vec<&str> = binding.split('+').map(str::trim).collect();
    let (key, mods) = parts.split_last()?;
    let mut out: Vec<&str> = Vec::with_capacity(parts.len());
    for m in mods {
        let name = match m.to_ascii_lowercase().as_str() {
            "mod" => "CmdOrCtrl",
            "ctrl" | "control" => "Ctrl",
            "alt" | "option" => "Alt",
            "shift" => "Shift",
            "meta" | "cmd" | "super" => "Super",
            _ => return None,
        };
        if out.contains(&name) {
            return None;
        }
        out.push(name);
    }
    let key = key_name(key)?;
    out.push(&key);
    Some(out.join("+"))
}

/// A key of a binding in muda's accelerator names (`-` is `Minus`, `Left` is `ArrowLeft`).
pub(crate) fn key_name(key: &str) -> Option<String> {
    let lower = key.to_ascii_lowercase();
    let named = match lower.as_str() {
        "-" | "minus" => "Minus",
        // The key that types `+` on most layouts (with Shift); `Mod+Plus` is the usual "zoom in".
        "plus" | "=" => "Equal",
        "," => "Comma",
        "." => "Period",
        "/" => "Slash",
        ";" => "Semicolon",
        "[" => "BracketLeft",
        "]" => "BracketRight",
        "\\" => "Backslash",
        "'" => "Quote",
        "`" => "Backquote",
        "enter" | "return" => "Enter",
        "tab" => "Tab",
        "escape" | "esc" => "Escape",
        "space" => "Space",
        "backspace" => "Backspace",
        "delete" => "Delete",
        "insert" => "Insert",
        "home" => "Home",
        "end" => "End",
        "pageup" => "PageUp",
        "pagedown" => "PageDown",
        "left" | "arrowleft" => "ArrowLeft",
        "right" | "arrowright" => "ArrowRight",
        "up" | "arrowup" => "ArrowUp",
        "down" | "arrowdown" => "ArrowDown",
        _ => "",
    };
    if !named.is_empty() {
        return Some(named.to_string());
    }
    let mut chars = key.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if c.is_ascii_alphanumeric() => Some(c.to_ascii_uppercase().to_string()),
        _ => {
            let digits = lower.strip_prefix('f')?;
            if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let n: u8 = digits.parse().ok()?;
            (1..=24).contains(&n).then(|| format!("F{n}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accelerators_follow_the_shell_bindings() {
        assert_eq!(
            accelerator("Mod+Shift+O").as_deref(),
            Some("CmdOrCtrl+Shift+O")
        );
        assert_eq!(accelerator("Mod+,").as_deref(), Some("CmdOrCtrl+Comma"));
        assert_eq!(accelerator("Alt+-").as_deref(), Some("Alt+Minus"));
        assert_eq!(accelerator("Mod+Plus").as_deref(), Some("CmdOrCtrl+Equal"));
        assert_eq!(accelerator("Mod+Minus").as_deref(), Some("CmdOrCtrl+Minus"));
        assert_eq!(accelerator("Ctrl+Alt+1").as_deref(), Some("Ctrl+Alt+1"));
        assert_eq!(
            accelerator("Shift+Alt+Enter").as_deref(),
            Some("Shift+Alt+Enter")
        );
        assert_eq!(accelerator("Alt+Left").as_deref(), Some("Alt+ArrowLeft"));
        assert_eq!(
            accelerator("Mod+Shift+F12").as_deref(),
            Some("CmdOrCtrl+Shift+F12")
        );
        assert_eq!(accelerator("F1").as_deref(), Some("F1"));
        assert_eq!(accelerator("Meta+k").as_deref(), Some("Super+K"));
        assert_eq!(accelerator("Mod+/").as_deref(), Some("CmdOrCtrl+Slash"));
        assert_eq!(accelerator("Ctrl+Insert").as_deref(), Some("Ctrl+Insert"));
        for bad in [
            "",
            "Mod+",
            "Hyper+K",
            "Mod+F25",
            "Mod+Fx",
            "Mod+F+1",
            "Mod+é",
            "Mod+Mod+K",
            "Mod+F-1",
            &"K+".repeat(40),
        ] {
            assert_eq!(accelerator(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn every_converted_key_name_is_one_tauri_accepts() {
        // Names of muda's accelerator parser (the one Tauri's menus use).
        let pairs = [
            ("-", "Minus"),
            (",", "Comma"),
            (".", "Period"),
            ("/", "Slash"),
            (";", "Semicolon"),
            ("=", "Equal"),
            ("[", "BracketLeft"),
            ("]", "BracketRight"),
            ("\\", "Backslash"),
            ("'", "Quote"),
            ("`", "Backquote"),
            ("Enter", "Enter"),
            ("Return", "Enter"),
            ("Tab", "Tab"),
            ("Esc", "Escape"),
            ("Space", "Space"),
            ("Backspace", "Backspace"),
            ("Delete", "Delete"),
            ("Home", "Home"),
            ("End", "End"),
            ("PageUp", "PageUp"),
            ("PageDown", "PageDown"),
            ("Left", "ArrowLeft"),
            ("Right", "ArrowRight"),
            ("Up", "ArrowUp"),
            ("ArrowDown", "ArrowDown"),
            ("a", "A"),
            ("7", "7"),
            ("F24", "F24"),
        ];
        for (k, want) in pairs {
            assert_eq!(key_name(k).as_deref(), Some(want), "{k}");
        }
    }
}
