# Commands

Use a private target dir: `CARGO_TARGET_DIR=~/.pitcrew-target CARGO_BUILD_JOBS=4`.

```sh
cargo test -p pitcrew-core                      # pure rules
cargo test -p pitcrew-service                   # incl. tests/supervision.rs (fake runner) and tests/transport.rs
cargo llvm-cov -p pitcrew-core --summary-only   # ≥ 99 % lines
cargo run -p pitcrew-service -- run             # supervise in the foreground (data dir: %ProgramData%\Pitcrew or ~/Library/Application Support/Pitcrew)
cargo run -p pitcrew-service -- check-config
(cd app && npm start)                           # UI against the mock backend
(cd app && npx tauri dev)                       # desktop app (builds the sidecar first)
(cd app && SCREENSHOTS=1 npx playwright test --project=screenshots)  # docs/screenshots (mock backend, port 4310)
(cd app && npx tauri build)                     # installer (NSIS / .dmg); unsigned without the updater secrets
(cd app && npx ng test --watch=false --coverage && npx ng lint && npx ng build)
node --test conformance/test/*.test.mjs   # (repo root) the shared kit scripts: commit lint, latest.json, publish, check-index
bash scripts/macos-bootstrap.sh && bash scripts/macos-verify.sh   # on a Mac (docs/macos.md)
```
