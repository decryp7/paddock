//! Windows stores the names of many firewall rules as indirect strings: `@{Pkg_1.0_x64__8wekyb3d8bbwe?ms-resource://Pkg/Resources/Name}`
//! for an app package, `@%SystemRoot%\system32\FirewallAPI.dll,-80204` for a resource in a DLL. The OS turns them into text with
//! `SHLoadIndirectString`; this module is the pure part (spotting one, the fallback, the bounded cache) so every OS tests it. A user
//! never reads a raw `@{...}` string.

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
/// How long a lookup that timed out is not tried again.
pub const RETRY_TIMED_OUT_AFTER: Duration = Duration::from_secs(600);

/// What a lookup came back with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Loaded {
    /// The text.
    Text(String),
    /// The OS answered that it cannot resolve this string (the reason, an HRESULT, is for the log); it will not later.
    Unresolvable(String),
    /// The lookup did not answer in time, or was not started because earlier ones are still stuck; it may later.
    TimedOut,
}

/// What turns an indirect string into text: `SHLoadIndirectString` on Windows, a fake in tests.
pub trait IndirectLoader: Send + Sync {
    /// Looks `source` up.
    fn load(&self, source: &str) -> Loaded;
}

/// A loader that resolves nothing: every indirect string gets its fallback.
pub struct NoLoader;

impl IndirectLoader for NoLoader {
    fn load(&self, _: &str) -> Loaded {
        Loaded::Unresolvable("no loader".into())
    }
}

/// What a name is for, so its fallback reads right where it is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    /// A rule's name.
    Name,
    /// A rule's group.
    Group,
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

/// A resource file on another computer (`@\\server\share\x.dll,-1`): asking the OS to load it would open a network connection from
/// merely reading the firewall, so it is never looked up.
fn is_remote(s: &str) -> bool {
    s.trim()
        .strip_prefix('@')
        .is_some_and(|p| p.starts_with("\\\\") || p.starts_with("//"))
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

/// A readable stand-in for an indirect string the OS could not resolve: the package or file it names, never the raw text or a path.
pub fn fallback(s: &str, field: Field) -> String {
    let s = s.trim();
    let (rules, plain) = match field {
        Field::Name => ("app rule", "Windows rule"),
        Field::Group => ("app rules", "Windows rule group"),
    };
    if let Some(rest) = s.strip_prefix("@{") {
        // `Pkg_1.0_x64__id?ms-resource://...}`: only a package string has the `?`; the name is the package folder's name before its first `_`.
        let Some((package, _)) = rest.split_once('?') else {
            return plain.into();
        };
        // A path to the package's resource file (`C:\...\WindowsApps\Pkg_1.0_x64__id\resources.pri`) names it in the folder with the `_`.
        let segments: Vec<&str> = package.split(['\\', '/']).collect();
        let part = segments
            .iter()
            .rev()
            .find(|s| s.contains('_'))
            .or(segments.last())
            .copied()
            .unwrap_or("");
        let named = words(part.split('_').next().unwrap_or(""));
        return if named.is_empty() {
            format!("Windows {rules}")
        } else {
            format!("{named} ({rules})")
        };
    }
    if let Some((file, id)) = s.trim_start_matches('@').rsplit_once(",-") {
        let stem = file.rsplit(['\\', '/']).next().unwrap_or(file);
        let stem = stem.rsplit_once('.').map_or(stem, |(a, _)| a);
        let id = id.split(';').next().unwrap_or("");
        if !stem.is_empty() && !id.is_empty() {
            return format!("{plain} ({} {id})", words(stem));
        }
    }
    plain.into()
}

enum Cached {
    /// The OS's text.
    Text(String),
    /// The OS cannot resolve it, now or later: the fallback, which depends on the field it is shown in.
    Unresolvable,
    /// The fallback until this monotonic time, then asked again.
    RetryAt(Duration),
}

/// What one read did, for the log (counts, never names).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReadStats {
    /// Strings that came back as text (a lookup now or a cache hit).
    pub resolved: usize,
    /// Strings shown as their fallback (unresolvable, timed out, remote, or over budget).
    pub fallbacks: usize,
    /// Of those, left over because the count or time budget ran out.
    pub over_budget: usize,
    /// Of those, lookups that timed out or were skipped as stuck.
    pub timed_out: usize,
    /// Lookups the OS was actually asked.
    pub lookups: usize,
    /// The first reason the OS gave for an unresolvable string.
    pub first_error: Option<String>,
}

/// One read's allowance and tally.
pub struct Budget {
    started: Duration,
    stats: ReadStats,
}

impl Budget {
    /// What the read did so far.
    pub fn stats(&self) -> &ReadStats {
        &self.stats
    }
}

/// Resolves indirect strings once per distinct string, within a count and a time budget per read, and never fails: what cannot be
/// resolved reads as its [`fallback`]. Plain text passes through unchanged.
pub struct IndirectNames {
    loader: Box<dyn IndirectLoader>,
    clock: Arc<dyn Clock>,
    cache: Mutex<HashMap<String, Cached>>,
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
            stats: ReadStats::default(),
        }
    }

    /// `s` as the user should read it.
    pub fn name(&self, s: &str, field: Field, budget: &mut Budget) -> String {
        if !is_indirect(s) {
            return s.to_string();
        }
        let key = s.trim();
        let now = self.clock.monotonic();
        let stats = &mut budget.stats;
        if is_remote(key) {
            stats.fallbacks += 1;
            return fallback(key, field);
        }
        let mut cache = self.cache.lock().unwrap_or_else(|p| p.into_inner());
        match cache.get(key) {
            Some(Cached::Text(text)) => {
                stats.resolved += 1;
                return text.clone();
            }
            Some(Cached::Unresolvable) => {
                stats.fallbacks += 1;
                return fallback(key, field);
            }
            Some(Cached::RetryAt(until)) if now < *until => {
                stats.fallbacks += 1;
                stats.timed_out += 1;
                return fallback(key, field);
            }
            _ => {}
        }
        if stats.lookups >= MAX_LOOKUPS_PER_READ || now.saturating_sub(budget.started) >= READ_BUDGET {
            stats.fallbacks += 1;
            stats.over_budget += 1;
            return fallback(key, field);
        }
        stats.lookups += 1;
        let (shown, entry) = match self.loader.load(key) {
            Loaded::Text(t) if !t.trim().is_empty() && !is_indirect(t.trim()) => {
                stats.resolved += 1;
                let text = clip(&t);
                (text.clone(), Cached::Text(text))
            }
            Loaded::Text(_) => {
                stats.fallbacks += 1;
                (fallback(key, field), Cached::Unresolvable)
            }
            Loaded::Unresolvable(why) => {
                stats.fallbacks += 1;
                stats.first_error.get_or_insert(why);
                (fallback(key, field), Cached::Unresolvable)
            }
            Loaded::TimedOut => {
                stats.fallbacks += 1;
                stats.timed_out += 1;
                (fallback(key, field), Cached::RetryAt(now + RETRY_TIMED_OUT_AFTER))
            }
        };
        if cache.len() >= MAX_CACHED {
            cache.clear();
        }
        cache.insert(key.to_string(), entry);
        shown
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
    const DLL: &str = r"@%SystemRoot%\system32\FirewallAPI.dll,-80204";

    #[test]
    fn spots_the_indirect_forms_and_nothing_else() {
        assert!(is_indirect(PKG));
        assert!(is_indirect("@{unterminated"));
        assert!(is_indirect(DLL));
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
        use Field::{Group, Name};
        assert_eq!(fallback(PKG, Name), "Microsoft Windows Client OOBE (app rule)");
        assert_eq!(fallback(PKG, Group), "Microsoft Windows Client OOBE (app rules)");
        assert_eq!(fallback(DLL, Name), "Windows rule (Firewall API 80204)");
        assert_eq!(fallback(DLL, Group), "Windows rule group (Firewall API 80204)");
        assert_eq!(fallback("@{}", Name), "Windows rule", "no package string");
        assert_eq!(fallback("@{?x}", Name), "Windows app rule");
        assert_eq!(fallback("@x", Group), "Windows rule group");
        for s in [PKG, "@{", "@a.dll,-1", "@@@", "@{my rule}"] {
            for f in [Name, Group] {
                let t = fallback(s, f);
                assert!(!t.contains("@{") && !t.starts_with('@') && !t.contains("ms-resource"), "{t}");
            }
        }
    }

    #[test]
    fn a_path_in_the_package_name_never_shows() {
        let with_path = r"@{C:\Program Files\WindowsApps\Contoso.Notes_1.2_x64__abc\x?ms-resource://Contoso.Notes/Resources/N}";
        assert_eq!(fallback(with_path, Field::Name), "Contoso Notes (app rule)");
        assert_eq!(
            fallback("@{my rule}", Field::Name),
            "Windows rule",
            "not a package string: no package name to show"
        );
    }

    struct Fake {
        calls: AtomicUsize,
        answer: std::sync::Mutex<Loaded>,
    }
    impl IndirectLoader for Arc<Fake> {
        fn load(&self, _: &str) -> Loaded {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.answer.lock().unwrap().clone()
        }
    }
    fn names(answer: Loaded) -> (IndirectNames, Arc<Fake>, Arc<FakeClock>) {
        let f = Arc::new(Fake {
            calls: AtomicUsize::new(0),
            answer: std::sync::Mutex::new(answer),
        });
        let clock = Arc::new(FakeClock::new());
        (IndirectNames::new(Box::new(f.clone()), clock.clone()), f, clock)
    }
    fn text(s: &str) -> Loaded {
        Loaded::Text(s.into())
    }
    fn calls(f: &Fake) -> usize {
        f.calls.load(Ordering::SeqCst)
    }
    fn name(n: &IndirectNames, s: &str, b: &mut Budget) -> String {
        n.name(s, Field::Name, b)
    }

    #[test]
    fn plain_text_passes_through_and_costs_no_lookup() {
        let (n, f, _) = names(text("x"));
        assert_eq!(name(&n, "Core Networking", &mut n.budget()), "Core Networking");
        assert_eq!(calls(&f), 0);
    }

    #[test]
    fn a_distinct_string_is_resolved_once_across_reads() {
        let (n, f, _) = names(text("Out of box experience"));
        for _ in 0..3 {
            let mut b = n.budget();
            assert_eq!(name(&n, PKG, &mut b), "Out of box experience");
            assert_eq!(
                name(&n, &format!("  {PKG} "), &mut b),
                "Out of box experience",
                "trimmed to the same key"
            );
            assert_eq!(b.stats().resolved, 2);
        }
        assert_eq!(calls(&f), 1);
        assert_eq!(n.cached(), 1);
    }

    #[test]
    fn an_unresolvable_or_still_raw_answer_falls_back_for_good_and_is_counted_as_a_fallback() {
        for answer in [Loaded::Unresolvable("0x80070002".into()), text("@{still raw}"), text("   ")] {
            let (n, f, _) = names(answer.clone());
            let mut b = n.budget();
            assert_eq!(name(&n, PKG, &mut b), "Microsoft Windows Client OOBE (app rule)");
            assert_eq!(
                (b.stats().resolved, b.stats().fallbacks, b.stats().lookups),
                (0, 1, 1),
                "{answer:?}"
            );
            if let Loaded::Unresolvable(_) = answer {
                assert_eq!(b.stats().first_error.as_deref(), Some("0x80070002"));
            }
            let mut b = n.budget();
            name(&n, PKG, &mut b);
            assert_eq!((calls(&f), b.stats().lookups), (1, 0), "not asked again");
        }
    }

    #[test]
    fn the_fallback_follows_the_field_it_is_shown_in_even_from_the_cache() {
        let (n, _, _) = names(Loaded::Unresolvable("x".into()));
        assert_eq!(n.name(DLL, Field::Name, &mut n.budget()), "Windows rule (Firewall API 80204)");
        assert_eq!(
            n.name(DLL, Field::Group, &mut n.budget()),
            "Windows rule group (Firewall API 80204)"
        );
    }

    #[test]
    fn a_timeout_is_not_kept_for_the_life_of_the_agent() {
        let (n, f, clock) = names(Loaded::TimedOut);
        let mut b = n.budget();
        assert_eq!(name(&n, PKG, &mut b), "Microsoft Windows Client OOBE (app rule)");
        assert_eq!((b.stats().timed_out, b.stats().fallbacks, calls(&f)), (1, 1, 1));
        // Within the retry window it is not asked again; the fallback shows.
        clock.advance(RETRY_TIMED_OUT_AFTER - Duration::from_secs(1));
        let mut b = n.budget();
        name(&n, PKG, &mut b);
        assert_eq!((calls(&f), b.stats().timed_out), (1, 1));
        // After it, it is asked again and the answer is used.
        *f.answer.lock().unwrap() = text("Out of box experience");
        clock.advance(Duration::from_secs(2));
        assert_eq!(name(&n, PKG, &mut n.budget()), "Out of box experience");
        assert_eq!(calls(&f), 2);
        assert_eq!(name(&n, PKG, &mut n.budget()), "Out of box experience");
        assert_eq!(calls(&f), 2, "now final");
    }

    #[test]
    fn a_read_is_bounded_by_count_and_time_and_the_rest_wait_for_the_next_read() {
        let (n, f, clock) = names(text("ok"));
        let mut b = n.budget();
        for i in 0..MAX_LOOKUPS_PER_READ + 5 {
            name(&n, &format!("@a.dll,-{i}"), &mut b);
        }
        assert_eq!(calls(&f), MAX_LOOKUPS_PER_READ);
        assert_eq!(
            (b.stats().resolved, b.stats().over_budget, b.stats().fallbacks),
            (MAX_LOOKUPS_PER_READ, 5, 5)
        );
        assert_eq!(
            name(&n, "@a.dll,-99999", &mut b),
            "Windows rule (a 99999)",
            "over the count: fallback"
        );
        let mut b = n.budget();
        assert_eq!(name(&n, "@a.dll,-99999", &mut b), "ok", "next read resolves it");
        let mut b = n.budget();
        clock.advance(READ_BUDGET);
        assert_eq!(
            name(&n, "@a.dll,-88888", &mut b),
            "Windows rule (a 88888)",
            "over the time: fallback"
        );
        assert_eq!(b.stats().over_budget, 1);
        // Over-budget strings are not remembered as failures.
        assert_eq!(name(&n, "@a.dll,-88888", &mut n.budget()), "ok");
    }

    #[test]
    fn a_resource_file_on_another_computer_is_never_opened() {
        for unc in [r"@\\fileserver\share\x.dll,-12", "@//fileserver/share/x.dll,-12"] {
            let (n, f, _) = names(text("from the network"));
            let mut b = n.budget();
            assert_eq!(name(&n, unc, &mut b), "Windows rule (x 12)", "{unc}");
            assert_eq!(
                (calls(&f), b.stats().lookups, b.stats().fallbacks),
                (0, 0, 1),
                "the OS is never asked to open {unc}"
            );
        }
        // A package string has `//` too, and is looked up.
        let (n, f, _) = names(text("ok"));
        name(&n, PKG, &mut n.budget());
        assert_eq!(calls(&f), 1);
    }

    #[test]
    fn a_full_cache_is_cleared_not_grown() {
        let (n, _, _) = names(text("ok"));
        for i in 0..MAX_CACHED + 3 {
            name(&n, &format!("@a.dll,-{i}"), &mut n.budget());
        }
        assert!(n.cached() <= MAX_CACHED);
    }
}
