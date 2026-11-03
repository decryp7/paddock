# Dyno Windows screen checks: tray icons and tray pop-up at 100, 125 and 150 % (build 49f87c671)

Run 2026-11-03 in the Windows test VM (ADR 0040), on the interactive session of `paddock-test`. Build under test: the test
installer from source `49f87c671` (`perf/dyno-cpu-2` + `design/icons-small`), Dyno 1.0.0 version string, x64 installer on
Windows 11 Arm64 (emulated). OS: Windows 11 26H2, build 10.0.26300.9711 (a pre-release-looking build number; ER-26 says test
machines stay on the stable channel, worth checking on the VM image). Display 1024x768 (the image is not yet the 1920x1080 the
ADR describes). Snapshot `gui-session-tools`, reverted afterwards.

## Result

| Check | Scale | Icon size | Result |
|---|---|---|---|
| (a) alert notch visible and whole-pixel below 24 px | 100 % | 16 px | PASS |
| (a) alert notch visible and whole-pixel below 24 px | 125 % | 20 px | PASS |
| (b) icons render correctly | 150 % | 24 px | PASS (at 24 px the alert is the full red tile with white digits, not the notch) |
| (b) tray pop-up renders correctly, stays open | 100 / 125 / 150 % | n/a | PASS |

No defect found.

## Evidence (`screenshots/windows-tray/`)

All pictures are exact captures of the session screen (`CopyFromScreen`, DPI aware); the `-icons-` files are crops zoomed
nearest-neighbour (x8; x10 at 150 %).

- 100 %: `100-desktop-tray.png`, `100-icons-x8.png`, `100-popup.png`. The two alerting icons (memory 33, CPU 17) are 16x16.
  Pixel map of the notch (R = strong red, r = light red track, # = dark glyph), rows from the icon's top-left corner:
  5, 3, 3, 2, 1 red pixels wide (a stepped triangle, 15 red pixels). No partly blended edge pixel exists in or around it.
  The unfilled part of the level bar is the only light-red area.
- 125 %: `125-desktop-tray.png`, `125-icons-x8.png`, `125-popup.png`. Icons are 20x20; notch rows 5, 4, 3, 2, 1
  (15 red pixels, a clean triangle), whole pixels, no blended edge pixels.
- 150 %: `150-desktop-tray.png`, `150-icons-x10.png`, `150-popup.png`. Icons are 24x24 with a MEM / CPU label; the alert is a red
  tile behind white digits, crisp, whole pixels. No notch at this size (by design, the check is for sizes below 24 px).
- Pop-up at every scale: opened by a real left click (`mouse_event`) on the CPU icon, still open 6 s later (100 %: 3 s and 6 s),
  inside the work area, text and controls not clipped, scaled with the display (Active alarms, Disk space, CPU and memory,
  Open Dyno). The Dyno 1.0.0 closes-at-once pop-up did not reproduce in this build.
- Note: the 16 px notch is slightly less even (rows 5, 3, 3, 2, 1) than the 20 px one (5, 4, 3, 2, 1). Still whole pixels and
  visible; reported for the designer, not a failure.

## How it was done (manual steps a harness should automate)

1. Revert to `gui-session-tools`, start headless, wait for SSH and the console session (id 1) of `paddock-test`.
2. Copy the installer by `scp`, check its SHA-256 (13bc6402...86fe8f), install with `/S` (per user).
3. Alert without burning CPU: write `settings.json` in `%LOCALAPPDATA%\Dyno` with two custom alarm rules (CPU above 1 %,
   memory above 1 %, hold 0 s) while the app is stopped, then start `dyno-app.exe` in the session. The notification-area
   registry entries (`HKCU\Control Panel\NotifyIconSettings\*`, `IsPromoted=1`) are set so the icons sit in the taskbar corner
   instead of the overflow flyout.
4. Run every step inside the session: a one-shot scheduled task (`/it`, run through a hidden `wscript` so no console shows)
   runs a PowerShell script that takes the screenshot, finds the notch (strong red pixels in the tray strip), clicks with
   `mouse_event`, and crops and zooms.
5. Scale: set `HKCU\Control Panel\Desktop\LogPixels` (120, 144), `Win8DpiScaling=1` and `WindowMetrics\AppliedDPI`, then
   reboot as `paddock-admin` (auto-logon restores the session); `GetDpiForSystem` confirmed 120 and 144. No sign-out and
   back in was possible without a reboot, since the auto-logon applies at boot only.
6. Revert to `gui-session-tools` at the end.

What the harness (ADR 0040 section 3 and 4) should automate from this: the `Get-TrayButtons` and `Shell_NotifyIconGetRect`
read of each icon's rectangle (instead of hand-found pixel columns), the icon-size check against the taskbar DPI
(16 / 20 / 24 / 32), a pixel judge for the notch (count of strong red pixels per row, no blended pixels) and for the 24 px tile,
the scale change plus reboot as the "two scales" pass, the alarm-rule seeding in a settings file for an alert state, and the
pop-up stays-open check over an observation window. Also: the VM image runs at 1024x768 while the ADR says 1920x1080, and its
shutdown needs the admin account.
