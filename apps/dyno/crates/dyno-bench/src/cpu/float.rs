//! Float work: a blocked `f64` matrix multiply with the scalar order fixed, so the checksum is exact and the same on every machine.
//! Reported as GFLOPS (2 n^3 operations per multiply).

use std::hint::black_box;
use std::time::{Duration, Instant};

/// The matrix size the timed kernel uses (three 128x128 `f64` matrices: 384 KiB, inside a core's cache on most machines).
pub const N: usize = 128;
const BLOCK: usize = 32;

/// `c += a * b` for `n x n` row-major matrices, blocked, in a fixed order (`i`, `k`, `j` inside each block).
pub fn matmul(n: usize, a: &[f64], b: &[f64], c: &mut [f64]) {
    for ii in (0..n).step_by(BLOCK) {
        for kk in (0..n).step_by(BLOCK) {
            for jj in (0..n).step_by(BLOCK) {
                for i in ii..(ii + BLOCK).min(n) {
                    for k in kk..(kk + BLOCK).min(n) {
                        let aik = a[i * n + k];
                        for j in jj..(jj + BLOCK).min(n) {
                            c[i * n + j] += aik * b[k * n + j];
                        }
                    }
                }
            }
        }
    }
}

/// Deterministic test matrices of small integers (products and sums stay exact in `f64`).
pub fn matrices(n: usize) -> (Vec<f64>, Vec<f64>) {
    let a = (0..n * n).map(|i| ((i * 7 + 3) % 11) as f64 - 5.0).collect();
    let b = (0..n * n).map(|i| ((i * 5 + 1) % 13) as f64 - 6.0).collect();
    (a, b)
}

/// Runs the multiply for about `budget`; returns GFLOPS.
pub fn gflops(budget: Duration) -> f64 {
    let (a, b) = matrices(N);
    let mut c = vec![0.0; N * N];
    let start = Instant::now();
    let mut done = 0_u64;
    while start.elapsed() < budget || done == 0 {
        c.iter_mut().for_each(|x| *x = 0.0);
        matmul(N, black_box(&a), black_box(&b), &mut c);
        black_box(&c);
        done += 1;
    }
    (done as f64) * 2.0 * (N as f64).powi(3) / start.elapsed().as_secs_f64().max(1e-9) / 1e9
}

#[cfg(test)]
mod tests {
    use super::*;

    fn naive(n: usize, a: &[f64], b: &[f64]) -> Vec<f64> {
        let mut c = vec![0.0; n * n];
        for i in 0..n {
            for j in 0..n {
                for k in 0..n {
                    c[i * n + j] += a[i * n + k] * b[k * n + j];
                }
            }
        }
        c
    }

    #[test]
    fn the_blocked_multiply_equals_the_plain_one_exactly_at_odd_sizes() {
        for n in [1, 5, 31, 33, 70] {
            let (a, b) = matrices(n);
            let mut c = vec![0.0; n * n];
            matmul(n, &a, &b, &mut c);
            assert_eq!(c, naive(n, &a, &b), "n = {n}");
        }
    }

    #[test]
    fn the_checksum_is_fixed() {
        let (a, b) = matrices(8);
        let mut c = vec![0.0; 64];
        matmul(8, &a, &b, &mut c);
        let sum: f64 = c.iter().sum();
        assert_eq!(sum, naive(8, &a, &b).iter().sum::<f64>());
        assert_eq!(sum.fract(), 0.0, "integer inputs give an exact sum");
    }

    #[test]
    fn a_short_run_reports_a_positive_rate() {
        assert!(gflops(Duration::from_millis(5)) > 0.0);
    }
}
