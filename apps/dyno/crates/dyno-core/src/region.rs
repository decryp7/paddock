//! The country of the OS region setting, as a last resort for "where am I" on the map when the public-address lookup is off or
//! failed. Pure parsing of what the OS reports; the agent reads the OS ([`dyno_agent::region`]). Nothing is guessed from a
//! language alone: `en` says nothing about a country, `en_SG` says Singapore.

/// The ISO 3166-1 country in a locale name (`en_SG.UTF-8`, `en-US`, `zh_Hans_CN`, `de_DE@euro`), upper case, or `None`.
pub fn country_from_locale(locale: &str) -> Option<String> {
    let name = locale.trim().trim_matches('"');
    if name.is_empty() || name == "C" || name == "POSIX" {
        return None;
    }
    // Drop the encoding and modifier, then look at the parts after the language.
    let name = name.split(['.', '@']).next().unwrap_or(name);
    name.split(['_', '-'])
        .skip(1)
        .find(|p| p.len() == 2 && p.bytes().all(|b| b.is_ascii_alphabetic()) && p.bytes().all(|b| b.is_ascii_uppercase()))
        .map(str::to_string)
}

/// The country in the output of `reg query "HKCU\Control Panel\International\Geo" /v Name` (`    Name    REG_SZ    SG`).
pub fn country_from_windows_geo(output: &str) -> Option<String> {
    output
        .lines()
        .find(|l| l.trim_start().starts_with("Name"))
        .and_then(|l| l.split_whitespace().last())
        .filter(|c| c.len() == 2 && c.bytes().all(|b| b.is_ascii_alphabetic()))
        .map(str::to_ascii_uppercase)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_locale_names_a_country_only_when_it_has_one() {
        for (l, want) in [
            ("en_SG.UTF-8", Some("SG")),
            ("en-US", Some("US")),
            ("zh_Hans_CN", Some("CN")),
            ("de_DE@euro", Some("DE")),
            ("pt_BR.ISO8859-1", Some("BR")),
            ("\"en_GB\"", Some("GB")),
            ("en", None),
            ("C", None),
            ("POSIX", None),
            ("", None),
            ("C.UTF-8", None),
            ("en_us", None),
            ("zh_Hans", None),
        ] {
            assert_eq!(country_from_locale(l).as_deref(), want, "{l}");
        }
    }

    #[test]
    fn the_windows_geo_value_is_read_from_the_registry_listing() {
        let out = "\r\nHKEY_CURRENT_USER\\Control Panel\\International\\Geo\r\n    Name    REG_SZ    sg\r\n\r\n";
        assert_eq!(country_from_windows_geo(out).as_deref(), Some("SG"));
        assert_eq!(
            country_from_windows_geo("ERROR: The system was unable to find the specified registry key"),
            None
        );
        assert_eq!(country_from_windows_geo("    Name    REG_SZ    Singapore"), None);
    }
}
