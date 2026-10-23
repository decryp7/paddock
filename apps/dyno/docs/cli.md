# The `dyno` command

`dyno` talks to the running Dyno agent over the same local protocol as the app (ADR 0014): it reads `agent.json` in the data folder
(`DYNO_DATA_DIR`, else `%LOCALAPPDATA%\Dyno`, `~/Library/Application Support/Dyno` or `~/.local/share/dyno`), connects with the
per-launch token and asks the same requests. It never links the platform adapters, so it is small and cannot do anything the app
cannot. Build it with `cargo build -p dyno-cli` (the binary is named `dyno`).

## Read-only commands

| Command | What it shows |
| --- | --- |
| `dyno status [--json]` | CPU, memory and pressure, swap, disks, network, battery, uptime, profile, Dyno's own cost |
| `dyno why-slow [--json]` | The diagnosis: headline, causes with confidence, who is involved, what you could do |
| `dyno top [--by cpu\|memory] [-n N] [--json]` | The busiest apps (default CPU, 10 rows) |
| `dyno alarms [--all] [--json]` | Alarms that need attention (`--all` includes shelved ones) |
| `dyno snapshot` | Everything measured now, with every process, as JSON. Nothing is stored |
| `dyno counters list [OBJECT] [--json]` | The performance counter objects, or one object's counters and instances |
| `dyno counters search WORDS... [--json]` | Counters matching all the words |
| `dyno counters get PATH... [--json]` | Reads counters now; paths look like `\Memory\Free` or `\Network Interface(en0)\Bytes Sent`. Rates are read twice a second apart |

`--json` output is data in camelCase, the same shapes as the app's contract (`status` and `top` have their own small stable shapes).
Human output has no command lines or file contents.

## Commands that change something

| Command | Effect |
| --- | --- |
| `dyno end PID --yes` | Asks the app with that process id to exit, then ends it |
| `dyno cap PID PERCENT --yes` | Limits the app to PERCENT (1 to 100) of the machine's CPU |
| `dyno undo ID --yes` | Undoes an entry of the activity log |

Without `--yes` nothing is sent and the command says what it would do. The target is resolved to the exact process (id and start time),
and the agent applies its protected list and records the activity as for a click, so a protected process is refused and `cap` prints
the `dyno undo` command that reverses it.

## Exit codes

`0` done, `1` failed (the message says why), `2` wrong usage or `--yes` missing, `3` this `dyno` and the running Dyno speak different
protocol versions (update the older one), `4` Dyno is not running.
