//! Vector work: an `f32` blocked multiply and a dot product, compiled for the widest vector unit present and chosen at run time
//! (the `multiversion` crate: AVX-512, AVX2 with FMA, NEON on aarch64; a portable version otherwise). The result names the width
//! used. Unsafe code is only what the crate generates for the dispatch.
#![allow(unsafe_code)]
// The macro-generated clones need the slice form; `as_chunks` would not change what is compiled.
#![allow(clippy::chunks_exact_to_as_chunks)]

use multiversion::multiversion;
use std::hint::black_box;
use std::time::{Duration, Instant};

/// Matrix size of the timed multiply.
pub const N: usize = 128;

/// The dot product of `a` and `b` over the shorter length, with 16 independent accumulators so the compiler can vectorise it.
#[multiversion(targets("x86_64+avx512f", "x86_64+avx2+fma", "aarch64+neon"))]
pub fn dot(a: &[f32], b: &[f32]) -> f32 {
    let mut acc = [0.0_f32; 16];
    let (ca, cb) = (a.chunks_exact(16), b.chunks_exact(16));
    let (ra, rb) = (ca.remainder(), cb.remainder());
    for (x, y) in ca.zip(cb) {
        for l in 0..16 {
            acc[l] += x[l] * y[l];
        }
    }
    let tail: f32 = ra.iter().zip(rb).map(|(x, y)| x * y).sum();
    acc.iter().sum::<f32>() + tail
}

/// `c += a * b` for `n x n` row-major matrices (`i`, `k`, `j` order: the inner loop streams `b` and `c`).
#[multiversion(targets("x86_64+avx512f", "x86_64+avx2+fma", "aarch64+neon"))]
pub fn matmul(n: usize, a: &[f32], b: &[f32], c: &mut [f32]) {
    for i in 0..n {
        let row = &mut c[i * n..(i + 1) * n];
        for k in 0..n {
            let aik = a[i * n + k];
            let brow = &b[k * n..(k + 1) * n];
            for j in 0..n {
                row[j] += aik * brow[j];
            }
        }
    }
}

/// The widest vector unit the dispatch can pick on this machine, in words for the result.
pub fn width() -> &'static str {
    #[cfg(target_arch = "x86_64")]
    {
        if std::is_x86_feature_detected!("avx512f") {
            return "AVX-512";
        }
        if std::is_x86_feature_detected!("avx2") && std::is_x86_feature_detected!("fma") {
            return "AVX2 with FMA";
        }
        "SSE2 (portable)"
    }
    #[cfg(target_arch = "aarch64")]
    {
        "NEON"
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        "portable"
    }
}

/// Runs the multiply and the dot product for about `budget`; returns GFLOPS (2 n^3 for the multiply, 2 n for the dot).
pub fn gflops(budget: Duration) -> f64 {
    let a: Vec<f32> = (0..N * N).map(|i| ((i * 7 + 3) % 11) as f32 - 5.0).collect();
    let b: Vec<f32> = (0..N * N).map(|i| ((i * 5 + 1) % 13) as f32 - 6.0).collect();
    let mut c = vec![0.0_f32; N * N];
    let start = Instant::now();
    let (mut flops, mut sink) = (0.0_f64, 0.0_f32);
    while start.elapsed() < budget || flops == 0.0 {
        c.iter_mut().for_each(|x| *x = 0.0);
        matmul(N, black_box(&a), black_box(&b), &mut c);
        sink += dot(black_box(&a), black_box(&b));
        black_box(&c);
        flops += 2.0 * (N as f64).powi(3) + 2.0 * (N * N) as f64;
    }
    black_box(sink);
    flops / start.elapsed().as_secs_f64().max(1e-9) / 1e9
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dot_product_is_exact_on_small_integers_at_every_length() {
        for n in [0, 1, 15, 16, 17, 100] {
            let a: Vec<f32> = (0..n).map(|i| (i % 5) as f32).collect();
            let b: Vec<f32> = (0..n).map(|i| (i % 3) as f32 + 1.0).collect();
            let want: f32 = a.iter().zip(&b).map(|(x, y)| x * y).sum();
            assert_eq!(dot(&a, &b), want, "n = {n}");
        }
    }

    #[test]
    fn the_multiply_equals_the_plain_one_exactly_on_small_integers() {
        let n = 20;
        let a: Vec<f32> = (0..n * n).map(|i| ((i * 7 + 3) % 11) as f32 - 5.0).collect();
        let b: Vec<f32> = (0..n * n).map(|i| ((i * 5 + 1) % 13) as f32 - 6.0).collect();
        let mut c = vec![0.0_f32; n * n];
        matmul(n, &a, &b, &mut c);
        for i in 0..n {
            for j in 0..n {
                let want: f32 = (0..n).map(|k| a[i * n + k] * b[k * n + j]).sum();
                assert_eq!(c[i * n + j], want);
            }
        }
    }

    #[test]
    fn the_width_is_named_and_a_short_run_is_positive() {
        assert!(!width().is_empty());
        assert!(gflops(Duration::from_millis(5)) > 0.0);
    }
}
