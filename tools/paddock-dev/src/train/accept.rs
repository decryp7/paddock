//! ER-12 made enforced: a flaky test may be accepted only when this train did not touch it or the source it exercises. The user's
//! rule is feature-level: the FLAKY blocks when the change touches the test itself or "the source the test exercises (its feature or
//! module: the files it drives, not just any file in the same app package or crate)".
//!
//! `--accept-flaky apps/<app>/app/<spec>:<line>[:<col>]` is refused when `git diff origin/main...head` changes
//!
//! * the spec file, or a helper in its own folder (another spec is a different test), or
//! * the source it drives, resolved in this order: a declaration next to the specs, `<package>/e2e/feature-sources.json`, a JSON
//!   object from a spec's file name to the path prefixes (relative to the package) it drives, which overrides the name match
//!   (`[]` says it drives nothing of the app's source); else the name match, a tracked file under `<package>/src` that has a
//!   folder or a file stem equal to the spec's name or its name without the last word (`live-tail-follow.e2e.ts` maps to
//!   `live-tail-follow` and `live-tail`).
//!
//! It fails closed: a spec with no declaration and no file its name matches has an unresolved feature, which is refused with the
//! file to add the declaration to, never read as "touches nothing". A declaration that cannot be read is refused too.

use super::verdict::{Accepted, location_of};
use serde_json::Value;

/// The declaration file next to the specs, relative to the package.
pub const DECLARATION: &str = "e2e/feature-sources.json";

/// What the resolver needs to know of a package, read by the real code from the tree at the train head.
pub struct Tree {
    /// The tracked files under `<package>/src` (repository paths).
    pub tracked: Vec<String>,
    /// The text of `<package>/e2e/feature-sources.json`, when the file exists.
    pub declaration: Option<String>,
}

fn is_spec(path: &str) -> bool {
    path.ends_with(".e2e.ts") || path.ends_with(".spec.ts")
}

fn stem_of(file: &str) -> &str {
    let name = file.rsplit('/').next().unwrap_or(file);
    name.strip_suffix(".e2e.ts")
        .or_else(|| name.strip_suffix(".spec.ts"))
        .unwrap_or(name)
}

/// The names that stand for the feature a spec drives.
fn feature_keys(stem: &str) -> Vec<String> {
    let words: Vec<&str> = stem.split('-').collect();
    let mut keys = vec![stem.to_string()];
    if words.len() > 2 {
        keys.push(words[..words.len() - 1].join("-"));
    }
    keys
}

/// Whether a path under `src` names the feature: a folder or a file stem equal to a key.
fn names_feature(path: &str, keys: &[String]) -> bool {
    let comps: Vec<&str> = path.split('/').collect();
    comps.contains(&"src")
        && comps.iter().any(|comp| {
            let stem = comp.split('.').next().unwrap_or(comp);
            keys.iter().any(|k| k == stem)
        })
}

/// The files of `changed` that make `accepted` unacceptable, one line each (empty: the train did not touch the test or what it
/// drives); `Err` when a feature cannot be resolved or a declaration cannot be read (fail closed).
pub fn touched(accepted: &[Accepted], changed: &[String], tree: &dyn Fn(&Accepted) -> Result<Tree, String>) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for a in accepted {
        let (pkg, spec) = (a.package(), a.file());
        let dir = spec.rsplit_once('/').map_or("", |(d, _)| d);
        let name = a.rel().rsplit('/').next().unwrap_or(a.rel());
        let keys = feature_keys(stem_of(a.rel()));
        let t = tree(a)?;
        // The drives-prefixes: declared, else resolved by name (and an unresolvable name is an error, not "nothing").
        let declared: Option<Vec<String>> = match &t.declaration {
            Some(text) => {
                let v: Value = serde_json::from_str(text).map_err(|e| format!("{pkg}/{DECLARATION} is not valid JSON ({e}); fix it"))?;
                let obj = v
                    .as_object()
                    .ok_or_else(|| format!("{pkg}/{DECLARATION} must be a JSON object from a spec's file name to path prefixes"))?;
                match obj.get(name) {
                    None => None,
                    Some(list) => Some(
                        list.as_array()
                            .and_then(|l| l.iter().map(|p| p.as_str().map(str::to_string)).collect::<Option<Vec<_>>>())
                            .ok_or_else(|| format!("{pkg}/{DECLARATION}: `{name}` must be a list of path prefixes"))?,
                    ),
                }
            }
            None => None,
        };
        if declared.is_none() && !t.tracked.iter().any(|f| names_feature(f, &keys)) {
            return Err(format!(
                "the feature that {spec} drives cannot be resolved: no tracked file under {pkg}/src is named {}, and {pkg}/{DECLARATION} does not list `{name}`. Add `\"{name}\": [\"src/...\"]` (the source prefixes it drives, relative to {pkg}) to that file",
                keys.join(" or ")
            ));
        }
        for c in changed {
            if *c == spec {
                out.push(format!("{c} is the test {} itself", a.rel()));
            } else if !is_spec(c) && c.rsplit_once('/').is_some_and(|(d, _)| d == dir) {
                out.push(format!("{c} is beside {} in {dir}", a.rel()));
            } else if let Some(prefixes) = &declared {
                if let Some(p) = prefixes
                    .iter()
                    .find(|p| c.strip_prefix(&format!("{pkg}/")).is_some_and(|r| r.starts_with(p.as_str())))
                {
                    out.push(format!("{c} is source that {} declares it drives ({p})", a.rel()));
                }
            } else if c.starts_with(&format!("{pkg}/")) && names_feature(c, &keys) {
                out.push(format!("{c} is source of the feature {} drives ({})", a.rel(), keys.join(" or ")));
            }
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}

/// The ER-12 check of one FLAKY test of a run: on the accepted list, and neither it nor what it drives touched by the train.
pub struct FlakyCheck<'a> {
    /// What `--accept-flaky` named.
    pub accepted: &'a [Accepted],
    /// The files the train changes against main.
    pub changed: &'a [String],
    /// The package's tracked source and declaration.
    pub tree: &'a dyn Fn(&Accepted) -> Result<Tree, String>,
}

impl FlakyCheck<'_> {
    /// `Ok` when the test (a name from a triage row) may be accepted.
    pub fn check(&self, app: &str, test: &str) -> Result<(), String> {
        let loc = location_of(test).ok_or_else(|| "the test name carries no spec location".to_string())?;
        let a = self.accepted.iter().find(|a| a.covers(&loc) && a.app() == app).ok_or_else(|| {
            if app.is_empty() {
                format!(
                    "{loc}: the app of the row cannot be told (no `app <name>` job, not an app's own workflow), so it cannot be accepted"
                )
            } else {
                format!("{loc} of app {app} is not on the --accept-flaky list")
            }
        })?;
        let touched = touched(std::slice::from_ref(a), self.changed, self.tree)?;
        if touched.is_empty() {
            Ok(())
        } else {
            Err(format!("the train touches what it accepts: {}", touched.join("; ")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PKG: &str = "apps/debrief/app";

    fn acc(l: &str) -> Vec<Accepted> {
        vec![Accepted::parse(&format!("{PKG}/{l}")).unwrap()]
    }
    fn ch(files: &[&str]) -> Vec<String> {
        files.iter().map(|s| format!("{PKG}/{s}")).collect()
    }
    fn tree(tracked: &[&str], declaration: Option<&str>) -> impl Fn(&Accepted) -> Result<Tree, String> {
        let tracked: Vec<String> = tracked.iter().map(|f| format!("{PKG}/{f}")).collect();
        let declaration = declaration.map(str::to_string);
        move |_| {
            Ok(Tree {
                tracked: tracked.clone(),
                declaration: declaration.clone(),
            })
        }
    }
    const SRC: [&str; 3] = [
        "src/app/live-tail/live-tail.component.ts",
        "src/app/events/live-tail-follow.ts",
        "src/app/explorer/grid.ts",
    ];

    /// Both ways: an untouched test is accepted (positive control), and each way of touching it refuses.
    #[test]
    fn a_test_the_train_did_not_touch_is_accepted() {
        let changed = ch(&["src/app/explorer/grid.ts", "e2e/other.e2e.ts"]);
        assert_eq!(
            touched(&acc("e2e/live-tail-follow.e2e.ts:240"), &changed, &tree(&SRC, None)),
            Ok(vec![])
        );
    }

    #[test]
    fn the_test_file_itself_refuses_and_so_does_a_helper_beside_it_but_not_another_spec() {
        let t = tree(&SRC, None);
        let a = acc("e2e/live-tail-follow.e2e.ts:240");
        let r = touched(&a, &ch(&["e2e/live-tail-follow.e2e.ts"]), &t).unwrap();
        assert!(r.len() == 1 && r[0].contains("is the test"), "{r:?}");
        assert!(touched(&a, &ch(&["e2e/fixtures.ts"]), &t).unwrap()[0].contains("beside"));
        assert!(
            touched(&a, &ch(&["e2e/helpers/wait.ts", "e2e/ux-home.e2e.ts"]), &t)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn the_source_of_the_feature_the_spec_drives_refuses_but_only_in_its_own_app() {
        let t = tree(&SRC, None);
        let a = acc("e2e/live-tail-follow.e2e.ts:240");
        for f in ["src/app/live-tail/live-tail.component.ts", "src/app/events/live-tail-follow.ts"] {
            let r = touched(&a, &ch(&[f]), &t).unwrap();
            assert_eq!(r.len(), 1, "{f}: {r:?}");
            assert!(r[0].contains("source of the feature"), "{r:?}");
        }
        // the same name in another app, or outside src, is not this feature
        let other = vec!["apps/dyno/app/src/app/live-tail/x.ts".to_string(), "docs/live-tail.md".to_string()];
        assert_eq!(touched(&a, &other, &t), Ok(vec![]));
    }

    #[test]
    fn a_declaration_overrides_the_name_match() {
        let decl = r#"{"live-tail-follow.e2e.ts": ["src/app/tail-core/"], "a11y.e2e.ts": []}"#;
        let t = tree(&SRC, Some(decl));
        let a = acc("e2e/live-tail-follow.e2e.ts:240");
        // declared source refuses; the name-matched file no longer does
        assert!(touched(&a, &ch(&["src/app/tail-core/x.ts"]), &t).unwrap()[0].contains("declares it drives"));
        assert_eq!(touched(&a, &ch(&["src/app/live-tail/live-tail.component.ts"]), &t), Ok(vec![]));
        // an empty list is a resolution: it drives none of the app's source
        let b = acc("e2e/a11y.e2e.ts:6:7");
        assert_eq!(touched(&b, &ch(&["src/app/explorer/grid.ts"]), &t), Ok(vec![]));
    }

    /// Fail closed: no declaration and no file named like the feature is an unresolved feature, never "touches nothing".
    #[test]
    fn an_unresolved_feature_is_refused_with_the_file_to_add_a_declaration_to() {
        let err = touched(&acc("e2e/a11y.e2e.ts:6:7"), &ch(&["docs/x.md"]), &tree(&SRC, None)).unwrap_err();
        assert!(
            err.contains("cannot be resolved") && err.contains("e2e/feature-sources.json") && err.contains("a11y.e2e.ts"),
            "{err}"
        );
        // a declaration that lists another spec does not resolve this one
        let t = tree(&SRC, Some(r#"{"other.e2e.ts": []}"#));
        assert!(touched(&acc("e2e/a11y.e2e.ts:6:7"), &[], &t).is_err());
        // an unreadable declaration is refused, not ignored
        for bad in ["not json", "[]", r#"{"a11y.e2e.ts": "src/"}"#] {
            let t = tree(&SRC, Some(bad));
            assert!(touched(&acc("e2e/a11y.e2e.ts:6:7"), &[], &t).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_flaky_check_wants_the_list_and_an_untouched_feature() {
        let t = tree(&SRC, None);
        let accepted = acc("e2e/live-tail-follow.e2e.ts:240");
        let log = "[webkit] › e2e/live-tail-follow.e2e.ts:240:7 › follows the tail";
        let untouched = ch(&["src/app/explorer/grid.ts"]);
        let check = FlakyCheck {
            accepted: &accepted,
            changed: &untouched,
            tree: &t,
        };
        assert_eq!(check.check("debrief", log), Ok(()));
        assert!(
            check
                .check("debrief", "[webkit] › e2e/other.e2e.ts:1:1 › x")
                .unwrap_err()
                .contains("not on the --accept-flaky list")
        );
        assert!(check.check("debrief", "no location here").is_err());
        let touched = ch(&["src/app/live-tail/live-tail.component.ts"]);
        let check = FlakyCheck {
            accepted: &accepted,
            changed: &touched,
            tree: &t,
        };
        assert!(check.check("debrief", log).unwrap_err().contains("touches what it accepts"));
        // an unresolved feature is refused here too
        let none = tree(&[], None);
        let check = FlakyCheck {
            accepted: &accepted,
            changed: &untouched,
            tree: &none,
        };
        assert!(check.check("debrief", log).unwrap_err().contains("cannot be resolved"));
    }

    /// Two apps share a spec name: an acceptance for one is no acceptance for the other's row.
    #[test]
    fn a_spec_name_shared_by_two_apps_is_accepted_for_its_own_app_only() {
        let accepted = vec![Accepted::parse("apps/debrief/app/e2e/a11y.e2e.ts:6:7").unwrap()];
        let t = |_: &Accepted| {
            Ok(Tree {
                tracked: vec!["apps/debrief/app/src/app/a11y/a11y.ts".to_string()],
                declaration: None,
            })
        };
        let none: Vec<String> = Vec::new();
        let check = FlakyCheck {
            accepted: &accepted,
            changed: &none,
            tree: &t,
        };
        let row = "[webkit] › e2e/a11y.e2e.ts:6:7 › main screen";
        assert_eq!(check.check("debrief", row), Ok(()));
        let err = check.check("dyno", row).unwrap_err();
        assert!(err.contains("of app dyno is not on the --accept-flaky list"), "{err}");
        assert!(check.check("", row).unwrap_err().contains("the app of the row cannot be told"));
    }
}
