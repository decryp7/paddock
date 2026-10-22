# Pitwall plugin template

A plugin is a small Rust program that Pitwall starts in its own sandboxed process (no network unless you ask for it
and the user allows it). It depends on `pitwall-plugin-api` only; its `main` is one line. See ADR 0008
(`docs/adr/0008-plugins.md`) for how plugins are run and contained.

## Three steps

1. **Copy and rename.** Copy this folder, set `name` in `Cargo.toml`, and in `pitwall-plugin.toml` set `id`, `name`,
   `kinds` and `executable` (the binary's name). Outside this repository, point the dependency at Pitwall's git
   repository instead of `workspace = true`.
2. **Implement.** Replace `Hello` in `src/main.rs` with your contribution: `Tools`, `Filter`, `Endpoints` or
   `Engine` (one per kind you list in the manifest), and register it on the `Plugin` (`.tools(..)`, `.filter(..)`,
   ...). Test it with `Plugin::answer` as the test in `src/main.rs` does: no IPC needed. Run `cargo test`.
3. **Install.** `cargo build --release`, then create `<Pitwall's plugins folder>/<id>/` holding your
   `pitwall-plugin.toml` and the binary from `target/release/`. Pitwall finds it on its next scan and shows its
   sandbox status; it starts on first use.

Rules of the road: never panic on input (return `PluginError`), keep within your memory cap, never log request
content, and ask for `network = true` only if the plugin cannot work without it.
