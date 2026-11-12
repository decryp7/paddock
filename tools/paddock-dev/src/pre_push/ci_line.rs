//! The conformance command CI runs for Paddock itself, read from `.gitea/workflows/ci.yml` in this one place (ADR 0038 §1: the
//! check list exists once; until the contract file of WP-3 holds it, the YAML line is the source and the push gate reads it).

/// The arguments after `node` of the one `node conformance/bin/paddock-conformance.mjs --app paddock ... --only ...` line, or why
/// there is not exactly one (zero or two lines make the check broken, not passed: ER-33).
pub fn conformance_args(ci_yml: &str) -> Result<Vec<String>, String> {
    let lines: Vec<&str> = ci_yml
        .lines()
        .map(str::trim)
        .filter(|l| !l.starts_with('#'))
        .map(|l| l.strip_prefix("run:").map_or(l, str::trim))
        .filter(|l| l.starts_with("node conformance/bin/paddock-conformance.mjs") && l.contains("--app paddock") && l.contains("--only"))
        .collect();
    match lines.as_slice() {
        [one] => Ok(one.split_whitespace().skip(1).map(str::to_string).collect()),
        [] => from_env(ci_yml),
        many => Err(format!(
            "ci.yml has {} conformance lines for Paddock; the push gate cannot tell which one CI runs",
            many.len()
        )),
    }
}

/// The step names the workflow env instead (`node conformance/bin/paddock-conformance.mjs $PADDOCK_CONFORMANCE_ARGS`, since the
/// merge queue's train-gates job runs the same line): the arguments are the one `PADDOCK_CONFORMANCE_ARGS: "..."` value.
fn from_env(ci_yml: &str) -> Result<Vec<String>, String> {
    const STEP: &str = "node conformance/bin/paddock-conformance.mjs $PADDOCK_CONFORMANCE_ARGS";
    let uses_env = ci_yml
        .lines()
        .map(str::trim)
        .filter(|l| !l.starts_with('#'))
        .map(|l| l.strip_prefix("- ").map_or(l, str::trim))
        .any(|l| l.strip_prefix("run:").map_or(l, str::trim) == STEP);
    if !uses_env {
        return Err("ci.yml has no `node conformance/bin/paddock-conformance.mjs --app paddock ... --only ...` line".into());
    }
    let values: Vec<&str> = ci_yml
        .lines()
        .map(str::trim)
        .filter_map(|l| l.strip_prefix("PADDOCK_CONFORMANCE_ARGS:"))
        .map(|v| v.trim().trim_matches('"'))
        .filter(|v| v.contains("--app paddock") && v.contains("--only"))
        .collect();
    match values.as_slice() {
        [one] => Ok(std::iter::once("conformance/bin/paddock-conformance.mjs")
            .chain(one.split_whitespace())
            .map(str::to_string)
            .collect()),
        [] => Err("ci.yml's conformance step runs $PADDOCK_CONFORMANCE_ARGS, but no `PADDOCK_CONFORMANCE_ARGS: \"--app paddock ... --only ...\"` is set".into()),
        many => Err(format!(
            "ci.yml sets PADDOCK_CONFORMANCE_ARGS {} times; the push gate cannot tell which one CI runs",
            many.len()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINE: &str = "node conformance/bin/paddock-conformance.mjs --app paddock --root . --only C06,C14 --strict C36";

    #[test]
    fn reads_the_one_line_with_its_flags() {
        let yml = format!(
            "jobs:\n  c:\n    steps:\n      # node conformance/bin/paddock-conformance.mjs --app paddock --only C01\n      - name: x\n        run: {LINE}\n      - run: node conformance/bin/paddock-conformance.mjs --app dyno --only C13\n"
        );
        let args = conformance_args(&yml).unwrap();
        assert_eq!(
            args,
            [
                "conformance/bin/paddock-conformance.mjs",
                "--app",
                "paddock",
                "--root",
                ".",
                "--only",
                "C06,C14",
                "--strict",
                "C36"
            ]
        );
    }

    #[test]
    fn reads_the_arguments_from_the_workflow_env_when_the_step_names_it() {
        // ci.yml since the merge queue (feat/merge-queue-r4): the step runs `$PADDOCK_CONFORMANCE_ARGS`, the env holds the line.
        let yml = "env:\n  PADDOCK_CONFORMANCE_ARGS: \"--app paddock --root . --only C06,C14 --strict C36\"\njobs:\n  c:\n    steps:\n      - run: node conformance/bin/paddock-conformance.mjs $PADDOCK_CONFORMANCE_ARGS\n";
        assert_eq!(
            conformance_args(yml).unwrap(),
            [
                "conformance/bin/paddock-conformance.mjs",
                "--app",
                "paddock",
                "--root",
                ".",
                "--only",
                "C06,C14",
                "--strict",
                "C36"
            ]
        );
        // The env named but missing is a broken check, not an empty run.
        let missing = "jobs:\n  c:\n    steps:\n      - run: node conformance/bin/paddock-conformance.mjs $PADDOCK_CONFORMANCE_ARGS\n";
        assert!(conformance_args(missing).is_err());
    }

    #[test]
    fn none_or_two_lines_is_a_broken_check() {
        assert!(conformance_args("jobs: {}\n").unwrap_err().contains("no `node"));
        assert!(
            conformance_args(&format!("run: {LINE}\nrun: {LINE}\n"))
                .unwrap_err()
                .contains("2 conformance lines")
        );
    }
}
