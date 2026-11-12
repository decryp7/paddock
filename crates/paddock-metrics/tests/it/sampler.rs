//! The sampler on a scripted probe and the fake clock.

use paddock_jobs::time::{Clock, FakeClock};
use paddock_metrics::{MachineProbe, Mount, Sampler};
use paddock_testkit::Scratch;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Default)]
struct Script {
    cpu: u32,
    used: u64,
    total: u64,
    free: u64,
    mount_calls: usize,
    own: u64,
    asked_pid: Option<u32>,
}

struct Fake {
    script: Arc<Mutex<Script>>,
    mount: PathBuf,
}

impl MachineProbe for Fake {
    fn cpu_permille(&mut self) -> u32 {
        self.script.lock().unwrap().cpu
    }
    fn memory(&mut self) -> (u64, u64) {
        let s = self.script.lock().unwrap();
        (s.used, s.total)
    }
    fn mounts(&mut self) -> Vec<Mount> {
        let mut s = self.script.lock().unwrap();
        s.mount_calls += 1;
        vec![
            Mount {
                path: PathBuf::from("/"),
                free: 1,
                total: 2,
            },
            Mount {
                path: self.mount.clone(),
                free: s.free,
                total: 1000,
            },
        ]
    }
    fn process_memory(&mut self, pid: u32) -> u64 {
        let mut s = self.script.lock().unwrap();
        s.asked_pid = Some(pid);
        s.own
    }
}

fn setup() -> (Scratch, Arc<Mutex<Script>>, FakeClock, Fake) {
    let scratch = Scratch::new();
    let mount = paddock_env::winpath::real(&scratch.work()).unwrap();
    let script = Arc::new(Mutex::new(Script {
        cpu: 250,
        used: 400,
        total: 800,
        free: 300,
        own: 77,
        ..Script::default()
    }));
    let fake = Fake {
        script: script.clone(),
        mount,
    };
    (
        scratch,
        script,
        FakeClock::at_wall_ms(1_700_000_000_000),
        fake,
    )
}

#[test]
fn a_sample_carries_the_machine_the_disk_and_the_process() {
    let (scratch, script, clock, fake) = setup();
    let mut sampler = Sampler::new(fake, Arc::new(clock))
        .watch_disk(scratch.work().join("not-yet"))
        .watch_process(42);
    let s = sampler.sample();
    assert_eq!(s.at, 1_700_000_000_000);
    assert_eq!((s.cpu_permille, s.cpu_peak_permille), (250, 250));
    assert_eq!((s.mem_used_bytes, s.mem_total_bytes), (400, 800));
    assert_eq!((s.disk_free_bytes, s.disk_total_bytes), (300, 1000));
    assert_eq!(s.own_mem_bytes, 77);
    assert_eq!(script.lock().unwrap().asked_pid, Some(42));
}

#[test]
fn readings_are_clamped_to_what_can_be_true() {
    let (_scratch, script, clock, fake) = setup();
    {
        let mut s = script.lock().unwrap();
        s.cpu = 5000;
        s.used = 900;
    }
    let mut sampler = Sampler::new(fake, Arc::new(clock));
    let s = sampler.sample();
    assert_eq!(s.cpu_permille, 1000);
    assert_eq!(s.mem_used_bytes, 800);
    assert_eq!(
        (s.disk_free_bytes, s.disk_total_bytes, s.own_mem_bytes),
        (0, 0, 0),
        "nothing watched reports zero"
    );
}

#[test]
fn a_clock_before_1970_stamps_zero() {
    let (_scratch, _script, _clock, fake) = setup();
    let clock = FakeClock::at_wall_ms(-5);
    let mut sampler = Sampler::new(fake, Arc::new(clock));
    assert_eq!(sampler.sample().at, 0);
}

#[test]
fn the_list_of_volumes_is_reused_until_it_is_stale() {
    let (scratch, script, clock, fake) = setup();
    let mut sampler = Sampler::new(fake, Arc::new(clock.clone()))
        .watch_disk(scratch.work())
        .mounts_every(Duration::from_secs(60));
    sampler.sample();
    sampler.sample();
    assert_eq!(script.lock().unwrap().mount_calls, 1);
    script.lock().unwrap().free = 100;
    clock.advance(Duration::from_secs(59));
    assert_eq!(
        sampler.sample().disk_free_bytes,
        300,
        "still the cached list"
    );
    clock.advance(Duration::from_secs(1));
    assert_eq!(sampler.sample().disk_free_bytes, 100);
    assert_eq!(script.lock().unwrap().mount_calls, 2);
    assert_eq!(clock.monotonic(), Duration::from_secs(60));
}

#[test]
fn a_path_no_volume_holds_has_no_space() {
    let (scratch, _script, clock, fake) = setup();
    let mut sampler = Sampler::new(fake, Arc::new(clock));
    assert!(sampler.disk_space(&scratch.work()).is_some());
    // A relative path that exists nowhere resolves to nothing under any mount.
    assert!(
        sampler
            .disk_space(std::path::Path::new("definitely/not/here/at/all"))
            .is_none()
    );
}
