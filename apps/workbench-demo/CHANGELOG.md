# Changelog

Paddock Workbench Demo's own changes. Releases are tagged `workbench-demo-v<version>`; the shared packages and crates have
their own changelog (../../CHANGELOG.md).

## [0.1.0] - 2026-10-25

### Added

- The workbench demo as an installable app on the `paddock-shell` and `paddock-workbench` plugins: the dock with
  perspectives and layouts kept in the config folder, the Explorer, the command palette and the Jobs view.
- Panels that pop out into windows of their own (with the drag ghost while a tab is dragged out), docking back, and the
  state shared between a window and its pop-outs.
- The assistant pane on Pitwall with one read-only sample tool; without Pitwall it shows the calm "Pitwall is not running"
  state with its fix.
- A sample re-index job on the shell's job registry, with phases and Cancel.
