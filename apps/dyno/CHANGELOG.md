# Changelog

## [1.0.0] - Unreleased

### Added

- Dyno measures your PC and explains why it is slow; it only looks and changes nothing on your computer (ADR 0021).
- Runs on the Paddock app shell: one frame, notifications and service health, jobs with Cancel, settings, About, updates and What's New, and the tray popover.
- A command palette: Mod+P goes to any page, Mod+Shift+P runs a command (the pages' own buttons, such as Read the system logs now, Check now, Run again, Save bundle, Add rule; each shows its page first and is off while the page's button is).
- Every dialog is on the shared dialog API (ADR 0026): the alert rule, the lookups and the lockers list are dialog windows with their contracts declared in Rust, deleting an alert rule asks with the shared confirmation, and the buttons follow each system's order.
