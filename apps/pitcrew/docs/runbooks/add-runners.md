# Runbook: add runners to a CI host (the interim before auto-scale)

- Why: [ADR 0021](../adr/0021-auto-scale.md), decision D8 (the owner, 2026-10-28). Until the scaler is built, the Mac
  and the Windows VM get extra runners that always run (d-gtr9 is one runner with capacity 4 and needs none). When the
  scaler lands, these runners become its pools' slots: nothing is registered twice.
- Who: the owner. Registering needs a registration token, and the hosts are only reachable by the owner.
- Checked on 2026-10-28 on the Mac's runner (`gitea-runner --version`: v5.2.3): `register --token-file`; `config set`,
  including map keys such as `runner.envs.X`; `config get runner.labels`, which prints one label per line; and the
  defaults `runner.timeout: 3h` and `runner.shutdown_timeout: 0s`. **Not run on d-gtr9 or the Windows VM.** That d-gtr9
  runs its runner as a Docker Compose service under `/root/docker-data/act_runner` comes from the review, not from a look
  at the host.

## The gate: family ADR 0025 first for the Mac and Windows

**Do not add runners on the Mac mini or the Windows VM before family ADR 0025 (CI job isolation) is merged with its
Rust slot lock.** On 2026-10-28 it is Proposed on branch `ci/concurrent-jobs`, not merged. On main, two host jobs on one
machine break each other, and a separate CI home per runner does not prevent it:

- Several CI files hard-code CI-home paths instead of following `PADDOCK_CI_HOME`. Examples: `setup-rust/action.yml`,
  `ci.yml`, `debrief.yml`, `conformance/ci/windows-node-modules.sh`, and the shell and workbench demo release workflows.
- `host-cleanup` kills test processes by exact name (`paddock.ci.json` `processes`). Dyno's and Pitwall's process
  patterns are anchored on `.paddock-ci/`, so they match every runner's jobs.
- sccache runs on one fixed port (4227) and is restarted by every job.

**d-gtr9 (Linux) needs no extra runners.** It is one runner instance with `runner.capacity: 4` (the owner, 2026-10-28),
so it already runs four container jobs at once, the Linux max decided in ADR 0021. Only step 1 applies there.

So the order is:

1. **Now, every host:** give the existing runner `runner.shutdown_timeout: 3h`, and on d-gtr9 also
   `stop_grace_period: 3h` (next section).
2. **After family ADR 0025 is merged:** add runners on the Mac and the Windows VM, all sharing the host's one CI home.

## Step 1 (every host, now): stop cancelling jobs on a stop

With the default `shutdown_timeout: 0s`, a runner that is stopped while a job runs cancels it. The Mac's runner log
shows 9 shutdowns that "cancelled in progress jobs" (counted on 2026-10-28). Set it to the job timeout:

- **Pitcrew** (the Mac, and the Windows VM if Pitcrew runs there): runner page ▸ Settings ▸ `runner.shutdown_timeout` =
  `3h`, Save. Pitcrew restarts the runner after its job.
- **By hand:** `<runner binary> config -c <its config.yaml> set runner.shutdown_timeout 3h`, then restart the runner
  between jobs.
- **d-gtr9 (Docker Compose):** set it in the runner's `config.yaml` under its `/data` folder, and give the service
  `stop_grace_period: 3h` in the compose file. Otherwise Docker kills the container after 10 s, whatever the runner's
  setting. Then `docker compose up -d <service>` between jobs.

A stop that catches a running job now waits for it (up to 3 h) instead of cancelling it.

## How many, and their names

| Host | Existing runner | Add | New names | Total | When |
|---|---|---|---|---|---|
| Linux `d-gtr9` | one runner, capacity 4 | 0 | - | 4 jobs at once | already met |
| Mac mini | `d7-mac-mini`, folder `/Users/decryp7/gitea-runner` | 2 | `d7-mac-mini-2`, `d7-mac-mini-3` | 3 (the scaler's max will be 4) | after family ADR 0025 |
| Windows VM `decryp7ology-dev` | its current runner (`<existing>`) | 1 | `<existing>-2` | 2 (the decided max) | after family ADR 0025 |

Every new runner gets exactly the labels of the existing runner on that host. The commands below read them from its
`config.yaml`; never take them from `.runner`, which holds the runner's credentials.

**Disk.** On the Mac, `~/.paddock-ci` holds 103 GB today (measured 2026-10-28); 308 GB were free. All but about 10 GB of
it (sccache, npm, tools) is kept per slot under family ADR 0025, so plan for about 90 GB per extra host runner,
bounded by `PADDOCK_CI_KEEP_SLOTS`.

## The token (every host)

1. In Gitea, open **User settings ▸ Actions ▸ Runners ▸ Create new runner** and copy the registration token. The
   runners are user-level (ADR 0021 D2). One token registers any number of runners until it is reset.
2. The commands below put it in a file only you can read and delete that file even when a step fails (`trap` /
   `finally`). Never paste the token on a command line, into Pitcrew's files or into a chat.
3. When done, clear the clipboard: copy something else, or on the Mac `pbcopy </dev/null`.

## Mac mini: after family ADR 0025 is merged

All runners on the Mac **share one CI home** (`~/.paddock-ci`). ADR 0025 gives each job a slot with its own kept folders
and ports, and `PADDOCK_CI_KEEP_SLOTS` bounds them: set it to the number of runners on the host.

```sh
set -eu
SRC=/Users/decryp7/gitea-runner
SERVER='<the server URL d7-mac-mini uses: Pitcrew card or server_url in pitcrew.toml>'
TOTAL=3                                            # runners on this Mac after this step
LABELS="$("$SRC/gitea-runner" config -c "$SRC/config.yaml" get runner.labels | paste -sd, -)"
echo "labels: $LABELS"                             # check before going on
umask 077
TOKEN_FILE=$(mktemp); trap 'rm -f "$TOKEN_FILE"' EXIT
pbpaste > "$TOKEN_FILE"; pbcopy </dev/null         # token copied first; clipboard cleared

"$SRC/gitea-runner" config -c "$SRC/config.yaml" set runner.envs.PADDOCK_CI_KEEP_SLOTS "$TOTAL"
for N in 2 3; do
  NAME="d7-mac-mini-$N"
  D="/Users/decryp7/gitea-runner-$N"
  mkdir -p "$D/work" "$D/cache"; chmod 700 "$D"
  [ -e "$D/config.yaml" ] || cp -p "$SRC/config.yaml" "$D/config.yaml"
  cp -p "$SRC/gitea-runner" "$D/"
  R="$D/gitea-runner"; C="$D/config.yaml"
  "$R" config -c "$C" set runner.file "$D/.runner"
  "$R" config -c "$C" set host.workdir_parent "$D/work"
  "$R" config -c "$C" set cache.dir "$D/cache"
  "$R" config -c "$C" set runner.timeout 3h
  "$R" config -c "$C" set runner.shutdown_timeout 3h
  "$R" config -c "$C" set runner.envs.PADDOCK_CI_KEEP_SLOTS "$TOTAL"
  if [ ! -s "$D/.runner" ]; then
    (cd "$D" && "$R" register -c "$C" --no-interactive --instance "$SERVER" --token-file "$TOKEN_FILE" \
        --name "$NAME" --labels "$LABELS")
  fi
  chmod 600 "$D/.runner" "$C"
done
```

The script can be run again: it keeps folders and registrations that already exist. Then, in Pitcrew, for each folder:
**Runners ▸ Register runner**, the same server, name `d7-mac-mini-N`, the same labels, **An existing runner folder** =
`/Users/decryp7/gitea-runner-N`. A folder with a `.runner` is adopted as it is, and no token is sent. Restart
`d7-mac-mini` after its job so that it picks up `PADDOCK_CI_KEEP_SLOTS`.

## Windows VM `decryp7ology-dev`: after family ADR 0025 is merged

In PowerShell, as the user that owns the existing runner's folder:

```powershell
$ErrorActionPreference = 'Stop'
$Src      = '<existing runner folder>'           # the folder on its Pitcrew card
$Existing = '<existing runner name>'
$Server   = '<the server URL the existing runner uses>'
$Total    = 2
$Bin      = Join-Path $Src 'gitea-runner.exe'    # or act_runner.exe: use the existing name
$Labels   = (& $Bin config -c "$Src\config.yaml" get runner.labels) -join ','
"labels: $Labels"                                 # check before going on
$TokenFile = Join-Path $env:TEMP ("rt-" + [guid]::NewGuid() + '.txt')
try {
  New-Item -ItemType File -Path $TokenFile | Out-Null
  icacls $TokenFile /inheritance:r /grant:r "${env:USERNAME}:F" | Out-Null
  Get-Clipboard | Set-Content -NoNewline $TokenFile; Set-Clipboard -Value ' '   # token copied first

  & $Bin config -c "$Src\config.yaml" set runner.envs.PADDOCK_CI_KEEP_SLOTS $Total
  $N = 2
  $D = "$Src-$N"; $R = Join-Path $D (Split-Path $Bin -Leaf); $C = "$D\config.yaml"
  New-Item -ItemType Directory -Path $D, "$D\work", "$D\cache" -Force | Out-Null
  if (-not (Test-Path $C)) { Copy-Item "$Src\config.yaml" $C }
  Copy-Item $Bin $D -Force
  & $R config -c $C set runner.file "$D\.runner"
  & $R config -c $C set host.workdir_parent "$D\work"
  & $R config -c $C set cache.dir "$D\cache"
  & $R config -c $C set runner.timeout 3h
  & $R config -c $C set runner.shutdown_timeout 3h
  & $R config -c $C set runner.envs.PADDOCK_CI_KEEP_SLOTS $Total
  if (-not (Test-Path "$D\.runner")) {
    Push-Location $D
    try { & $R register -c $C --no-interactive --instance $Server --token-file $TokenFile --name "$Existing-$N" --labels $Labels }
    finally { Pop-Location }
  }
} finally {
  Remove-Item $TokenFile -ErrorAction SilentlyContinue
}
```

Then adopt the folder in Pitcrew as on the Mac. In step 4 of the wizard, choose the same session as the existing runner.
`runner.envs` is the runner's own setting and applies on Windows; Pitcrew's per-runner `env` does not yet (roadmap).

## Migration: from a separate CI home per runner to the shared one

An earlier draft of this runbook gave each extra runner its own `runner.envs.PADDOCK_CI_HOME`. Under family ADR 0025,
all runners of a host share one CI home, and the scaler rejects a per-runner one (ADR 0021 §1). If a runner has it:

1. Drain the runner in Pitcrew and wait for *Paused*.
2. Remove the `PADDOCK_CI_HOME:` line under `runner.envs` in its `config.yaml`, with Pitcrew's Advanced editor or a text
   editor (`config set` cannot delete a key). Add `PADDOCK_CI_KEEP_SLOTS` as above.
3. Resume the runner, and delete its old CI home (`~/.paddock-ci-N`) to free the disk.

## Verify that they take jobs

1. **Gitea:** User settings ▸ Actions ▸ Runners lists the new runners as online (Idle) with the same labels as the
   existing one.
2. **Pitcrew** (where it runs): every card reads Idle, and the runner's Logs show its `supervisor:` start line with no
   registration error.
3. **A real job:** at the next busy period (or dispatch `ci.yml` for an app as usual), two or more runners of one host
   are busy at once (Gitea's runner list, Pitcrew's cards). The job log's first step prints `runner: <name>`. On the Mac
   and Windows, the job's isolation step names its slot (`s1`, `s2`, …), and two concurrent jobs never have the same one.
4. **Queue:** the Insights page (ADR 0015) shows the queue wait falling over the next day.
5. **Stop safety (once per host):** while a runner runs a short job, stop it (Pitcrew **Stop**, or
   on d-gtr9 `docker compose stop <service>`). It waits for the job, and the job finishes green.

## Roll back

For each added runner:

1. Stop it after its job. In Pitcrew: **Drain**, wait for *Paused*, then remove the runner (the folder stays).
2. In Gitea: User settings ▸ Actions ▸ Runners ▸ the runner ▸ **Delete**. A runner still running exits when its
   registration is removed.
3. Delete its folder (`gitea-runner-N`), which holds its `.runner`. Lower
   `PADDOCK_CI_KEEP_SLOTS` on the remaining runners of the host; ADR 0025 sweeps the kept folders of slots above it.
4. If the token may have leaked (a file left behind, a paste in the wrong place): **Reset** the registration token on
   the same Gitea page. Registered runners keep working; the old token stops registering new ones.

Leave `runner.shutdown_timeout: 3h` (and `stop_grace_period: 3h` on d-gtr9) in place; they only make stops safer.
