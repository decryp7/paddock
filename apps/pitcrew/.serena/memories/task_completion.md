# Done = these pass

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny --workspace check
node --test conformance/test/*.test.mjs   # (repo root) the shared kit scripts: commit lint, latest.json, publish, check-index
(cd app && npm audit --omit=dev --audit-level=high && npx ng lint && npx ng build && npx ng test --watch=false)
```
Plus, as relevant: Playwright screenshots (docs/screenshots/) for UI changes; the macOS CI job for macOS code
(it runs on the owner's Mac: keep it lean, leave nothing behind); update README/ADRs for decisions; Conventional
Commits without attribution trailers; worktree clean. Never install the app or run elevated commands on a shared dev machine.
