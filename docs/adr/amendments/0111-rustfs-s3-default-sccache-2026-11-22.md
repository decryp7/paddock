# Amendment to ADR 0111 (2026-11-22): RustFS S3 replaces Redis as the way to share compiles

- Status: Accepted (user, 2026-11-21). Amends ADR 0111, Consequences: "Redis stays the way to share compiles". Built on `feat/sccache-s3-default`; see `setup-rust/action.yml`.

The compile cache that lanes share is the RustFS S3 server by default, with the local disk as the fallback (see
`0014-rustfs-s3-default-sccache-2026-11-22.md`). Release, rule 54 and tag builds stay on the local disk. 0111's rule is unchanged: no
writable folder is shared between concurrent lanes, so each lane keeps its own local sccache folder for the fallback.
