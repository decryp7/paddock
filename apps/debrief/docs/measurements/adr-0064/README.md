# ADR 0064 measurements

Harnesses behind the 2026-11-05 blocks in [performance.md](../../performance.md) and
[benchmarks.md](../../../../../docs/benchmarks.md). They are kept as text so no build or scanner picks them up. They were
rewritten from the run's source and not re-run from here. They measure RSS, so ADR 0064's SI-0 re-runs them in footprint,
and adds a dense EVTX chunk, `to_record` and the store.

Source tree: `git archive 3168b6145` of `apps/debrief`, `crates` and `.cargo`. The small crates sit under that tree, so the
repo's cargo config applies. All runs went through `plock run adr-stream --`, with `CARGO_BUILD_JOBS=2`, `CARGO_NET_OFFLINE=true`
and a `TMPDIR` on the scratch disk.

`evtx_m.rs.txt` needs `evtx = { version = "=0.15.1", default-features = false }`:

```sh
evtx-m make crates/debrief-plugin-windows/tests/fixtures/security.evtx l256.evtx 4096
evtx-m make crates/debrief-plugin-windows/tests/fixtures/security.evtx l1464.evtx 23424
for p in none always seq; do /usr/bin/time -l evtx-m run l256.evtx $p; done
/usr/bin/time -l evtx-m run l1464.evtx seq
```

`ipc_m.rs.txt` needs a path dependency on `crates/paddock-ipc`. `wire_m.rs.txt` also needs `serde` with `derive` and
`postcard` with `alloc`:

```sh
ipc-m
wire-m wire.log 4000000
```

`adr_stream.rs.txt` goes in `apps/debrief/crates/debrief-core/tests/adr_stream.rs`:

```sh
cargo test --release -p debrief-core --test adr_stream --no-run
/usr/bin/time -l <test binary> --ignored --nocapture many_small
/usr/bin/time -l <test binary> --ignored --nocapture bundle_shape
ADR_ONLY=8  /usr/bin/time -l <test binary> --ignored --nocapture utf16_vs_utf8
ADR_ONLY=16 /usr/bin/time -l <test binary> --ignored --nocapture utf16_vs_utf8
```

For the A/B on `many_small`, the change to `ReaderChunks::fill` in `ingest/chunk.rs` is a new field `grow: usize`, starting at
`64 << 10`, then `let want = self.block.min(self.grow); self.grow = self.grow.saturating_mul(2);`, with `want` replacing
`self.block` in `fill`. Build both binaries first, then run base, change, base, change.
