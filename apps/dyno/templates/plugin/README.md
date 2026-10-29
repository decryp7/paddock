# Dyno plugin template

A copyable starting point for a Dyno plugin: a small Rust executable that Dyno starts out of process and talks to
over its private IPC. The full guide is [docs/plugins.md](../../docs/plugins.md).

1. Copy this folder and rename `dyno-plugin-mine` and `mine` (in `Cargo.toml`, `plugin.toml` and `src/main.rs`).
   The id in `plugin.toml`, the id in the manifest and the plugin's folder name must be the same.
2. Outside a Dyno checkout, replace the `path` dependency in `Cargo.toml` with the git dependency shown there.
3. Implement `SensorProvider` and list `sensors` in both the manifest and `contributes` in `plugin.toml`.
4. Build: `cargo build --release`.
5. Install: copy `plugin.toml` and the executable from `target/release/` into
   `<Dyno data dir>/plugins/<id>/` and restart Dyno.

Running the executable by hand only prints that Dyno starts it itself: it reads its connection details from stdin.
