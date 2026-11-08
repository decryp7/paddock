//! Branchy work: a hand-written quicksort of a 4 MiB `u32` array (insertion sort for small parts), so branch prediction counts.
//! Reported as million elements sorted per second.

use std::hint::black_box;
use std::time::{Duration, Instant};

/// Elements sorted per pass: 1,048,576 `u32` = 4 MiB.
pub const ELEMENTS: usize = 1 << 20;

/// Sorts `v` ascending.
pub fn sort(v: &mut [u32]) {
    if v.len() <= 16 {
        for i in 1..v.len() {
            let mut j = i;
            while j > 0 && v[j - 1] > v[j] {
                v.swap(j - 1, j);
                j -= 1;
            }
        }
        return;
    }
    let mid = v.len() / 2;
    let last = v.len() - 1;
    // Median of three as the pivot.
    if v[0] > v[mid] {
        v.swap(0, mid);
    }
    if v[mid] > v[last] {
        v.swap(mid, last);
    }
    if v[0] > v[mid] {
        v.swap(0, mid);
    }
    let pivot = v[mid];
    let (mut i, mut j) = (0_usize, last);
    loop {
        while v[i] < pivot {
            i += 1;
        }
        while v[j] > pivot {
            j -= 1;
        }
        if i >= j {
            break;
        }
        v.swap(i, j);
        i += 1;
        j -= 1;
    }
    let (left, right) = v.split_at_mut(j + 1);
    sort(left);
    sort(right);
}

/// Fills `v` with pseudo-random numbers from `seed` (fixed algorithm, so every machine sorts the same data).
pub fn fill(v: &mut [u32], mut seed: u64) {
    for x in v {
        seed = crate::cpu::mix(seed);
        *x = (seed >> 32) as u32;
    }
}

/// Sorts fresh data repeatedly for about `budget`; returns million elements per second.
pub fn mega_elements_per_second(budget: Duration, elements: usize) -> f64 {
    let mut v = vec![0_u32; elements];
    let start = Instant::now();
    let (mut done, mut seed) = (0_u64, 1_u64);
    while start.elapsed() < budget || done == 0 {
        fill(&mut v, seed);
        sort(black_box(&mut v));
        black_box(&v);
        seed += 1;
        done += 1;
    }
    (done * elements as u64) as f64 / start.elapsed().as_secs_f64().max(1e-9) / 1e6
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_sorts_and_keeps_every_element() {
        for n in [0, 1, 2, 15, 16, 17, 100, 5000] {
            let mut v = vec![0_u32; n];
            fill(&mut v, 42);
            let mut want = v.clone();
            want.sort_unstable();
            sort(&mut v);
            assert_eq!(v, want, "n = {n}");
        }
    }

    #[test]
    fn it_handles_runs_of_equal_and_already_sorted_values() {
        let mut v = vec![7_u32; 1000];
        sort(&mut v);
        assert!(v.iter().all(|x| *x == 7));
        let mut asc: Vec<u32> = (0..1000).collect();
        sort(&mut asc);
        assert!(asc.windows(2).all(|w| w[0] <= w[1]));
        let mut desc: Vec<u32> = (0..1000).rev().collect();
        sort(&mut desc);
        assert_eq!(desc, (0..1000).collect::<Vec<u32>>());
    }

    #[test]
    fn the_same_seed_gives_the_same_data() {
        let (mut a, mut b) = (vec![0; 64], vec![0; 64]);
        fill(&mut a, 9);
        fill(&mut b, 9);
        assert_eq!(a, b);
    }

    #[test]
    fn a_short_run_reports_a_positive_rate() {
        assert!(mega_elements_per_second(Duration::from_millis(5), 4096) > 0.0);
    }
}
