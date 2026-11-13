//! The conformance command CI runs for Paddock itself, read from `.gitea/workflows/ci.yml` in this one place (ADR 0038 §1: the
//! check list exists once; until the contract file of WP-3 holds it, the YAML line is the source and the push gate reads it).

/// The workflow the line is read from, relative to the repository root.
pub const WORKFLOW: &str = ".gitea/workflows/ci.yml";

/// `./dev ci conformance-args` (ADR 0098 WP-2): the one reader of CI's conformance line, as a command, for whoever needs the
/// line and is not Rust (the generator registry's `conformance-line` entry, a person). Prints the arguments after `node`, one
/// line, space separated; a workflow that cannot be read or has no single line is NOT MEASURED (exit 6), never an empty line.
pub fn command(root: &std::path::Path) -> Result<paddock_cli::Report, paddock_cli::CliError> {
    let path = root.join(WORKFLOW);
    let not_measured = |why: String| {
        paddock_cli::CliError::new(
            paddock_cli::Exit::NotMeasured,
            format!(
                "::error::conformance args NOT MEASURED: {why}. Next step: fix {WORKFLOW} so it has one conformance line (inline, or PADDOCK_CONFORMANCE_ARGS), then run it again."
            ),
        )
    };
    let text = std::fs::read_to_string(&path).map_err(|e| not_measured(format!("{} cannot be read: {e}", path.display())))?;
    let args = conformance_args(&text).map_err(not_measured)?;
    Ok(paddock_cli::Report::new(serde_json::json!({ "args": args }), args.join(" ")))
}

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
    fn the_command_prints_the_line_and_is_not_measured_without_one() {
        let dir = tempfile::tempdir().unwrap();
        let wf = dir.path().join(WORKFLOW);
        std::fs::create_dir_all(wf.parent().unwrap()).unwrap();
        let err = command(dir.path()).err().unwrap();
        assert_eq!(err.exit, paddock_cli::Exit::NotMeasured);
        assert!(err.message.contains("::error::conformance args NOT MEASURED"), "{}", err.message);
        std::fs::write(&wf, format!("steps:\n  - name: x\n    run: {LINE}\n")).unwrap();
        let report = command(dir.path()).unwrap();
        assert_eq!(
            report.text,
            "conformance/bin/paddock-conformance.mjs --app paddock --root . --only C06,C14 --strict C36"
        );
        std::fs::write(&wf, "jobs: {}\n").unwrap();
        assert_eq!(command(dir.path()).err().unwrap().exit, paddock_cli::Exit::NotMeasured);
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
