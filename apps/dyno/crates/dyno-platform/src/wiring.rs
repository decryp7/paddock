//! The wiring check each OS's `platform()` test runs (ADR 0033 section 2): no port of the real platform is a test double.
//! It reads `Platform::adapters()` and calls no port method.

/// The ports whose adapter is a test double: a type path through `::fake::` or `::testing::`, or a last segment that starts with
/// `Fake`. Each entry reads `port=adapter`.
pub(crate) fn fakes_in(adapters: &[(String, &'static str)]) -> Vec<String> {
    adapters
        .iter()
        .filter(|(_, a)| {
            let last = a.rsplit("::").next().unwrap_or(a);
            a.contains("::fake::") || a.contains("::testing::") || last.starts_with("Fake")
        })
        .map(|(port, a)| format!("{port}={a}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_platform() -> crate::Platform {
        crate::fake::Fake::new().platform()
    }

    #[test]
    fn a_platform_built_from_the_fake_is_all_fake() {
        let found = fakes_in(&fake_platform().adapters());
        for port in ["locks", "startup", "facts", "sensors", "wol.udp", "hardware.battery"] {
            assert!(
                found.iter().any(|f| f.starts_with(&format!("{port}="))),
                "{port} is not reported: {found:?}"
            );
        }
    }

    #[test]
    fn the_report_names_the_fake_type() {
        let found = fakes_in(&fake_platform().adapters());
        assert!(found.contains(&"startup=dyno_platform::fake::Fake".to_string()), "{found:?}");
    }

    #[test]
    fn a_spread_over_the_fake_is_caught() {
        // What Windows did until ADR 0033: some ports real, the rest spread in from the fake.
        let real = crate::unsupported::NotReadHere::new("Startup items", "x");
        let mut mixed = fake_platform();
        mixed.startup = Box::new(real);
        let found = fakes_in(&mixed.adapters());
        assert!(found.iter().all(|f| !f.starts_with("startup=")));
        assert!(found.iter().any(|f| f.starts_with("locks=")), "{found:?}");
    }

    #[test]
    fn honest_gaps_are_not_fakes() {
        let list = vec![
            ("startup".to_string(), "dyno_platform::unsupported::NotReadHere"),
            ("power.os".to_string(), crate::UNNAMED_ADAPTER),
        ];
        assert!(fakes_in(&list).is_empty());
    }

    #[test]
    fn the_wired_line_names_every_port_and_adapter() {
        let line = fake_platform().wired_line();
        assert!(line.starts_with("sensors=dyno_platform::fake::"), "{line}");
        assert!(line.contains(", startup=dyno_platform::fake::Fake, "), "{line}");
        assert!(line.contains("power.os=(unnamed)"), "{line}");
    }

    #[test]
    fn every_port_is_listed_once() {
        let list = fake_platform().adapters();
        let mut names: Vec<_> = list.iter().map(|(p, _)| p.clone()).collect();
        let n = names.len();
        names.sort();
        names.dedup();
        assert_eq!(n, names.len());
        assert!(n >= 25, "{n}");
    }
}
