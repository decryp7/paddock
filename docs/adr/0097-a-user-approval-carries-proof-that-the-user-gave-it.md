# 97. A user approval carries proof that the user gave it

- Status: Proposed 2026-11-10. Drafted at the coordinator's request of 2026-11-10 under ER-21 (a class seen three or more
  times gets a redesign ADR), with requirements R1 to R5 from the ADR 0093 review. Nothing here is built yet. ER-23 does
  not apply (no drawings).
- Date: 2026-11-10
- Related: ER-5, ER-14 (tests never touch real keys or the keychain), ER-15, ER-21 / rule 40, ER-31, ER-33 / rule 71,
  ER-36; rule 55 (`Copies-date: ... approved: <coordinator|user>`); ADR 0022 and its build-inputs amendment (main-only
  publish bot for `./dev`); ADR 0034 and 0075 (copy waivers, `[[date_move]] approved`); ADR 0060 (`docs/gates.toml`,
  `approved`, `Gates-date:`, C100); ADR 0073 (`approved = "user ..."` on shipped-double exceptions, `Doubles-date:`); ADR
  0078 (shared test scratch); ADR 0089 (`Outcome`: exit 5 findings, 6 not measured, 7 check broken); draft ADR 0091
  (`evidence-heads`); draft ADR 0093 (`origin/docs/adr-scheduled-owners`: §5 waivers and the gate mode, which defer to this
  ADR, and its requirements R1 to R4); draft ADR 0096 (decision register: `status = "accepted"` rows).
- Number: 0097 was free on every `origin` branch on 2026-11-10 (0090 to 0096 are taken; no file 0097 or higher on any
  branch). Check id: **C108** (C105 to C107 are claimed by drafts 0091 to 0093; no branch names C108).
- Evidence: read in git on 2026-11-10 at the heads below; the Gitea API of this instance (version 28.3.0), read-only, with
  the agents' `GITEA_TOKEN`; local probes with throw-away keys in the agent's scratch folder (never a real key, the
  keychain or ssh-agent; ER-14). Sources outside the repository are listed with what was read and when (§ Sources).
  Statements marked *(secondary, unverified)* come from community write-ups only.

```evidence-heads
read: 2026-11-10
origin/train/2026-11-09-3 c151422a4
origin/main b51c72509
origin/docs/adr-scheduled-owners 6270bbbbe
origin/docs/adr-decision-register baa0f01c6
origin/docs/adr-evidence-heads b32655fef
origin/docs/adr-private-files fdfb17c8f
origin/docs/adr-text-cuts-repo-copies 2e5017b46
origin/docs/adr-captured-tool-output 9c2cd4d8e
```

## Context: the instances

Paddock records many things as "approved by the user". Every one of them is text that an agent can type.

| # | Where | What stands for the user's consent | Who can write it today |
|---|---|---|---|
| 1 | `docs/gates.toml` (ADR 0060 §3.1, §3.2) | `approved = "user"`: one live entry on the train, Debrief's C13 `[[ratchet]]` (line 1023), whose comment says "approved: user 2026-11-08"; it was committed unsigned by `fa6bfcded` (`%G?` = `N`). C100 parses the value in `tools/paddock-dev/src/gates/register.rs` (`Approver::User`, lines 53 to 74, 238 to 242) and requires it for a ratchet (line 367) | any agent with commit access |
| 2 | `Gates-date: <id> <old> -> <new>; approved: <coordinator\|user>` (ADR 0060 §3.1; `register.rs` lines 497 to 528) | a commit trailer; a second move needs `approved: user` | any agent writing a commit message |
| 3 | `Copies-date: ... approved: <coordinator\|user>` (rule 55 (d), `docs/rules.md` lines 298 to 300) and its replacement `[[date_move]] approved = "user"` (ADR 0075 §1, line 107) | a trailer, then a data field | any agent |
| 4 | ADR 0073's register (`approved = "user 2026-11-08"`, its lines 528 to 553; `Doubles-date:`; "a permanent kind needs `approved: user`") | a data field and a trailer | any agent |
| 5 | Draft ADR 0093 D5 (its revision 1, `34e7eeb00`: a `waive:` Gitea comment by the user's account, after a bot move); revision 3 (`3bf12da60`) moved waivers and the gate mode into signed records and deferred the design here | a Gitea comment, then a record | any agent: the token agents hold is the user's (below) |
| 6 | Draft ADR 0096 (decision register: `status = "accepted"`, `source = "memory ..."`) and 67 ADR files on the train whose status line says Accepted, most "by the user" | a status line | any agent |
| 7 | The standing practice that agents never write `approved` lines for the user (0093 revision 1 §5: "the brief rule already in force") | an instruction | honoured, not enforced |

None of this code is in a release: `tools/paddock-dev/src/gates/register.rs` was added by `3a38c41ed`, and `git tag
--contains 3a38c41ed` and `git tag --contains fa6bfcded` print nothing; neither is an ancestor of `origin/main`
(`b51c72509`). All 6 860 commits on the train are unsigned (`git log --format=%G?` over the last 3 000: all `N`).

**Agents act as the user** (verified on 2026-11-10 unless marked):

- Agents run as the user's macOS account.
- `git config --show-origin credential.helper` prints `file:/opt/homebrew/etc/gitconfig osxkeychain` and the remote is
  `https://dev.decryptology.net/decryp7/paddock.git`, so `git credential fill` yields the user's Gitea credential (from
  the 0093 review; not re-run here, because it reads the keychain).
- `GET /api/v1/user` with the agents' `GITEA_TOKEN` answers `decryp7`, `is_admin: true`. The instance's users are
  `decryp7` (admin), `paddock-ci-reader`, `paddock-ci-writer` and `renovate-bot`: bot accounts exist, agents do not use one.
- The repository has **no branch protection** (`GET /repos/decryp7/paddock/branch_protections` returns an empty list),
  so the history of `main` is only as fixed as the agents' discipline (the force-push hook is untracked, ADR 0075 line 23).
- The claude-in-chrome tools act in the user's signed-in Chrome (from the 0093 review).
- CI secrets are readable by any workflow an agent pushes; a PreToolUse hook is bypassed by a script file or the browser.
- Gitea has no SSH or GPG key on the user's account (`/user/keys`: 0, `/user/gpg_keys`: 0) and no instance signing key
  (`/signing-key.gpg`: "no signing key"); the train's head reports `verified: false, reason: gpg.error.not_signed_commit`.

## Pattern

**An approval that the user must give is recorded as text, and every party that can write the text is an agent.**
Whatever the record (a field, a trailer, a comment, a label, a status line, a switch), its proof is the author name on
a commit or a comment, and agents hold the user's name. Each new mechanism re-invents the field (`"user"`,
`"user 2026-11-08"`, `approved: user`) and re-states "agents never write it" as an instruction. Hooks cannot close it:
agents can reach the same effect through a script, the keychain credential or the browser.

**Invariant.** A record that claims the user's consent counts only if it carries a signature that needs the user to be
physically present (a touch or Touch ID), made with a key in a trust root that agents cannot change, and only for the
exact lines the user signed, read from history agents cannot rewrite. Anything else is "not approved", with a clear
message, and the stricter state applies.

## Decision

### 1. What a proof is: a user-presence signature on the commit that wrote the lines

A user approval is a **commit signed by a user-presence key** (`gpg.format=ssh`), and the approval is exactly the lines
that commit wrote. Git signs commits with SSH keys through `ssh-keygen -Y sign` in the `git` namespace and verifies
against `gpg.ssh.allowedSignersFile` (git-config, `gpg.format` accepts `ssh`; local `git help config`, git 2.56.0).

Keys (D3):
- **FIDO2 security key**, `sk-ssh-ed25519@openssh.com`, created with the default touch requirement (and `verify-required`
  for a PIN if the key supports it). The private key never leaves the device; each signature needs a touch (local
  `ssh-keygen(1)`, "FIDO AUTHENTICATOR").
- **Secure Enclave key**, through the system's `/usr/lib/ssh-keychain.dylib` (present on this Mac, macOS 27.2; local
  `ssh-keychain(8)`: "ecdsa256 for Secure Key module"), created with `sc_auth create-ctk-identity -l <label> -k p-256-ne
  -t bio` (local `sc_auth(8)`: `-ne` non-exportable, `-t bio|none` the private key protection). It appears to ssh as
  `sk-ecdsa-sha2-nistp256@openssh.com` *(secondary, unverified: community write-ups of macOS 26, 2025-11)*.
- Not accepted: plain software keys, and agent-held keys such as Secretive's (MIT, v4.0.0 of 2026-09-21; plain ECDSA
  signatures, so whether a use needed Touch ID is a setting the verifier cannot see).

**The verifier checks the user-presence flag itself.** An `sk-*` signature carries a flags byte and a counter (OpenSSH
`PROTOCOL.u2f`). `ssh-keygen -Y verify`, which `git verify-commit` calls, does **not** reject a signature without user
presence: `ssh-keygen.c`'s `sig_verify()` only logs `sk_flags` at debug level and `sshsig.c` never reads them (both read on
2026-11-10 at `openssh-portable` `master`), and GitLab issue 545178 (2025) shows git reporting "Good" for commits signed
with `-O no-touch-required` keys. An agent with the key handle file could ask the authenticator for a silent assertion
(CTAP2 `up=false`); the signature is then valid but has the presence bit clear. So the Paddock verifier (§5) parses the
signature and requires the presence bit (and the verification bit when the signer line says so). A `git log %G?` or `git
verify-commit` result alone is **not** a proof under this ADR, contrary to the wording of 0093 §5 (revision 4, `6270bbbbe`,
line 262).

**Namespaces and scope of a key.** The approval keys are used for approvals only, so a touch request the user did not
start is an alarm. The probe below shows `namespaces="git"` in the signer line refuses a signature made for another
namespace.

### 2. The trust root: `docs/approvals/allowed_signers`, chained from a pinned genesis (R1)

The file lists the user's approval keys in `ssh-keygen(1)` ALLOWED SIGNERS form, one principal
(`decryp7@approvals.paddock.invalid`), `namespaces="git"`, and `valid-after`/`valid-before` for rotation. It is protected
three ways:

1. **Chain.** Every line of the file blames (§3) to a commit signed by a key that is valid in the version of the file at
   that commit's first parent. A key can enter only through a signature by a key already trusted. The **genesis** commit,
   which creates the file, is signed by a key it lists.
2. **Pin outside the repository.** `PADDOCK_APPROVAL_ROOT` = `<genesis commit sha> <sha256 of the current file>`, a Gitea
   Actions repository variable that only an administrator can change. The verifier refuses (exit 5) when the file on
   `main` does not hash to the pin, or the genesis is not that commit, and reports not measured (6) when it cannot read the
   pin. A rotation therefore needs the user's signed commit **and** the user's own change of the variable.
3. **Shown to the user.** Every release summary prints the trust root (key fingerprints, genesis sha), and the user keeps
   the genesis sha and fingerprints in their own password manager at bootstrap, so a substitution is visible at the next
   release even if both checks above were defeated.

Honest limit: while agents hold the user's administrator token, the browser session and the keychain credential, they
can change the variable and force-push `main`; the chain then still holds but the pin does not. §8 (D8) closes that by
taking those credentials out of the agents' reach. Before D8 is done, this ADR makes a forged approval a deliberate,
visible act (a change of the trust root, the pin or the verifier, each shown in the release summary), not a typo.

### 3. Every line of a record blames to a trusted signed commit (R2)

A **record** is a TOML table (from its header line to the line before the next header or the end of file, including
comments inside it) in a file of §4. A record counts as the user's only if **every** line of it blames to a commit whose
signature verifies (§1, §5) against the trust root valid at that commit (§2). So an unsigned later edit of `until`,
`reason`, `mode` or a signer line voids the record; a line moved from one record to another blames to the commit that
moved it; a line a merge's conflict resolution changed blames to the merge. All of these fail closed.

The blame runs with a fixed configuration, so the tree under check cannot steer it: `GIT_CONFIG_NOSYSTEM=1`,
`GIT_CONFIG_GLOBAL=/dev/null`, `GIT_NO_REPLACE_OBJECTS=1`, `-c blame.ignoreRevsFile=`, no `-M`/`-C`, `--porcelain`, on
`origin/main` (§6). Grafts and a shallow boundary make blame land on a boundary commit; a boundary inside a record is
reported as not measured (6), never as a pass.

**Fields agents maintain.** Some kinds hold both the user's decision and a list agents shrink (ADR 0060's ratchet:
`target` is the user's, `not_yet` shrinks with each opted-in crate). A kind declares such fields `agent_maintained` in
the subject register (§4), with the check that keeps them only tightening (C100's shrink-only rule for `not_yet`). Their
lines are exempt from the blame rule; every other line of the record is not.

Trailers: an `approved: user` trailer (`Gates-date:`, `Copies-date:`, `Doubles-date:`) counts only when the commit that
carries it verifies under §1 and §5. A trailer is part of the signed commit object, so it needs no blame.

### 4. What carries an approval, and what each artifact becomes (R3)

`docs/approvals/subjects.toml`, itself a trust-root-protected file (every line blames to a signed commit), declares each
**subject**: a file, the table kind whose records are the user's, and the kind's `agent_maintained` fields. The verifier
never infers subjects from wording.

| Today | Becomes |
|---|---|
| `approved = "user"` in `docs/gates.toml` (ADR 0060), ADR 0073's register, ADR 0075's `[[date_move]]` | unchanged text; the record counts as the user's only under §3. `approved = "coordinator"` stays as it is and claims nothing about the user |
| `Gates-date:` / `Copies-date:` / `Doubles-date:` with `approved: user` | honoured only on a commit that verifies (§3, trailers) |
| 0093's `[[scheduled_waiver]]` (workflow, job, signature, reason, `until` at most 14 days) | a subject of kind `waiver` in `docs/gates.toml`; expiry as 0093 says |
| **Gate switches**: 0093's `[scheduled_gate]` (`mode`, `report_until`), ADR 0060's `report_only`, `not_gated`, `conditional` entries with `approved = "user"`, and any later user-only setting | a subject of kind `switch`. A switch that **lowers** a gate guarding a tag (enforce to report or off, a later `report_until`) needs a user record; raising never does. A switch in a repository variable, environment value or workflow input is not honoured by a gate (0093 revision 3 already says so for its mode) |
| A Gitea comment, label, issue state or the author of anything in Gitea | never a proof; display only |
| ADR status lines "Accepted ... by the user", 0096's `status = "accepted"` rows | going forward, an acceptance commit is a signed user commit (D5); history before the cutover is grandfathered by ancestry (§9) |

**How the user gives an approval: `./dev approve`.** Agents write proposals as ordinary, inert files under
`docs/approvals/pending/` (a record and its subject). In the user's own terminal, `./dev approve` prints each pending
record and the subject lines it changes, asks y/N per record, writes the accepted ones into their subject files in a
temporary worktree on a new branch `approvals/<date>-<n>`, and makes **one** signed commit for the batch: one touch. The
commit touches only subject files and `pending/` (the verifier refuses a signed approval commit that touches anything
else, so a signature never blesses code by accident). An agent pushes and lands the branch (a docs-only change, rule 50
(d)); the signature, not the pusher, is the proof. `./dev approve` never runs in an agent's session: it refuses when
`CLAUDECODE` or a non-interactive stdin is present, which is a guard against mistakes, not a security boundary.

### 5. The verifier: `./dev approvals verify` and check C108

Rust, a new module `tools/paddock-dev/src/approvals/` (ADR 0022: tooling is Rust), with ADR 0089's `Outcome` for exit
codes and JSON. It:

- parses SSHSIG and verifies `ssh-ed25519`, `sk-ssh-ed25519@openssh.com` and `sk-ecdsa-sha2-nistp256@openssh.com`
  signatures **in process**, requiring the user-presence bit for `sk-*`. The RustCrypto `ssh-key` crate (0.6.7,
  Apache-2.0 OR MIT) has `SshSig`; whether it verifies the `sk-*` algorithms and exposes the flags is not confirmed by
  its docs page, so WP-1 starts with a spike and otherwise verifies the few formats directly over `ed25519-dalek` and
  `p256`. In process means the Windows and Linux runners need no particular OpenSSH: they **only verify**, never sign,
  and have no approval key;
- parses allowed signers (principals, `namespaces`, `valid-after`, `valid-before`); `valid-*` is judged against the
  **first-parent time on `main`** at which the commit landed (§6), not the committer date, which an agent can set;
- drives `git blame` and `git cat-file` as in §3, walks the chain of §2 and reads the pin;
- caches by commit sha: one verification per distinct approval commit.

**C108** runs in two places:
- **On branches** (conformance, report only): lists records that will not count and why ("line 4 of
  `[[scheduled_waiver]] debrief-bench` blames to unsigned `1a2b3c4d5`: ask the user to sign it with `./dev approve`").
- **As a tag gate**, a step in each release workflow (§7) and in `./dev release tag` once it exists. ADR 0060's C100,
  ADR 0075's `./dev copies` (C58) and ADR 0073's check call the same library: an `approved = "user"` entry whose record
  does not verify is treated as if it said nothing, so the stricter state applies (a ratchet without a valid target
  approval is enforced at its default; an unverified waiver waives nothing; an unverified lowering switch is ignored).

### 6. Records are read from `origin/main`'s first-parent history (R5)

Every user-only record (approval, waiver, switch) is read from **`origin/main`**, fetched at gate time, never from the
tagged tree or a branch: an agent can add or delete records on a release branch. So:
- an approval reaches a gate only after its signed commit has landed on `main` (on a merge, the record's lines still blame
  to the signed commit on the second-parent side);
- the verifier walks `main`'s **first-parent** history for the subject files. Once a record id has appeared verified on
  `main`, its later absence is a finding unless the commit that removed it verifies (a signed deletion) or the record
  expired by its own `until`: **a missing record fails closed** and the gate says "record `<id>` removed by unsigned
  `<sha>` on main: restore it or have the user sign the removal". Absence never loosens anything either: the default of
  every switch is its strictest value;
- a tag whose commit is not reachable from a release line the gate knows (ER-31 release branches) is judged with
  `main`'s records only.

This differs from 0093 revision 4 (`6270bbbbe`, line 244), which reads a signed mode record "on the tagged tree and on
`origin/main` alike": under this ADR the tagged tree's copy is ignored, so a record added or removed on a release
branch has no effect (PC-12). 0093 is to follow §6 when both are accepted.

### 7. The release workflow runs from the tagged tree: the residual risk and its controls

A tag run executes the workflow files of the **tagged** commit (GitHub Actions' documented behaviour, which Gitea Actions follows; not tested on this instance). An agent could drop
or weaken the C108 step, or the gate it feeds, on a release branch and tag it; the run would then publish. Two controls:

- **Post-tag audit from `main` (detects; D7 recommended).** A workflow on `main` runs on every push to `main` and in the
  night schedule (the 2026-11-10 decision: scheduled work at night only; scheduled runs use the default branch's file, so
  a release branch cannot remove it), with `./dev` built from `main`'s inputs. It is a single Linux job of a few seconds
  that only reads the API and git (rule 50). For every release tag since its last run it: (1) compares the tag run's job and step list from the
  Actions API with the job and step list of `main`'s release workflow for that app, requiring every gate step of `main`'s
  version to have run and succeeded in the tag run; (2) re-runs C108 and the gates that read approvals against `main`'s
  records at the tag's time; (3) verifies the release tag's signature if D7 (b) is chosen. A mismatch opens a
  `release-blocker` issue, marks the release "unverified" in its notes through the API, and fails the audit run with an
  `::error::` naming the tag, the missing step and the next step (ER-27).
- **Who may create release tags.** Gitea protected tags (docs for Gitea 28: a pattern, allowed users or teams; "If you
  leave these fields empty no one is allowed to create or modify this tag") can restrict `*-v*` and `v*` to named accounts.
  If the agents' bot (D8) is not on the list, agents cannot tag: the user tags, which changes the standing "tag when
  green" practice. The middle way (D7 (b)): agents keep tagging, but a release tag counts only when it is an annotated tag
  signed by an approval key (one touch per release) and the post-tag audit verifies it. The page does not say whether
  admins bypass a rule or whether deletion is covered *(unverified; WP-6 tests both on a scratch repository)*.

What stays: between a tag push and the next audit the release is published (the audit runs at the next push to `main`
or that night at the latest; the steward's tag checklist runs it by hand right after each tag). Making the publish wait for the audit (a draft release promoted by the audit) is the
stronger form and is D7 (c).

### 8. Separate identities (defence in depth; D8)

Signatures prove consent; separate identities protect the pin (§2), `main`'s history (§6) and the tag list (§7):
- agents and CI use a bot account (`paddock-agent`: write to code and issues, no admin) instead of `decryp7`'s token, as
  0093 proposes; CI secrets that write move to the existing `paddock-ci-writer`;
- the user's Gitea credential leaves the agents' reach: it is removed from the login keychain item that
  `git-credential-osxkeychain` reads (the agents' git uses the bot token through a helper in their environment), and
  claude-in-chrome's site permissions block `dev.decryptology.net`;
- branch protection on `main` and `train/*`: no force push, no deletion; protected tags as D7 decides;
- the repository variable `PADDOCK_APPROVAL_ROOT` is then writable only by the user.

The full boundary is agents in a **separate macOS account** (no shared keychain, browser profile, ssh files or global git
config). That is a larger change to worktrees, build folders and Docker, and is left to its own ADR after the releases
(D8 (b)).

### 9. Bootstrapping, key loss and rotation, migration

**Bootstrap (WP-0, the user's hands, about 30 minutes, not measured).** The user creates two keys on two devices (D3:
a FIDO2 key and the Mac's Secure Enclave, or two FIDO2 keys), and runs `./dev approve --genesis`, which writes
`docs/approvals/allowed_signers` with both and makes the genesis commit signed by the first key (one touch). The user
notes the genesis sha and fingerprints in their password manager, sets `PADDOCK_APPROVAL_ROOT` in the repository's
Actions variables, and adds the public keys to their Gitea profile so the web view shows signed approval commits as
verified (display only: Gitea's own trust is an admin-writable key list, and its handling of `sk-*` keys is not verified).

**Key loss.** With the second key the user signs a commit that sets `valid-before=<today>` on the lost key (old
approvals keep verifying, judged by their landing time on `main`, §5) and adds a replacement, then updates the pin.
**Key compromise.** The same commit removes the key outright, and the user re-signs the live records it signed
(`./dev approvals verify --by-key <fp>` lists them), because a stolen key can sign with any date. **All keys lost.** A new
genesis, a new pin, and a re-sign of every live record; the verifier treats records signed under the old root as
unverified until re-signed. A Secure Enclave key dies with its Mac, so it is never the only key.

**Migration (D9).** The live approvals that a check or gate reads today are one record (Debrief's C13 ratchet in
`docs/gates.toml`); 0073's and 0093's are not on the train yet. They are re-signed in the first `./dev approve` batch
after genesis (one touch). History is grandfathered **by ancestry, not date** (dates are agent-writable): a cutover
commit on `main`, pinned in `subjects.toml`, and every trailer or status line in commits that are ancestors of it counts
as it did, with a notice "grandfathered (before cutover `<sha>`)". Nothing grandfathered can lower a gate after the
cutover.

## Positive controls (ER-33)

Each is a test over a synthetic repository in the shared test scratch (ADR 0078), with keys generated by the test
(software Ed25519, and `sk-*` signatures built by the test over the U2F message format so the presence bit can be set or
cleared without hardware), `HOME` in scratch, `GIT_CONFIG_GLOBAL=/dev/null`, `GIT_CONFIG_NOSYSTEM=1`, `SSH_AUTH_SOCK`
unset: never a real key, the keychain or ssh-agent (ER-14). Each must turn the verdict red (or 6 where named); the green
twin is in the same test.

| # | Requirement | Planted | Expected |
|---|---|---|---|
| PC-1 | R1 | a key appended to `allowed_signers` in an unsigned commit | refused, "line n blames to unsigned" |
| PC-2 | R1 | a key appended in a commit signed by that new key | refused, "signer not trusted at parent" |
| PC-3 | R1 | file on `main` differs from the pin; pin unreadable | refused (5); not measured (6) |
| PC-4 | R1 | a second root commit creating the file (new genesis) | refused, genesis mismatch |
| PC-5 | R2 | a signed waiver, then an unsigned edit of `until`; of `reason`; a signed line moved into another record; a line changed in a merge's conflict resolution | each record does not count |
| PC-6 | R4 | an `sk-ssh-ed25519` signature with the presence bit clear; an `sk-ecdsa` one with it set | refused; accepted |
| PC-7 | R3 | `[scheduled_gate] mode` from `enforce` to `report` in an unsigned commit; the mode set by an environment value | `enforce` stays, a finding; ignored |
| PC-8 | R2 | a signature in another namespace; a key past its `valid-before` at landing time | refused |
| PC-9 | R2 | a planted `.git-blame-ignore-revs` and `blame.ignoreRevsFile` in the repo config; a `refs/replace` object | no effect on the verdict |
| PC-10 | R2 | a shallow clone whose boundary is inside a record | not measured (6) |
| PC-11 | R3 | `Gates-date: ...; approved: user` on an unsigned commit | not honoured |
| PC-12 | R5 | a signed record only on a release branch, not on `main` | not honoured at that branch's tag |
| PC-13 | R5 | a verified record deleted on `main` by an unsigned commit | the gate refuses, "removed by unsigned" |
| PC-14 | §7 | a recorded Actions API reply (synthetic JSON) for a tag run without `main`'s C108 step | the audit fails and names the step |
| PC-15 | §7 | an unsigned release tag (D7 (b)) | the audit flags it |
| PC-16 | §4 | a signed approval commit that also changes a `.rs` file | refused, "approval commit touches non-subject paths" |

Measured here, with throw-away keys in the scratch folder (no device): a commit signed with the listed key verifies
(`git verify-commit` exit 0, `%G?` `G`); an unsigned commit gives exit 1 (`N`); a commit signed by an unlisted key prints
"Good ... signature" **and** "No principal matched", exit 1, `%G?` `U`; a signer line limited to `namespaces="git"`
refuses a detached signature made in another namespace ("Could not verify signature"). A checker that tests only for
"Good" in the output would accept the unlisted key: the verifier reads exit codes and its own result, never text.

## What it gates

From enforcement (D9: after WP-0 to WP-5 and the re-sign of the live record): every `<app>-v*` tag and every `v*` tag,
through C108 in the release workflows and the post-tag audit. Concretely, a tag is refused when a gate it depends on is
lowered by a record that does not verify: 0093's waivers and mode (0093 has no waivers until this ADR is accepted, its
§5), Debrief's C13 ratchet target (`debrief-v1.2.0`), ADR 0073's permanent exceptions, and rule 55's date moves to a
later release line. Until enforcement, C108 is report-only and prints its verdict in each release summary. Landings on
`main` and trains are not gated by C108 (an unsigned record just does not count). No release currently in flight depends
on a user record other than Debrief's ratchet, which is one touch to re-sign.

## Parallelism and cost (rule 51)

Verification is bounded by the number of distinct approval commits, not records: one blame per subject file and one
signature check per commit, cached by sha, run in parallel up to cores minus one. Measured as separate processes on this
Mac at load 9 to 13 (docs/benchmarks.md, "Approval signature verification (ADR 0097, 2026-11-10)"): 22 to 52 ms per `git
verify-commit`, 8 to 12 ms per `ssh-keygen -Y verify`, both dominated by process start; the in-process verifier avoids
that and is not measured yet. At today's counts (one live record) the cost is the `git fetch` of `main`.

**Cost to the user**: one touch (or Touch ID) per `./dev approve` batch plus reading what it shows; one touch and one
variable change per rotation; at bootstrap about 30 minutes. With D7 (b), one more touch per release tag. Not measured
(no device was used, by design).

## Consequences

- An agent can no longer make the user approve anything. The coordinator's job becomes preparing pending records and
  asking for a batch.
- Every "approved by the user" in data becomes either verifiable or visibly unverified; the `"coordinator"` value stays
  as an honest name for an agent's decision.
- ADR 0093's waivers and mode, and future user-only switches, have one mechanism instead of each ADR inventing one.
- Approvals arrive on `main` as their own small docs-only branches, outside trains.
- The release summary prints the trust root on every tag.

## Rollback

C108 is a check, not a runtime change. If it misbehaves, its tag-gate step returns to report-only in one signed switch
record (a lowering, so the user signs it) and the gates fall back to treating every user record as unverified, which is
the strict state. Rolling back never loosens a gate.

## Alternatives

- **(b) alone, separate identities.** Proves which account wrote a comment or a commit, but while agents run as the
  user's macOS account they can reach the user's credential, browser and keychain; the account boundary is only as good
  as that separation, which is not complete short of a separate macOS account. Kept as defence in depth (§8).
- **(c) Honour system plus hooks.** Today's state: instance 7, bypassed by a script or the browser. Rejected.
- **Detached signatures per record** (`ssh-keygen -Y sign -n paddock-approval@...` over each record, the signature in a
  file beside it). Binds the record's bytes without blame and survives any history shape, but doubles the files, has no
  Gitea display, and Paddock never rewrites history (ADR 0075), so blame is reliable. The fallback for D2.
- **Signed tags as approvals.** A tag signs a whole tree and cannot say which lines are approved. Rejected for records;
  used only for release tags (D7 (b)).
- **Gitea `require_signed_commits`** on `main`. Requires some trusted signature on every commit, the bot's included; it
  says nothing about the user. Not used.

## Decisions for the user

| # | Decision | Options | Recommendation |
|---|---|---|---|
| D1 | Mechanism | (a) user-presence signatures; (b) separate identities; (c) honour plus hooks | **(a), with (b) as defence in depth (D8)** |
| D2 | What carries the signature | (a) signed commits, every record line blamed to a trusted signed commit (§3); (b) detached signatures per record in their own namespace; (c) signed tags | **(a)**; fallback (b) |
| D3 | Keys | (a) two FIDO2 keys (`sk-ssh-ed25519`, touch, PIN if supported); (b) one FIDO2 key plus the Mac's Secure Enclave key (`sc_auth ... -k p-256-ne -t bio`); (c) Secure Enclave only | **(b)** if the user has one FIDO2 key, else (a) after buying two; never (c) (one Mac is one point of loss) |
| D4 | Trust root | (a) chained `allowed_signers` from a pinned genesis, the pin in an admin-only Actions variable and in the release summary (§2); (b) keys only in a CI variable; (c) repository file only | **(a)** |
| D5 | Scope | (a) every record a check or gate reads as the user's, trailers with `approved: user`, gate switches that lower a tag gate, and ADR acceptances from the cutover; (b) the same without ADR acceptances | **(a)** |
| D6 | Lowering switches | (a) a lowering of a tag-guarding gate always needs a user record, raising never; switches outside files are ignored; (b) the coordinator may lower for at most 72 h with an unsigned record, the user signs to extend | **(a)** |
| D7 | The tagged-tree risk (§7) | (a) post-tag audit from `main` only; (b) (a) plus release tags signed by an approval key (one touch per tag); (c) (b) plus publishing as a draft until the audit promotes it; (d) protected tags limited to the user (the user tags) | **(b)**; (c) once the audit has run clean for two releases |
| D8 | Identities | (a) bot account for agents and CI, the user's credential out of the keychain item agents' git reads, claude-in-chrome blocked on the Gitea host, branch protection (no force push or deletion) on `main` and trains; (b) agents in a separate macOS account | **(a) now; (b) as its own ADR after the releases** |
| D9 | Migration and enforcement | (a) re-sign the live record(s) at genesis, grandfather history by ancestry of a pinned cutover commit, report-only until WP-0 to WP-5 are done, then enforce at tags; (b) re-sign everything, including the 67 accepted ADR status lines (one touch, but 67 lines to read) | **(a)** |
| D10 | Rule | (a) a Paddock rule and a new ER (wording below); (b) ADR only | **(a)** |

**Proposed rule** (a Paddock rule, number on acceptance, and a new ER after ER-36; wording for the user's approval, not
applied while Proposed): "A record that stands for the user's consent (an approval, a waiver, a date move, a switch that
lowers a gate, an acceptance) counts only when every line of it comes from a commit signed with one of the user's
presence keys in the trust root, read from `main`. Agents prepare such records; they never make them count. Anything
that does not verify is treated as not approved, and the stricter state applies."

## Work packages (agent-days)

| WP | What | Days | Needs |
|---|---|---|---|
| WP-0 | Bootstrap: `./dev approve --genesis`, `docs/approvals/README.md`, `subjects.toml`, the user's key creation and pin (user's hands, about 30 min) | 0.25 | D3, D4 |
| WP-1 | Verifier core: SSHSIG parse, Ed25519 / `sk-ssh-ed25519` / `sk-ecdsa-sha2-nistp256` verification with the presence bit, allowed-signers parser; spike on `ssh-key` first; test vectors built in the test (PC-6, PC-8) | 1.5 | D2 |
| WP-2 | History: fixed-config blame driver, chain and genesis, pin read with the bot's read token, first-parent walk of `main`, deletion rule, `Outcome`s (PC-1 to PC-5, PC-9, PC-10, PC-12, PC-13) | 1.5 | WP-1 |
| WP-3 | Subjects and callers: kinds, `agent_maintained` fields, trailers; C100, `./dev copies` (C58) and ADR 0073's check call the library; switches (PC-7, PC-11) | 1.5 | WP-2 |
| WP-4 | `./dev approve`: pending records, display, per-record y/N, temporary worktree, one signed commit; path rule (PC-16) | 1 | WP-2 |
| WP-5 | C108 in conformance (report) and as a step in every release workflow; release summary prints the trust root; runs on the Linux and Windows runners | 1 | WP-3 |
| WP-6 | Post-tag audit workflow on `main` (job and step comparison, signed release tags if D7 (b), issue and release note), recorded-API tests (PC-14, PC-15); protected-tag behaviour tested on a scratch repository | 1.5 | WP-5, D7 |
| WP-7 | Identities (D8 (a)): `paddock-agent`, token swap in agents' environment and CI secrets, keychain item and git helper, claude-in-chrome block, branch protection (user's hands for the admin steps) | 1 | D8 |
| WP-8 | Migration and rules: re-sign batch, cutover commit pinned, rule text, briefs (coordinator, integrator, reviewer), lessons entry; 0093 §5 and ADR 0060/0073/0075 references | 0.5 | WP-4, D9 |
| | **Total** | **9.75 agent-days** | |

A first cut that lets 0093's waivers and mode be signed and checked at tags (WP-0 to WP-3, WP-5): 5.75 agent-days. WP-7
can run in parallel from the start.

## Risks

- **Touch fatigue and confused deputy.** A touch proves presence, not understanding; an agent could start a signing
  request while the user expects one. Mitigations: the keys sign approvals only, `./dev approve` is the only signer and
  shows each record before the touch, and an unexpected blink or Touch ID prompt is an alarm. Residual while agents share
  the account: an agent could alter `./dev approve` so it shows one thing and commits another. The commit is small and
  path-limited (PC-16), lands through review, and the user can read it with `git show` before pushing; D8 (b) removes the
  residual.
- **The verifier is repository code.** An agent could weaken it on a branch. Release gates and the audit run `./dev` built
  from `main`'s inputs (ADR 0022 amendment: main-only publish), and changes to `tools/paddock-dev/src/approvals/**` are a
  subject (a signed approval is needed for them to count at a tag).
- **Before D8, the pin and `main`'s history are agent-writable.** Stated in §2; the release summary makes changes visible.
- **`sk-*` support in third-party crates and in Gitea is not verified.** WP-1's spike and WP-0's display check settle it.
- **Secure Enclave details are from community sources.** The man pages confirm `sc_auth` and the library; how macOS
  presents `p-256-ne -t bio` keys to ssh is checked during WP-0 with the user's own key, not by an agent.
- **History must be present.** Release jobs that check out shallowly report not measured; CI's release workflows already
  use `fetch-depth: 0` in several jobs, and WP-5 makes the C108 step fetch `main` in full.

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
- macOS: local `ssh-keychain(8)` and `sc_auth(8)` on macOS 27.2 (build 26B123); `/usr/lib/ssh-keychain.dylib` present.
  *(Secondary, unverified)*: https://gist.github.com/arianvp/5f59f1783e3eaf1a2d4cd8e952bb4acf and
  https://notes.billmill.org/link_blog/2025/11/Native_Secure_Enclave_backed_ssh_keys_on_MacOS.html (macOS 26 Secure
  Enclave ssh keys through `sc_auth` and the library).
- Secretive: https://github.com/maxgoedjen/Secretive (MIT; latest release v4.0.0, 2026-09-21; Touch ID per access,
  notifications).
- `ssh-key` crate: https://docs.rs/ssh-key/latest/ssh_key/struct.SshSig.html (0.6.7, Apache-2.0 OR MIT).
- Gitea: https://docs.gitea.com/administration/signing (version 28: verification against keys in Gitea's database and
  `TRUSTED_SSH_KEYS`); https://docs.gitea.com/usage/access-control/protected-tags (version 28); the instance's API
  (version 28.3.0) for users, keys, branch protections and commit verification; release notes
  https://blog.gitea.com/release-of-1.16.0-and-1.16.1/ (SSH commit signing and verification, git 2.34+, OpenSSH 8.1+).
