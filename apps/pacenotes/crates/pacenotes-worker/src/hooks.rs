//! Test hooks of the worker (feature `test-hooks`, never in a release build): this process aborts, exits, spins, blocks or
//! overflows its stack on a file name with a magic suffix. Left out of the coverage gate: the code ends the process it runs in.

/// Behaviours for the crash tests, chosen by the file name's suffix. Only in builds with the `test-hooks` feature.
pub fn run(path: &str) {
    #[allow(unconditional_recursion)]
    fn recurse(n: u64) -> u64 {
        let pad = [n as u8; 4096];
        std::hint::black_box(&pad);
        recurse(n + 1) + u64::from(pad[0])
    }
    if path.ends_with(".abort") {
        std::process::abort();
    } else if path.ends_with(".exit") {
        std::process::exit(7);
    } else if path.ends_with(".spin") {
        loop {
            std::hint::black_box(0u64);
        }
    } else if path.ends_with(".block") {
        loop {
            std::thread::park();
        }
    } else if path.ends_with(".overflow") {
        std::hint::black_box(recurse(0));
    }
}
