# Changelog

Paddock Shell Demo's own changes (What's New shows the running version's notes). Releases are tagged
`shell-demo-v<version>`; the shared packages and crates have their own changelog (../../CHANGELOG.md).

## [0.1.0] - 2026-10-24

### Added

- The shell demo as an installable app on the `paddock-shell` plugin: the pages frame with sample pages, Settings
  (General and Storage: one field of every kind), About, Updates and What's New.
- Service health for real: the demo engine (its Restart works) and the search index (its Reset fails, and the
  notification keeps the cause), from Notifications > Break the demo engine and Break the search index.
- Sample jobs on the shell's job registry: an import with three phases on their own scales and a step that cannot be
  cancelled, and a quiet job that reports nothing until you cancel it.
- Dialogs of every kind as real windows: a decision, a modal form, a viewer, a tool and a required choice.
- The tray icon with its popover: the app's state, Show, Start sample job and Quit.
