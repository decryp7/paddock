# Done = these pass

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny --workspace check
node --test scripts/commit-lint.test.mjs scripts/release/latest-json.test.mjs
(cd app && npm audit --omit=dev --audit-level=high && npx ng lint && npx ng build && npx ng test --watch=false)
```
Plus: dyno-core ≥ 99 % lines; Angular ≥ 80 % overall; screenshots re-captured for UI changes (docs/screenshots, both themes); Conventional Commit without attribution trailers.
