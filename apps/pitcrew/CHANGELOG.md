# Changelog

What's New in Pitcrew. Notes for the running version are shown in the app (Help > What's New); the full history is generated
from Conventional Commits by git-cliff (`cliff.toml`).

## Unreleased

### Changed

- Pitcrew runs on the Paddock app shell: one frame, notifications and service health with Repair, jobs with progress and
  Cancel for long work, dialogs as their own windows, and Settings, About, Updates and What's New from the shell.
- Cleaning a disk, registering a runner, pruning the container engine and trimming the Actions cache run as jobs that show
  their progress and can be cancelled; the window no longer gives up on them after a fixed time.

### Fixed

- The window no longer stays blank while Pitcrew loads.
