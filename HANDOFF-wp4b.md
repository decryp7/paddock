# WP-4b hand-off (round-2 review), 2026-11-08

Not committed on purpose (a hand-off note, not repo content): delete it, or move it, before committing.

## Where things are
- Worktree `/Volumes/d7sst7/dev-targets/wt/pitwall-wp4b`, branch `pitwall/1.1-wp4b` (from `origin/pitwall/1.1-wp3b`), clean tree.
  Commits: `35fc7a32f` (phase events fix), `2beae9131` (shared composer), `cd070ff14` (Pitwall attachments), `0ef631f7f` (round 1
  shared: whole chat, files counted, meter in words), `906a32c47` (round 1 Pitwall). Nothing pushed.
- ADR note branch `docs/pitwall-1-1-plan-refresh-wp4b-note` (`e537e4f88`), its own sparse worktree
  `/Volumes/d7sst7/dev-targets/wt/pitwall-wp4b-note` (only `apps/pitwall/docs/`). The ADR text is in the "Revision 3" list of
  `apps/pitwall/docs/adr/0023-playground-chat-vision-speech-mcp.md`; add the two new lines there (window source; tool results not
  re-checked mid-turn).
- Build: `CARGO_TARGET_DIR=/Volumes/d7sst7/dev-targets/target/pitwall-wp4b CARGO_BUILD_JOBS=2`, every heavy step through
  `/Volumes/d8sst7/dev.decryptology.net/engineering-rules/scripts/heavy.sh`. A gate script used for rounds 1 and 2 is at
  `/private/tmp/claude-501/-Volumes-d8sst7-dev-decryptology-net-paddock/ce0d1520-d3c0-4819-824c-e85acd63af17/scratchpad/rust-gates.sh`
  (fmt, clippy -D warnings on paddock-assistant + paddock-workbench and pitwall-app with and without `chat`, pitwall-app tests,
  workbench-demo tests). Its `cargo fmt --all --check` in `apps/pitwall` also lists `crates/paddock-assistant` files under Pitwall's
  140-width config: a false positive. Check per workspace instead: `cargo fmt -p paddock-assistant --check` in `crates/`,
  `cargo fmt -p pitwall-app --check` in `apps/pitwall` (ignore `/crates/` lines); format your own files with `rustfmt` from each
  workspace's folder (Pitwall: `--edition 2021 --config-path rustfmt.toml`; crates: `--edition 2024`).
- UI: `npm run build -w ui` after any `ui/` change (the e2e build guard refuses a stale `ui/dist`). Gates: `cd ui && npx ng test
  paddock-ui --watch=false`, `npx ng test workbench-demo --watch=false`, **`npm run test:tokens`** (has a 14 px floor for pd-glyph
  sizes), eslint on touched folders; `cd apps/pitwall/app && npx ng test --watch=false`; e2e Chromium:
  `npx playwright test e2e/a11y.e2e.ts e2e/layout.e2e.ts e2e/keyboard.e2e.ts e2e/chats.e2e.ts e2e/connected-apps.e2e.ts
  e2e/chat-composer.e2e.ts e2e/playground.e2e.ts --project=chromium` (137 passed last time). Conformance:
  `node conformance/bin/paddock-conformance.mjs --app paddock --root . --only C06,C14,C15,C16,C24,C29,C30,C31,C34,C35,C36,C37,C39,C43,C48,C53,C54,C56,C57,C59 --strict C36`.
- Regenerate: `PADDOCK_WRITE_TS=1 cargo test -p paddock-assistant --all-features` (TS types), `PADDOCK_WRITE_TS=1 cargo test -p
  paddock-workbench --all-features --lib generated` (after a command-table change), and in `apps/pitwall/app/src-tauri`
  `PADDOCK_WRITE_TS=1 cargo test --features chat --test it shell` (app-commands.ts).
- Shot sheet: `cd apps/pitwall/app && rm -rf .angular/wp4b-shots && WP4B_SHOTS=1 npx playwright test -c
  e2e/screenshots/playwright.config.ts e2e/screenshots/wp4b.shot.ts` → `.angular/wp4b-shots/index.html` (112 pictures last time).
  Mock windows in it: `?mock-context=88` gives high, `=70` full, `=unknown` unknown; these depend on the mock meter's numbers, so
  re-tune them after P2-F. Hooks: `__pitwallMock.holdAttachments()/releaseAttachments()`, `__pitwallSpeech.tick/hold/release/fail`.

## Round-2 findings: what to change, with pointers

### P1-A: the window is the per-slot context, and the server never trims
- Today `chat_host/commands.rs:225` (`models_for`) does `provider.set_context_length(m.context_max …)`: the trained max. The server
  runs `--ctx-size = per-slot × slots` (`pitwall-core/src/llama.rs:30`, `:61`; `slots.rs:70 launch_context`), so a slot holds the
  per-slot context: `slots.rs:106 context_per_slot(requested, preferred, default, trained_max)`.
- Use `min(context_max, per-slot context)`: the per-slot value from Settings (the same inputs `context_per_slot` takes; look at
  how the server's load path calls it), or `/props` `n_ctx` (parsed at `llama.rs:346`, `Props.n_ctx` at `:331`) when the model is
  loaded. Put the choice in a pure function (pitwall-core or chat_host/models.rs) with the test: context_max 131072, per-slot 8192 →
  `limit_tokens == 8192 - reserve` (reserve is `Budgets::max_answer_tokens`, 1024 by default).
- `--no-context-shift`: add it to llama-server's arguments in `llama.rs` (near `:61`), with an argument test like the one at
  `:415`. First check that the pinned llama.cpp (`b11669`; the pin is in `pitwall-core` runtimes catalog) has the flag in its server
  README / `--help`; if not, report it and leave a TODO.

### P2
- **B (shared engine, `crates/paddock-assistant/src/service.rs` `Assistant::context`):** use the measured count (`self.measured`) only
  when `cfg.history_chars == usize::MAX`; otherwise estimate over the trimmed history. Add a test in `tests/it/context.rs` with a
  finite `history_chars` (via `rig_cfg`).
- **C:** when pending files make a new chat full (no `activeId`, or pending files alone over the room), the banner says "These files
  are too long for the model. Remove a file." with no Start-a-new-chat button. Pane: `ui/projects/paddock-ui/workbench/assistant/src/assistant.ts`
  (the `banner full` block; `t.contextFull`, `t.startNewChat`); the store has `attached()` and `activeId()`. The engine's own
  words for a new chat are in `check_fits` in service.rs.
- **D:** the early size refusal in `assistant.store.ts` (`attach()`, the `too-big` message) should use Rust's words: append "Attach a
  smaller file, or paste the part that matters." (Rust's text is in `apps/pitwall/app/src-tauri/src/chat_host/attach.rs` `prepare`).
- **A and 6b:** under about 760 px height, cap the files tray (`.files-tray`, now `max-height: 92px`) at two rows with an expanding
  "N files" summary chip, give `.log` a `min-height` of about 120 px, and let the empty state shrink (or hide its duplicate
  Connected apps button) at small heights. Use a container or media query on the pane's height.
- **E:** in `e2e/screenshots/wp4b.shot.ts`, wait for the UI to settle before `8-dropping` (900, dark): the drop class is set, then
  take the shot after `expect(...).toHaveClass(/dropping/)` and one more settled frame.
- **F:** `MemoryAssistantBackend.context` in `workbench/assistant/src/backend.ts`: `reserveTokens: 1024` (limit = window − 1024)
  and about bytes/4 per file (it only has attachments, not bytes: give the memory engine a size per file, for example from the
  name's length or a map filled by `MemoryAssistantAttachments`; Pitwall's browser build uses Pitwall's mock `chatAttach`, which
  knows `bytes.length`).
- **G:** `chat_host/attach.rs` `with_files`: a kept file over the read bound is described as "too large to send" rather than "no
  longer available". The provider's reader (`chat_host/commands.rs` compose: `attachment_up_to(sha, MAX_TEXT_BYTES)`) returns `None`
  for both cases: return a typed result (gone / too large / bytes) so `with_files` can tell them apart; test both.
- **Dark-theme disabled primary:** `ui/projects/paddock-ui/scss/_controls.scss` `.pd-btn--primary:disabled` (now
  `border-strong` outline, `text-subtle`): dim the outline or the icon a bit more in dark; keep `npm run test:tokens` green.
- **ADR note:** on the note branch, add "tool results that grow mid-turn are not re-checked before later model calls" (the check is
  only in `Assistant::send`; the loop is `run.rs run_turn`).

## Rules
Tests first; no push; no trailers of any kind; no force; no stash; no `git add -A`; subjects ≤ 100 characters; screenshots go to the
coordinator for independent review.
