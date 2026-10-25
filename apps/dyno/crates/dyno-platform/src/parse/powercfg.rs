//! `powercfg /getactivescheme` output, e.g.
//! `Power Scheme GUID: 381b4222-f694-41f0-9685-ff5bb260df2e  (Balanced)`. The label text is localised, so the parser
//! looks for the GUID and the parenthesised name only.

use dyno_core::facts::PowerPlan;

/// The built-in "Power saver" scheme.
pub const POWER_SAVER_GUID: &str = "a1841308-3541-4fab-bc81-f71556f20b4a";

/// The active power plan in `powercfg /getactivescheme` output.
pub fn parse_active_scheme(text: &str) -> Option<PowerPlan> {
    let line = text.lines().find(|l| find_guid(l).is_some())?;
    let (start, guid) = find_guid(line)?;
    let rest = &line[start + guid.len()..];
    let name = match (rest.find('('), rest.rfind(')')) {
        (Some(a), Some(b)) if b > a => rest[a + 1..b].trim().to_string(),
        _ => String::new(),
    };
    let id = guid.to_ascii_lowercase();
    Some(PowerPlan {
        power_saver: id == POWER_SAVER_GUID,
        name: if name.is_empty() { id.clone() } else { name },
        id,
    })
}

/// The first GUID-shaped token (8-4-4-4-12 hex digits) and its byte offset.
fn find_guid(line: &str) -> Option<(usize, &str)> {
    let bytes = line.as_bytes();
    (0..bytes.len().saturating_sub(35)).find_map(|i| {
        let cand = line.get(i..i + 36)?;
        is_guid(cand).then_some((i, cand))
    })
}

fn is_guid(s: &str) -> bool {
    s.len() == 36
        && s.char_indices().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_english_and_localised_output() {
        let p = parse_active_scheme("Power Scheme GUID: 381b4222-f694-41f0-9685-ff5bb260df2e  (Balanced)\r\n").unwrap();
        assert_eq!(p.id, "381b4222-f694-41f0-9685-ff5bb260df2e");
        assert_eq!(p.name, "Balanced");
        assert!(!p.power_saver);
        let p =
            parse_active_scheme("\nGUID du mode de gestion de l'alimentation : A1841308-3541-4FAB-BC81-F71556F20B4A  (Économie d'énergie)")
                .unwrap();
        assert!(p.power_saver);
        assert_eq!(p.name, "Économie d'énergie");
    }

    #[test]
    fn missing_names_and_garbage() {
        let p = parse_active_scheme("GUID: 8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c").unwrap();
        assert_eq!(p.name, p.id);
        assert!(parse_active_scheme("").is_none());
        assert!(parse_active_scheme("no guid here (Balanced)").is_none());
        assert!(parse_active_scheme("8c5e7fda-e8bf-4a96-9a85-a6e23a8c635Z").is_none());
        assert!(!is_guid("8c5e7fdae-8bf-4a96-9a85-a6e23a8c635c"));
    }
}
