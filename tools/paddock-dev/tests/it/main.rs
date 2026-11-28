//! paddock-dev's integration tests, one binary (C37): the command line, the planner against its recorded answers, the
//! `inputs.txt` list, the slot commands with the real binary, the CI kit's job-isolation.sh over it and the `./dev` wrapper.
mod build_cost;
mod build_env;
mod build_jobs;
mod build_jobs_golden;
mod cargo_target_lint;
mod check;
mod check_allow_list;
// `mod check_c<n>;` for every tests/it/check_c<n>.rs: written by build.rs from the file names (user decision 2026-11-28, ER-40), so a new
// check's test file needs no line here.
include!(concat!(env!("OUT_DIR"), "/check_test_mods.rs"));
mod check_engine;
mod check_growth_approval;
mod check_ids;
mod check_lex;
mod check_lists;
mod check_renames;
mod check_units;
mod check_util;
mod ci_isolation;
mod ci_isolation_windows;
mod cli;
mod contracts;
mod dev_bin;
mod dev_tool_publish;
mod dev_tool_workflow;
mod doctests_off;
mod dyno_check_bundle;
mod escape_gate;
mod gates_planner;
mod generators;
mod golden;
mod heavy_install;
mod host_steps;
mod host_steps_golden;
mod hostile;
mod inputs;
mod ipc_endpoints;
mod isolation;
mod job_model;
mod lessons_merge;
mod locks;
mod locks_guard;
mod member_audits;
mod merge_check;
mod os_scoped;
mod parity_corpus;
mod plan_graph_ci;
mod pr;
mod pre_push;
mod recorded;
mod release_isolation;
mod repo_graph_net;
mod safe_remove_lint;
mod sccache_eviction;
mod setup_rust;
mod slots;
mod sparse;
mod state_folders;
mod test_features;
mod tree;
mod worktree;
mod wrapper;
mod wrapper_download;

/// The repository root, after checking that the tree around it is whole: the tests that read the repository fail with one
/// clear line in a sparse worktree (`sparse worktree: ...`), not with a missing file deep inside a test.
pub(crate) fn full_tree(root: &std::path::Path) -> std::path::PathBuf {
    if let Some(m) = paddock_dev::full_tree::sparse_problem(root) {
        panic!("{m}");
    }
    root.to_path_buf()
}

/// The `mod` lines above are hand-written (Rust has no glob of modules), so a test file under `tests/it/` that they do not name is
/// never compiled and its tests never run: every `tests/it/<name>.rs` and `tests/it/<name>/mod.rs` must have its line (ER-42). The
/// one exception is a check's own `check_c<n>.rs`, whose line build.rs writes (the `include!` above), so it cannot be missing.
#[test]
#[allow(clippy::unwrap_used)]
fn every_test_file_in_tests_it_is_a_module_of_main() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/it");
    let main = std::fs::read_to_string(dir.join("main.rs")).unwrap();
    let declared: Vec<&str> = main.lines().filter_map(|l| l.strip_prefix("mod ")?.strip_suffix(';')).collect();
    let mut missing: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| {
            let e = e.ok()?;
            let name = e.file_name().into_string().ok()?;
            let module = match name.strip_suffix(".rs") {
                Some("main") => return None,
                Some(m) => m.to_string(),
                None if e.path().join("mod.rs").is_file() => name,
                None => return None,
            };
            let generated = module
                .strip_prefix("check_c")
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
            (!generated && !declared.contains(&module.as_str())).then_some(module)
        })
        .collect();
    missing.sort();
    assert!(
        missing.is_empty(),
        "tests/it/main.rs does not compile these test files; add the lines\n{}",
        missing.iter().map(|m| format!("mod {m};")).collect::<Vec<_>>().join("\n")
    );
}
