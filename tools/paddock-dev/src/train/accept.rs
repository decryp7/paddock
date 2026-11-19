//! ER-12 made enforced: a flaky test may be accepted only when this train did not touch it. `--accept-flaky <location>` is refused
//! when the train's diff against main (`git diff origin/main...head`) changes the test file, a helper beside it, or the source of
//! the feature the spec drives. "The feature" is mapped by name, not by import graph: a source file counts when a folder or a file
//! stem on its path equals the spec's own name or its name without the last word (`live-tail-follow.e2e.ts` maps to `live-tail-follow`
//! and `live-tail`; `a11y.e2e.ts` to `a11y`), under the package that holds the spec's `e2e` or `src` folder. That is a stated
//! approximation of the rule (a spec that drives code under another name is not caught by it); the test file and the folder it
//! sits in are always checked.

use super::verdict::Accepted;

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

/// The files of `changed` that make `accepted` unacceptable, one line each; empty when the train did not touch the test.
pub fn touched(accepted: &[Accepted], changed: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for a in accepted {
        let file = a.file();
        let dir = file.rsplit_once('/').map_or("", |(d, _)| d);
        let keys = feature_keys(stem_of(file));
        for c in changed {
            let same_file = c == file || c.ends_with(&format!("/{file}"));
            // A helper or fixture in the spec's own folder (another spec is a different test).
            let beside =
                !dir.is_empty() && !is_spec(c) && c.rsplit_once('/').is_some_and(|(d, _)| d == dir || d.ends_with(&format!("/{dir}")));
            // The feature's source: a path component (folder or file stem) named like the feature, under a `src` folder.
            let comps: Vec<&str> = c.split('/').collect();
            let in_src = comps.contains(&"src");
            let feature = in_src
                && comps.iter().any(|comp| {
                    let stem = comp.split('.').next().unwrap_or(comp);
                    keys.iter().any(|k| k == stem)
                });
            if same_file {
                out.push(format!("{c} is the test {file} itself"));
            } else if beside {
                out.push(format!("{c} is beside {file} in {dir}"));
            } else if feature {
                out.push(format!("{c} is source of the feature {file} drives ({})", keys.join(" or ")));
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn acc(l: &str) -> Vec<Accepted> {
        vec![Accepted::parse(l).unwrap()]
    }
    fn ch(files: &[&str]) -> Vec<String> {
        files.iter().map(|s| s.to_string()).collect()
    }

    /// Both ways: an untouched test is accepted (positive control), and each way of touching it refuses.
    #[test]
    fn a_test_the_train_did_not_touch_is_accepted() {
        let changed = ch(&[
            "docs/lessons.md",
            "apps/debrief/app/src/app/explorer/grid.ts",
            "apps/debrief/app/e2e/other.e2e.ts",
        ]);
        assert!(touched(&acc("e2e/live-tail-follow.e2e.ts:240"), &changed).is_empty());
        assert!(touched(&acc("e2e/a11y.e2e.ts:6:7"), &changed).is_empty());
    }

    #[test]
    fn the_test_file_itself_refuses() {
        let r = touched(&acc("e2e/a11y.e2e.ts:6:7"), &ch(&["apps/debrief/app/e2e/a11y.e2e.ts"]));
        assert_eq!(r.len(), 1);
        assert!(r[0].contains("is the test"), "{r:?}");
    }

    #[test]
    fn a_helper_in_the_specs_folder_refuses_but_another_spec_does_not() {
        let helper = ch(&["apps/debrief/app/e2e/helpers/wait.ts"]);
        // a different folder: `e2e/helpers` is not `e2e`
        assert!(touched(&acc("e2e/a11y.e2e.ts:6:7"), &helper).is_empty());
        let beside = ch(&["apps/debrief/app/e2e/fixtures.ts"]);
        assert!(touched(&acc("e2e/a11y.e2e.ts:6:7"), &beside)[0].contains("beside"));
        assert!(touched(&acc("e2e/a11y.e2e.ts:6:7"), &ch(&["apps/debrief/app/e2e/ux-home.e2e.ts"])).is_empty());
    }

    #[test]
    fn the_source_of_the_feature_the_spec_drives_refuses() {
        let a = acc("e2e/live-tail-follow.e2e.ts:240");
        for f in [
            "apps/debrief/app/src/app/live-tail/live-tail.component.ts",
            "apps/debrief/app/src/app/events/live-tail-follow.ts",
        ] {
            let r = touched(&a, &ch(&[f]));
            assert_eq!(r.len(), 1, "{f}: {r:?}");
            assert!(r[0].contains("source of the feature"), "{r:?}");
        }
        // the same name outside a src folder is not source
        assert!(touched(&a, &ch(&["docs/live-tail.md"])).is_empty());
    }
}
