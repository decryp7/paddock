## Releasing from a tag

Releases are made by CI from a `v*` tag, never from a workstation (rule 9). A local build for you to try is fine; it is
never published.

1. Make sure `main` is green in CI, the version in `tauri.conf.json`, `Cargo.toml` and `package.json` agree, and
   `CHANGELOG.md` has the version's section.
2. Before tagging (rule 25): `cargo deny check advisories`, one CI build with bundles (the workflow's manual run), the
   icon check, signing secrets stripped of whitespace, the release notes' "First launch" section (until the apps are
   signed), and, for an app that starts a browser or opens real windows in tests, its real-browser and real-app checks in
   the local test VMs (docs/dev-setup.md, "Test VMs and real browsers"), recorded in the release notes as
   "VM smoke: <date> <version> pass".
3. Tag the commit (`git tag vX.Y.Z`, never a `[skip ci]` commit) and push the tag. The tag workflow runs the gates again,
   builds the installers on all platforms, verifies the macOS bundle (`codesign --verify --deep --strict` on the .app
   and the DMG), publishes the release with its notes and `latest.json`.
4. After the release: install the published build (not the CI artifact) and run the app's installed-app verify script.
