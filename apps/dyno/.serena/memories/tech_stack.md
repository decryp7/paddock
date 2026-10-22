# Tech stack

- Rust 2021 (rust-version 1.88), workspace; sysinfo 0.39, windows 0.62, starship-battery, wmi, libc, plist, rusqlite 0.40 (bundled), trash 5, postcard, serde/serde_json, thiserror, toml.
- Tauri 2.12 (tray-icon, notification, updater), Angular 22 + @ngrx/signals + CDK/Material, ECharts, Vitest, Playwright.
- CI: Gitea Actions `.gitea/workflows` — Linux (catthehacker/ubuntu:act-latest), `windows-latest` host runner, `macos-latest` host runner (Apple Silicon, the user's real Mac: lean jobs, main + tags only, observe-only tests).
