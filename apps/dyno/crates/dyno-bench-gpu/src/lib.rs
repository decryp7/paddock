#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
//! The GPU runner's measuring (ADR 0030 section 1): `wgpu` with Metal on macOS, DX12 then Vulkan on Windows and Vulkan on Linux,
//! no GL. Compute: a long dependent chain of `vec4<f32>` fused multiply-adds (GFLOPS) and a storage-buffer copy (GB/s). Graphics,
//! off-screen at 3840x2160: full-screen triangles with blending (Gpixel/s) and instanced small triangles (Mtri/s). Nothing is shown
//! on screen. Every submission is sized to take about 100 ms and halved if one runs past 250 ms (`dyno_core::bench::gpu`).
//! Timing is wall clock around the submit and the wait for the queue, summed over submissions until a second of work is counted.

mod device;
mod measure;

pub use device::{list_adapters, run, Outcome, Settings};
pub use measure::measure;
