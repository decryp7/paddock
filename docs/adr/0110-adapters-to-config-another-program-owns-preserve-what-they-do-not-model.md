# 0110. Adapters to state another program owns preserve what they do not model, or touch only what they marked

- Status: Accepted 2026-11-20 (user, relayed by the coordinator). The rule, the Pitcrew editor (decision P) and the Debrief
  guard (decision D) below were decided by the user on 2026-11-20; this ADR records them, it does not offer them as options.
- Date: 2026-11-20
- Related: ER-21 / rule 40 (a recurring class gets a redesign); ER-22 / rule 42 (share first: the editor is extracted, not
  copied); ER-37 / rule 76 (common practice and cost); ER-44 (the complete fix lands in the same round); ER-11 (failing test
  first); engineering-rules PRINCIPLES, "Contracts", "State another program owns" (commit 00d9279, same day); Pitcrew ADR
  [0011](../../apps/pitcrew/docs/adr/0011-runner-config-editing.md) (runner config editing) and
  [0022](../../apps/pitcrew/docs/adr/0022-host-environment-and-shared-folders.md) WP-7 (replaced, see its amendment line).
- Number: 0110. On 2026-11-20 the highest ADR on any `origin` branch was 0109 (`origin/docs/os-only-failures-research`; 0104 is
  on `origin/docs/adr-safe-by-default-under-tests`), read with `git ls-tree docs/adr/` over every remote branch.

Evidence heads read (ADR 0091), re-read before the commit: `origin/main` e0fd0f511 (this branch's base; every `file:line` below
is at this commit unless a tag is named). Shipped claims were read at the tag: `debrief-v1.1.1` (33eba7841) has
`perfmon_control.rs:136` `if replace && self.exists(name)? {` and `perfmon_tools.rs:160` `c.import(&template.name, &file, true)`
(the same code is in `debrief-v1.0.0`); `pitcrew-v1.0.1` (207fff4f5) has the form, the "Advanced (YAML)" mode
(`config-panel.html:107`) showing `store.rawText()` unmasked (`:286`), and `cache.external_server` in the schema
(`runner_schema.rs:128`) but not in `RunnerSettings`.

## Context: the class

**An app writes or shows configuration that another program owns through its own partial model of it.** The owner (a
runner, the OS, a server) defines the format and adds keys or objects over time; the app models a subset. Whatever falls
outside the subset is hidden, lost on save, overwritten, or shown in a way the owner never meant (secrets in clear text).

Two instances, both shipped:

| # | App | Instance | Shipped in |
|---|---|---|---|
| a | Pitcrew | The runner settings form edits 14 fields (`apps/pitcrew/crates/pitcrew-core/src/runner_settings.rs`, `RunnerSettings`) of a `config.yaml` whose reference has 87 typed keys (`runner_schema.rs`, the gitea-runner 5.2.0 `config.reference.yaml` table). The form models `cache.enabled`, `cache.dir` and `cache.size_limit` but not `cache.external_server`, so a runner's external Actions cache server was invisible in the form. The escape hatch, "Advanced (YAML)" (`apps/pitcrew/app/src/app/features/runners/config-panel.html:107`, textarea at `:270-288`), shows the raw file with `cache.external_secret` and the S3 keys in clear text. | pitcrew-v1.0.1 |
| b | Debrief | PerfMon "Install & start" (`apps/debrief/app/src/app/tools/perfmon/perfmon-view.ts:34-35` collector name, `:109` the button) calls `install` (`apps/debrief/crates/debrief-plugin-tools/src/perfmon_tools.rs:164`, `c.import(&template.name, &file, true)`), and `import` with `replace` runs `logman delete -n <name>` on any existing Data Collector Set of that name (`perfmon_control.rs:133-140`), whoever made it. Typing the name of a set the user or an administrator built in Performance Monitor deletes it without a word. The status card's Delete button (`perfmon-view.ts:146`, `perfmon_tools.rs:183-184`) reaches the same `delete` for any typed name. | debrief-v1.0.0 to debrief-v1.1.1 |

Why fixing instances does not end it: every new settings page starts as a typed model of the keys someone needs today, and
every "install" or "apply" starts as "replace what is there". Neither shape says whose the state is, so neither asks what it
must leave alone.

### Adapters already conforming (audit of 2026-11-20)

| App | Adapter | Why it conforms |
|---|---|---|
| Pitcrew | `config.yaml` saves (`pitcrew-core/src/yaml_patch.rs`, `pitcrew-service/src/runner_files.rs`) | preserving: only the changed keys' lines are touched, comments and unknown keys stay byte for byte; a timestamped backup and an atomic replace |
| Pitcrew | Register wizard (`app/src/app/features/register/`) | create-only: it makes a new runner folder and file, never edits one it did not make |
| Debrief | `debrief-agent.toml` (`apps/debrief/crates/debrief-agent/src/config.rs:370-377` `to_toml_over`, over `toml_merge::update_preserving`; called from `app/src-tauri/src/agent_commands.rs:116`) | preserving: a three-way text merge keeps comments, order and keys the model leaves out |
| Debrief | Config diff (`app/src-tauri/src/config_diff.rs`) | read-only |
| Dyno | Wakes (`apps/dyno/crates/dyno-platform/src/wakes.rs:1-4`) | owner-marked: an event is Dyno's only when its owner string starts with Dyno's prefix; listing and cancelling look at those alone |
| Dyno | Firewall view (`dyno-platform/src/firewall_readers.rs`, `windows_firewall.rs`) | read-only |
| Pitwall | llama-server flags and presets (`apps/pitwall/crates/pitwall-core/src/presets.rs`) | app-owned: Pitwall builds the command line of the server it starts |
| Spotter | Imports | read-only |

## Comparable practice (ER-37)

Two patterns, both standard; this ADR adopts both and invents nothing.

- **Raw view for an open-ended format.** Tools that front a program with a large or growing config give the file itself,
  not only a form. Portainer edits a stack's Compose file in its Editor tab and redeploys it
  (https://docs.portainer.io/user/docker/stacks/edit). Home Assistant's automation editor switches between the visual
  editor and "Edit in YAML" (https://www.home-assistant.io/docs/automation/editor/). Grafana exposes the whole dashboard as
  its JSON Model tab, edited and saved in place
  (https://grafana.com/docs/grafana/latest/dashboards/build-dashboards/view-dashboard-json-model/).
- **Touch only what you own.** Kubernetes: "Owner references help different parts of Kubernetes avoid interfering with
  objects they don't control" (https://kubernetes.io/docs/concepts/overview/working-with-objects/owners-dependents/); the
  recommended `app.kubernetes.io/managed-by` label names "the tool being used to manage the operation of an application"
  (https://kubernetes.io/docs/concepts/overview/working-with-objects/common-labels/); Server-Side Apply raises a conflict
  "when an Apply operation tries to change a field that another manager also claims to manage. This prevents an applier
  from unintentionally overwriting the value set by another user"
  (https://kubernetes.io/docs/reference/using-api/server-side-apply/). Terraform binds each remote object to one resource
  through its state (https://developer.hashicorp.com/terraform/language/state/purpose) and takes over an existing object
  only through an explicit import (https://developer.hashicorp.com/terraform/language/import).
- **The vendor's own spec for the Debrief case.** `logman import -n <name> -xml <file>` has no overwrite option
  (https://learn.microsoft.com/en-us/windows-server/administration/windows-commands/logman-import-export); a Data Collector
  Set carries a `Description` and `Keywords` "that describe the data collector set"
  (https://learn.microsoft.com/en-us/windows/win32/api/pla/nf-pla-idatacollectorset-get_keywords), so a marker fits the
  owner's own schema. Debrief's template already writes `Description` ("Debrief performance capture ...",
  `perfmon_template.rs:317-330`).

**Is this more than comparable apps do?** No for the rule and the Debrief guard: they are the Kubernetes and Terraform
practice at the smallest scale. The Pitcrew part is common practice (a raw editor, as Portainer) plus two things the
references above do not all have: masked secrets (Portainer and Grafana show their text as is; Home Assistant keeps
secrets out of the file with `!secret`) and a shared editor component. Both were the user's choice; the cheaper option is
recorded below.

| Option | What | Cost (agent-days) |
|---|---|---|
| Common practice, cheapest (not chosen) | Delete the form; keep Pitcrew's existing raw mode as is (it already has a gutter and findings per line); add the secret mask | about 1 |
| **Chosen (P)** | Delete the form; one shared paddock-ui editor extracted from Debrief's TOML editor, with YAML, masked secrets, schema hints in Rust and the preservation test | **2.5 to 3.5**, in Pitcrew's release; less WP-7 (2) of Pitcrew ADR 0022, which it replaces |
| **Chosen (D)** | Debrief marks the sets it creates and refuses to replace or delete an unmarked one | **about 0.5**, in Debrief 1.2 |
| This ADR and the principle line | docs only | about 0.25 |

The chosen set is about 1.5 to 2.5 days above the cheapest option, all of it in the shared editor (ER-22: Pitcrew's raw mode
and Debrief's `toml-editor.ts` are already two copies of one line-numbered editor; a third config page would make a third).

## Decision

### The rule

Every adapter that writes state another program owns (a config file, an OS object, a server-side resource) declares, in its
module's doc comment, **the owner and the spec it follows** (the reference file, the vendor's documentation, with the date
read), and does one of:

1. **Preserve**: write only what it changes and keep everything it does not model, byte for byte where the format allows
   (comments, order, unknown keys and values, line endings), with a backup or an atomic replace where the owner gives no
   transaction. Models: Pitcrew's `yaml_patch.rs` with `runner_files.rs`; Debrief's `toml_merge::update_preserving`.
2. **Own by mark**: touch only items it marked as its own when it created them (a prefix, a label, a keyword the owner's
   schema carries), and refuse, with a message and a next step, to replace or delete an unmarked item. Model: Dyno's wake
   owner prefix (`dyno-platform/src/wakes.rs:1-4`).

Read-only, create-only and app-owned adapters (the table above) need nothing more than the declaration.

An adapter that **shows** an open-ended external format (one whose owner adds keys over time) offers a **raw view** of the
whole thing; a form, if any, is a convenience over it, never the only way in. Secrets in a raw view are masked by default
with a reveal toggle, and the mask is never written back.

Each adapter has a test that saves (or installs) over an input carrying things the model does not know and shows they
survive, or, for an owned-by-mark adapter, a test that an unmarked item is left alone (ER-33: the test fails if the
preserving or marking code is removed).

### P. Pitcrew: the runner `config.yaml` is a raw YAML editor only (decided 2026-11-20)

- The runner settings form and its model (`RunnerSettings`, the form branch of `config-panel.html`) are removed; ADR 0011's
  form is superseded by this ADR, and Pitcrew ADR 0022 WP-7 (the projection of `cache.external_server` through the form's
  multi-runner diff) is replaced by WP-7R below.
- The editor is a **shared paddock-ui component** extracted from Debrief's `apps/debrief/app/src/app/profile/toml-editor.ts`
  (its `TomlDocument` port and `TOML_DOCUMENT` token, line numbers, the backend's problem pinned to its line), generalised to a
  document port with a language (TOML, YAML), several findings per document, and masked ranges. Debrief's profile editor and
  Pitcrew's runner editor both use it; Pitcrew's own gutter and textarea copy (`config-panel.html:268-288`) is deleted.
- Validation runs in Rust against `runner_schema.rs` and is shown as hints and warnings at their lines. An unknown key never
  refuses a save (a newer runner may add keys; `runner_schema.rs` already rates it a warning). The severity of a wrongly typed
  documented key is unchanged by this decision.
- Secrets (`cache.external_secret`, `cache.s3.access_key` and `cache.s3.secret_key`; the schema gains a secret flag per key so the list lives in Rust, and the `*_file` keys, which hold paths, stay visible) are masked in the view with
  a reveal toggle. The mask is a view: the text sent to save is the file with the real values, and a save whose text still
  contains the mask is refused in Rust.
- Comments, order, unknown keys and line endings are preserved: the save writes the edited text through the existing backup and
  atomic replace (`runner_files.rs`).
- First failing test: saving a `config.yaml` that has `cache.external_server` and `cache.external_secret` after an unrelated
  edit keeps both values and every comment byte for byte, and the saved file never contains the mask.

### D. Debrief 1.2: PerfMon touches only the sets Debrief made (decided 2026-11-20)

- Debrief's template marks every set it creates (a fixed keyword, or a fixed `Description` prefix; the implementer picks the
  one `logman` reads back reliably on the CI Windows runner and records that measured output in the doc comment).
- `install` with an existing set of the same name replaces it only when the set carries the marker; otherwise it returns an
  error that names the set and says "This set was not made by Debrief. Choose another collector name, or delete it in
  Performance Monitor first." The Delete action takes the same check (ER-44: it reaches the same `logman delete` for any typed
  name; added by the author of this ADR as the registered copy of the same cause).
- First failing test (in `perfmon_tools.rs` tests): install against a `FakeRunner` where `logman query <name>` reports a set
  without the marker returns an error, and the fake records no `logman delete`. If the measured `logman query` output does not
  carry the marker, the reader uses `logman export -n <name> -xml <file>` and the test fakes that call instead; its assertion
  (an error and no `delete`) is unchanged.

## Work packages

| WP | What | Where tracked | Days | Gates |
|---|---|---|---|---|
| WP-1 | This ADR and the PRINCIPLES line | this branch; engineering-rules main | 0.25 | none |
| WP-2 (P) | Shared editor, YAML, masking, schema hints, form removed, preservation test | Pitcrew ADR 0022 amendment (WP-7R); `apps/pitcrew/docs/plans/pitcrew-1-1.md` | 2.5-3.5 | the Pitcrew 1.1 tag |
| WP-3 (D) | PerfMon marker and refusal, failing test first | `docs/plans/debrief-1-2.md` §10.8, row C6 | 0.5 | the Debrief 1.2 tag |

No conformance check is added: two instances in six apps do not pay for one (ER-37). A review of any new adapter to external
state checks the declaration and the test.

## Consequences

- A user can see and edit every key of a runner's `config.yaml`, including ones Pitcrew has never heard of; the form's
  guided fields are gone, and the schema hints carry the guidance instead.
- Secrets no longer appear on screen by default in Pitcrew.
- A Performance Monitor set the user made is never deleted by Debrief; a user who reuses a name gets a message instead.
- Debrief's profile editor moves onto the shared component in the same change that extracts it (ER-22, one change for a shared
  change and its adoption).
- Risk accepted: a set Debrief made with 1.1.1 or earlier has no marker, so Debrief 1.2 refuses to replace it; the message's
  next step (delete it in Performance Monitor) covers it. No migration (the family has none before v1).
