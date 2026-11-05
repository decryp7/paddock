//! Windows stores the names of many firewall rules as indirect strings: `@{Pkg_1.0_x64__8wekyb3d8bbwe?ms-resource://Pkg/Resources/Name}`
//! for an app package, `@%SystemRoot%\system32\FirewallAPI.dll,-80204` for a resource in a DLL. The OS turns them into text with
//! `SHLoadIndirectString`; this module is the pure part (spotting one, the readable fallback, the bounded cache) so every OS tests
//! it. A user never reads a raw `@{...}` string.

use dyno_core::firewall::clip;
use paddock_jobs::time::Clock;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Distinct strings looked up in one read; the rest get their fallback until a later read (the answers so far stay cached).
pub const MAX_LOOKUPS_PER_READ: usize = 600;
/// The time one read may spend resolving.
pub const READ_BUDGET: Duration = Duration::from_secs(5);
/// The most answers kept; a full cache is cleared rather than grown.
pub const MAX_CACHED: usize = 4096;

/// What turns an indirect string into text: `SHLoadIndirectString` on Windows, a fake in tests.
pub trait IndirectLoader: Send + Sync {
    /// The text, or `None` when the OS cannot resolve it (or took too long).
    fn load(&self, source: &str) -> Option<String>;
}

/// A loader that resolves nothing: every indirect string gets its fallback.
pub struct NoLoader;

impl IndirectLoader for NoLoader {
    fn load(&self, _: &str) -> Option<String> {
        None
    }
}

/// Whether `s` is an indirect resource string (`@{...}` or `@file,-id`).
pub fn is_indirect(s: &str) -> bool {
    let s = s.trim();
    if s.starts_with("@{") {
        return true;
    }
    s.strip_prefix('@')
        .and_then(|rest| rest.rsplit_once(",-"))
        .is_some_and(|(file, id)| {
            !file.is_empty()
                && id
                    .split(';')
                    .next()
                    .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        })
}

/// Splits `FooBar` and `Foo.Bar` into words: `MicrosoftWindows.Client.OOBE` becomes `Microsoft Windows Client OOBE`.
fn words(package: &str) -> String {
    let mut out = String::new();
    let mut prev: Option<char> = None;
    for c in package.chars() {
        if c == '.' || c == '-' || c == '_' {
            if !out.ends_with(' ') && !out.is_empty() {
                out.push(' ');
            }
        } else {
            if prev.is_some_and(|p| p.is_lowercase() && c.is_uppercase()) {
                out.push(' ');
            }
            out.push(c);
        }
        prev = Some(c);
    }
    out.trim().to_string()
}

/// A readable stand-in for an indirect string the OS could not resolve: the package or file it names, never the raw text.
pub fn fallback(s: &str) -> String {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix("@{") {
        let package = rest.split(['?', '_', '}']).next().unwrap_or("");
        let named = words(package);
        return if named.is_empty() {
            "Windows app rule".into()
        } else {
            format!("{named} (app rule)")
        };
    }
    if let Some((file, id)) = s.trim_start_matches('@').rsplit_once(",-") {
        let stem = file.rsplit(['\\', '/']).next().unwrap_or(file);
        let stem = stem.rsplit_once('.').map_or(stem, |(a, _)| a);
        let id = id.split(';').next().unwrap_or("");
        if !stem.is_empty() && !id.is_empty() {
            return format!("Windows rule ({} {id})", words(stem));
        }
    }
    "Windows rule".into()
}

/// Resolves indirect strings once per distinct string, within a count and a time budget per read, and never fails: what cannot be
/// resolved reads as its [`fallback`]. Plain text passes through unchanged.
pub struct IndirectNames {
    loader: Box<dyn IndirectLoader>,
    clock: Arc<dyn Clock>,
    cache: Mutex<HashMap<String, String>>,
}

/// One read's allowance.
pub struct Budget {
    started: Duration,
    lookups: usize,
}

impl IndirectNames {
    /// Names resolved through `loader`, timed by `clock`.
    pub fn new(loader: Box<dyn IndirectLoader>, clock: Arc<dyn Clock>) -> Self {
        IndirectNames {
            loader,
            clock,
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// The allowance for one read.
    pub fn budget(&self) -> Budget {
        Budget {
            started: self.clock.monotonic(),
            lookups: 0,
        }
    }

    /// `s` as the user should read it.
    pub fn name(&self, s: &str, budget: &mut Budget) -> String {
        if !is_indirect(s) {
            return s.to_string();
        }
        let key = s.trim();
        let mut cache = self.cache.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(hit) = cache.get(key) {
            return hit.clone();
        }
        let spent = self.clock.monotonic().saturating_sub(budget.started);
        if budget.lookups >= MAX_LOOKUPS_PER_READ || spent >= READ_BUDGET {
            return fallback(key);
        }
        budget.lookups += 1;
        let resolved = self
            .loader
            .load(key)
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty() && !is_indirect(t))
            .map(|t| clip(&t))
            .unwrap_or_else(|| fallback(key));
        if cache.len() >= MAX_CACHED {
            cache.clear();
        }
        cache.insert(key.to_string(), resolved.clone());
        resolved
    }

    /// How many distinct strings are cached.
    pub fn cached(&self) -> usize {
        self.cache.lock().unwrap_or_else(|p| p.into_inner()).len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_jobs::time::FakeClock;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const PKG: &str =
        "@{MicrosoftWindows.Client.OOBE_1000.26100.1.0_x64__cw5n1h2txyewy?ms-resource://MicrosoftWindows.Client.OOBE/Resources/AppName}";

    #[test]
    fn spots_the_indirect_forms_and_nothing_else() {
        assert!(is_indirect(PKG));
        assert!(is_indirect("@{unterminated"));
        assert!(is_indirect(r"@%SystemRoot%\system32\FirewallAPI.dll,-80204"));
        assert!(is_indirect("@FirewallAPI.dll,-1234;Group"));
        for plain in [
            "",
            "Core Networking - DNS",
            "@",
            "@home",
            "me@example,-x",
            "name,-12",
            "@file.dll,-",
        ] {
            assert!(!is_indirect(plain), "{plain}");
        }
    }

    #[test]
    fn the_fallback_is_readable_and_never_raw() {
        assert_eq!(fallback(PKG), "Microsoft Windows Client OOBE (app rule)");
        assert_eq!(
            fallback(r"@%SystemRoot%\system32\FirewallAPI.dll,-80204"),
            "Windows rule (Firewall API 80204)"
        );
        assert_eq!(fallback("@{}"), "Windows app rule");
        assert_eq!(fallback("@{"), "Windows app rule");
        assert_eq!(fallback("@x"), "Windows rule");
        for s in [PKG, "@{", "@a.dll,-1", "@@@"] {
            let f = fallback(s);
            assert!(!f.contains("@{") && !f.starts_with('@') && !f.contains("ms-resource"), "{f}");
        }
    }

    struct Counting(AtomicUsize, Option<&'static str>);
    impl IndirectLoader for Counting {
        fn load(&self, _: &str) -> Option<String> {
            self.0.fetch_add(1, Ordering::SeqCst);
            self.1.map(String::from)
        }
    }
    struct Shared(Arc<Counting>);
    impl IndirectLoader for Shared {
        fn load(&self, s: &str) -> Option<String> {
            self.0.load(s)
        }
    }
    fn names(answer: Option<&'static str>) -> (IndirectNames, Arc<Counting>, Arc<FakeClock>) {
        let c = Arc::new(Counting(AtomicUsize::new(0), answer));
        let clock = Arc::new(FakeClock::new());
        (IndirectNames::new(Box::new(Shared(c.clone())), clock.clone()), c, clock)
    }

    #[test]
    fn plain_text_passes_through_and_costs_no_lookup() {
        let (n, c, _) = names(Some("x"));
        let mut b = n.budget();
        assert_eq!(n.name("Core Networking", &mut b), "Core Networking");
        assert_eq!(c.0.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_distinct_string_is_resolved_once_across_reads() {
        let (n, c, _) = names(Some("Out of box experience"));
        for _ in 0..3 {
            let mut b = n.budget();
            assert_eq!(n.name(PKG, &mut b), "Out of box experience");
            assert_eq!(
                n.name(&format!("  {PKG} "), &mut b),
                "Out of box experience",
                "trimmed to the same key"
            );
        }
        assert_eq!(c.0.load(Ordering::SeqCst), 1);
        assert_eq!(n.cached(), 1);
    }

    #[test]
    fn a_failed_or_still_raw_answer_falls_back() {
        let (n, _, _) = names(None);
        assert_eq!(n.name(PKG, &mut n.budget()), "Microsoft Windows Client OOBE (app rule)");
        let (n, _, _) = names(Some("@{still raw}"));
        assert_eq!(n.name(PKG, &mut n.budget()), "Microsoft Windows Client OOBE (app rule)");
        let (n, _, _) = names(Some("   "));
        assert_eq!(n.name("@a.dll,-5", &mut n.budget()), "Windows rule (a 5)");
    }

    #[test]
    fn a_read_is_bounded_by_count_and_time_and_the_rest_wait_for_the_next_read() {
        let (n, c, clock) = names(Some("ok"));
        let mut b = n.budget();
        for i in 0..MAX_LOOKUPS_PER_READ + 5 {
            n.name(&format!("@a.dll,-{i}"), &mut b);
        }
        assert_eq!(c.0.load(Ordering::SeqCst), MAX_LOOKUPS_PER_READ);
        assert_eq!(
            n.name("@a.dll,-99999", &mut b),
            "Windows rule (a 99999)",
            "over the count: fallback"
        );
        let mut b = n.budget();
        assert_eq!(n.name("@a.dll,-99999", &mut b), "ok", "next read resolves it");
        let mut b = n.budget();
        clock.advance(READ_BUDGET);
        assert_eq!(n.name("@a.dll,-88888", &mut b), "Windows rule (a 88888)", "over the time: fallback");
    }

    #[test]
    fn a_path_in_the_package_name_never_shows_and_a_remote_file_is_never_opened() {
        let with_path = r"@{C:\Program Files\WindowsApps\Contoso.Notes_1.2_x64__abc\x?ms-resource://Contoso.Notes/Resources/N}";
        assert_eq!(fallback(with_path), "Contoso Notes (app rule)");
        assert_eq!(fallback("@{my rule}"), "Windows rule", "not a package string: no package name to show");
        for unc in [r"@\\fileserver\share\x.dll,-12", "@//fileserver/share/x.dll,-12"] {
            let (n, c, _) = names(Some("from the network"));
            assert_eq!(n.name(unc, &mut n.budget()), "Windows rule (x 12)", "{unc}");
            assert_eq!(c.0.load(Ordering::SeqCst), 0, "the OS is never asked to open {unc}");
        }
    }

    #[test]
    fn a_full_cache_is_cleared_not_grown() {
        let (n, _, _) = names(Some("ok"));
        for i in 0..MAX_CACHED + 3 {
            n.name(&format!("@a.dll,-{i}"), &mut n.budget());
        }
        assert!(n.cached() <= MAX_CACHED);
    }
}
