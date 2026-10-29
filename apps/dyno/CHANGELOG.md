# Changelog

## [1.0.0] - Unreleased

### Added

- Dyno measures your PC and explains why it is slow; it only looks and changes nothing on your computer (ADR 0021).
- Runs on the Paddock app shell: one frame, notifications and service health, jobs with Cancel, settings, About, updates and What's New, and the tray popover.
- Every dialog is on the shared dialog API (ADR 0026): the alert rule, the lookups and the lockers list are dialog windows with their contracts declared in Rust, deleting an alert rule asks with the shared confirmation, and the buttons follow each system's order.
