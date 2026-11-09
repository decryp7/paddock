# C102's positive control (ER-33)

One production-style listen site per primitive of `tools/paddock-dev/src/ipc_endpoints.rs` (`PRIMITIVES`), the forms the scan
must skip (`#[cfg(test)]` items, `#[test]` functions, the file of a `#[cfg(test)] mod x;`), and calls it must not report. These
files are never compiled; `tests/ipc_endpoints.rs` scans them and asserts the exact list of sites, and that removing any one
fixture file makes that list differ.
