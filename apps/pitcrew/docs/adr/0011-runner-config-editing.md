# 11. Editing a runner's configuration from Pitcrew

- Status: accepted (2026-10-22)
- Related: ADR 0001 (architecture), ADR 0004 (IPC), ADR 0009 (runner environment); code:
  `pitcrew_core::{yaml_patch, runner_settings, runner_schema, runner_config, text_diff}`,
  `pitcrew_service::{runner_files, supervisor::config_requests}`, `app/src/app/features/runners/` (Configuration tab)

## Context

A runner's behaviour (jobs at once, labels, timeouts, polling, cache, container options, log level) lives in its
`config.yaml`, a file the owner has usually annotated and arranged by hand (`gitea-runner generate-config` writes the
whole reference with comments). Changing it meant opening a file, knowing the keys and restarting the runner at a safe
moment. Pitcrew should do all of that, without damaging the file and without ever stopping a running job.

## Decision

**Two ways to edit, one pipeline.** A *form* for the common settings (capacity, labels, job timeout, fetch interval and
timeout, cache enabled/dir/size, job environment variables, container network/privileged/options/valid volumes for
container runners, log level) and an *Advanced* raw YAML editor. Both produce the new file text; `runner_config::preview`
returns that text, a line diff against the file as it is now (`text_diff`), every finding and whether the runner must
restart. Saving is refused while any finding is an error.

**Comments and layout are preserved: edit only the changed keys.** `yaml_patch` is a small pure patcher, not a YAML
re-serialiser. It changes the lines of the keys it is asked to change and nothing else, byte for byte: a changed value
keeps its key text, spacing and trailing comment; a new key goes to the end of its section (after a commented-out
`#key: default` line of the same key, the style of `config.reference.yaml`); list items that stay keep their lines and
comments; a removed key takes its own lines. Line endings and the final newline are kept. Block style is supported
(sections, scalars, lists of scalars, mappings of scalars, `[a, b]` and `{}` on one line). A block scalar, anchor, list of
mappings or tab indentation makes the patch fail with a message naming the key and pointing at the Advanced editor.
The form computes `edits(old, new)` from typed `RunnerSettings`, so unchanged settings produce no edit at all. Property
tests (proptest) check that a set value reads back, that other values and every comment survive, that edits are
idempotent, that setting the current value is a no-op, that lists and maps round-trip, and that arbitrary text never
panics. Chosen over `serde_norway` round-tripping (loses comments) and over a comment-preserving YAML crate (none is
maintained with a stable API); `serde_norway` (MIT/Apache, the maintained fork of `serde_yaml`) is used only to *read*.

**Validation.** `runner_schema` holds the key table of gitea-runner 5.x's `config.reference.yaml` (checked against 5.2.0,
2026-10-22): a wrongly typed value is an error (the runner would refuse to start), an unknown key is a warning with a
"did you mean" suggestion (newer runners add keys), `${NAME}` values pass any type check, durations and sizes use the
runner's formats. `runner_settings::validate` adds the value rules with messages that say what to fix: capacity 1 to 64,
job timeout 1 minute to 7 days, fetch interval 1 s to 1 h, fetch timeout at most 60 s (the runner caps it), label syntax
(`name`, `name:host`, `name:docker://image`, unique names), variable names, cache size, container network and options. The
service repeats the checks on save (the app is not trusted). The Advanced editor shows the same findings with line numbers.

**Backups.** Before a save the old file is copied to `config.yaml.bak-YYYYMMDD-HHMMSS` (UTC) next to it; the newest 10 are
kept (`MAX_BACKUPS`), older ones are deleted; the runner's own plain `config.yaml.bak` is never touched. The new file is
written to a temporary file in the same folder, synced, given the old permissions and renamed over the target (a symbolic
link is followed, so a dotfiles link stays a link). A save carries the fingerprint (SHA-256) of the text the editor
loaded; if the file changed on disk since, the save is refused with "reload and make your change again".

**Applying.** The runner reads its configuration only when it starts, so any change in meaning needs a restart; a change
of comments or layout does not (the parsed documents are compared). The service issues the controller's own
`Input::Restart`, so the runner restarts **after the current job** (immediately when idle) and the UI shows "Restarts
after the current job". A stopped runner just saves (the next start uses the file). The result is one of
`notNeeded`, `notRunning`, `afterJob` (`RestartPlan`).

**Verifying the labels.** After a save that sets labels, the service remembers the expected label names and the runner
generation. When a *later* runner process logs "declare successfully" with its label list
(`logline::declared_labels`), the names are compared (`labels_mismatch`; the runner declares `name:host` labels by name):
agreement is recorded as `LabelsVerified`; a difference as `LabelsMismatch`, a log line in the runner's log, a warning
notification and a note in the Configuration tab. A file without labels cannot be checked (the registration's labels apply).

**Protocol (version 3).** `GetRunnerConfig`, `PreviewRunnerConfig` (queries, no side effects) and `SaveRunnerConfig`
(mutation); app commands `runner_config_get`, `runner_config_preview`, `runner_config_save` mirrored in `api.types.ts` and
the mock. The text of the file can hold secrets (`cache.external_secret`, S3 keys): `ConfigEdit`'s `Debug` prints only its
size, history and logs record changed *key names* only, never values or text, and the form does not display secret keys.
The service reads the file with a size bound (256 KB).

## Consequences

- The owner's comments and layout survive every form edit; a file the patcher cannot edit safely is refused with a reason
  instead of being rewritten.
- The Advanced editor can still produce anything YAML allows; only syntax and types are checked, not whether the runner
  will like the values.
- `config.yaml` backups accumulate (at most 10 per file) next to the file.
- The label check relies on the runner's log line format (`with labels: [...] declare successfully`, gitea-runner 5.2.0).
