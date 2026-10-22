# Tech stack

- Rust stable (1.99 here), workspace: pitcrew-core, pitcrew-service, app/src-tauri (Tauri 2.12).
- Service: tokio (IPC only), sysinfo (processes, metrics, disks), ureq 3 + rustls with the platform verifier,
  windows-service + windows 0.62 (Windows), nix (Unix signals), chrono (local time).
- App: tauri plugins notification, opener, autostart, updater (minisign); keyring 4 (Credential Manager / Keychain);
  zip (diagnostics); objc2-service-management (SMAppService on macOS); winreg (taskbar theme).
- UI: Angular 22 (standalone, signals), @ngrx/signals, Angular CDK + Material (selectively), Apache ECharts,
  Vitest (`ng test`), Playwright (e2e and screenshots against the mock backend).
- CI: Gitea Actions `.gitea/workflows/build.yml` (Linux container jobs, the Windows host runner, the macOS host
  runner `macos-latest`).
