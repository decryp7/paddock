# Plan: proof of concept, driving a VMware VM from its console

Status: in progress (2026-10-24), branch `poc/vm-console`. A throwaway-quality but clean proof of concept the user tries on
a Windows PC with VMware Workstation. Design background: [proposal.md](../proposal.md) §7.6, §7.15 and §8.

## Goal

From the Windows host, Sighter drives a Windows VM through the VM's own console (VNC) and runs one end-to-end test:
sign in at the lock screen, wait for the desktop, start Notepad, type a unique line, save, close, reopen the file and
check the line is there. No training: text recognition, templates and the keyboard only. A hotkey stops it at any time.

## Scope

In: VNC target (live view, keyboard and mouse), Windows OCR (`Windows.Media.Ocr`), template match from a saved crop, a
small script format and runner, the stop hotkey (Ctrl+Alt+Shift+F12), failure screenshots, a Windows zip from CI.
Out: snapshots, RDP, the detector and training, Pitwall, the studio and the real app UI.

## Shape

```
sighter-poc (Rust, poc/ folder)
  target/   Target trait: frames(), input(); VncTarget (RFB 3.8: VNC auth, Raw/CopyRect/ZRLE or Tight, pointer, keys)
  resolve/  ocr (Windows OCR; stub elsewhere), template (normalised cross-correlation), screen waits with timeouts
  script/   TOML steps: wait_text, wait_any, click_text, click_image, type, type_env, press (incl. Win, Ctrl+Alt+Del),
            if_text, assert_text, screenshot
  stop/     RegisterHotKey on the host; stop releases held keys and buttons, ends the run, keeps the session open
  cli       run | view (live view window) | locate --text
```

## Milestones

| # | Milestone | Proven by |
| --- | --- | --- |
| 1 | VNC client: connect, auth, frames, keys and pointer | tests against an in-process fake VNC server |
| 2 | Resolver: OCR on Windows, template match everywhere | unit tests with fixed images |
| 3 | Script runner with waits, branches, asserts, failure screenshots | script tests against the fake server |
| 4 | Stop hotkey | test: stop during a long wait ends at once and releases held input |
| 5 | Live view and `locate` | manual on Windows |
| 6 | Windows zip from CI (exe, README, example script, sample crops) | a `workflow_dispatch` artifact; not a release |
| 7 | The user runs the example on their PC | user feedback, log and failure screenshots |

## What the user prepares on the Windows PC

- A Windows VM in VMware Workstation with a local test account that signs in with a password (a PIN also works but is
  harder to automate), a fixed display resolution and 100% scaling.
- VNC enabled for that VM in VMware Workstation (VM settings: VNC connections; port and password), exact steps in the
  zip's README for the installed Workstation version.
- The password in an environment variable for the run; it is never written to the script or the log.

## Risks

- VMware's VNC encodings and key handling (Ctrl+Alt+Del, Win key) may differ by version: test early with milestone 1.
- OCR accuracy at small font sizes: keep 100% scaling; fall back to a template for short words.
- The unsigned exe triggers SmartScreen: "More info, Run anyway" (documented).
- If VNC is unavailable in the user's Workstation version, the fallback is RDP into the guest (sign-in then via NLA).
