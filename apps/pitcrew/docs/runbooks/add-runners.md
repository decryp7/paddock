# Runbook: add runners to a CI host (the interim before auto-scale)

- Why: [ADR 0021](../adr/0021-auto-scale.md), decision D8 (the owner, 2026-10-28). Until the scaler is built, each host
  gets 2-3 more runners that always run. When the scaler lands, these runners become its pools' slots: nothing is
  registered twice.
- Who: the owner. Registering needs a registration token, and the hosts are only reachable by the owner.
- Checked on 2026-10-28 against the Mac's runner (`gitea-runner` 5.2.0): `register --token-file`, `config set` (map keys
  such as `runner.envs.X` included) and the defaults `runner.timeout: 3h` and `runner.shutdown_timeout: 0s`. The steps
  for the Linux host and the Windows VM follow the same runner CLI but were not run there.

## Read this first: jobs on one host share folders

Two jobs that run at once on one host can get in each other's way.

- **Linux (d-gtr9):** the jobs run in containers with their own workspace. Extra runners are safe.
- **Mac mini and Windows VM:** the jobs run on the host (`:host` labels), and the CI kit keeps folders under
  `${PADDOCK_CI_HOME:-~/.paddock-ci}`. These are the stable checkout `src/<repo>-<job>`, the build folders
  `target-<app>-<job>`, and the node and tool caches (`.gitea/actions/stable-workspace`, `conformance/ci/build-folder.sh`,
  `node-caches`, `host-tools`). If two runners run the same job of two branches at once, the second one replaces the
  first one's checkout while that job is still building. That is why this runbook gives **every extra runner its own
  `PADDOCK_CI_HOME` and its own temp folder** through the runner's `runner.envs`.

Two risks are left that this runbook cannot remove:

- At the end of a job, `host-cleanup` kills the app's test processes by exact name (`paddock.ci.json` `processes`). If two
  jobs of the same app run on one host, one job's cleanup can kill the other's tests.
- `setup-rust` uses a fixed sccache port (4227) and restarts the server. A concurrent job then misses the cache, but it
  does not fail (`SCCACHE_IGNORE_SERVER_IO_ERROR=1`).

Both are fixed by family ADR 0025 (CI job isolation, branch `ci/concurrent-jobs`, Proposed, not merged on 2026-10-28).
**Recommended order:** add the Linux runners now. Add the Mac and Windows runners now with the isolation below, and
accept a rare failed test run until ADR 0025 is merged; re-run such a job.

Disk: each extra host runner grows its own CI home. On the Mac, `~/.paddock-ci` is about 80 GB today (`src` 22 GB plus
the build folders). Check the free space first; 310 GB were free on 2026-10-28.

Load: these runners always run. Nothing yet keeps them from starting jobs on a busy machine (rule 41). On the Mac, drain
the extra runners in Pitcrew (**Drain**) while agents do heavy work, and resume them afterwards.

## How many, and their names

| Host | Existing runner | Add | New names | Total |
|---|---|---|---|---|
| Mac mini | `d7-mac-mini`, folder `/Users/decryp7/gitea-runner`, labels `macos-latest:host`, `macos-arm64:host` | 2 | `d7-mac-mini-2`, `d7-mac-mini-3` | 3 (the scaler's max will be 4) |
| Linux `d-gtr9` | its current runner (`<existing>`) | 3 | `<existing>-2` … `<existing>-4` | 4 |
| Windows VM `decryp7ology-dev` | its current runner (`<existing>`) | 1 | `<existing>-2` | 2 (the decided max) |

On the Mac the third extra runner is left out on purpose: without load control the Mac also carries the agents. Add
`d7-mac-mini-4` the same way if the queue still builds up.

Every new runner gets **exactly the labels of the existing runner on that host.** Read them from the existing
`config.yaml` (`runner.labels`), from the Pitcrew card, or from Gitea's runner list. Never from `.runner`, which holds
the runner's credentials.

## Common steps (every host)

1. **Get the user-level registration token** (the runners are registered at user level, ADR 0021 D2): in Gitea, open
   **User settings ▸ Actions ▸ Runners ▸ Create new runner** and copy the token. One token registers any number of
   runners until it is reset.
2. **Put it in a file only you can read.** Delete the file when done (step 6 of each host). Never paste the token on a
   command line, into Pitcrew's files or into a chat.
3. **Find the server URL the existing runner uses** (the Pitcrew card or `server_url` in `pitcrew.toml`) and use the
   same one for `--instance`.
4. For each new runner, create its own folder with its own `work/` and `cache/`, a copy of the existing binary and
   `config.yaml`, then change these keys in the copy:
   - `runner.file`: the new folder's `.runner`
   - `host.workdir_parent`: the new folder's `work`
   - `cache.dir`: the new folder's `cache` (the `external_server` and its secret file stay shared)
   - `runner.timeout: 3h` and `runner.shutdown_timeout: 3h`. A stop then waits for a job the runner has just taken
     instead of cancelling it (ADR 0021 §4; the default `0s` cancels it).
   - host runners only (the Mac, Windows): `runner.envs.PADDOCK_CI_HOME`, a CI home of its own, and a temp folder of
     its own: `runner.envs.TMPDIR` on macOS, `runner.envs.TMP` and `runner.envs.TEMP` on Windows. Not for container
     runners: a host path in a job container's environment points nowhere.
5. Register it from its folder with `--token-file`, then let Pitcrew run it (Runners ▸ Register runner ▸ **An existing
   runner folder**: "a folder with a `.runner` file is used as it is and the token is not sent").

Also give the **existing** runner `runner.shutdown_timeout: 3h`, with Pitcrew's config editor (runner page ▸ Settings)
or `config set`. It has the same race on every stop.

## Mac mini

In Terminal, as the user `decryp7`:

```sh
set -eu
SRC=/Users/decryp7/gitea-runner
SERVER='<the server URL d7-mac-mini uses>'        # step 3 above
TOKEN_FILE=$(mktemp); chmod 600 "$TOKEN_FILE"
pbpaste > "$TOKEN_FILE"                            # copy the token first; check: wc -c "$TOKEN_FILE"

for N in 2 3; do
  NAME="d7-mac-mini-$N"
  D="/Users/decryp7/gitea-runner-$N"
  HOME_CI="/Users/decryp7/.paddock-ci-$N"
  mkdir -m 700 "$D" "$D/work" "$D/cache" "$D/tmp"
  mkdir -p "$HOME_CI"
  cp -p "$SRC/gitea-runner" "$SRC/config.yaml" "$D/"
  R="$D/gitea-runner"; C="$D/config.yaml"
  "$R" config -c "$C" set runner.file "$D/.runner"
  "$R" config -c "$C" set host.workdir_parent "$D/work"
  "$R" config -c "$C" set cache.dir "$D/cache"
  "$R" config -c "$C" set runner.timeout 3h
  "$R" config -c "$C" set runner.shutdown_timeout 3h
  "$R" config -c "$C" set runner.envs.PADDOCK_CI_HOME "$HOME_CI"
  "$R" config -c "$C" set runner.envs.TMPDIR "$D/tmp"
  (cd "$D" && "$R" register -c "$C" --no-interactive --instance "$SERVER" --token-file "$TOKEN_FILE" \
      --name "$NAME" --labels "macos-latest:host,macos-arm64:host")
  chmod 600 "$D/.runner" "$C"
done

rm -f "$TOKEN_FILE"
```

Then, in Pitcrew, for each folder: **Runners ▸ Register runner**. Use the server URL above. Name it `d7-mac-mini-2`
(then `-3`) with the same labels, choose **An existing runner folder**, and enter `/Users/decryp7/gitea-runner-2` (then
`-3`). The card shows **Idle** once the server accepts it.

## Linux runner host `d-gtr9`

Check first: `runner.capacity` in the existing `config.yaml`. If it is already above 1, that runner already runs several
container jobs; add runners only until the total number of jobs at once (runners × capacity) is 4.

As the account that runs the existing runner (`<SRC>` is its folder, `<binary>` its file name, usually `gitea-runner`):

```sh
set -eu
SRC='<existing runner folder>'
BIN='<binary>'
EXISTING='<existing runner name>'
SERVER='<the server URL the existing runner uses>'
LABELS="$("$SRC/$BIN" config -c "$SRC/config.yaml" get runner.labels | paste -sd, -)"   # check that it prints the labels
TOKEN_FILE=$(mktemp); chmod 600 "$TOKEN_FILE"
${EDITOR:-nano} "$TOKEN_FILE"                      # paste the token, save

for N in 2 3 4; do
  NAME="$EXISTING-$N"
  D="$SRC-$N"
  mkdir -m 700 "$D" "$D/work" "$D/cache"
  cp -p "$SRC/$BIN" "$SRC/config.yaml" "$D/"
  R="$D/$BIN"; C="$D/config.yaml"
  "$R" config -c "$C" set runner.file "$D/.runner"
  "$R" config -c "$C" set host.workdir_parent "$D/work"
  "$R" config -c "$C" set cache.dir "$D/cache"
  "$R" config -c "$C" set runner.timeout 3h
  "$R" config -c "$C" set runner.shutdown_timeout 3h
  (cd "$D" && "$R" register -c "$C" --no-interactive --instance "$SERVER" --token-file "$TOKEN_FILE" \
      --name "$NAME" --labels "$LABELS")
  chmod 600 "$D/.runner" "$C"
done
rm -f "$TOKEN_FILE"
```

`config get runner.labels` prints one label per line (checked with 5.2.0); `paste` joins them with commas. If any label
ends in `:host`, that runner runs jobs on the host: add `PADDOCK_CI_HOME` and `TMPDIR` as on the Mac.

**Run them:**

- **If Pitcrew runs on d-gtr9:** adopt each folder as on the Mac.
- **If not:** start each runner the way the existing one is started. For a systemd user service, copy its unit to
  `gitea-runner-N.service` with `WorkingDirectory=$D` and `ExecStart=$D/$BIN daemon --config $D/config.yaml`. Then run
  `systemctl --user daemon-reload` and `systemctl --user enable --now gitea-runner-N`, and check
  `systemctl --user status gitea-runner-N`. Set `TimeoutStopSec=3h` in the unit, so that systemd does not kill the runner
  while `shutdown_timeout` lets a job finish.

Container jobs: each running job takes a Docker network from the daemon's address pool. Four at once fit the default
pools. Pitcrew's config check warns above that.

## Windows VM `decryp7ology-dev`

In PowerShell, as the user that owns the existing runner's folder:

```powershell
$ErrorActionPreference = 'Stop'
$Src      = '<existing runner folder>'          # e.g. the folder on its Pitcrew card
$Existing = '<existing runner name>'
$Server   = '<the server URL the existing runner uses>'
$Labels   = '<labels of the existing runner, comma-separated>'
$TokenFile = Join-Path $env:TEMP ("rt-" + [guid]::NewGuid() + '.txt')
New-Item -ItemType File -Path $TokenFile | Out-Null
icacls $TokenFile /inheritance:r /grant:r "${env:USERNAME}:F" | Out-Null
notepad $TokenFile                               # paste the token, save, close

$N = 2
$D = "$Src-$N"; $Name = "$Existing-$N"; $HomeCi = "$env:USERPROFILE\.paddock-ci-$N"
New-Item -ItemType Directory -Path $D, "$D\work", "$D\cache", "$D\tmp", $HomeCi -Force | Out-Null
Copy-Item "$Src\gitea-runner.exe", "$Src\config.yaml" $D
$R = "$D\gitea-runner.exe"; $C = "$D\config.yaml"
& $R config -c $C set runner.file "$D\.runner"
& $R config -c $C set host.workdir_parent "$D\work"
& $R config -c $C set cache.dir "$D\cache"
& $R config -c $C set runner.timeout 3h
& $R config -c $C set runner.shutdown_timeout 3h
& $R config -c $C set runner.envs.PADDOCK_CI_HOME $HomeCi
& $R config -c $C set runner.envs.TMP "$D\tmp"
& $R config -c $C set runner.envs.TEMP "$D\tmp"
Push-Location $D
& $R register -c $C --no-interactive --instance $Server --token-file $TokenFile --name $Name --labels $Labels
Pop-Location
Remove-Item $TokenFile
```

Then adopt the folder in Pitcrew as on the Mac. In step 4 of the wizard, choose the same session as the existing runner
(the logged-on user's desktop if its jobs need a desktop). Pitcrew's own per-runner `env` is not applied on Windows yet
(roadmap); `runner.envs` is the runner's own setting and does apply.

If the binary has another name (`act_runner.exe`), use that name.

## Verify that they take jobs

1. **Gitea:** User settings ▸ Actions ▸ Runners lists the new runners as online (Idle) with the same labels as the
   existing one.
2. **Pitcrew:** every card reads Idle. The Logs page of a new runner shows its `supervisor:` start line and no
   registration error.
3. **A real job:** at the next busy period (or dispatch `ci.yml` for an app as usual), two or more runners of one host
   read **Busy** at once.
   - The job log's first step prints `runner: <name>`.
   - On the Mac and Windows, `stable-workspace` prints `building from <PADDOCK_CI_HOME of that runner>/src/…`. If it
     names `~/.paddock-ci` for a new runner, `runner.envs` did not reach the job: drain that runner and check its
     `config.yaml`.
4. **Queue:** the Insights page (ADR 0015) shows the queue wait falling over the next day.
5. **Stop safety (once per host):** while a new runner runs a short job, press **Stop** in Pitcrew. The card reads
   *Stopping* until the job ends, and the job finishes green.

## Roll back

For each added runner:

1. In Pitcrew: **Drain**, wait for *Paused*, then remove the runner. It stops after its job, and Pitcrew forgets it but
   keeps the folder. Without Pitcrew: `systemctl --user disable --now gitea-runner-N`, or stop it the way it was
   started.
2. In Gitea: User settings ▸ Actions ▸ Runners ▸ the runner ▸ **Delete**. A runner still running exits when its
   registration is removed.
3. Delete its folder (`gitea-runner-N`, which holds its `.runner`) and its CI home (`~/.paddock-ci-N`) to free the disk.
4. If the token may have leaked (a file not deleted, a paste in the wrong place): **Reset** the registration token on
   the same Gitea page. Registered runners keep working; the old token stops registering new ones.

Leave the existing runner's `runner.shutdown_timeout: 3h` in place; it only makes stops safer.
