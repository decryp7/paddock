//! paddock-dev's integration tests, one binary (C37): the command line, the planner against `scripts/affected.mjs`, the
//! `inputs.txt` list, the slot commands with the real binary, the CI kit's job-isolation.sh over it and the `./dev` wrapper.
mod build_jobs;
mod build_jobs_golden;
mod check;
mod ci_isolation;
mod ci_isolation_windows;
mod cli;
mod contracts;
mod dev_tool_publish;
mod dev_tool_workflow;
mod generators;
mod golden;
mod host_steps;
mod host_steps_golden;
mod hostile;
mod inputs;
mod ipc_endpoints;
mod isolation;
mod locks;
mod merge_check;
mod pr;
mod pre_push;
mod release_isolation;
mod safe_remove_lint;
mod setup_rust;
mod slots;
mod sparse;
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
