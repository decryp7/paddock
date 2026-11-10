# 97. A user approval carries proof that the user gave it

- Status: Accepted 2026-11-10 (user); D3: no interim signer; (a) when two FIDO2 keys arrive; D7: signed release intents,
  CI tags when green (changed by the user on 2026-11-10 from signed release tags); D8 (a) declined for now, (b) later
  (changed by the user on 2026-11-10); other decisions as recommended. D3 was first recorded as an interim Secure
  Enclave signer (`79b0f7ca7`) and corrected the same day: the user's Mac mini has no Touch ID. The independent review
  approved revision 5 subject to P1-E, which revision 6 (`5ae86c263`) made; the delta reviews of D3 and D7 followed, and
  the review approved after P1-I (`630f96357`). History: Proposed 2026-11-10; revision 6 made the last fix the review
  asked for (P1-E); revision 5 answered the re-review of revision 4, `db150467b`: P1-D and the P2s; revision 4 answered
  the re-review of revision 3, `849a4ab13`: P1-C; revision 3 answered the re-review of revision 2, `c591c5668`: P1-A,
  P1-B and the P2s; revision 2 answered the security review of revision 1, `d21eddcbf`; see the "Review answers"
  sections. Drafted at the coordinator's request of 2026-11-10 under ER-21 (a class seen three or more times gets a
  redesign ADR), with requirements R1 to R5 from the ADR 0093 review. Nothing here is built yet. ER-23 does not apply
  (no drawings).
- Date: 2026-11-10
- Related: ER-5, ER-14 (tests never touch real keys or the keychain), ER-15, ER-21 / rule 40, ER-27, ER-31, ER-33 /
  rule 71, ER-36; rule 55 (`Copies-date: ... approved: <coordinator|user>`); ADR 0022 and its build-inputs amendment
  (main-only publish bot for `./dev`); ADR 0034 and 0075 (copy waivers, `[[date_move]] approved`); ADR 0060
  (`docs/gates.toml`, `approved`, `Gates-date:`, C100); ADR 0073 (`approved = "user ..."` on shipped-double exceptions,
  `Doubles-date:`); ADR 0078 (shared test scratch); ADR 0089 (`Outcome`: exit 5 findings, 6 not measured, 7 check
  broken); draft ADR 0091 (`evidence-heads`); ADR 0092 (accepted at `e2f7e946b`: trust-bearing files); ADR 0093
  (accepted at `390ec29bd`: §5 waivers and the gate mode, which use this ADR, its requirements R1 to R5, and its
  server-recorded go-live time); ADR 0096 (accepted at `39e79d355`: decision register `docs/adr/register/<id>.toml`,
  `./dev adr status`).
- Number: 0097 was free on every `origin` branch on 2026-11-10 (0090 to 0096 are taken; no file 0097 or higher on any
  other branch). Check id: **C108** (C105 to C107 are claimed by ADRs 0091 to 0093).
- Evidence: read in git on 2026-11-10 at the heads below; the Gitea API of this instance (version 28.3.0), read-only,
  with the agents' `GITEA_TOKEN`; local probes with throw-away keys in the agent's scratch folder (never a real key, the
  keychain or ssh-agent; ER-14). Sources outside the repository are listed with what was read and when (§ Sources).
  Statements marked *(secondary, unverified)* come from community write-ups only; *(per the review)* marks facts the
  independent review established and this revision did not re-read.

```evidence-heads
read: 2026-11-10
origin/train/2026-11-09-3 c151422a4
origin/main c151422a4
origin/docs/adr-scheduled-owners 390ec29bd
origin/docs/adr-decision-register 39e79d355
origin/docs/adr-evidence-heads b32655fef
origin/docs/adr-private-files e2f7e946b
origin/docs/adr-text-cuts-repo-copies 06c77a417
origin/docs/adr-captured-tool-output 9c2cd4d8e
origin/ci/build-inputs-wp1 3f5206a40
origin/fix/build-inputs-copies-date eba981aed
origin/feat/dev-merge-check 2c364af74
```

## Context: the instances

Paddock records many things as "approved by the user". Every one of them is text that an agent can type.

| # | Where | What stands for the user's consent | Who can write it today |
|---|---|---|---|
| 1 | `docs/gates.toml` (ADR 0060 §3.1, §3.2) | `approved = "user"`: one live entry on the train, Debrief's C13 `[[ratchet]]` (line 1023), committed unsigned by `fa6bfcded` (`%G?` = `N`). Its comment "approved: user 2026-11-08" is on line 1015, above the `[[ratchet]]` header, so by the span rule of §3 it belongs to the previous record. C100 parses the value in `tools/paddock-dev/src/gates/register.rs` (`Approver::User`, lines 53 to 74, 238 to 242) and requires it for a ratchet (line 367) | any agent with commit access |
| 2 | `Gates-date: <id> <old> -> <new>; approved: <coordinator\|user>` (ADR 0060 §3.1; `register.rs` lines 497 to 528) | a commit trailer; a second move needs `approved: user` | any agent writing a commit message |
| 3 | `Copies-date: ... approved: <coordinator\|user>` (rule 55 (d), `docs/rules.md` lines 298 to 300) and its replacement `[[date_move]] approved = "user"` (ADR 0075 §1, line 107) | a trailer, then a data field | any agent |
| 4 | ADR 0073's register (`approved = "user 2026-11-08"`, its lines 528 to 553; `Doubles-date:`; "a permanent kind needs `approved: user`") | a data field and a trailer | any agent |
| 5 | ADR 0093 D5 (revision 1, `34e7eeb00`: a `waive:` Gitea comment by the user's account); accepted at `390ec29bd` with signed `[[scheduled_waiver]]` and `[scheduled_gate]` records that use this ADR | a Gitea comment, then a record | any agent: the token agents hold is the user's (below) |
| 6 | ADR 0096 (decision register rows `status = "accepted"`, copied by agents) and 67 ADR files on the train whose status line says Accepted, most "by the user" | a status line, a register row | any agent |
| 7 | The standing practice that agents never write `approved` lines for the user | an instruction | honoured, not enforced |

None of this code is in a release: `tools/paddock-dev/src/gates/register.rs` was added by `3a38c41ed`, and `git tag
--contains 3a38c41ed` and `git tag --contains fa6bfcded` print nothing. Both became ancestors of `origin/main` when the
train landed (`c151422a4`, re-read 2026-11-10); they are still in no tag. All 6 860 commits on the train are unsigned
(`git log --format=%G?` over the last 3 000: all `N`).

**Unlanded approvals.** `a5153d114` on `origin/ci/build-inputs-wp1` carries `Copies-date: ... approved: coordinator:
debrief entries move to debrief-v1.3.0`, a move to a later release line that rule 55 (d) gives to the user. The user
then chose `debrief-v1.2.0` (a tightening), recorded by `eba981aed` on `origin/fix/build-inputs-copies-date`: three
`Copies-date: ... debrief-v1.3.0 -> debrief-v1.2.0: approved: user: user decision 2026-11-10` trailers, written by an
agent at the user's explicit chat instruction and unsigned. It is the newest instance of the class; the migration (§10)
lists it.

**Agents act as the user** (verified on 2026-11-10 unless marked):

- Agents run as the user's macOS account.
- `git config --show-origin credential.helper` prints `file:/opt/homebrew/etc/gitconfig osxkeychain` and the remote is
  `https://dev.decryptology.net/decryp7/paddock.git`, so `git credential fill` yields the user's Gitea credential (from
  the 0093 review; not re-run here, because it reads the keychain).
- `GET /api/v1/user` with the agents' `GITEA_TOKEN` answers `decryp7`, `is_admin: true`. The instance's users are
  `decryp7` (admin), `paddock-ci-reader`, `paddock-ci-writer` and `renovate-bot`: bot accounts exist, agents do not use
  one.
- The repository has **no branch protection** (`GET /repos/decryp7/paddock/branch_protections` returns an empty list),
  so the history of `main` is only as fixed as the agents' discipline (the force-push hook is untracked, ADR 0075 line
  23).
- The claude-in-chrome tools act in the user's signed-in Chrome (from the 0093 review).
- Repository secrets are readable by any workflow an agent pushes, `paddock-ci-writer`'s token included; a PreToolUse
  hook is bypassed by a script file or the browser.
- Gitea has no SSH or GPG key on the user's account (`/user/keys`: 0, `/user/gpg_keys`: 0) and no instance signing key
  (`/signing-key.gpg`: "no signing key"); the train's head reports `verified: false, reason:
  gpg.error.not_signed_commit`.

## Pattern

**An approval that the user must give is recorded as text, and every party that can write the text is an agent.**
Whatever the record (a field, a trailer, a comment, a label, a status line, a switch), its proof is the author name on
a commit or a comment, and agents hold the user's name. Each new mechanism re-invents the field (`"user"`,
`"user 2026-11-08"`, `approved: user`) and re-states "agents never write it" as an instruction. Hooks cannot close it:
agents can reach the same effect through a script, the keychain credential or the browser.

**Invariant.** A record that claims the user's consent counts only if its exact current value was written by a commit
that carries a signature needing the user to be physically present (a touch of a FIDO2 key), made
with a key in a trust root that agents cannot change, read from history agents cannot rewrite. Anything else is "not
approved", with a clear message, and the stricter state applies.

## Decision

### 1. What a proof is: a user-presence signature on the commit that wrote the record

A user approval is a **commit signed by a user-presence key** (`gpg.format=ssh`). Git signs commits with SSH keys
through `ssh-keygen -Y sign` in the `git` namespace and verifies against `gpg.ssh.allowedSignersFile` (git-config,
`gpg.format` accepts `ssh`; local `git help config`, git 2.56.0).

Keys (D3):
- **FIDO2 security key**, `sk-ssh-ed25519@openssh.com`, created with the default touch requirement (and
  `verify-required` for a PIN if the key supports it). The private key never leaves the device. **The routine signing
  key.** That the authenticator demands a touch is the device's property; what the verifier can check is the presence
  bit in the signature (below), never the documentation of a local tool. **This Mac cannot sign with it out of the
  box:** Apple's `/usr/bin/ssh-keygen` (OpenSSH 10.5p1) uses `/usr/libexec/ssh-sk-helper`, which is built without
  internal security-key support (its strings contain "internal security key support not enabled" and it imports no
  `fido_*` symbol; checked 2026-11-10). A FIDO middleware is needed (`SSH_SK_PROVIDER`, local `ssh-keygen(1)`
  ENVIRONMENT). It must not come from an agent-writable path: `/opt/homebrew` is owned by the user's account
  (`drwxr-xr-x decryp7 admin`), so agents can replace anything in it. WP-0 installs the middleware (an `sk-libfido2`
  provider library built from a pinned OpenSSH release) to a **root-owned** folder, `/usr/local/libexec/paddock/`
  (`/usr/local` is `root:wheel`, `drwxr-xr-x`; the subfolder does not exist yet), which needs the user's `sudo`; its
  sha256 is part of a code pin (§4), and `./dev approve` checks the hash before it passes the path as `SSH_SK_PROVIDER`
  (PC-18b). That check compares two values agents wrote (the pinned hash was computed by an agent, and `./dev approve`
  is agent-built), so it catches mistakes and a later swap, not a determined agent. That is acceptable: each signature
  still needs a hardware touch, and a bad middleware only widens the confused-deputy risk already in §8. Whether a
  self-built `sk-libfido2` dylib loads in Apple's `ssh-sk-helper` at all (library validation, `dlopen` of an unsigned
  library) is **unverified**; WP-0's first step is a real signature through it. Fallback: install the pinned OpenSSH
  release's own `ssh-keygen` root-owned beside the provider and point `gpg.ssh.program` at it.
- **Secure Enclave key**, through the system's `/usr/lib/ssh-keychain.dylib` (present on this Mac, macOS 27.2; local
  `ssh-keychain(8)`: "ecdsa256 for Secure Key module"), created with `sc_auth create-ctk-identity -l <label> -k p-256-ne
  -t bio` (local `sc_auth(8)`: `-ne` non-exportable, `-t bio|none` the private key protection). It appears to ssh as
  `sk-ecdsa-sha2-nistp256@openssh.com` *(secondary, unverified)*. Its flags byte is most likely composed in software by
  `ssh-keychain.dylib`, not attested by hardware, and a reused LocalAuthentication context might allow several
  signatures per Touch ID *(per the review; unverified)*. Only if a Touch ID keyboard is added would a test check both
  with the user's own key, including an agent-written process that holds its own LocalAuthentication context with a
  reuse window and signs twice. **Not used
  on this Mac:** the user's Mac mini has no Touch ID (no Touch ID keyboard, per the user on 2026-11-10), so `-t bio`
  cannot require biometric presence and a Secure Enclave key there proves nothing about the user being present. A Magic
  Keyboard with Touch ID would make this path possible; the ADR does not recommend it, and the reuse test above stays an
  option for that case only.
- **Recovery keys sign only trust-file changes.** A `[[key]]` entry has `role = "routine"` or `role = "recovery"`; the
  verifier refuses a record, trailer or code pin whose deciding commit is signed by a recovery key, and accepts recovery
  signatures only on commits that change trust files (key replacement, §10). The offline software recovery key (WP-0)
  signs on another machine, never on this Mac.
- Not accepted: plain software keys on the Mac, and agent-held keys such as Secretive's (MIT, v4.0.0 of 2026-09-21;
  plain ECDSA signatures, so whether a use needed Touch ID is a setting the verifier cannot see). The accepted D3 keeps
  one software key **offline** (made and stored off the Mac, never on it) for recovery only.

**The verifier checks the user-presence flag itself.** An `sk-*` signature carries a flags byte and a counter (OpenSSH
`PROTOCOL.u2f`). `ssh-keygen -Y verify`, which `git verify-commit` calls, does **not** reject a signature without user
presence: `ssh-keygen.c`'s `sig_verify()` only logs `sk_flags` at debug level and `sshsig.c` never reads them (both read
on 2026-11-10 at `openssh-portable` `master`); GitLab issue 545178 (2025) shows git reporting "Good" for commits signed
with `-O no-touch-required` keys; and the independent review reproduced it: OpenSSH 10.5p1 `ssh-keygen -Y verify`
accepts an `sk-ssh-ed25519` signature with flags `0x00`. An agent holding a key handle file can ask the authenticator
for a silent assertion (CTAP2 `up=false`); the signature is then valid with the presence bit clear. So `git log %G?`,
`git verify-commit` and `ssh-keygen -Y verify` are never a proof under this ADR (ADR 0093 as accepted says the same, its
line 276).

**Where the presence requirement lives.** The allowed-signers format has no presence option, and an unknown option makes
`sshsig.c` reject the whole line *(per the review)*. So `allowed_signers` stays plain, and the per-key requirement
(`presence = "touch"` or `"touch+verify"`, the key's fingerprint and its role, routine or recovery) is in
`docs/approvals/subjects.toml` under `[[key]]`, protected like the trust root (§2).

**Namespaces and scope of a key.** The approval keys are used for approvals only, so a touch request the user did not
start is an alarm. The probe below shows `namespaces="git"` in the signer line refuses a signature made for another
namespace.

### 2. The trust root: chained from a pinned genesis, bound to this repository (R1)

`docs/approvals/allowed_signers` lists the user's approval keys in `ssh-keygen(1)` ALLOWED SIGNERS form, one principal
(`decryp7@approvals.paddock.invalid`), `namespaces="git"`, and `valid-after`/`valid-before` for rotation. It, the
`[[key]]` entries and the subject declarations of `subjects.toml` (§4) are the **trust files**, protected four ways:

1. **Chain.** A trust file counts only if the newest commit that changed it (§3's rule, applied to the whole file)
   verifies under §5 with a key valid in the trust files of that commit's first parent. A key can enter only through a
   signature by a key already trusted. The **genesis** commit, which creates the files, is signed by a key it lists.
2. **Bound to the repository.** Every approval commit, and every commit that changes a trust file, must descend from the
   genesis commit. A signed commit copied from another repository, or made before the genesis, is refused.
3. **Pin outside the repository.** `PADDOCK_APPROVAL_ROOT` = `<genesis commit sha> <sha256 of allowed_signers>`, a Gitea
   Actions repository variable that only an administrator can change. The verifier refuses (exit 5) when the file on
   `main` does not hash to the pin, or the genesis is not that commit, and reports not measured (6) when it cannot read
   the pin. A rotation therefore needs the user's signed commit **and** the user's own change of the variable.
4. **Shown to the user.** Every release summary prints the trust root (key fingerprints, genesis sha), and the user
   keeps the genesis sha and fingerprints in their own password manager at bootstrap.

**Time.** `valid-after` and `valid-before` are judged against a **server-recorded time**, as ADR 0093 does for its
go-live anchor (its line 244): the `created_at` of the first run of **`approvals-audit.yml`** (§7) on `origin/main`
whose head commit contains the approval commit. That workflow runs on every push to `main` with no path filter, so a
docs-only landing, which rule 50 keeps out of `ci.yml`'s heavy jobs, still gets a run. When no such run exists (the
workflow was down, or the commit landed before WP-6), the time is not measured and the verdict is 6, never a guess. An
administrator can delete runs; the next run then supplies a later time, which is stricter for `valid-before` but laxer
for `valid-after`. So the verifier also requires run ids to increase with landing order along `main`'s first-parent
chain, and reports 6 where they do not.
Committer and author dates are never used; an agent can set them.

Honest limit: while agents hold the user's administrator token, the browser session and the keychain credential, they
can change the variable and force-push `main` (table in §8). D8 (a) would close that; it is declined for now (user,
2026-11-10), so it stays forgeable but visible.

### 3. A record counts only as the signed commit left it (R2)

**Records.** Each subject kind (§4) has a strict schema: its file is parsed as TOML, every record is one table (`[x]` or
`[[x]]`) with a **declared id key** (for example `id` for `[[scheduled_waiver]]`, `id` plus `app` for `[[ratchet]]`),
unique per kind in the file (a duplicate id is a schema error), unknown fields are denied, and subtables (`[x.sub]`),
dotted keys (`a.b = 1`) and inline tables that add fields to a record from outside its span are errors. The record's
**span** is its header line to the line before the next header or the end of the file; comments above a header belong to
the previous record (so `docs/gates.toml` line 1015 moves inside the ratchet in the migration, §10).

**The rule.** A kind declares its **user fields** (all fields unless some are `agent_maintained`, such as a ratchet's
`not_yet`, which C100 already keeps shrink-only). Let `projection(r)` be the record's id and user fields as parsed
values, or *absent*.

**The walk is structural, never by date.** `git log --first-parent -L` attributes a record's change to the unsigned
merge that landed it (measured on the train: `docs/gates.toml` lines 1016 to 1024 map to the merges `8443b5b77` and
`4bb899377` and to `1b85776a7`), so every merged approval would fail; without `--first-parent`, "newest" follows commit
dates, which agents set. So the verifier finds the commit that **introduced** the current value by walking the commit
graph from `main`'s tip with `v = projection(record at tip)`:

1. at a commit `c` whose value is `v`: if `c` has no parent, or its first parent's value differs and `c` is not a merge,
   `c` introduced `v`: it must verify (§5) with a routine key trusted at its first parent and descend from the genesis;
2. a non-merge commit whose parent's value is `v`: continue at the parent;
3. a merge with parents `p` and `q` and merge base `b` (one `git merge-base`; more than one base, or more than two
   parents, while the parents' values differ, is treated as a change): the merge may **carry** `v` from `p` only if
   `value(p) = v` and `value(q) = value(b)`, that is, only `p`'s side changed the value since the base; the walk then
   continues at `p`. The rule is symmetric in `p` and `q`. If both parents have `v`, the walk continues at the first
   parent. Otherwise **the merge itself changed the value** (a conflict resolution, an evil merge, or a merge that
   brings back an old value): it must verify like step 1, and an unsigned merge makes the record fail. This stops a
   replay without a force-push: a merge of `main`'s tip with an older ancestor that still had a superseded signed value
   (made with `git commit-tree` or `-s ours`) has that ancestor as its base, the tip's side changed the value since,
   so the merge must be signed, whichever parent is first;
4. results are memoized per (commit, record id), so the walk visits each commit at most once per record; parents are
   taken in their recorded order, never by date.

In addition, every line of the record's span except `agent_maintained` lines blames (`git blame --porcelain`) to a
commit that verifies, as ADR 0093 accepted requires.

So: an unsigned deletion of `until` or `report_until` changes `v` and voids the record; a removed element of a
restricting list, a removed closing condition, a field added by a dotted key elsewhere (a schema error), a line moved
from one record to another, and a merge's conflict resolution that changes the value all fail closed; a signed approval
that lands through an unsigned merge passes (step 3); an unsigned side-branch change with a backdated date, merged after
the approval, is where the walk ends, so it fails; a merge that restores a superseded value fails. Blame alone cannot
see deleted lines, and blame also passes the replay; the walk is what catches both.

**One rule for merges.** "A merge may carry only a change that one side made since the merge base" is the same rule the
merge check (`./dev merge check` on `origin/feat/dev-merge-check`: `b18bd1ad8` "finds hunks and tests a merge silently
dropped", then `2c364af74` "judges three-way, follows renames and skips generated files") applies to hunks and files.
Its whole-file detector now exposes `merge_changed_it(base, mine, others, result)` in
`tools/paddock-dev/src/merge_check/whole_file.rs` (line 26 at `2c364af74`); WP-2 moves that function into
`crates/paddock-merge-rule` and the merge check calls it from there. The two share one implementation (ER-22) in a small
crate of its own, `crates/paddock-merge-rule`, which both `paddock-approvals` and the merge check depend on. It is
pinned (§4) and small and stable, so fixes to the merge check itself, and to `paddock-repo-graph`, need no touch. WP-2
builds it.

The history commands run with a fixed configuration, so the tree under check cannot steer them:
`GIT_CONFIG_NOSYSTEM=1`, `GIT_CONFIG_GLOBAL=/dev/null`, `GIT_NO_REPLACE_OBJECTS=1`, `-c blame.ignoreRevsFile=`, no
`-M`/`-C`. Grafts and a shallow boundary inside a record's history are reported as not measured (6), never as a pass.

**Trailers.** An `approved: user` trailer (`Gates-date:`, `Copies-date:`, `Doubles-date:`) counts only when the commit
that carries it verifies under §5, descends from the genesis and is on `main` (§6). A trailer is part of the signed
commit object, so it needs no projection.

### 4. What carries an approval, and what each artifact becomes (R3)

`docs/approvals/subjects.toml`, a trust file (§2), declares each **subject**: a file, the table kind, its id key, its
user fields and `agent_maintained` fields, its schema, and whether it is a code subject (below). The verifier never
infers subjects from wording.

| Today | Becomes |
|---|---|
| `approved = "user"` in `docs/gates.toml` (ADR 0060), ADR 0073's register, ADR 0075's `[[date_move]]` | the same text; the record counts as the user's only under §3. `approved = "coordinator"` stays as it is and claims nothing about the user |
| `Gates-date:` / `Copies-date:` / `Doubles-date:` with `approved: user` | honoured only on a verified commit on `main` (§3, trailers) |
| A release (D7, §7) | a subject of kind `release_intent` in `docs/approvals/release-intents.toml`: `id`, `app`, `version`, `gates` (each a workflow path and job id, for example `.gitea/workflows/dyno.yml#app-macos`; never a commit status), `expires`, and optionally `commit` or `branch`. All fields are user fields, so lowering it (a gate removed, a later `expires`, another version or commit) needs a new signature |
| ADR 0093's `[[scheduled_waiver]]` and `[scheduled_gate]` | subjects of kind `waiver` and `switch` in `docs/gates.toml`, as 0093 accepted them |
| **Gate switches**: ADR 0060's `report_only`, `not_gated`, `conditional` entries with `approved = "user"`, and any later user-only setting | kind `switch`. A switch that **lowers** a gate guarding a tag (enforce to report or off, a later `report_until`) needs a user record; raising never does. A switch in a repository variable, environment value or workflow input is not honoured by a gate |
| A Gitea comment, label, issue state or the author of anything in Gitea | never a proof; display only |
| ADR 0096 register rows `status = "accepted"` and ADR status lines "Accepted ... by the user" | from the cutover, a register row's `status` and `accepted` are user fields of a subject of kind `acceptance` (`docs/adr/register/<id>.toml`); `./dev adr status` (ADR 0096) calls the library and reports an unverified acceptance as `proposed`. The ADR's status line is a copy for people; ADR 0096's check compares it with the row. History before the cutover is grandfathered by ancestry (§10) |

**ADR 0075's records and the main-only read.** ADR 0075 binds `[[waiver]]` and `[[removal]]` records to the landing
unit; they are agents' records and stay as they are. A `[[date_move]]` with `approved = "user"` is a user record: under
§6 it must be on `main`, verified, before a tag that relies on it. So the user's approval lands first, as its own
docs-only branch from `./dev approve`, and the unit's own record names it by id (`approval = "<id>"`); C58's tag mode
reads the approval from `main`.

**Code subjects (P1-3).** An approval commit may touch only declared subject files (PC-16): a signature never blesses
code by accident. Code that decides whether approvals count is protected by a **code pin** instead: a record of kind
`code_pin` (`id`, `paths`, `tree`) whose `tree` is the hash git computes for those paths at a named commit of `main`
(`git rev-parse <commit>:<path>` per path, combined in a declared order).

**What is pinned is the decision point and the gate, not the callers.** Pinning only the code reachable *from* a
verifier module leaves its callers free: an agent could change `main.rs` so that `./dev approvals verify` succeeds
without calling the module, make `gates/register.rs` treat `approved = "user"` as verified, or edit the gate step, and
no pin would change. So the decision lives where callers cannot reach into it:

1. **Its own crate and binary**, `crates/paddock-approvals` (a library and a `paddock-approvals` binary with its own
   `main`, §5). The pin covers the whole crate folder (so `build.rs`, `#[path]` and `include!` inside it are covered
   without a closure walk), its `Cargo.toml`, the in-repository crates it depends on by path (whole folders; only
   `crates/paddock-merge-rule`; the crate drives git itself and does not depend on `paddock-repo-graph`), and the
   `Cargo.lock` `[[package]]` entries of its dependency closure (name, version, checksum, as canonical text extracted by
   the pinned crate). A path dependency outside the pin is a C108 error.
2. **One gate file**, `.gitea/actions/approvals-check/action.yml`, pinned: it **always builds** the binary in the gated
   job with `cargo build --locked -p paddock-approvals` from the checked-out, pinned sources (about a minute per run),
   **never fetches** one (a fetched binary is covered by no pin, and the build-inputs publish credential is reachable
   from pushed workflows), runs `paddock-approvals gate --tag <tag>`, and fails the job on any exit code but 0. Every
   release workflow calls this action and nothing else for C108; the post-tag audit's step comparison (§7) requires a
   step that uses exactly this action and succeeded.
3. **The audit workflow**, `.gitea/workflows/approvals-audit.yml`, pinned: it is small and changes rarely.
4. **The tag-cutting workflow**, `.gitea/workflows/release-cut.yml` (§7), pinned: it holds the tag credential's only
   legitimate use, and the action and the audit check its pin before trusting a tag it made.
5. `docs/approvals/**` itself, and the sha256 of the FIDO middleware installed by WP-0 (a `file_sha256` entry, checked
   by `./dev approve` on the user's machine, not by CI).
6. **The execution closure of every pinned job.** A pinned workflow or action pins nothing if it `uses:` code that is
   not pinned: a local action such as `./.gitea/actions/setup-rust` runs inside the cut run with the tag credential and
   could forge its `(intent, tag, sha)` artifact. So the pin covers, for `approvals-check/action.yml`,
   `approvals-audit.yml` and `release-cut.yml`, their whole execution closure: the workflow, every local action reached
   through `uses:` (recursively), every repository file a `run:` line in that closure executes or reads as code, and the
   built binary (item 1). **External actions are allowed only by a full 40-hex commit sha** (`actions/checkout@<sha>`),
   never by tag or branch, so their content is fixed by the reference. C108 computes the closure mechanically from the
   YAML (`uses:` paths, and the repository paths named in `run:` lines) and fails on any `uses:` or script in it that no
   pin covers, on an external action referenced by anything but a full sha, and on a `run:` line that calls a tool built
   from unpinned repository sources (for example `./dev`, `npm run` or `node scripts/...`): pinned jobs run only `cargo
   build --locked -p paddock-approvals`, the binary it builds, and pinned files.

**Invariant: everything between the pinned sources and the gate's exit code is either pinned or built inside the gated
job.** The sources, the lock entries, the action and the two workflows (`approvals-audit.yml`, `release-cut.yml`) and
their whole execution closure (item 6) are pinned, external actions are fixed by full commit sha, and the binary is
built in the same job from them; the audit and `release-cut.yml` (§7) do the same from `main`. Nothing on that path is
downloaded, cached between jobs or taken from a runner's disk. The toolchain (`rust-toolchain`, the runner image) is the
one exception, shared with every build the family makes.

**Callers stay unpinned, on purpose.** `./dev approvals verify`, C100, `./dev copies`, ADR 0073's check and `./dev adr
status` call the library for early reports on branches. Because the pinned action is an independent gate at every tag
and the pinned audit re-runs the binary from `main`, a caller that lies only loses its own early report; it cannot make
a tag pass. The user re-signs a pin after reviewing a change of the pinned paths (one touch). The post-tag audit
recomputes the pins in the tagged tree and on `main` and refuses a tag that differs. The release workflow files
themselves change too often to pin; the audit compares them with `main`'s and requires the pinned action's step (D11).

### 5. The verifier: `./dev approvals verify` and check C108

Rust (ADR 0022: tooling is Rust), the crate `crates/paddock-approvals` with its own binary (§4: it is the pinned
decision point), using ADR 0089's `Outcome` for exit codes and JSON; `./dev approvals verify` and the other callers use
its library. It:

- verifies SSHSIG signatures **in process** with the RustCrypto `ssh-key` crate 0.6.7 (Apache-2.0 OR MIT), whose
  `Verifier` supports `SkEd25519` and `SkEcdsaSha2NistP256` and does **not** check the flags; the flags byte and the
  counter are the last five bytes of `Signature::as_bytes()` (its `src/signature.rs` lines 181 and 419 to 455, *per the
  review*). The verifier reads them and requires the presence bit, and the verification bit where the `[[key]]` entry
  says `touch+verify`. In process means the Windows and Linux runners need no particular OpenSSH: they **only verify**,
  never sign, and have no approval key;
- parses allowed signers (principals, `namespaces`, `valid-after`, `valid-before`) and the `[[key]]` entries;
- runs the history rules of §2, §3 and §6 with the fixed configuration;
- **records the FIDO counter** of every approval signature in its report and compares consecutive approval commits by
  the same key: a counter that went down is a finding (a cloned or replayed signature), a gap is a notice naming how
  many signatures the device made in between. Whether the counter is per credential or per device depends on the
  authenticator *(unverified; many FIDO2 keys use one counter for all credentials)*, so a gap is an alarm for the user
  to judge, not proof of misuse;
- caches by commit sha: one verification per distinct approval commit.

**C108** runs in two places:
- **On branches** (conformance, report only): lists records that will not count and why ("`[[scheduled_waiver]]
  debrief-bench`: `until` removed by unsigned `1a2b3c4d5`; ask the user to sign it with `./dev approve`").
- **As a tag gate**, the pinned action `.gitea/actions/approvals-check` in each release workflow (§4, §7) and in `./dev
  release tag` once it exists. ADR 0060's C100 calls the same library now for its early report: an `approved = "user"`
  entry whose record does not verify is treated as if it said nothing, so the stricter state applies (a ratchet without
  a valid target approval keeps its target; an unverified waiver waives nothing; an unverified lowering switch is
  ignored). ADR 0075's `./dev copies` (pending), ADR 0073's check (not built) and ADR 0096's `./dev adr status` call it
  when they are built; their own work packages carry that wiring (WP-3 provides the library API and the C100 wiring
  only).

### 6. Records are read from `origin/main`'s first-parent history (R5)

Every user-only record (approval, waiver, switch, code pin, acceptance) is read from **`origin/main`**, fetched at gate
time, never from the tagged tree or a branch: an agent can add or delete records on a release branch. ADR 0093 as
accepted reads its mode and waivers the same way (its lines 249 and 250).

- An approval reaches a gate only after its signed commit has landed on `main`. On a merge, the record's lines and its
  `git log -L` history still lead to the signed commit on the second-parent side; the projection rule (§3) is what is
  checked, not who merged.
- **Deletion fails closed.** The verifier walks `main`'s first-parent history of each subject file. Once a record id has
  appeared verified on `main`, its later absence is a finding unless the commit that removed it verifies (a signed
  deletion) or the record expired by its own `until`: the gate says "record `<id>` removed by unsigned `<sha>` on main:
  restore it or have the user sign the removal". Absence never loosens anything: the default of every switch is its
  strictest value.
- A tag whose commit is not reachable from a release line the gate knows (ER-31 release branches) is judged with
  `main`'s records only.

### 7. The release workflow runs from the tagged tree: controls and what they cannot do

A tag run executes the workflow files of the **tagged** commit (GitHub Actions' documented behaviour, which Gitea
Actions follows; not tested on this instance). An agent could drop or weaken the C108 step on a release branch and tag
it, and the run would publish.

**Release tags come from signed release intents (D7, accepted 2026-11-10).** The user signs ahead, with one touch (and
several intents in one `./dev approve` batch), a `release_intent` record: app, version, the gates that must be green, an
expiry, and optionally the commit or branch it applies to. The intent lands on `main` like every user record (§6). The
pinned workflow `release-cut.yml` (§4 item 4, built and run like the audit) runs on `main` only, on pushes to `main`, on
the night schedule and on dispatch, in one concurrency group (`release-cut`, never cancelled in progress), so two runs
never cut at once. For each verifying, unexpired, unconsumed intent it:

1. **resolves the commit:** the named commit; or the head of the named ER-31 release branch; or, with neither, a commit
   on `main`'s **first-parent chain** that **landed after the intent did**. "Landed after" is judged by position on that
   chain, not by time: the commit must be the intent's landing commit or a later commit of the first-parent chain, so
   code that arrives in the same push as the intent counts, and an older green commit never does;
2. **checks the named gates** only through Actions runs: for each `workflow#job`, a successful run of exactly that
   workflow path whose head is the resolved commit, with that job successful; commit statuses are never read, because
   any write token can post one. The workflow file's blob at the resolved commit must equal its blob on `main` at the
   commit where the intent landed, so the gate is the definition the user saw when signing. If that workflow file has
   changed since, even legitimately, the intent cannot be used: `release-cut.yml` reports "gate definition changed since
   the intent was signed: sign a new intent" and cuts nothing;
3. **creates the annotated tag** `<app>-v<version>` (or `v<version>`) with the intent id in the message, using CI's
   tag credential, and **records `(intent id, tag, sha)`** as an artifact of its own successful run, only for a tag it
   created itself in that run: if the tag already exists, it records nothing and reports a finding. The artifact gets
   the longest retention the instance allows (the default is not verified on Gitea 28.3.0; WP-6 sets and checks it),
   and the audit stores each tag it has verified in its ledger as settled, so an expired artifact does not turn a
   verified tag into an unverifiable one. That ledger stays editable with the admin token while D8 (a) is declined (§8).

**A tag counts only if `release-cut.yml` made it.** The pinned C108 action in the tag run and the audit look up, through
the Actions API, the `release-cut.yml` run that recorded this tag: its workflow path must be
`.gitea/workflows/release-cut.yml`, it must have run on `main` and succeeded, the file at its head must equal the pin
(§4), and the tag's sha must equal the recorded sha. No such run is a finding (exit 5); an unreadable API is not
measured (6); neither is ever a pass. So an agent holding the tag secret cannot choose another green commit, and a tag
deleted and re-created elsewhere before the first audit no longer matches its record.

**Where the pinned jobs run.** The instance has five online runners (admin API, read-only, 2026-11-10): three macOS
runners named `d7-mac-mini`, `d7-mac-mini-2` and `d7-mac-mini-3`, one Windows runner `decryp7ology-dev` and one Linux
runner `Gitea Action Runner (d-gtr9)`. The macOS runners serve this Mac mini, the user's account that agents share, so
`release-cut.yml`, `approvals-audit.yml` and the C108 action's job in release workflows never run on a macOS runner.
**`d-gtr9` is outside the user's Mac account (user, 2026-11-10):** a separate machine or VM that agents on the Mac
cannot log into, so pinned jobs on it close the runner residual. Pinned jobs say `runs-on: ubuntu-latest` (no
dedicated label: the user decided on 2026-11-10 not to add one). The guard is the audit: it reads the runner of every
pinned job from the Actions API and requires `d-gtr9`, by the runner id and name WP-7 records, so a pinned job that ran
anywhere else is a finding. C108 fails on a pinned job whose `runs-on` is not `ubuntu-latest`. A runner registered
later with the `ubuntu-latest` label (registration needs the admin token, which agents hold while D8 (a) is declined)
could take a pinned job; that is forgeable but visible (§8). The macOS runners stay the D8 (b)
residual for everything else they run.

**What an intent approves.** An intent that names a commit approves that code. An intent without a commit approves an
app, a version and a time window, not the code: whatever lands on `main` in that window and passes the named gates is
released. A named ER-31 release branch narrows which line ships, not what is on it, because agents can push to that
branch. **Only a named commit approves the code**; the user should name one when the content matters, and `./dev
approve` proposes a short default expiry (3 days). **The gates bind definitions, not their integrity:** step 2 binds the
workflow and job definitions the user saw at signing time; whether those definitions really test what they claim is
ADR 0093's and ADR 0060's subject, and for workflows agents write it stays an honour-system matter, visible in history
and in review. The binding stops at the workflow file: the local actions, scripts and `./dev` commands a gate job calls
are not bound by the intent and are honour-system too.

Agents no longer cut release tags themselves; the standing "tag when green" practice becomes "the user signs an intent,
CI tags when green". The user can be away.

- **Post-tag audit from `main` (detects; D7).** A workflow on `main` runs on every push to `main` and in the night
  schedule (the 2026-11-10 decision: scheduled work at night only; scheduled runs use the default branch's file, so a
  release branch cannot remove it), one Linux job that builds `paddock-approvals` with `cargo build --locked` from
  `main`'s pinned sources (about a minute, never a fetched binary) and then only reads the API and git (rule 50). It:
  1. fails when the last audited `main` sha (recorded in the audit's own output and in a `release-audit` issue) is not
     on the new tip's **first-parent chain**; being an ancestor is not enough, because a merge with swapped parents
     rewrites the first-parent chain without a force-push;
  2. for every release tag since its last run, compares the tag run's job and step list from the Actions API with the
     job and step list of `main`'s release workflow for that app, requiring every gate step of `main`'s version to have
     run and succeeded in the tag run;
  3. re-runs C108 and the gates that read approvals against `main`'s records at the tag's time, and compares the tagged
     tree's code pins (§4);
  4. lists every **published release and package** (Gitea releases, the package registry) and requires each to come from
     an audited tag run; anything published otherwise (an admin token, a release token used from a pushed workflow, a
     hand upload) is a finding;
  5. compares **every release tag with a consumed intent**: a `release-cut.yml` run as above whose recorded sha equals
     the tag's; an intent that verifies under §3 on `main`, with the same app and version; not expired at the cut run's
     server time (its `created_at`, never the tagger date); the commit rule of step 1 and the gate rule of step 2 above,
     re-checked from `main`; and the intent used once (no other tag, past or present, names the same intent id). A tag
     that fails any of these is a finding; a tag whose checks cannot be read is 6.

  A mismatch opens a `release-blocker` issue, marks the release "unverified" in its notes, and fails the audit run with
  an `::error::` naming the tag, the missing step and the next step (ER-27).
- **Who may create release tags.** Gitea protected tags (docs for Gitea 28: a pattern, allowed users or teams; "If you
  leave these fields empty no one is allowed to create or modify this tag") can restrict `*-v*` and `v*` to named
  accounts. Whether admins bypass a rule and whether deletion is covered is not stated *(unverified; WP-6 tests both on
  a scratch repository)*. They are not set while D8 (a) is declined for now. The tag secret stays a repository
  secret, so any workflow an agent pushes can read it and push a tag without an intent: the audit finds it (item 5), it
  does not prevent it.
- **Not a control: publishing as a draft until the audit promotes it.** The promote step would hold a release or package
  credential; repository secrets are readable by any workflow an agent pushes, so an agent could promote or publish
  without the audit. A draft stage adds strength only after publish credentials are unreachable from agent-pushable
  workflows, which needs D8 (a) (declined for now).

### 8. What this ADR makes impossible, what only visible, and what closes the rest (P1-4)

| Impossible from WP-5 on, without D8, unless the trust root is swapped (column 2) | Forgeable but visible (detected by C108, the audit or the release summary) | Would be closed by |
|---|---|---|
| A record that verifies without a touch of a routine key | A swap of the trust root or the pin through the admin token (the release summary and the user's password-manager copy of the genesis and fingerprints show it) | D8 (a), declined for now |
| An unsigned edit, deletion or move of a signed record's value counting as the user's | A bypass at a tag: the release workflow on a release branch not calling the pinned action, or changed pinned files in the tagged tree (the audit from `main`: step comparison and code pins); a lying caller (C100, `./dev`) only loses its own early report | D8 (a) plus publish and tag credentials held only by workflows agents cannot change before they run, declined for now |
| An approval copied from another repository or made before the genesis; a superseded signed value restored by an unsigned merge (§3 step 3) | **Force-push replay**: a force-push of `main` that drops a later signed deletion and leaves an older signed value as the newest (the audit's first-parent check, as long as its ledger survives) | branch protection without force push or deletion (D8 (a)), declined for now |
| A trailer or status line on an unsigned commit counting as the user's | **Out-of-band publishes**: a release or package published with a token read from a pushed workflow or with the admin token (the audit's release and package listing) | publish credentials out of agents' reach (D8 (a)), declined for now |
| A release tag that passes the audit without a verifying, unexpired, unused intent for that app and version, or not made by a pinned `release-cut.yml` run on `main` at the sha that run recorded | **Out-of-band tags**: a tag pushed without an intent or at another commit, with CI's tag secret read from a pushed workflow or with the admin token (the audit compares every tag with a consumed intent and its cut run); **agent edits to the audit ledger**: its issue and run outputs edited or deleted with the admin token, which can hide earlier findings, so visibility then rests on the release summary of each tag and the user's own copy of the trust root | D8 (a), declined for now |
| **Bound to the definitions at signing** (not impossible): the named gates are the workflow paths and jobs, with their file contents, that were on `main` when the intent landed | Changes to those gate definitions before the intent was signed are visible in history and review only; whether a gate tests what it claims is ADR 0093's and ADR 0060's subject and an honour-system matter for agent-written workflows | not closed by this ADR |
| | **A same-label runner**: a runner an agent registers with the admin token and the `ubuntu-latest` label could pick up a pinned job (the audit's runner check: a pinned job on any runner but `d-gtr9`, by its recorded id and name, is a finding) | D8 (a), declined for now |
| | A touch that signs an agent-chosen commit (the confused deputy, §9: the printed sha and counter, the counter gaps); every other CI job on the macOS runners `d7-mac-mini*`, which serve the user's account | D8 (b): agents in a separate macOS account (later) |

With D8 (a) declined for now (user, 2026-11-10), every item in the middle column stays **forgeable but visible**: an
agent with the user's administrator token, keychain credential or browser session can do it, and the audit, C108 or
the release summary shows it afterwards. What stays **impossible** does not depend on D8: a record that verifies without
a touch, an unsigned change to a signed value, a merge that replays a superseded value, an approval from another
repository, and a release tag that passes the audit without a consumed intent and its pinned cut run.

### 9. How the user gives an approval: `./dev approve`, and its limits (P1-2)

Agents write proposals as ordinary, inert files under `docs/approvals/pending/`. In the user's own terminal, `./dev
approve`:

1. makes a **fresh clone** of `origin/main` (the remote `https://dev.decryptology.net/decryp7/paddock.git`, read with
   the bot's read token, or anonymously since the repository is public; never the user's keychain credential) into a new
   temporary folder (never a worktree, which shares `.git/config`, hooks and refs with the agents' checkouts);
2. runs every git and ssh command in it with a **cleared environment** (only `PATH=/usr/bin:/bin`, `HOME` set to the
   temporary folder, `SSH_AUTH_SOCK` never passed (the key handle file is named explicitly, so no ssh-agent is
   involved), `SSH_SK_PROVIDER` set to the pinned middleware of §1 after its hash check (the FIDO2 key;
   `/usr/lib/ssh-keychain.dylib` only if a Touch ID keyboard is ever added and the Secure Enclave path chosen), nothing
   else), `GIT_CONFIG_NOSYSTEM=1`, `GIT_CONFIG_GLOBAL=/dev/null`, `-c gpg.format=ssh -c
   gpg.ssh.program=/usr/bin/ssh-keygen -c core.hooksPath=/dev/null` and the key handle path given explicitly;
3. prints each pending record and the subject lines it changes, asks y/N per record, writes the accepted ones, makes
   **one** signed commit for the batch (one touch), and then prints the **commit sha and the FIDO counter** of the
   signature, so the user can compare the sha with what the agent later pushes and see a counter jump;
4. leaves the commit on a branch `approvals/<date>-<n>` that an agent pushes and lands (a docs-only change, rule 50
   (d)); the signature, not the pusher, is the proof.

The clone takes whatever refs the remote holds, and agents can push refs there. That is harmless: the commit `./dev
approve` signs contains only the records the user confirmed on screen, and the walk of §3 judges values, not refs.

It refuses when `CLAUDECODE` is set or stdin is not a terminal; that guards against mistakes, it is not a security
boundary. **Limit, stated plainly:** `./dev approve` is a binary agents can build and change, and agents can edit the
user's shell files and `PATH` because they share the account. Without a touch, no record verifies; **with** a touch,
what was signed is only as trustworthy as the signer process, until D8 (b) moves agents to their own account. The
printed sha and counter, the path rule (PC-16) and the code pins narrow it; they do not close it.

### 10. Bootstrapping, key loss and rotation, migration

**Bootstrap (the user's hands, about 30 minutes in all, not measured).** In WP-0, now: the user makes the offline
recovery key on another machine and keeps it there (only its public key comes to the Mac), and an agent prepares the
FIDO middleware build and its sha256. **When the first FIDO2 key arrives** (WP-8's key step): with `sudo`, the user
installs the middleware into `/usr/local/libexec/paddock/` (root-owned; §1) after comparing its sha256 with the one
printed from the pinned OpenSSH source (agents cannot do this step), creates the FIDO2 key's ssh credential, and runs
`./dev approve --genesis`, which writes `docs/approvals/allowed_signers` and `subjects.toml` (the FIDO2 key as
`routine`, the offline key as `recovery`) and makes the genesis commit signed by the FIDO2 key (one touch).

**Why genesis waits for the FIDO2 key rather than being signed by the offline key.** Both are allowed by the chain rule,
and waiting is the safer one: no record can verify before a routine presence key exists, so an earlier genesis gains
nothing; a genesis signed by the offline key would make a software key without any presence proof the root of the
chain; and signing off the Mac means carrying an unsigned commit to another machine and the signed one back, more steps
in which the user could sign something other than what they meant. The FIDO2 key's own touch on the genesis is the
strongest root available. The user notes the genesis sha and fingerprints in their password
manager, sets `PADDOCK_APPROVAL_ROOT` in the repository's Actions variables, and adds the public keys to their Gitea
profile so the web view shows signed approval commits as verified (display only: Gitea's own trust is an admin-writable
key list, and its handling of `sk-*` keys is not verified).

**Key loss.** With the recovery key the user signs a commit that sets `valid-before=<today>` on the lost key (old
approvals keep verifying, judged by server-recorded time, §2) and adds a replacement, then updates the pin. **Key
compromise.** The same commit removes the key outright, and the user re-signs the live records it signed (`./dev
approvals verify --by-key <fp>` lists them). **All keys lost.** A new genesis, a new pin, and a re-sign of every live
record; records signed under the old root are unverified until re-signed.

**Interim while the keys are bought (accepted D3).** There is no interim signer. Until the first FIDO2 key is enrolled
(genesis), no record verifies, so the stricter state holds: ADR 0093 has no waivers and its mode follows its accepted
go-live rules, a ratchet keeps its target, and date moves needing the user do not count. Waivers stay impossible. The
offline recovery key exists from WP-0 but signs nothing until there is a trust root, and afterwards only trust-file
changes. **Key steps** (WP-8, as the keys arrive): the first FIDO2 key makes the genesis (above); the second is enrolled
as `recovery` in a trust-file commit signed by the first, kept apart from it, and the pin is updated. The offline
software key is then retired with `valid-before=<today>` in the same commit, unless the user wants to keep it as an
off-site backup (it remains recovery-only either way).

**Migration inventory (D9).**

| Item | Where | Action |
|---|---|---|
| Debrief's C13 `[[ratchet]]`, `approved = "user"` | `docs/gates.toml` on the train (lines 1015 to 1023; the comment moves inside the span) | re-signed in the first `./dev approve` batch after genesis |
| `Copies-date: ...: approved: coordinator: debrief entries move to debrief-v1.3.0` | `a5153d114` on `origin/ci/build-inputs-wp1`, not landed | superseded by the user's choice of `debrief-v1.2.0` (next row) |
| `Copies-date: ... debrief-v1.3.0 -> debrief-v1.2.0: approved: user` (three trailers) | `eba981aed` on `origin/fix/build-inputs-copies-date`, not landed; unsigned, agent-written at the user's chat instruction | a tightening, so it needs no user proof to take effect (moving a date earlier is the stricter state); if it lands before the cutover it is grandfathered, after it the trailer is honoured as `coordinator` and the move still stands because it tightens |
| ADRs 0089 to 0096, accepted 2026-11-10 | on docs branches only (none on `origin/main` or the train) | grandfathering is by ancestry of the cutover commit, so they must land on `main` **before** the cutover commit to count as accepted without a signature; an acceptance that lands after it is reported `proposed` by `./dev adr status` until the user signs its register row (one batch, one touch). WP-8 lands them first (ADR 0096 already asks for same-day landing) |
| ADR 0073's register entries with `approved = "user ..."` | not on the train | written as pending records when ADR 0073's register lands |
| ADR 0093's `[[scheduled_waiver]]` and `[scheduled_gate]` | not on the train | signed when ADR 0093's WP-6 lands |
| `approved: user` trailers and accepted ADR status lines in history | the train and `main` | grandfathered by ancestry (below) |

The inventory is regenerated by WP-8 across every `origin` branch (`git log --all --grep 'approved: user'`, the subject
files on each branch head), not only the train. History is grandfathered **by ancestry, not date** (dates are
agent-writable): a cutover commit on `main`, pinned in `subjects.toml`, and every trailer or status line in commits that
are ancestors of it counts as it did, with a notice "grandfathered (before cutover `<sha>`)". Nothing grandfathered can
lower a gate after the cutover.

## Positive controls (ER-33)

Each is a test over a synthetic repository in the shared test scratch (ADR 0078), with keys generated by the test
(software Ed25519, and `sk-*` signatures built by the test over the U2F message format so the presence bit and counter
can be set freely without hardware), `HOME` in scratch, `GIT_CONFIG_GLOBAL=/dev/null`, `GIT_CONFIG_NOSYSTEM=1`,
`SSH_AUTH_SOCK` unset: never a real key, the keychain or ssh-agent (ER-14). Each must turn the verdict red (or 6 where
named); the green twin is in the same test.

| # | Requirement | Planted | Expected |
|---|---|---|---|
| PC-1 | R1 | a key appended to `allowed_signers` in an unsigned commit | refused, "changed by unsigned" |
| PC-2 | R1 | a key appended in a commit signed by that new key | refused, "signer not trusted at parent" |
| PC-3 | R1 | file on `main` differs from the pin; pin unreadable | refused (5); not measured (6) |
| PC-4 | R1 | a second root commit creating the files; a signed approval commit that does not descend from the genesis | refused, genesis mismatch; refused, "not bound to this repository" |
| PC-5 | R2 | a signed waiver, then an unsigned edit of `until`; of `reason`; a signed line moved into another record; a line changed in a merge's conflict resolution | each record does not count |
| PC-5b | R2 | a signed waiver, then an unsigned commit that deletes its `until` line; a signed switch whose closing condition is deleted unsigned; a restricting list element removed unsigned | none counts (the projection rule) |
| PC-5e | R2 | a merge of `main`'s tip with an older ancestor that restores a superseded signed `mode = "report"` (made with `git commit-tree`); the same with the parents swapped | fails; fails |
| PC-5d | R2 | a signed approval landed through an unsigned merge; an unsigned side-branch change with a backdated date, merged after the approval; an unsigned conflict resolution in a merge that changes the value | passes; fails; fails |
| PC-5c | R2 | a field added to a signed record by a dotted key or a `[kind.sub]` table outside its span; an unknown field | schema error, record does not count |
| PC-6 | R4 | an `sk-ssh-ed25519` signature with flags `0x00`; an `sk-ecdsa` one with the presence bit set; a `touch+verify` key whose signature lacks the verification bit | refused; accepted; refused |
| PC-7 | R3 | `[scheduled_gate] mode` from `enforce` to `report` in an unsigned commit; the mode set by an environment value | `enforce` stays, a finding; ignored |
| PC-8 | R2 | a signature in another namespace; a key past its `valid-before` at the server-recorded time while the committer date is earlier | refused; refused |
| PC-9 | R2 | a planted `.git-blame-ignore-revs` and `blame.ignoreRevsFile` in the repo config; a `refs/replace` object | no effect on the verdict |
| PC-10 | R2 | a shallow clone whose boundary is inside a record's history | not measured (6) |
| PC-11 | R3 | `Gates-date: ...; approved: user` on an unsigned commit | not honoured |
| PC-12 | R5 | a signed record only on a release branch, not on `main` | not honoured at that branch's tag |
| PC-13 | R5 | a verified record deleted on `main` by an unsigned commit | the gate refuses, "removed by unsigned" |
| PC-14 | §7 | a recorded Actions API reply (synthetic JSON) for a tag run without `main`'s C108 step | the audit fails and names the step |
| PC-15 | §7 | a release listed by the API with no audited tag run; a new tip whose first-parent chain does not contain the last audited sha although it descends from it (swapped parents) | each flagged |
| PC-16 | §4 | a signed approval commit that also changes a `.rs` file | refused, "approval commit touches non-subject paths" |
| PC-17 | §4 | a tagged tree, or the head of the `release-cut.yml` run, whose `crates/paddock-approvals/`, its lock entries, `.gitea/actions/approvals-check/action.yml`, `approvals-audit.yml` or `release-cut.yml` differs from the signed code pin | the audit refuses the tag |
| PC-21 | §7, D7 | release tags checked by the audit: a tag with no intent; with an expired intent (expiry before the tag run's server time); with an intent already consumed by an earlier tag; with an intent for another version or app; with an intent whose named gate is red on the tagged commit; an intent whose `expires` or `gates` was changed by an unsigned commit; an agent's tag at another green commit pushed with the tag secret before `release-cut.yml` ran; a tag deleted and re-created at another commit before the first audit; a tag recorded by a `release-cut.yml` run from a branch, not `main`; a tag with no `release-cut.yml` run at all; a gate satisfied only by a posted commit status; a gate whose workflow file differs from its blob on `main` when the intent landed; a no-commit intent resolved to a green commit that landed before the intent; and a tag cut by `release-cut.yml` from a valid intent | fail; fail; fail; fail; fail; fail (the intent does not verify); fail; fail; fail; fail (5, or 6 if the API is unreadable); fail; fail; fail; passes |
| PC-17e | §7 | a pinned job with `runs-on` other than `ubuntu-latest`; a pinned job run (recorded API reply) on a runner whose id or name is not `d-gtr9`'s, including a runner registered later with the `ubuntu-latest` label | refused by C108; a finding of the audit |
| PC-17d | §4 | `release-cut.yml` (or the action, or the audit) calling a local action whose content changed on `main` after the pin; an external action referenced by a tag or branch instead of a full commit sha; a `run:` line in a pinned job calling `./dev` or a script outside the pin | refused by C108 and by the audit (the closure is not covered by the pin) |
| PC-17c | §4 | a gate run whose binary was fetched (a planted download step, or a binary placed in the cache or on the runner's disk) or differs from the one built in the job | refused: the pinned action builds in the job and runs only that build's path; the audit requires the build step in the tag run's step list |
| PC-17b | §4 | a caller-side no-op: `./dev`'s `main.rs` returns success for `approvals verify` without calling the crate; the release workflow's C108 step replaced by an `echo` (or the action's `run` line by `true`) | the first leaves every pinned check red at the tag (only the early report lies); the second is refused by the audit (missing pinned action step) or by the pin (changed action) |
| PC-18 | §9 | `./dev approve` run with a planted `gpg.ssh.program`, `core.hooksPath`, `SSH_SK_PROVIDER`, `SSH_AUTH_SOCK` and a `PATH` entry in the agents' checkout and environment | none is used (the fresh clone's command lines and environment are asserted by the test, which signs with a test key) |
| PC-18b | §1, §9 | the middleware file in the test's stand-in for `/usr/local/libexec/paddock/` replaced by another file; the folder made writable by the test user | `./dev approve` refuses before signing ("middleware hash differs from code pin"; "provider folder is writable by you") |
| PC-20 | §1 | a record whose deciding commit is signed by a `role = "recovery"` key; a trust-file change signed by the same key | refused; accepted |
| PC-19 | §5 | two approval commits by the same key with counters 7 then 5; 7 then 12 | finding; notice "5 signatures in between" |

Measured here, with throw-away keys in the scratch folder (no device): a commit signed with the listed key verifies
(`git verify-commit` exit 0, `%G?` `G`); an unsigned commit gives exit 1 (`N`); a commit signed by an unlisted key
prints "Good ... signature" **and** "No principal matched", exit 1, `%G?` `U`; a signer line limited to
`namespaces="git"` refuses a detached signature made in another namespace ("Could not verify signature"). A checker that
tests only for "Good" in the output would accept the unlisted key: the verifier reads its own result, never text.

## What it gates

From enforcement (D9: after WP-0 to WP-6 and the re-sign of the live record): every `<app>-v*` tag and every `v*` tag,
through C108 in the release workflows and the post-tag audit. A tag is refused when a gate it depends on is lowered by a
record that does not verify: ADR 0093's waivers and mode, Debrief's C13 ratchet target (`debrief-v1.2.0`), ADR 0073's
permanent exceptions, and rule 55's date moves to a later release line. From the same point, a release tag counts only
when it was cut from a consumed release intent (D7, §7); without one the C108 action refuses to publish and the audit
reports the tag. Until enforcement, C108 is report-only and prints its verdict in each release summary. Landings on
`main` and trains are not gated by C108 (an unsigned record just does not count). No release in flight depends on a user
record other than Debrief's ratchet (one touch to re-sign); the Debrief date move of `eba981aed` tightens and needs
none.

## Parallelism and cost (rule 51)

Verification is bounded by the number of approval records and distinct approval commits: one `git log -L` per record,
one blame per subject file, one signature check per commit (cached by sha), run in parallel up to cores minus one.
Measured as separate processes on this Mac (`docs/benchmarks.md`, "Approval signature verification (ADR 0097,
2026-11-10)"): 22 to 52 ms per `git verify-commit` and 8 to 12 ms per `ssh-keygen -Y verify` at load 9 to 13; on
`docs/gates.toml` (1 023 lines, 14 commits) 15 to 31 ms per whole-file blame, 44 to 52 ms per `git log -L` of one record
and 43 to 53 ms for its first-parent log, at load 16 to 20. So 200 approval commits checked by `git verify-commit` one
by one would take 4 to 10 s; the in-process verifier avoids the process start and is not measured yet. At today's count
(one live record) the cost is the `git fetch` of `main`.

**Cost to the user**: one touch per `./dev approve` batch plus reading what it shows and comparing the printed sha; one
touch and one variable change per rotation; one touch per change of a code pin; at bootstrap about 30 minutes; one touch
per release intent, signed ahead (several intents fit one batch), and none at tag time. Not measured (no device was
used, by design).

## Consequences

- Without a touch, no record verifies: an agent alone can no longer make anything count as the user's approval.
- With a touch, what was signed is only as trustworthy as the signer process (§9) until D8 (b); the printed sha and
  counter and the path rule narrow that gap.
- Every "approved by the user" in data becomes either verifiable or visibly unverified; `"coordinator"` stays as an
  honest name for an agent's decision.
- ADR 0093's waivers and mode, and future user-only switches, have one mechanism.
- Approvals arrive on `main` as their own small docs-only branches, outside trains, before the tags that rely on them.
- The release summary prints the trust root on every tag; the post-tag audit lists every published release.

## Rollback

C108 is a check, not a runtime change. If it misbehaves, its tag-gate step returns to report-only through a signed
switch record (a lowering, so the user signs it), and the gates treat every user record as unverified, which is the
strict state. Rolling back never loosens a gate.

## Alternatives

- **(b) alone, separate identities.** Proves which account wrote a comment or a commit, but while agents run as the
  user's macOS account they can reach the user's credential, browser and keychain. Recommended as defence in depth; declined for now by the user (D8, §8).
- **(c) Honour system plus hooks.** Today's state: instance 7, bypassed by a script or the browser. Rejected.
- **Detached signatures per record** (`ssh-keygen -Y sign -n paddock-approval@...` over each record's canonical bytes,
  the signature beside it). Binds the value directly, so it needs neither blame nor `git log -L`, and deletions of a
  field break the signature by construction; but it doubles the files, has no Gitea display, and still needs §6's
  deletion rule for whole records. The fallback for D2; if the projection rule proves fragile in WP-2, switch to it.
- **Signed tags as approvals.** A tag signs a whole tree and cannot say which lines are approved. Rejected for records.
  Signing each release tag was D7 (b) until the user chose signed release intents (2026-11-10): an intent is signed
  ahead, so the release does not wait for the user.
- **Gitea `require_signed_commits`** on `main`. Requires some trusted signature on every commit, the bot's included; it
  says nothing about the user. Not used.

## Decisions for the user

| # | Decision | Options | Recommendation |
|---|---|---|---|
| D1 | Mechanism | (a) user-presence signatures; (b) separate identities; (c) honour plus hooks | **(a)**; (b) declined for now (D8) |
| D2 | What carries the signature | (a) signed commits, each record judged by the projection rule and per-line blame (§3); (b) detached signatures per record in their own namespace; (c) signed tags | **(a)**; fallback (b) |
| D3 | Keys | (a) two FIDO2 keys (`sk-ssh-ed25519`, touch, PIN if supported), one routine, one recovery kept apart; (b) one FIDO2 key for routine signing and the Mac's Secure Enclave key as recovery; (c) no FIDO2 key: the Secure Enclave key for routine signing (needs Touch ID, which the user's Mac mini does not have) plus an offline software recovery key made, kept and used off the Mac. Recovery keys sign trust-file changes only. Every FIDO2 option needs the root-owned middleware of §1 (`sudo`) | **Accepted 2026-11-10: no interim signer; (a) when two FIDO2 keys arrive** (the user has none yet, and the Mac has no Touch ID, so (b) and (c) are out). Until the first FIDO2 key is enrolled no record verifies (the stricter state; waivers stay impossible). The offline recovery key is made off the Mac in WP-0; genesis waits for the first FIDO2 key (§10) |
| D4 | Trust root | (a) chained trust files from a pinned genesis, bound to the repository, pin in an admin-only Actions variable and in the release summary (§2); (b) keys only in a CI variable; (c) repository file only | **(a)** |
| D5 | Scope | (a) every record a check or gate reads as the user's, trailers with `approved: user`, gate switches that lower a tag gate, code pins, and ADR 0096 acceptances from the cutover; (b) the same without acceptances | **(a)** |
| D6 | Lowering switches | (a) a lowering of a tag-guarding gate always needs a user record, raising never; switches outside files are ignored; (b) the coordinator may lower for at most 72 h with an unsigned record, the user signs to extend | **(a)** |
| D7 | The tagged-tree risk and who cuts release tags (§7) | (a) post-tag audit from `main` only; (b) (a) plus release tags signed by an approval key (one touch per tag); (c) protected tags limited to the user once D8 (a) is done (the user tags); (d) (a) plus signed release intents: the user signs ahead (app, version, gates, expiry, optional commit or branch), CI's pinned `release-cut.yml` tags when every named gate is green, and the audit compares every tag with a consumed intent | **Accepted 2026-11-10 (user, changed from (b)): (d)** |
| D8 | Identities | (a) bot account for agents and CI, the user's credential out of the keychain item agents' git reads, claude-in-chrome blocked on the Gitea host, branch protection (no force push or deletion) on `main` and trains, publish credentials only in workflows agents cannot change before they run; (b) agents in a separate macOS account | **Recommended (a) now and (b) later; the user declined (a) for now on 2026-11-10 (no bot account, no branch protection, publish credentials stay where they are) and keeps (b) for later as its own ADR.** Everything (a) would close stays forgeable but visible (§8) |
| D9 | Migration and enforcement | (a) re-sign the live record at genesis, land ADRs 0089 to 0096 before the cutover, grandfather history by ancestry of a pinned cutover commit, report-only until WP-0 to WP-6 are done, then enforce at tags; (b) re-sign everything, including the 67 accepted ADR status lines | **(a)** |
| D10 | Rule | (a) a Paddock rule and a new ER (wording below); (b) ADR only | **(a)** |
| D11 | Protecting the code that decides | (a) the decision in its own pinned crate and binary (`paddock-approvals`, with its manifest, `paddock-merge-rule` and lock entries), always built in the gated job, one pinned gate action, the pinned audit workflow, `docs/approvals/**` and the middleware hash; release workflows compared by the audit; callers unpinned; (b) pins over all of `tools/paddock-dev` (a touch per `./dev` change); (c) the post-tag audit from `main` only | **(a)** |

**Proposed rule** (a Paddock rule, number on acceptance, and a new ER after ER-36; wording for the user's approval, not
applied while Proposed): "A record that stands for the user's consent (an approval, a waiver, a date move, a switch that
lowers a gate, an acceptance) counts only when its current value was written by a commit on `main` signed with one of
the user's presence keys in the trust root. Agents prepare such records; they never make them count. Anything that does
not verify is treated as not approved, and the stricter state applies."

## Work packages (agent-days)

| WP | What | Days | Needs |
|---|---|---|---|
| WP-0 | Preparation while the keys are bought: the offline software recovery key made on another machine by the user (public key only to the Mac); the FIDO middleware built from a pinned OpenSSH release with its sha256 printed (fallback: the pinned `ssh-keygen` beside it); `./dev approve --genesis`, `docs/approvals/README.md`, `subjects.toml` written and tested with test keys. The Secure Enclave reuse test is not done (no Touch ID); it is an option only if a Touch ID keyboard is added later | 1 | D3, D4 |
| WP-1 | Verifier core on `ssh-key` 0.6.7: flags and counter read from the signature bytes, `[[key]]` presence rules, allowed-signers parser; test vectors built in the test (PC-6, PC-8, PC-19) | 1 | D2 |
| WP-2 | History: strict schemas and record ids, the structural walk of §3 (memoized, parents in recorded order), fixed-config blame, chain, genesis descent, pin read with the bot's read token, server time from `approvals-audit.yml` runs (6 when missing or not monotonic), the merge-base rule in `crates/paddock-merge-rule`, shared with the merge check, deletion rule, `Outcome`s (PC-1 to PC-5e, PC-9 to PC-13, PC-20) | 2.75 | WP-1 |
| WP-3 | Library API for callers and the C100 wiring; switches and trailers (PC-7, PC-11). ADR 0075's `./dev copies`, ADR 0073's check and ADR 0096's `./dev adr status` wire it in their own work packages | 1.5 | WP-2 |
| WP-4 | `./dev approve`: fresh clone, cleared environment, fixed signer, middleware hash and folder-owner check, per-record y/N, sha and counter printed; path rule (PC-16, PC-18, PC-18b); the `release_intent` kind (prompts for app, version, gates from the app's release workflow, expiry with a short default, optional commit or branch; shows what CI will tag) | 2.25 | WP-2 |
| WP-5 | C108 in conformance (report); the crate's own binary and the pinned `.gitea/actions/approvals-check` action called by every release workflow; release summary prints the trust root; runs on the Linux and Windows runners (PC-17b); the execution-closure computation over `uses:` and `run:` and the full-sha rule for external actions (PC-17d) | 1.75 | WP-3 |
| WP-6 | Post-tag audit on `main`: descent check, job and step comparison, code pins, release and package listing, the intent comparison (item 5), issue and release note; the pinned `approvals-audit.yml`; the pinned `release-cut.yml` (resolve the commit, wait for the named gates through the Actions API, cut the annotated tag once, idempotent on reruns) and the C108 action's intent check at tag runs; the cut run's `(intent id, tag, sha)` artifact and its lookup by the action and the audit, the first-parent and landed-after rule, gates bound to workflow path, job and blob; recorded-API tests (PC-14, PC-15, PC-17, PC-17b, PC-21); protected-tag behaviour on a scratch repository | 4 | WP-5, D7, D11 |
| WP-7 | Record and check what exists (D8 (a) declined): the runners with their ids, names and labels, and "d-gtr9 is outside the user's Mac account (user, 2026-11-10)"; the tokens agents and CI hold (the admin token included); where the tag and publish credentials live; branch protections and protected tags (none today). The audit prints this inventory in its summary, and a change is a notice | 0.5 | none |
| WP-8 | Migration: inventory across every `origin` branch (including `eba981aed`), ADRs 0089 to 0096 landed before the cutover, re-sign batch, cutover commit pinned, rule text, briefs (coordinator, integrator, reviewer), lessons entry; **D3 key steps** as the FIDO2 keys arrive (§10): middleware installed with `sudo` and a real signature through it first, genesis signed by the first key (offline key enrolled as recovery), pin set; the second key enrolled as recovery in a commit signed by the first, the offline key retired or kept recovery-only, pin updated | 1.25 | WP-4, D9, the keys |
| | **Total** | **16 agent-days** (15.75 to 17.75 with the risks below) | |

**Enforcement needs WP-0 to WP-6: 14.25 agent-days.** Without WP-6 nothing detects a bypass at a tag (§7) and there is
no server time (§2), so `valid-*` checks would report 6, and no release intent can be cut. WP-0 to WP-5 (10.25 days)
give a report-only C108 that already shows which records would count. WP-7 can run in parallel from the start.

## Risks

- **The confused deputy (§9).** A touch proves presence, not understanding, and the signer process shares the account
  with the agents until D8 (b). Mitigations: approval-only keys, the fresh clone and cleared environment, the printed
  sha and counter, path-limited commits, code pins.
- **Counter semantics differ by authenticator.** A per-device counter makes every unrelated use a gap; the alarm is a
  notice for the user, not a refusal.
- **D8 (a) is declined for now, so the pin, `main`'s history, the audit ledger and the tag and publish credentials stay
  agent-reachable.** §8's table: forgeable but visible through the audit and the release summary, and the audit's
  memory can itself be edited with the admin token.
- **Nothing verifies until the first FIDO2 key arrives.** That is the stricter state by design: ADR 0093 waivers stay
  impossible and gates that need a user approval stay closed. The delay is the time to buy the keys.
- **The FIDO middleware is extra software on the signing path.** It is built from a pinned OpenSSH release, installed
  root-owned by the user, and its hash is in a code pin checked before each use; a macOS update that ships a working
  `ssh-sk-helper` would let WP-0's successor drop it.
- **The projection rule depends on `git log -L` following a record's span.** A rename of a subject file or a large
  re-ordering may lose the trail; then the record is unverified (fail closed) and the user re-signs it. If that happens
  often, D2 falls back to detached signatures.
- **Callers that are not built yet.** ADR 0073's check and ADR 0075's `./dev copies` must call the library when they are
  built; until then those approvals are judged by C108's report and the post-tag audit only.
- **History must be present.** Release jobs that check out shallowly report not measured; several release workflows
  already use `fetch-depth: 0`, and WP-5 makes the C108 step fetch `main` in full.

## Review answers (revision 2)

The independent security review of revision 1 (`d21eddcbf`) returned CHANGES with four P1s and P2s.

- **P1-1, blame cannot see deleted lines:** §3 now judges a record by its parsed projection at the newest commit that
  changed it (`git log -L`), with strict schemas, declared id keys and no dotted keys or subtables; blame stays as a
  second condition. PC-5b and PC-5c added.
- **P1-2, signer integrity:** §9 signs in a fresh clone with a cleared environment and a fixed signer, prints the sha
  and counter; the verifier records counters (§5, PC-18, PC-19); Consequences reworded.
- **P1-3, code protection contradicted the path rule:** code pins (§4, D11, PC-17); release workflows are protected by
  the post-tag audit only, said so.
- **P1-4, residual claims too strong:** §8's table; draft-until-audit withdrawn as a control; the audit checks descent,
  published releases and packages.
- **P2s:** heads refreshed (ADR 0093, 0092, 0096 accepted); the "contrary to 0093" remark dropped; server-recorded time
  for `valid-*` (§2); the `ssh-key` spike answered and its fallback dropped (§5); presence kept in `subjects.toml`, not
  in `allowed_signers` (§1); Secure Enclave marked unverified and moved to recovery (§1, D3); D3 (c) and the interim
  added; WPs re-estimated (12 days, first cut 8 or 10); genesis descent (§2); the line 1015 comment (Context, §3, §10);
  ADR 0075 and ADR 0096 interplay (§4); the unlanded `a5153d114` (§10); the benchmark arithmetic corrected and blame
  measured.

## Review answers (revision 3)

The re-review of revision 2 (`c591c5668`) returned CHANGES with two new P1s.

- **P1-A, the walk:** confirmed: `git log --first-parent -L1016,1024:docs/gates.toml` on the train lists `1b85776a7`,
  `8443b5b77` and `4bb899377` (two of them landing merges). §3 now walks the graph structurally from `main`'s tip
  (memoized, parents in recorded order, a merge passes only a value one parent already had); PC-5d added.
- **P1-B, the FIDO2 signer:** confirmed: `/usr/libexec/ssh-sk-helper` contains "internal security key support not
  enabled" and imports no `fido_*` symbol; `/opt/homebrew` is owned by `decryp7`. §1 adds a root-owned middleware in
  `/usr/local/libexec/paddock/`, installed with the user's `sudo` in WP-0, pinned by hash and checked by `./dev
  approve`; PC-18b added; the claim that local `ssh-keygen(1)` documentation proves a touch is dropped; WP-0 and WP-4
  grow by 0.5 d each.
- **P2s:** §8's left column qualified; code pins cover `Cargo.toml`, `Cargo.lock`, the shared modules and the
  middleware; recovery keys sign trust files only (PC-20); WP-0 tests an agent-held LocalAuthentication context; §9
  names the clone's source and credential, never passes `SSH_AUTH_SOCK`, and notes agent-pushed refs; `eba981aed` and
  ADRs 0089 to 0096 in the migration; server time from `approvals-audit.yml`, 6 when missing; record ids unique per
  kind; long lines wrapped.

## Review answers (revision 4)

The re-review of revision 3 (`849a4ab13`) returned CHANGES with one P1.

- **P1-C, replay by merge:** step 3 of the walk now carries a value from a parent only if the other parent still has the
  merge base's value, symmetrically; otherwise the merge changed the value and must be signed (PC-5e). The audit's
  descent check requires the last audited sha on the new tip's first-parent chain. §8 no longer says a replay needs a
  force-push. The same rule is named for the merge check. WP-2 grows by 0.25 d.
- **P2s:** the middleware hash check is said to compare agent-written values; loading a self-built provider in
  `ssh-sk-helper` is marked unverified with a fallback, and WP-0 starts with a real signature; the code-pin scope is the
  module closure, checked by C108; deleted audit runs and non-monotonic run ids give 6; enforcement needs WP-0 to WP-6.

## Review answers (revision 5)

The re-review of revision 4 (`db150467b`) returned CHANGES for one P1; P1-C was verified case by case.

- **P1-D, callers were not pinned:** the decision moves into its own crate and binary, `crates/paddock-approvals`,
  pinned whole with its manifest, in-repository path dependencies and lock entries; the tag gate is one pinned action,
  `.gitea/actions/approvals-check/action.yml`, which every release workflow calls and the audit requires;
  `approvals-audit.yml` is pinned; C100 and the other callers stay unpinned and only lose their early report if they
  lie. §8 corrected; PC-17 widened, PC-17b added; WP-5 grows by 0.5 d (total 13.75, enforcement 11.75).
- **P2s:** the crate pin covers `build.rs`, `#[path]` and `include!` by covering the whole folder; the merge-rule note
  cites `origin/feat/dev-merge-check` (`b18bd1ad8`) instead of an agent memory note (a search of `docs/lessons.md` on
  `origin/main` and the train found no merge-check lesson).

## Review answers (revision 6)

The review of revision 5 (`a01e7a07d`) asked for one last fix and approves with it.

- **P1-E, a fetched binary is unpinned:** the action and the audit always build `paddock-approvals` with `cargo build
  --locked` from the pinned sources in the gated job and never fetch; the invariant "everything between the pinned
  sources and the gate's exit code is either pinned or built inside the gated job" is stated in §4; PC-17c added.
- **P2:** the merge-base rule moves into `crates/paddock-merge-rule`, which `paddock-approvals` and the merge check both
  depend on; `paddock-repo-graph` leaves the pin set.

## Sources

Read on 2026-11-10.
- git: local `git help config` for git 2.56.0 (`gpg.format`, `gpg.ssh.allowedSignersFile`, `gpg.ssh.defaultKeyCommand`);
  https://git-scm.com/docs/git-config (page "last updated in 2.56.0").
- OpenSSH: local `ssh-keygen(1)` of OpenSSH 10.5p1 (FIDO AUTHENTICATOR, ALLOWED SIGNERS: `namespaces`, `valid-after`,
  `valid-before`; no presence option for signers);
  https://raw.githubusercontent.com/openssh/openssh-portable/master/PROTOCOL.u2f (flags byte and counter in `sk`
  signatures); https://raw.githubusercontent.com/openssh/openssh-portable/master/ssh-keygen.c and `.../sshsig.c`
  (`sig_verify()` logs `sk_flags`, nothing rejects a clear presence bit).
- GitLab issue https://gitlab.com/gitlab-org/gitlab/-/issues/545178 (2025): git reports "Good" for `no-touch-required`
  signatures. Go x/crypto commit https://go.googlesource.com/crypto/+/b61cf853a89d82cad68da5e12a6beca2116f8456 (enforces
  presence by default in that library).
- macOS: local `ssh-keychain(8)` and `sc_auth(8)` on macOS 27.2 (build 26B123); `/usr/lib/ssh-keychain.dylib` present;
  `/usr/libexec/ssh-sk-helper` (root-owned, 2026-10-23) contains "internal security key support not enabled" and `nm -u`
  lists no `fido_` symbol; `/usr/local` is `root:wheel`, `/opt/homebrew` is `decryp7:admin`.
  *(Secondary, unverified)*: https://gist.github.com/arianvp/5f59f1783e3eaf1a2d4cd8e952bb4acf and
  https://notes.billmill.org/link_blog/2025/11/Native_Secure_Enclave_backed_ssh_keys_on_MacOS.html (macOS 26 Secure
  Enclave ssh keys through `sc_auth` and the library).
- Secretive: https://github.com/maxgoedjen/Secretive (MIT; latest release v4.0.0, 2026-09-21; Touch ID per access,
  notifications).
- `ssh-key` crate: https://docs.rs/ssh-key/latest/ssh_key/struct.SshSig.html (0.6.7, Apache-2.0 OR MIT); `sk-*` support
  and the flags position per the independent review of revision 1.
- Gitea: https://docs.gitea.com/administration/signing (version 28: verification against keys in Gitea's database and
  `TRUSTED_SSH_KEYS`); https://docs.gitea.com/usage/access-control/protected-tags (version 28); the instance's API
  (version 28.3.0) for users, keys, branch protections and commit verification; release notes
  https://blog.gitea.com/release-of-1.16.0-and-1.16.1/ (SSH commit signing and verification, git 2.34+, OpenSSH 8.1+).
