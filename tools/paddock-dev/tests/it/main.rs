//! paddock-dev's integration tests, one binary (C37): the command line, the planner against `scripts/affected.mjs`, the
//! `inputs.txt` list, the slot commands with the real binary, the CI kit's job-isolation.sh over it and the `./dev` wrapper.
mod cli;
mod contracts;
mod dev_tool_publish;
mod dev_tool_workflow;
mod golden;
mod hostile;
mod inputs;
mod ipc_endpoints;
mod isolation;
mod slots;
mod tree;
mod worktree;
mod wrapper;
mod wrapper_download;
