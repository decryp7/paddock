//! paddock-dev's integration tests, one binary (C37): the command line, the planner against `scripts/affected.mjs`, the
//! `inputs.txt` list, the slot commands and the VM broker with the real binary (a fake `tart`), the CI kit's job-isolation.sh over it and the `./dev` wrapper.
mod cli;
mod golden;
mod hostile;
mod inputs;
mod isolation;
mod slots;
mod vm;
mod wrapper;
