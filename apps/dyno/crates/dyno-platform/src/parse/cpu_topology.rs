//! Which logical cores are performance and which are efficiency cores, from what each OS exposes. Pure.

use dyno_core::metrics::CoreKind;

/// Largest CPU index accepted from a cpulist (guards against absurd input).
const MAX_CPUS: usize = 4096;

/// A kernel cpulist such as `0-7,16,18-19` → the indices. Junk, reversed or oversized ranges are skipped.
pub fn parse_cpulist(text: &str) -> Vec<usize> {
    let mut out = Vec::new();
    for part in text.trim().split(',') {
        let part = part.trim();
        let (a, b) = match part.split_once('-') {
            Some((a, b)) => (a.trim().parse::<usize>(), b.trim().parse::<usize>()),
            None => (part.parse::<usize>(), part.parse::<usize>()),
        };
        if let (Ok(a), Ok(b)) = (a, b) {
            if a <= b && b < MAX_CPUS {
                out.extend(a..=b);
            }
        }
    }
    out
}

/// Intel hybrid: `cpu_core` (P) and `cpu_atom` (E) cpulists. Empty when either is missing or they overlap or do not
/// cover `total` cores.
pub fn kinds_from_core_atom(total: usize, core_list: &str, atom_list: &str) -> Vec<CoreKind> {
    let (p, e) = (parse_cpulist(core_list), parse_cpulist(atom_list));
    if p.is_empty() || e.is_empty() || p.len() + e.len() != total {
        return Vec::new();
    }
    let mut kinds = vec![CoreKind::Unknown; total];
    for i in &p {
        if let Some(k) = kinds.get_mut(*i) {
            *k = CoreKind::Performance;
        }
    }
    for i in &e {
        match kinds.get_mut(*i) {
            Some(k) if *k == CoreKind::Unknown => *k = CoreKind::Efficiency,
            _ => return Vec::new(),
        }
    }
    if kinds.contains(&CoreKind::Unknown) {
        Vec::new()
    } else {
        kinds
    }
}

/// Efficiency classes (Windows `EfficiencyClass`, Linux/ARM `cpu_capacity`): the highest value is Performance, any
/// lower value Efficiency. `higher_is_faster` says which direction means faster (true for both). Uniform → empty.
pub fn kinds_from_classes(classes: &[u32]) -> Vec<CoreKind> {
    let (Some(max), Some(min)) = (classes.iter().max(), classes.iter().min()) else {
        return Vec::new();
    };
    if max == min {
        return Vec::new();
    }
    classes
        .iter()
        .map(|c| if c == max { CoreKind::Performance } else { CoreKind::Efficiency })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use CoreKind::*;

    #[test]
    fn cpulists() {
        assert_eq!(parse_cpulist("0-3,8,10-11\n"), vec![0, 1, 2, 3, 8, 10, 11]);
        assert_eq!(parse_cpulist(""), Vec::<usize>::new());
        assert_eq!(parse_cpulist("3-1,x,9999999,2"), vec![2]);
    }

    #[test]
    fn intel_hybrid() {
        assert_eq!(
            kinds_from_core_atom(6, "0-3", "4-5"),
            vec![Performance, Performance, Performance, Performance, Efficiency, Efficiency]
        );
        assert!(kinds_from_core_atom(6, "0-3", "3-4").is_empty(), "overlap");
        assert!(kinds_from_core_atom(8, "0-3", "4-5").is_empty(), "does not cover");
        assert!(kinds_from_core_atom(4, "0-3", "").is_empty());
    }

    #[test]
    fn classes() {
        assert_eq!(
            kinds_from_classes(&[1, 1, 0, 0]),
            vec![Performance, Performance, Efficiency, Efficiency]
        );
        assert_eq!(kinds_from_classes(&[1024, 512, 1024]), vec![Performance, Efficiency, Performance]);
        assert!(kinds_from_classes(&[5, 5]).is_empty());
        assert!(kinds_from_classes(&[]).is_empty());
    }
}
