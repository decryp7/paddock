# The `dyno` command

`dyno` talks to the running Dyno agent over the same local protocol as the app (ADR 0014): it reads `agent.json` in the data folder
(`DYNO_DATA_DIR`, else `%LOCALAPPDATA%\Dyno`, `~/Library/Application Support/Dyno` or `~/.local/share/dyno`), connects with the
per-launch token and asks the same requests. It never links the platform adapters, so it is small. The awake and wake commands ask the agent to act, and the agent validates each request ([ADR 0032](adr/0032-power-keep-awake-wake-on-lan-idle-actions-energy.md)). Build it with `cargo build -p dyno-cli` (the binary is named `dyno`).

## Installing and putting it on your PATH

The installers ship `dyno` next to the app, as a sidecar like `dyno-agent` (`bundle.externalBin`, built by
`app/src-tauri/scripts/sidecar.mjs`; `scripts/check-bundle.sh` fails the build when it is missing). Dyno never edits your PATH,
your shell profile or system folders for you; here is how to do it yourself, once:

| OS | Where it lands | Putting it on PATH |
| --- | --- | --- |
| Linux (`.deb`) | `/usr/bin/dyno` | Nothing to do: `/usr/bin` is already on PATH |
| macOS | `Dyno.app/Contents/MacOS/dyno` | `sudo ln -s /Applications/Dyno.app/Contents/MacOS/dyno /usr/local/bin/dyno`, or add `export PATH="$PATH:/Applications/Dyno.app/Contents/MacOS"` to `~/.zshrc` |
| Windows (installer) | `%LOCALAPPDATA%\Programs\Dyno\dyno.exe` | Settings, System, About, Advanced system settings, Environment Variables: add that folder to your user `Path`; or run `dyno.exe` by its full path |

## Commands

| Command | What it shows |
| --- | --- |
| `dyno status [--json]` | CPU, memory and pressure, swap, disks, network, battery, uptime, Dyno's own cost |
| `dyno why-slow [--json]` | The diagnosis: headline, causes with confidence, who is involved, what you could do |
| `dyno top [--by cpu\|memory] [-n N] [--json]` | The busiest apps (default CPU, 10 rows) |
| `dyno alarms [--all] [--json]` | Alarms that need attention (`--all` includes shelved ones) |
| `dyno snapshot` | Everything measured now, with every process, as JSON. Nothing is stored |
| `dyno counters list [OBJECT] [--json]` | The performance counter objects, or one object's counters and instances |
| `dyno counters search WORDS... [--json]` | Counters matching all the words |
| `dyno counters get PATH... [--json]` | Reads counters now; paths look like `\Memory\Free` or `\Network Interface(en0)\Bytes Sent`. Rates are read twice a second apart |

`--json` output is data in camelCase, the same shapes as the app's contract (`status` and `top` have their own small stable shapes).
Human output has no command lines or file contents.

## Exit codes

`0` done, `1` failed (the message says why), `2` wrong usage, `3` this `dyno` and the running Dyno speak different
protocol versions (update the older one), `4` Dyno is not running.
