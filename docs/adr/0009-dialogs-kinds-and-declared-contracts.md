# 9. Dialogs as windows: kinds and declared contracts

- Status: Accepted (2026-10-24; user: per-dialog command lists, everything in 1.1; incident-summary becomes a modeless viewer; a saved layout that does not fit is fitted with one Undo notification; shared crates `paddock-dialogs` and `paddock-health`)
- Date: 2026-10-24
- Related: rule 17 (dialogs are real OS windows), rule 34 (each window its own bootstrap), rule 39 (shared parts live in
  `paddock-ui`), rule 40 (recurring bugs mean a missing design rule), ADR 0006 (consumption), ADR 0008 (the `paddock-jobs` set-up)
- First adopter: Debrief ADR 0054 (amends Debrief ADR 0050). Read it for the full kind table, checks and migration.

## Context

Debrief made every dialog an OS window (ADR 0050) and then met five bug kinds in 51 dialogs: blank windows from missing
providers, a dialog denied the command it needed, wrong sizes (too tall, too short, a tiny "Explain event"), missing
Close buttons once native close was disabled, and inconsistent modal/modeless. Each was fixed per dialog and recurred.
Rule 40 says: write the design rule. The other apps will hit the same classes as they adopt rule 17.

Survey of the apps today (read-only, 2026-10-24):

| App | Dialogs and windows today | Exposure |
|---|---|---|
| Pitcrew | no dialogs or `role=dialog` found; confirmations are inline in pages (`register-wizard.ts`, settings sections); one tray window (`tray.rs`) | Low now; the register wizard and confirmations become dialogs when rule 17 is applied |
| Pitwall | about 6 in-page dialog components (`create-app-dialog`, `key-secret-dialog`, `quit-prompt`, `service-consent`, shared `confirm`); windows via `shell.rs` and a tray popover | High: every one is rule-17 work; credentials and consent are `decision`/`form` modal |
| Spotter | one `confirm-dialog.ts`; tray window (`tray.rs`) | Low: mostly pages |
| Dyno | in-page dialogs (`rule-dialog`, `fix-dialog`, `process-dialogs`, `web-search-dialog`); popover and extra windows (`window.rs`, `popover.rs`) | Medium: five or so dialogs plus popovers (popovers stay in-app) |
| Sighter, Pacenotes | design stage, no code | Adopt from day one: first dialogs are written with kinds |

None has a dialog registry, a window-by-kind creator or the checks yet, so the first dialog in each will be built by
hand and copy the Debrief bugs unless the parts are shared.

## Decision

1. **Every dialog declares one kind**: `decision` (modal, fits its content, OK/Cancel), `form` (modal or modeless,
   fixed comfortable size, pinned footer), `viewer` (modeless, about 680x560, grows, never shrinks, Close) or `tool`
   (modeless, large, remembers size). The kind decides size, buttons, modality and keyboard (Escape and Cmd/Ctrl+W
   close a modeless dialog, press Cancel on a modal). Native close/minimize/maximize are disabled in every Paddock
   dialog window and the shell guarantees a visible Close/Cancel. A modal is a quick decision, never a long wait:
   long work becomes a background job (ADR 0008) plus a notification (ADR 0010).
2. **One declared contract per dialog**: id, kind, typed input, typed result, required provider groups, and the IPC
   commands it may call. The TypeScript registry is the source; a generated JSON copy is verified by Rust, which
   creates the window and checks every command from a dialog window against that dialog's list (the base commands are
   added by the kind).
3. **Shared parts** (rule 39): in `@decryp7/paddock-ui`: `DialogKind`, `defineDialog`, typed `DialogService`,
   `DialogShell` and footer, in-page backend (browser build, e2e), modal gate, bounds persistence, the
   boot-every-dialog test harness and the command lint. In a shared Rust crate `paddock-dialogs` (same set-up as
   `paddock-jobs`: no Tauri in the core, a Tauri adapter behind a feature): window creation by kind, sizing rules,
   contract schema and the command guard. Each app keeps only its registry entries, its command table and its dialogs.
4. **Three checks are part of the shared harness and required in every app**: boot every registered dialog with its
   declared providers and fixture input (no missing provider, no empty body, no denied command); every invoked command
   is in the dialog's list and vice versa; every dialog has a working visible Close/Cancel and Escape per kind.

## Consequences

- The five Debrief bug kinds become failing tests when a dialog is added in any app.
- Apps with in-page dialogs get a clear migration: register, pick a kind, move to a window; popovers, menus and
  tooltips stay in-app (rule 17).
- Shared packages need a release path (ADR 0006); until the Debrief shapes settle (one release), Debrief keeps the
  code and the others copy the registry shape only, so extraction is a move, not a redesign.

## Migration plan

1. Debrief adopts (ADR 0054 steps 1-3 before 1.1).
2. Extract `dialog-windows` from Debrief into `paddock-ui` and `paddock-dialogs` after Debrief 1.1 (ADR 0054 step 4).
3. Pitwall adopts first (most dialogs), then Dyno, Pitcrew and Spotter as they gain dialogs; Sighter and Pacenotes start with it.
4. The audit page of rule 17 lists, per app, its dialogs with kinds; CI runs the three checks.

## Alternatives considered

- **Each app keeps its own dialog code.** The family's bugs are identical; copies drift (rule 39).
- **Only the rule, no shared code.** Rule 17 already exists; Debrief shows rules alone do not prevent the bugs.
- **Per-dialog Tauri capability files.** They scope plugin permissions, not an app's own `invoke_handler` commands.
