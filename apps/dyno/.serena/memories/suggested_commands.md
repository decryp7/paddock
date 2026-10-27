# Commands (PowerShell)

```powershell
$env:CARGO_TARGET_DIR="C:\Users\decryp7\dev.decryptology.net\dyno\target"; $env:CARGO_BUILD_JOBS=4
cargo test -p dyno-core
cargo llvm-cov -p dyno-core --summary-only          # use a separate CARGO_TARGET_DIR (target-cov)
cargo clippy -p dyno-platform --all-targets --target x86_64-apple-darwin -- -D warnings
cd app; npm ci; npm start                             # mock backend at http://localhost:4200
npx ng test --watch=false --coverage; npx ng lint; npx ng build
npx tauri build --bundles nsis -- --profile dist      # check free memory first (≥ 6 GB)
```
Check free memory before heavy builds: `(Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory`.
