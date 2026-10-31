# Changelog

What's New in Pitcrew. Notes for the running version are shown in the app (Help > What's New); the full history is generated
from Conventional Commits by git-cliff (`cliff.toml`).

## Unreleased

### Changed

- Pitcrew runs on the Paddock app shell: one frame, notifications and service health with Repair, jobs with progress and
  Cancel for long work, dialogs as their own windows, and Settings, About, Updates and What's New from the shell.
- Cleaning a disk, registering a runner, pruning the container engine and trimming the Actions cache run as jobs that show
  their progress; the window no longer gives up on them after a fixed time. (Cancelling a registration comes with the
  cancellation work before 1.1.0.)
- Updates and release notes come from the Paddock releases (`pitcrew-latest`, the `pitcrew-v` tags). A copy installed before
  this version is reinstalled once by hand.
- Dialog buttons follow the platform: Cancel then the main action on the right on macOS and Linux, the main action first on
  Windows. A confirmation is the shared confirmation window, and a dialog whose action fails stays open with the reason and
  a Try again button.
- Runner downloads and updates use the Paddock downloader: parallel ranges, per-host connection caps, back-off when a server
  answers 429 or 503, resume of a stopped download, the checksum computed while writing and compared with the publisher's
  (two sources must agree), a free-space check, and Cancel in every phase. Each download is a job with phases in the Jobs list.
- The pause windows editor is the shared one: days as a row of toggles, times from a list or typed, and a window that
  starts after it ends is marked as running overnight. A new window starts on Monday to Friday, and a window with no days
  counts as every day, as the engine always did (Save no longer refuses it).
- The sparklines on the dashboard tiles have the shared look: an end dot, a scale from 0 to the shown maximum, a light fill
  under the line, and no clamping of values. Screen readers skip them; the tile's own label has the numbers.
- The Copy menu for selected text is the shared one (a keyboard-navigable menu). Right-clicking only opens it inside the
  selection, and it opens when the mouse button is released, so the click that ends the right-click no longer closes it.

### Fixed

- The window no longer stays blank while Pitcrew loads.
