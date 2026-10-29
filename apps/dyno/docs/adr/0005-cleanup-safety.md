# 5. Cleanup safety: nothing is offered, selected or deleted unless the policy admits it, and deletes are recoverable

- Status: superseded by ADR 0021 (2026-10-28); Dyno no longer deletes or moves files. Kept for history
- Related: ADR 0006 (observe-only), 0007 (packs), `dyno_core::cleanup` (`policy`, `catalog`), `dyno_agent::cleanup`

## Context

Cleanup is the one feature that deletes the user's files. Disk cleaners have a record of deleting too much (registry
"cleaners", over-eager Downloads sweeps, `rm -rf` of a mis-expanded path). Dyno scans an untrusted file system whose
names it does not control, and categories will come from declarative packs (ADR 0007) that must not widen what Dyno may
delete.

## Decision

1. **The scanner proposes, the core admits.** The agent (or a plugin or pack) produces `Candidate`s; only those that
   pass `policy::admit` become preview items. A candidate is refused when it:
   - is a symbolic link, junction or reparse point (never followed, never deleted);
   - contains a `..` segment (not a plain path);
   - lies outside every resolved root of its category (`is_inside` compares normalised, case-folded paths with a
     separator boundary, so `/data/cache-evil` is not inside `/data/cache`);
   - lies inside a **personal folder** (`PERSONAL_DIRS`: Documents, Desktop, Pictures, Music, Videos, Movies, OneDrive,
     iCloud Drive, Mobile Documents, `.ssh`, `.gnupg`, …) unless the category's own root is that folder (Downloads);
   - was modified more recently than the category's minimum age.
2. **Risk decides pre-selection.** `Safe` categories (regenerated automatically) are pre-selected; `Review` (build
   output, package caches) and `Personal` (Downloads) are never pre-selected, and `Personal` is never scheduled without
   asking.
3. **Preview before delete.** A scan lists every item with its path, size and reason (at most
   `MAX_ITEMS_PER_CATEGORY` = 1000 per category, `truncated` says more exist; a run deletes only listed items).
4. **A run deletes only what the user saw.** (The policy below is built and tested in the core; the agent's scanner
   and deleter are still a stub, see Consequences.) `policy::chosen` requires the scan to be younger than `SCAN_VALID_MS`
   (one hour), every id to belong to it and at least one to be chosen; the agent **re-checks each item immediately
   before deleting** (it still exists, is still not a link, is still inside its root, has not changed since the scan).
5. **Deletes are recoverable.** Items go to the OS Recycle Bin / Trash or into Dyno's **quarantine** for 7 days
   (`DeleteMode`), restorable from Activity; a quarantine records its items and expiry, and an expired one is deleted
   for good by the agent. A run reports freed bytes honestly (Recycle Bin space is freed when it is emptied; quarantine
   after expiry) and lists items that failed with a reason ("in use by Teams", "access denied: needs the helper").
6. **Files in use are skipped with the name of the program holding them** (`FileLocks`: Restart Manager, `lsof`,
   `/proc/*/fd`), never force-deleted.
7. **System changes are not cleanup.** Anything beyond the user's own files (Windows Update cache, system logs) needs
   the privileged helper and its closed action set (ADR 0006); the policy never reaches it.
8. **Packs can narrow but not widen.** A cleanup pack names roots by template; the core expands them, and every path
   still passes `admit`. Registry cleaners and RAM boosters are not offered at all.

## Consequences

- Built (WP-04): the policy, the catalogue (now the embedded TOML pack `builtin.toml`, ADR 0007), root resolution for Windows,
  macOS and Linux, the scanner, the re-check before each delete (`dyno_agent::cleanup::guard`), the deleter, the quarantine
  store with restore and expiry, file-lock reporting, the bounded disk-space map and the Cleanup page. Items 4 to 6 are
  enforced and each refusal (link, `..`, outside the root, personal folder, stale scan, changed item, locked file) has a test.
- `freed_bytes` is the size of what left the scanned folders; the space is released when the quarantine expires or the
  Recycle Bin is emptied, and the page says so.
- Dyno never empties the user's Trash/Recycle Bin (that would be a permanent delete outside the quarantine), so the Linux
  category list has no Trash entry. Docker images are not removed yet (they are not files; see the roadmap).
- Not built: scheduled cleanups (the setting exists; nothing runs on a schedule), the privileged helper for system folders
  (items the user cannot delete are reported as failed with the OS's reason).
- The core owns the rules and is exhaustively unit-tested; the agent's scanners are thin and tested against temp folders
  they create.
- The scan-age and re-check rules mean a long-open preview must be refreshed before cleaning; the UI says so.
