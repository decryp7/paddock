//! paddock-dev's integration tests, one binary (C37): the command line, the planner against `scripts/affected.mjs`, the
//! `inputs.txt` list, `dev fetch` against a loopback server, the slot commands with the real binary and the `./dev` wrapper.
mod cli;
mod fetch;
mod golden;
mod hostile;
mod inputs;
mod slots;
mod wrapper;
