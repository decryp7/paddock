//! paddock-dev's integration tests, one binary (C37): the command line, the planner against `scripts/affected.mjs`, the
//! `inputs.txt` list, the slot commands with the real binary, the CI kit's job-isolation.sh over it and the `./dev` wrapper.
mod cli;
mod golden;
mod hostile;
mod inputs;
mod isolation;
mod repo;
mod slots;
mod wrapper;
