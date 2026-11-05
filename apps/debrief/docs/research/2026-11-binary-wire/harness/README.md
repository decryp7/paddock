# Binary wire harness (ADR 0063)

Throwaway measurement code, not product code and not part of any workspace. Every payload is invented from a fixed seed. Results
are in [performance.md](../../../performance.md#2026-11-05-binary-wire-formats-debrief-adr-0063-research).

```sh
OUT=<new scratch folder>
export CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=<a build folder outside your home directory>
(cd rs && cargo build --release)
$CARGO_TARGET_DIR/release/binwire run "$OUT"        # encodes every payload; writes bytes and rust.json
$CARGO_TARGET_DIR/release/binwire serde             # serde attribute census
$CARGO_TARGET_DIR/release/binwire fuzz msgpack 200000   # also json, flex, cbor; arrow with 20000
(cd rsduck && DUCKDB_LIB_DIR=<libduckdb 2.0.0> DUCKDB_INCLUDE_DIR=<same> cargo build --release)
DYLD_LIBRARY_PATH=<libduckdb 2.0.0> $CARGO_TARGET_DIR/release/binwire-duck "$OUT"
cd js
echo '{"type":"module"}' > package.json
npm install --ignore-scripts @msgpack/msgpack msgpackr cbor-x flatbuffers apache-arrow @uwdata/flechette ion-js bson esbuild@0.25 playwright@1.64.1
node node_modules/esbuild/bin/esbuild entry-browser.mjs --bundle --format=iife --platform=browser --minify-syntax --outfile=bundle.js
node --expose-gc equiv.mjs "$OUT"; node --expose-gc run-node.mjs "$OUT" "$OUT/node.json"; node run-browsers.mjs "$OUT" "$OUT"
node extra.mjs "$OUT" "$OUT/extra.json"; node csp.mjs "$OUT"; node sizes.mjs; node args.mjs
```

Two things differ from what a plain install gives:

- **FlexBuffers reader patched.** `node_modules/flatbuffers/js/flexbuffers/reference-util.js`, as published, throws on any buffer that
  holds a 64-bit-wide value. `readInt` and `readUInt` were changed to return a `Number` for a safe integer. Without the patch the
  FlexBuffers rows of a page cannot run at all. Run `equiv.mjs` before patching to see the failure.
- **`flatc` was not run.** The FlatBuffers readers and builders are written by hand against the runtime, with the calls its generated code
  makes. `flatc` cost, generated-code size and generated-code speed are therefore not measured.

Run heavy steps through the machine's lock. Playwright uses its own pinned browsers (ER-14).
