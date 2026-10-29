## Releasing from a tag

Releases are made by CI from the tag `{{app}}-vX.Y.Z`, never from a workstation (rule 9). A local build for you to try is
fine; it is never published.

1. Make sure `main` is green in CI and the version in `tauri.conf.json`, `Cargo.toml` and `package.json` agree. The release
   notes are generated from the commits since the previous `{{app}}-v*` tag (git-cliff); there is no committed changelog
   file to update.
2. Before tagging (rule 25): `cargo deny check advisories`, one CI build with bundles (the workflow's manual run), the
   icon check, signing secrets stripped of whitespace, and the release notes' "First launch" section (until the apps are
   signed).
3. Tag the commit (`git tag {{app}}-vX.Y.Z`, never a `[skip ci]` commit) and push the tag. The tag workflow runs the gates
   again, builds the installers on all platforms, verifies the macOS bundle (`codesign --verify --deep --strict` on the .app
   and the DMG), publishes the release with its notes and `latest.json`.
4. After the release: install the published build (not the CI artifact) and run the app's installed-app verify script.
