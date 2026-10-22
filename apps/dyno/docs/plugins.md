# Writing Dyno plugins

A Dyno plugin is a small **Rust** executable that adds sensors, cleanup categories or rule actions. Dyno's agent
starts it as a separate process and talks to it over Dyno's one IPC; a plugin can never take Dyno down.

- Start from [`templates/plugin`](../templates/plugin) (copy it) and read
  [`plugins/dyno-plugin-example`](../plugins/dyno-plugin-example) for a complete plugin with a sensor and a cleanup
  category.
- A plugin depends on **`dyno-plugin-api` only**. Its `main` is one line: `dyno_plugin_api::serve(MyPlugin)`.

## What a plugin can contribute

| Contribution | Trait | Called | Timeout |
|---|---|---|---|
| `sensors` | `SensorProvider::sensors() -> Vec<SensorReading>` | about once a second | 2 s |
| `cleanup` | `CleanupProvider::categories()`, `scan(category, limits) -> Result<Vec<Candidate>, PluginError>` | when the user scans | 60 s |
| `actions` | `ActionProvider::actions()`, `run(action_id, RuleEvent) -> Result<String, PluginError>` | when a rule fires | 10 s |

`Plugin::manifest()` names the plugin and lists its contributions; `Plugin::as_sensors()` / `as_cleanup()` / `as_actions()`
return `Some(self)` for each one it implements. Calls can arrive on several threads at once (a scan does not block
sensor readings), so providers take `&self`; use a `Mutex` for state that changes.

**Cleanup plugins only propose.** A `Candidate` (absolute path, size, modification time, reason) is a suggestion:
Dyno drops relative paths, applies its cleanup safety policy (protected locations, personal files, Recycle Bin /
Trash or quarantine), shows the preview and deletes only what the user accepts. A plugin must never delete anything
itself. Choose the category's `risk` honestly: `safe` (regenerated caches), `review`, or `personal`.

Return a `PluginError` for an expected failure ("the folder is missing"): it is shown to the user and does not count
against the plugin. Panics, hangs and crashes do.

## plugin.toml

Every plugin ships a `plugin.toml` next to its executable:

```toml
id = "example"                     # 1–64 of a-z 0-9 - _; also the folder name
name = "Dyno example"
version = "0.1.0"
api_version = 1                    # dyno_plugin_api::API_VERSION it was built against
executable = "dyno-plugin-example" # a file name in the same folder; ".exe" is added on Windows; default: the id
contributes = ["sensors", "cleanup"]
```

The id, the folder name and the manifest's id must agree, and `api_version` must be the one this Dyno speaks;
otherwise the plugin is listed as failed with a message saying what to fix. Unknown keys are refused.

## Build and install

```sh
cargo build --release
```

Copy `plugin.toml` and the executable into `<Dyno data dir>/plugins/<id>/`:

- Windows: `%LOCALAPPDATA%\Dyno\plugins\<id>\`
- macOS: `~/Library/Application Support/Dyno/plugins/<id>/`

Restart Dyno. The plugin appears in the plugin list as stopped, starts on first use, and then runs. Settings a plugin
needs come from its environment or a file in its folder: Dyno starts it with its folder as the working directory and
sets `DYNO_PLUGIN_ID` and `DYNO_PLUGIN_DIR`. The example plugin reads `DYNO_EXAMPLE_TMP_DIR`.

Running a plugin by hand only prints that Dyno starts it itself.

## How Dyno runs a plugin

- **Out of process, always.** Dyno creates a private endpoint (a randomly named, owner-only, local-only named pipe
  on Windows; a socket in a fresh `0700` directory on macOS) and a 256-bit random token, starts the plugin with no
  console window and passes both on its **stdin** as one JSON line. The plugin connects and presents the token; Dyno
  checks it in constant time, checks the protocol version and that the connecting process is the one it started.
- **The lifeline.** stdin stays open; when Dyno exits or stops the plugin, stdin closes and the plugin exits. On
  Windows the plugin also runs in a job object that is killed with Dyno and caps its memory (256 MiB by default);
  past the cap its allocations fail and it stops.
- **The wire.** Frames of a 4-byte little-endian length and a [postcard](https://docs.rs/postcard) body, at most
  4 MiB. The body is `dyno_plugin_api::PluginMessage`: `PluginRequest` from Dyno, `PluginReply` from the plugin.
  Both enums are append-only, so adding a request does not break older plugins; anything incompatible is a new
  `API_VERSION`, refused with a message.
- **Failures are the plugin's.** A panic, a hang (killed after the timeout), an exit, garbage on the wire or a
  manifest that disagrees with `plugin.toml` is reported as an error of that plugin; every other plugin and Dyno
  keep working. Dyno restarts a failed plugin with exponential backoff (1 s, 2 s, 4 s, … up to 60 s) and
  **quarantines** it after 3 failures in 10 minutes. A quarantined plugin stays off until the user clears it.

## Testing a plugin

Test providers as plain Rust: they are ordinary methods. `dyno_plugin_api::handle(&plugin, request)` runs the same
dispatch `serve` uses, without a connection, so a test can send `PluginRequest`s and check the `PluginReply`s.
