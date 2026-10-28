//! How durations are written in messages (sizes: `paddock_units::format_whole`): the largest unit the value is a whole multiple of.

/// `90000` → `"90 s"`, `3600000` → `"1 h"`, `1500` → `"1500 ms"`.
pub fn format_ms(ms: u64) -> String {
    const UNITS: [(u64, &str); 3] = [(3_600_000, "h"), (60_000, "min"), (1000, "s")];
    largest(ms, &UNITS, "ms")
}

fn largest(n: u64, units: &[(u64, &str)], base: &str) -> String {
    units
        .iter()
        .find(|(size, _)| n >= *size && n.is_multiple_of(*size))
        .map_or_else(
            || format!("{n} {base}"),
            |(size, name)| format!("{} {name}", n / size),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_largest_whole_unit() {
        assert_eq!(format_ms(0), "0 ms");
        assert_eq!(format_ms(1500), "1500 ms");
        assert_eq!(format_ms(90_000), "90 s");
        assert_eq!(format_ms(120_000), "2 min");
        assert_eq!(format_ms(7_200_000), "2 h");
    }
}
