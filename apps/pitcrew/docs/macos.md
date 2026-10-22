# Pitcrew on macOS: build, install, register a runner, verify

Pitcrew on macOS is the app (menu-bar icon and window) plus the Pitcrew service running as **your own launch agent**
(ADR 0003): it starts when you log in, needs no administrator rights, and runs the runners in your session, so jobs
get your keychain, GUI session and tools (Xcode, simulators, WebKit).

The steps below take about 15 minutes on a Mac with Xcode, Rust and Node installed.

## 1. Build

```sh
git clone https://dev.decryptology.net/decryp7/Pitcrew.git && cd Pitcrew
bash scripts/macos-bootstrap.sh
```

It checks macOS 13+, the Xcode command line tools, Rust and Node 24, runs `npm ci` and `npx tauri build --bundles
app,dmg`, and prints where `Pitcrew.app` and the `.dmg` are (`target/dist/bundle/`). It installs nothing.

Apple Silicon builds an arm64 app; on an Intel Mac it is x86_64. Two things in the build matter on macOS:

- **The bundle is sealed with an ad-hoc signature** (`"signingIdentity": "-"` in `app/src-tauri/tauri.macos.conf.json`).
  This is required: with no identity the bundle is only linker-signed (no sealed resources) and `SMAppService`
  refuses the launch agent plist with codesigning error -67056. A release build overrides it with a Developer ID.
- **`index.html` must not contain inline `<style>` or `<script>`.** Tauri adds a nonce or hash for each to the CSP, and
  a `style-src` with a nonce makes the webview ignore `'unsafe-inline'`, which blocks the `<style>` elements Angular
  adds at run time: the app then renders without its component styles. `inlineCritical` is therefore `false` in
  `angular.json`, and `scripts/check-index.mjs` (run by `beforeBuildCommand` after `ng build`) fails the bundle if
  the built `index.html` has inline elements. The runner Pitcrew installs matches the Mac
(`gitea-runner-<v>-darwin-arm64` or `-darwin-amd64`).

## 2. Install

1. Open the `.dmg`, drag **Pitcrew** to **Applications**.
2. The build is **not notarised** (no Apple Developer ID yet). The first start: right-click Pitcrew in Applications ▸
   **Open** ▸ **Open**, or System Settings ▸ Privacy & Security ▸ **Open Anyway**.
3. In Pitcrew, **Settings ▸ Service ▸ Turn on**. Pitcrew registers its launch agent with `SMAppService`; macOS shows a
   notification and lists Pitcrew in **System Settings ▸ General ▸ Login Items ▸ Allow in the Background** — switch it
   on. (Pitcrew opens that page for you if approval is needed.)
4. Allow notifications when macOS asks (stalls, crash loops, updates and low disk are announced).

Permissions: **Login Items** (required). **Notifications** (recommended). **Full Disk Access is not needed** for
Pitcrew; grant it to the runner only if your jobs read protected folders (Desktop, Documents, Mail…), and then to
`~/Library/Application Support/Pitcrew/runners/<name>/bin/gitea-runner`.

## 3. Register the runner

Before Pitcrew takes over, stop whatever supervised the runner folder until now, or two supervisors fight over one
runner. Pitcrew does not do this for you:

```sh
launchctl bootout gui/$(id -u)/<label of the old launchd job>
pgrep -fl supervisor          # a supervisor script that is still running (its loop restarts the runner)
kill <its pid>                # kill the script first, then stop the runner it started if it still runs
```

In Pitcrew: **Runners ▸ Add runner**.

- **Server URL**: e.g. `https://dev.decryptology.net`.
- **Registration token**: from the server (Site Administration, organisation or repository ▸ Actions ▸ Runners ▸
  Create new runner). Tick "remember" to keep it in your Keychain for further runners; it is never written to
  Pitcrew's files.
- **Name**: e.g. `mac-mini`; the server shows the display name.
- **Labels**: `macos-latest,macos-arm64` (add `:host` forms such as `macos-latest:host` if your server expects them).
- **Folder**: the default (`~/Library/Application Support/Pitcrew/runners/<name>`) or an existing registered runner
  folder to adopt.

**Adopting an existing folder** (verified on a real Mac with a `~/gitea-runner` folder): choose "An existing runner
folder". A folder that is registered already (it has a `.runner` file, which Pitcrew never reads) is used as it is:
no registration happens and the token is not sent; the runner log says so. Both layouts work: Pitcrew's own
(`<folder>/bin/gitea-runner`) and the legacy one with the binary next to `config.yaml` in the folder itself.

Pitcrew downloads the latest Gitea runner for the Mac, verifies its SHA-256 against both `dl.gitea.com` and the
gitea.com release, writes its configuration (jobs and cache inside the runner folder, so housekeeping finds them),
registers it and starts it. The Dashboard shows it as **Idle** once the server accepts it.

Optional: **Settings ▸ Gitea API** — a read token (scopes `read:repository`) shows workflow names, job outcomes and
queue times on the dashboard. It is kept in your Keychain.

### The environment jobs get

launchd starts the agent, and so the runners and every job, with a minimal `PATH` (`/usr/bin:/bin:/usr/sbin:/sbin`):
`node`, Homebrew tools or `~/.cargo/bin` would not be found ("Cannot find: node in PATH"). Pitcrew therefore asks your
login shell (`$SHELL -l -i -c`, 5 s timeout, so `~/.zshrc` and nvm run) for the `PATH` a terminal has and gives it to
every runner at each start and restart. When the shell does not answer, a fallback is used (nvm's newest Node,
`~/.cargo/bin`, `/opt/homebrew/bin`, `/usr/local/bin`, system folders). To set variables for one runner, add a table to
`pitcrew.toml` (validated: at most 32 variables; `PITCREW_*` and `GITEA_RUNNER_*` are reserved):

```toml
[runners.env]
NODE_ENV = "ci"
PATH = "/opt/toolchain/bin:/usr/bin:/bin"   # replaces the login shell's PATH for this runner
```

A stopgap `runner.envs: PATH` entry in the runner's `config.yaml` is no longer needed; remove it.

### Privacy prompts from CI jobs

macOS asks "Pitcrew.app would like to access files in your Documents folder" when a *job* reads `~/Documents`,
`~/Desktop`, `~/Downloads`, iCloud Drive or a removable volume; Pitcrew itself never touches them. macOS blames the
*responsible process*, by default the app that started the process, and the runner and every job are started by the
Pitcrew service. Since this version Pitcrew starts each runner with `responsibility_spawnattrs_setdisclaim` (what
Terminal, iTerm2 and VS Code do, [ADR 0020](adr/0020-macos-responsible-process.md)), so the runner and its jobs are
their own responsible processes and Pitcrew.app is no longer named. The prompt can still appear, now for the runner
binary (`gitea-runner`): that is the job's doing. Best fix: **jobs should not use protected folders at all** (check out
and build under the runner's `work/` folder or `/tmp`, and keep caches outside `~/Documents`). If a job genuinely needs
one, grant the runner binary access in System Settings ▸ Privacy & Security ▸ Files and Folders / Full Disk Access.
To clear an old decision made for Pitcrew.app, run `tccutil reset SystemPolicyDocumentsFolder net.decryptology.pitcrew`
(likewise `SystemPolicyDesktopFolder`, `SystemPolicyDownloadsFolder`); an ad-hoc signed build gets a new identity on
every reinstall, so a stale decision may also be simply forgotten by macOS.

## 4. A dedicated build Mac

The agent runs while you are logged in. For a Mac that only builds:

- System Settings ▸ Users & Groups ▸ **Automatically log in as** the CI user (needs FileVault off on most Macs; with
  FileVault on, macOS asks for the password after a restart).
- System Settings ▸ Energy ▸ **Prevent automatic sleeping**; Lock Screen ▸ "Turn display off" is fine (jobs keep
  running).
- Software Update: install updates manually, so a restart never happens in the middle of a job.

## 5. Verify

```sh
bash scripts/macos-verify.sh
```

It checks the bundle, the launch agent in launchd, the service's socket and answer (`pitcrew-service status`), and
prints the end of each runner's log, including whether the server accepted it ("declare successfully"). Then push a
commit to a repository with a job such as:

```yaml
jobs:
  hello:
    runs-on: macos-latest
    steps:
      - run: sw_vers && uname -m
```

The dashboard shows the job while it runs (repository, job, elapsed time, a link to the run).

## After updating Pitcrew

launchd/systemd keep the old service process running after the app is updated or reinstalled. The service notices that
its own program changed on disk and restarts itself once no job (or update) is running; the new process adopts the
runners. (AppImage on Linux: turn the service on again once after an update to refresh its copy.) ADR 0009.

## Logs and files

| What | Where |
| --- | --- |
| Configuration (`pitcrew.toml`), state | `~/Library/Application Support/Pitcrew/` |
| Service log | `~/Library/Application Support/Pitcrew/logs/service.log` |
| Runner folder, log | `~/Library/Application Support/Pitcrew/runners/<name>/`, `…/logs/runner.log` |
| Service socket | `~/Library/Application Support/Pitcrew/pitcrew.sock` (only you can connect) |

## Removing Pitcrew

Settings ▸ Service ▸ **Turn off** (unregisters the agent; runners stop after their jobs), quit Pitcrew, delete it from
Applications. The runner folders stay; remove the runners on the server if you no longer need them.

## What was verified where

On a real Mac (Apple Silicon) on 2026-10-22: the sealed bundle, `SMAppService` registration of the launch agent
(after the ad-hoc signature fix), the styled window under the CSP (after the `inlineCritical` fix) and adopting a
registered `~/gitea-runner` folder.

The macOS code paths (launch agent, Unix socket with mode 0600, SIGTERM stops, Keychain, SMAppService) are built and
tested by the `macos` CI job on a `macos-latest` runner; the `.dmg` is built by the `bundle` job on tags, nightly and
manual runs. See the README for the current state.
