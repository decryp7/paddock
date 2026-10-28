# Sighter crates

Sighter's own Cargo workspace (`../Cargo.toml`, own `Cargo.lock`). Pure crates first; every process, socket, model and OS
concern sits behind a port of `sighter-core` and is implemented by a later adapter crate.

| Crate | Package | What |
|---|---|---|
| `sighter-core` | SG1 | `Frame`, geometry, `RawScore` / `CalibratedScore` / `Calibrator`, `fuse` and `decide` (the margin rule), the `Outcome` enum, the ports (`FrameSource`, `InputDriver`, `Target`, `Detector`, `LabelSource`, `Vlm`) |
| `sighter-frames` | SG2 | grid and difference hashes, SSIM, scene change, near-duplicate removal, bounded PNG decode/encode |
| `sighter-match` | SG3 | NCC template matching on a region (1:1 scale), `locate` = match + calibrate + margin rule |

The design behind the types is `../docs/review/2026-10-design-review.md` (P1s in 1.1, 1.3, 1.5); the proof of concept these
were ported from is the `poc/app` and `poc/vision` branches of the old `sighter` repository.
