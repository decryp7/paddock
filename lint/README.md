# Lint presets

| File | Used by |
|---|---|
| `clippy.toml` | an app's root `clippy.toml` must list every `disallowed-methods` path of this file (C13) |
| `workspace-lints.toml` | an app's `[workspace.lints.rust]` and `[workspace.lints.clippy]` must have these lints at this level or stricter, and every member crate sets `[lints] workspace = true` (C13) |
| `../eslint/` | `@decryp7/paddock-eslint` (rules 27 to 39) |

`paddock-conformance --app <name> --paddock <paddock>` runs C13 on the app.
