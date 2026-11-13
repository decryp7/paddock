# Amendment to ADR 0087 (2026-11-13, draft): the test VM runs tests the way the Windows CI runner does

- Status: Proposed 2026-11-13. Nothing here is built. ADR 0087 (accepted 2026-11-10) is on
  `origin/docs/adr-windows-gate-ratchets` (`db7782e01`) only; this amendment changes its §8 ("Not covered"), which lists
  "the runner account" as a difference only CI sees, and adds a step to its §1.1 guest part.
- Related: ER-9 (fix locally first), ER-14 (never touch the CI runner PC from an agent), ER-33 (a probe must show it can
  see what it checks), ER-37 (common practice and cost); ADR 0021 and its LF-checkouts amendment (2026-11-11), ADR 0025
  (job isolation), `docs/testing/windows-test-vm.md`.
- The brief asked for the diagnostic job to be "report-only per ER-41". ER-41 is not in `engineering-rules/RULES.md`
  (it ends at ER-38, read 2026-11-13); the job below is report-only on its own merits (D-M2).

```evidence-heads
read: 2026-11-13
origin/main b519fd27a
origin/docs/adr-windows-gate-ratchets db7782e01
origin/train/2026-11-12-7 (contains c75dc801c, 0ba535754, eba8b9335)
```

## Trigger

On the night of 2026-11-12 four fixes passed on the test VM and then failed on the Windows CI runner. I checked two of
them in the Gitea job logs:

| # | Run, job (checked: conclusion `failure`) | What the log shows | Fix | Why the VM passed |
|---|---|---|---|---|
| 1 | 8035, job 37301 "app dyno (windows-latest)" | `Dyno did not start: C:\gitea-runner\work\91ad23fc57fc8ebf\tmp\.tmp5F6Dst is not used for private data: its access list grants access to S-1-5-11` (log line 1053, and more after it) | `6445233db` (testkit `private_tempdir`) | The VM's `%TEMP%` is `C:\Users\paddock-test\AppData\Local\Temp`, and its ACL grants only SYSTEM, Administrators and paddock-test (measured 2026-11-13, below) |
| 2 | 8034, job 37281 "paddock-dev (windows-latest)" | `queue::tests::the_lockfile_loop_of_the_train_gates_runs_on_the_real_repository_and_visits_no_fixture` failed twice: `/c/Users/***/.paddock-ci/src/paddock-checkout-Windows-s1/apps/debrief has no Cargo.toml` | `c75dc801c` | The VM run went through PowerShell over SSH, so `pwd` never printed a Git-bash path |

The coordinator reported the other two as a `core.autocrlf=true` difference (`0ba535754`, "pre-push autocrlf") and the
runner's account and PATH (`eba8b9335`, `npm` is `npm.cmd`). I did not find the failing run for these two, so they are
not in the table.

**Pattern.** The VM matches the runner's OS family, but not how the runner starts a test: its temp folder, checkout,
shell, account and environment. A test that depends on any of these passes on the VM and fails in CI.

## What the runner does, and what the VM does today

Runner facts come from the job logs of runs 8034 and 8035 (jobs 37281, 37301, 37302), read through the Gitea API on
2026-11-13. VM facts come from a boot of `clean-baseline` on 2026-11-13 under the test-machines lock (23 s to SSH,
reverted afterwards).

| # | Setting | CI runner (source) | Test VM today | Mirror? |
|---|---|---|---|---|
| S1 | CPU, cores, RAM | `windows/amd64`, 16 cores, 32700 MB (37302: "Operating System", `build-jobs: this host`) | Arm64, 4 vCPU, 8 GB | No. ADR 0087 §1.2's `--arch x64` (Prism) covers x64-only code |
| S2 | Runner | `gitea-runner` v6.0.0, host mode (37302 line 1). `.gitea/actions/README.md` still says v5.2.2 | none | Record only |
| S3 | Account | A normal user with the profile `C:\Users\***` (the name is masked because it equals a secret). Groups, admin, integrity level and logon type are **not known** | `paddock-test`, a standard user (admin in `admin-baseline`), network logon over OpenSSH | Pick the snapshot that matches S3 once D-M1's job has read it |
| S4 | Temp folder | `TMP`, `TEMP`, `TMPDIR` and `PADDOCK_JOB_TEMP` = `C:\gitea-runner\work\<16 hex>\tmp`, set by `conformance/ci/job-tmpdir.sh` (37302: `job-tmpdir: temp folders in ...`). Its ACL grants Authenticated Users (S-1-5-11) write (run 8035) | `C:\Users\paddock-test\AppData\Local\Temp`, owner-only | **Yes**: folder shape and ACL |
| S5 | Checkout and line ends | A git clone kept per slot at `C:\Users\***\.paddock-ci\src\paddock-checkout-Windows-s1` (`kept-clone`, 37302), checked out by the runner's git. Line ends are set by `core.autocrlf` (the brief says `true`) unless `.gitattributes` sets `eol` | Git for Windows 2.56.0.windows.2, system `core.autocrlf=true` (measured). But agents copy LF files from the Mac with `scp` or `tar`, so git never checks anything out | **Yes**: check out inside the guest with the runner's git config |
| S6 | Shell | Every `run:` is `bash --noprofile --norc -e -o pipefail {0}` from `C:\Program Files\Git\bin`, prepended through `GITHUB_PATH` (ci.yml lines 739-745; 37302 lines 114 and 133). `pwd` prints `/c/...` | PowerShell, OpenSSH's default shell | **Yes** |
| S7 | PATH and environment | Git `bin` first, then the runner's PATH (rustup, Node and npm: **not known** in full); `CARGO_BUILD_JOBS=5`, `NEXTEST_TEST_THREADS=8`, `CARGO_TERM_COLOR=always`, `PADDOCK_*`, `CI`, `GITHUB_ACTIONS`, `RUNNER_*` (37302) | System PATH plus `C:\Program Files\Git\cmd`, with no Git `bin` and no Node (measured) | **Yes**, the variable names and values that are not secret |
| S8 | Git config | Not printed in the logs: **not known** | system: `core.autocrlf=true`, `core.symlinks=false`, `core.fscache=true` (measured) | Yes, after D-M1 |
| S9 | OS settings | Not known: Windows build, `LongPathsEnabled`, Developer Mode and the symlink privilege, Defender exclusions, code page, time zone | build 26300, Defender on, no Dev Drive (`windows-test-vm.md`) | Record. Mirror the ones a test can see (long paths, symlink privilege, code page) |

## Decision (proposed)

### 1. Read the runner without touching it: a report-only `runner-env` job

Agents never log in to the runner PC (ER-14). The runner tells us its settings through a CI job.

- **Where.** A new job in the existing `.gitea/workflows/debrief-runner-check.yml`, renamed to `runner-check.yml` (it is
  already the "check a runner after changing it" workflow), started by `workflow_dispatch` only. A schedule would need an
  owner under ADR 0093, so it is not scheduled.
- **Steps.** The same first steps as `ci.yml`'s Windows crate leg (Git bash on PATH, `build-jobs`, `job-isolation`,
  `kept-clone`), so the probe sees the real temp folder and checkout. Then one PowerShell step and one bash step print a
  JSON fingerprint and upload it as an artifact:
  - S3: `whoami /user /groups /priv` (SID type, admin group, integrity level, `SeCreateSymbolicLinkPrivilege`), session
    id and logon type, the parent process of the job. Never the account name: the logs mask it, and the JSON must too.
  - S4: `TMP`, `TEMP`, `RUNNER_TEMP`, `PADDOCK_JOB_TEMP`, and `Get-Acl` SDDL of the job's `tmp`, its parent and
    `C:\gitea-runner`.
  - S5 and S8: `git config --show-origin --list` (system, global and the clone's local config, with credential lines
    removed), `git --version`, and `git ls-files --eol` for three marker files.
  - S6: `command -v bash`, `bash --version`, `pwd` and `cygpath -w "$(pwd)"`, and `MSYSTEM`.
  - S7: the PATH entries in order, and the names of all environment variables. Values only for an allow-list of
    non-secret names.
  - S9: `[Environment]::OSVersion`, `LongPathsEnabled`, Developer Mode, code page (`chcp`), time zone. Also
    `Get-MpPreference` exclusions, which needs admin: when it is refused the job prints `not measured` (ER-33), never an
    empty list.
- **Report-only.** Differences never fail the job. It fails (`::error::` with the next step) only when the probe itself
  breaks, for example when the step cannot read the ACL. A broken probe must never show up as "no difference".
- **Positive control (ER-33).** The fingerprint must contain the job's own `RUNNER_TEMP` path and the slot name printed
  by `job-isolation`. If not, the job printed some other machine's values.

### 2. Keep the fingerprint in the repository

`scripts/vm-win/runner-profile.json` stores the fingerprint, minus anything secret, with the run and job it came from.
At first a person updates it by hand from the artifact (D-M3). `vm-win.sh mirror --check` prints the differences between
this file and a fingerprint taken in the guest by the same script, so drift shows up as a diff and not as a red CI run.

### 3. Apply it on the VM for every run (no new snapshot)

A new step, `scripts/vm-win.sh mirror`, runs over SSH after `start` and before any test. It reads `runner-profile.json`
and does the following. The VM is reverted after every run, so none of this needs to last.

- **S4 temp.** Make `C:\gitea-runner\work\<16 random hex>\{hostexecutor,tmp}`. Set the ACL of `C:\gitea-runner` from the
  recorded SDDL so that `tmp` inherits it, which grants Authenticated Users write as on the runner. Export `TMP`, `TEMP`,
  `TMPDIR`, `RUNNER_TEMP` and `PADDOCK_JOB_TEMP` to that folder.
- **S5 checkout.** Do not copy the sources over. Send a `git bundle` of the snapshot commit, then clone and check it out
  inside the guest into `C:\Users\<test user>\.paddock-ci\src\paddock-checkout-Windows-s1` (the same path shape and
  depth), using the recorded system and global git config (`git -c` for each key). This replaces ADR 0087 §1.1 step 3's
  host-side `checkout-index`: git in the guest applies `.gitattributes` and `autocrlf` exactly as on the runner.
- **S6 shell.** Run the test command as `"C:\Program Files\Git\bin\bash.exe" --noprofile --norc -e -o pipefail <script>`,
  with Git `bin` first on PATH, so `pwd`, `$HOME` and argument conversion behave as in CI.
- **S7 environment.** Prepend the recorded PATH entries that exist in the guest, set the allow-listed variables (`CI`,
  `GITHUB_ACTIONS`, `RUNNER_OS=Windows`, `CARGO_BUILD_JOBS`, `NEXTEST_TEST_THREADS` scaled to 3 test threads, and so on),
  and print every recorded entry that is missing in the guest, such as an `npm.cmd` without Node.
- **S3 account.** Choose `clean-baseline` or `admin-baseline` to match the runner's admin group. A gap that remains, such
  as an interactive logon against SSH's network logon, is printed in the run's report as a known difference.
- **Controls.** `mirror` ends by checking its own work: the temp ACL contains S-1-5-11, `pwd` in bash starts with `/c/`, a
  file with no `eol` attribute is CRLF and an `eol=lf` file is LF, and `bash` is Git's. If one check fails, the run stops
  as `not measured` and does not pass.

### 4. ADR 0087 §8, amended

Remove "the runner account" from "Not covered". The account, temp folder, checkout, shell and environment are now
mirrored. CPU architecture and core count (S1) and Defender's exclusions stay with CI's Windows leg.

## Common practice (ER-37)

- **GitHub** publishes each hosted runner image's software list (for example
  [Windows2022-Readme.md](https://github.com/actions/runner-images/blob/main/images/windows/Windows2022-Readme.md), read
  2026-11-13). It lists versions and shells (`gitbash.exe` → `C:\Program Files\Git\bin\bash.exe`), but not the git config,
  `TEMP` or the runner account. Projects that need an exact match build their own image from the same Packer templates,
  or run the test on a real runner.
- GitHub's [workflow syntax](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax)
  documents the Windows `bash` shell as Git for Windows' bash, run as `bash --noprofile --norc -eo pipefail {0}`. Our
  runner does the same (S6).
- **Microsoft** ([GetTempPath2](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-gettemppath2w),
  read 2026-11-13): a non-SYSTEM process takes its temp folder from `TMP`, then `TEMP`, then `USERPROFILE`. Microsoft
  ACLs `C:\Windows\SystemTemp` tightly "to prevent common path redirection issues". So a temp folder that other users can
  write is a CI layout choice, and a test VM has to recreate it on purpose.
- **Git** ([gitattributes](https://git-scm.com/docs/gitattributes)): a file's line ends come from `core.autocrlf` only
  when its `eol` attribute is unset. A checkout only shows CI's bytes if it is a real git checkout with CI's config.
- Local runners such as `act` run workflows in containers. That does not work for Windows host jobs.

## Cost (raw agent-days, one agent) and the cheaper option

| WP | Content | Days |
|---|---|---|
| WP-M0 | `runner-env` job in `runner-check.yml` (rename), fingerprint JSON, masking, positive control, `not measured` paths; one dispatch, read by a person | 0.5 |
| WP-M1 | `scripts/vm-win/runner-profile.json` from that artifact; the guest fingerprint script; `mirror --check` diff | 0.5 |
| WP-M2 | `vm-win.sh mirror`: temp folder and ACL, bundle-and-clone checkout, Git-bash runner, environment, snapshot choice; tests in `vm-win.sh`'s fake mode | 1.5 |
| WP-M3 | The four controls of §3, shared with ADR 0087 WP-3's canary where they overlap; replay incidents 1 and 2 (fail before the fixes, pass after) | 0.5 |
| WP-M4 | `docs/testing/windows-test-vm.md` ("Like the CI runner" row), `.gitea/actions/README.md` (runner v6.0.0), ADR 0087 §8 text | 0.25 |
| | **Total** | **3.25** |

**Cheaper option (common practice), about 1 day:** skip WP-M0 and WP-M1. Read S4 to S7 by hand once from existing job
logs (done above, except S3, S8 and S9). Write a fixed `scripts/vm-win/ci-like.ps1` that does §3's temp, clone, bash and
PATH steps with hard-coded values. It covers both checked incidents. It cannot see drift when the runner changes, and S3,
S8 and S9 stay unknown.

**Cheapest option, no build:** run Windows-only tests on the real runner through ADR 0087's CI fallback (a targeted
dispatch of the failed legs). It is exact, but costs runner time and a CI round trip per try, which ER-9 asks us to avoid.

## Decisions for the user

| # | Question | Options | Recommendation |
|---|---|---|---|
| D-M1 | How much to mirror | (a) full: probe job, recorded profile, `mirror` step (3.25 d); (b) the 1-day fixed `ci-like.ps1`; (c) none: CI fallback only | **(b) now**, then (a) once ADR 0087 WP-1 (`./dev test --windows`) is built. (b) closes the two checked incidents. (a) only pays off when the runner changes or more crates are enrolled |
| D-M2 | The probe job fails on differences, or reports them | (a) report-only; (b) fail | **(a)**: the runner is the reference, and a difference means the VM needs updating, not that CI is broken |
| D-M3 | Who updates `runner-profile.json` | (a) a person, after a runner change; (b) a bot commit from the job | **(a)**: a personal tool, and a bot that writes to main needs ADR 0022's token rules |
| D-M4 | Account (S3) | (a) mirror through the snapshot choice; (b) make a `paddock-ci` user in the guest that matches the runner's groups exactly | **(a)** until WP-M0 shows the runner's groups; (b) only if they differ from both snapshots |

## Prerequisites and what exists

- On `origin/main` (`b519fd27a`): `scripts/vm-win.sh` (start, `ssh`, `put`, `stop`, `revert`), `scripts/vm-lock-run.sh`,
  `.gitea/workflows/debrief-runner-check.yml`, `.gitea/actions/job-tmpdir`, `job-isolation`, `kept-clone`.
- Not built: ADR 0087's `./dev test --windows` (WP-1) and its canary (WP-3). `vm-win.sh mirror` also works for the manual
  runs agents do today, so it does not wait for them.
- Nothing here changes the CI runner. The probe is an ordinary CI job dispatched by a person or the coordinator.
