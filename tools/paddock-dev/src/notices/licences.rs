//! The licences an app may ship, and the SPDX expression check. One list: `templates/repo/deny.toml` `[licenses].allow`,
//! `templates/repo/about.toml` `accepted` and [`ALLOWED`] are kept equal by a test (here and in `conformance/test/notices.test.mjs`),
//! so a licence cannot be accepted by one tool and refused by another.

use super::sources::{Kind, Package};
use super::text::js_trim;
use std::collections::HashSet;

/// Accepted for Rust crates and npm packages (the deny.toml allow list).
pub const ALLOWED: [&str; 15] = [
    "MIT",
    "MIT-0",
    "Apache-2.0",
    "Apache-2.0 WITH LLVM-exception",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "ISC",
    "Zlib",
    "0BSD",
    "CC0-1.0",
    "BSL-1.0",
    "Unicode-3.0",
    "Unlicense",
    "MPL-2.0",
    "CDLA-Permissive-2.0",
];

/// An exception to [`ALLOWED`]: one licence id (or a whole non-SPDX expression a package declares) for named packages of one
/// ecosystem (cargo-deny never sees npm packages). A new package with the same licence is refused until it is added with a reason.
pub struct Exception {
    /// The ecosystem it applies to.
    pub kind: Kind,
    /// The licence id or whole declared expression.
    pub license: &'static str,
    /// The packages it covers.
    pub packages: &'static [&'static str],
    /// Why (for readers of this list).
    pub reason: &'static str,
}

/// The exceptions. Pre-approved by the user for npm packages only (none ships today, so they are not listed): Python-2.0 and
/// BlueOak-1.0.0. Whoever adds a package that needs one adds an exception for that package here, with a reason.
pub const EXCEPTIONS: [Exception; 2] = [
    Exception {
        kind: Kind::Npm,
        license: "OFL-1.1",
        packages: &[
            "@fontsource/geist-mono",
            "@fontsource/geist-sans",
            "@fontsource/ibm-plex-mono",
            "@fontsource/ibm-plex-sans",
            "@fontsource/inter",
            "@fontsource/jetbrains-mono",
        ],
        reason: "bundled UI fonts under the SIL Open Font Licence; their licence texts are in the notices",
    },
    Exception {
        kind: Kind::Npm,
        license: "CC-BY-4.0",
        packages: &["caniuse-lite"],
        reason: "browser-support data used at build time; the attribution it requires is in the notices",
    },
];

/// The licence ids package `name` of ecosystem `kind` may use: the allow list plus the exceptions that name it.
pub fn allowed_for(kind: Kind, name: &str) -> HashSet<&'static str> {
    let mut set: HashSet<&'static str> = ALLOWED.into_iter().collect();
    set.extend(
        EXCEPTIONS
            .iter()
            .filter(|e| e.kind == kind && e.packages.contains(&name))
            .map(|e| e.license),
    );
    set
}

fn is_op(token: Option<&&str>, op: &str) -> bool {
    token.is_some_and(|t| t.eq_ignore_ascii_case(op))
}

struct Parser<'a, 'b> {
    tokens: Vec<&'a str>,
    at: usize,
    allowed: &'b HashSet<&'static str>,
}

impl<'a> Parser<'a, '_> {
    fn peek(&self) -> Option<&&'a str> {
        self.tokens.get(self.at)
    }

    fn next(&mut self) -> Option<&'a str> {
        let t = self.tokens.get(self.at).copied();
        self.at += 1;
        t
    }

    fn term(&mut self) -> Option<bool> {
        let t = self.next()?;
        if t == ")" || ["AND", "OR", "WITH"].iter().any(|op| t.eq_ignore_ascii_case(op)) {
            return None;
        }
        if t == "(" {
            let v = self.or()?;
            return (self.next() == Some(")")).then_some(v);
        }
        if is_op(self.peek(), "WITH") {
            self.at += 1;
            let exception = self.next()?;
            if exception == "(" || exception == ")" {
                return None;
            }
            return Some(self.allowed.contains(format!("{t} WITH {exception}").as_str()));
        }
        Some(self.allowed.contains(t.strip_suffix('+').unwrap_or(t)))
    }

    fn and(&mut self) -> Option<bool> {
        let mut v = self.term()?;
        while is_op(self.peek(), "AND") {
            self.at += 1;
            let r = self.term()?;
            v = v && r;
        }
        Some(v)
    }

    fn or(&mut self) -> Option<bool> {
        let mut v = self.and()?;
        while is_op(self.peek(), "OR") {
            self.at += 1;
            let r = self.and()?;
            v = v || r;
        }
        Some(v)
    }
}

/// Whether an SPDX expression is acceptable under `allowed`: AND needs every term, OR at least one (AND binds tighter),
/// parentheses group, `X WITH exception` is allowed only when the whole phrase is listed, as cargo-deny does (an unknown
/// exception id is refused, whatever X is). A trailing `+` (the deprecated "or later" spelling) is dropped. An expression that
/// does not parse is refused.
pub fn expression_allowed(expr: &str, allowed: &HashSet<&'static str>) -> bool {
    let spaced = expr.replace('(', " ( ").replace(')', " ) ");
    let tokens: Vec<&str> = spaced.split(super::text::is_js_space).filter(|t| !t.is_empty()).collect();
    let count = tokens.len();
    let mut parser = Parser { tokens, at: 0, allowed };
    matches!(parser.or(), Some(true)) && parser.at == count
}

/// Packages whose licence is not acceptable for their ecosystem ("rust x@1: GPL-3.0-only"). Fail-closed: a package is accepted
/// only when its parsed expression evaluates true (so a missing or empty licence, NOASSERTION, UNLICENSED, `SEE LICENSE IN ...`,
/// `LicenseRef-*` and imprecise names such as "BSD" are refused) or when a package-scoped exception names the package and its whole
/// declared expression.
pub fn non_permissive(packages: &[Package]) -> Vec<String> {
    packages
        .iter()
        .filter(|p| {
            let expr = js_trim(&p.license);
            if EXCEPTIONS
                .iter()
                .any(|e| e.kind == p.kind && e.packages.contains(&p.name.as_str()) && e.license == expr)
            {
                return false;
            }
            !(!expr.is_empty() && expression_allowed(expr, &allowed_for(p.kind, &p.name)))
        })
        .map(|p| {
            let shown = if p.license.is_empty() {
                "(no licence declared)"
            } else {
                p.license.as_str()
            };
            format!("{} {}@{}: {shown}", p.kind.as_str(), p.name, p.version)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notices::text::spdx_ids;

    fn pkg(kind: Kind, name: &str, license: &str) -> Package {
        Package {
            kind,
            name: name.into(),
            version: "1".into(),
            license: license.into(),
            texts: vec![],
            spdx: spdx_ids(license),
        }
    }

    fn ok(expr: &str) -> bool {
        expression_allowed(expr, &allowed_for(Kind::Rust, "x"))
    }

    #[test]
    fn the_list_equals_deny_toml_and_about_toml() {
        let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .unwrap()
            .to_path_buf();
        let list_of = |file: &str, key: &str| -> Vec<String> {
            let text = std::fs::read_to_string(repo.join("templates/repo").join(file)).unwrap();
            let start = text.find(&format!("{key} = [")).unwrap();
            let body = &text[start..text[start..].find(']').map(|i| start + i).unwrap()];
            body.split('"').skip(1).step_by(2).map(String::from).collect()
        };
        assert_eq!(list_of("deny.toml", "allow"), ALLOWED);
        assert_eq!(list_of("about.toml", "accepted"), ALLOWED);
    }

    #[test]
    fn and_needs_every_term_or_needs_one_and_binds_tighter() {
        assert!(ok("MIT"));
        assert!(ok("MIT OR GPL-3.0-only"));
        assert!(!ok("MIT AND GPL-3.0-only"));
        assert!(ok("MIT OR GPL-3.0-only AND LGPL-2.1"));
        assert!(!ok("(MIT OR GPL-3.0-only) AND LGPL-2.1"));
        assert!(ok("(MIT AND (ISC OR GPL-2.0-only))"));
        assert!(ok("MIT+"));
        assert!(!ok("mit or isc"), "ids are case-sensitive, operators are not");
        assert!(ok("MIT or ISC"));
    }

    #[test]
    fn with_is_allowed_only_as_a_whole_listed_phrase() {
        assert!(ok("Apache-2.0 WITH LLVM-exception"));
        assert!(!ok("Apache-2.0 WITH Bison-exception"));
        assert!(!ok("MIT WITH LLVM-exception"));
        assert!(
            ok("Apache-2.0 with LLVM-exception"),
            "the operator is case-insensitive and the phrase is built with the upper-case word"
        );
    }

    #[test]
    fn expressions_that_do_not_parse_are_refused() {
        for bad in [
            "",
            "(",
            ")",
            "MIT OR",
            "AND MIT",
            "MIT MIT",
            "(MIT",
            "MIT)",
            "MIT WITH",
            "MIT WITH (",
            "SEE LICENSE IN LICENSE.md",
        ] {
            assert!(!ok(bad), "{bad:?}");
        }
    }

    #[test]
    fn exceptions_cover_only_their_own_licence_for_only_the_named_packages_of_their_ecosystem() {
        let bad = non_permissive(&[
            pkg(Kind::Npm, "@fontsource/inter", "OFL-1.1"),
            pkg(Kind::Npm, "@fontsource/other", "OFL-1.1"),
            pkg(Kind::Rust, "@fontsource/inter", "OFL-1.1"),
            pkg(Kind::Npm, "caniuse-lite", "CC-BY-4.0"),
            pkg(Kind::Npm, "caniuse-lite", "GPL-3.0-only"),
            pkg(Kind::Rust, "none", ""),
            pkg(Kind::Rust, "pad", "  MIT  "),
        ]);
        assert_eq!(
            bad,
            [
                "npm @fontsource/other@1: OFL-1.1",
                "rust @fontsource/inter@1: OFL-1.1",
                "npm caniuse-lite@1: GPL-3.0-only",
                "rust none@1: (no licence declared)"
            ]
        );
    }
}
