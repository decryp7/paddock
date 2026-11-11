//! Speech capture through its port (ADR 0023 Decision 5, WP-7 slice 1): the recorder turns whatever the capture delivers into a
//! 16 kHz mono clip, stops at its cap, refuses a second start, and a cancel throws the clip away. The capture is the test double
//! of `test-support`; no test opens a microphone (ER-14, rule 7).

use pitwall_app_lib::speech::fake::FakeCapture;
use pitwall_app_lib::speech::{CaptureError, RecordError, Recorder, Status};
use std::sync::Arc;

fn second_of_48k_stereo() -> Vec<Vec<f32>> {
    // 50 chunks of 20 ms, as a device delivers them (a script fits the 64-chunk hand-over).
    (0..50).map(|_| vec![0.25_f32; 960 * 2]).collect()
}

#[test]
fn records_downmixes_and_resamples_what_the_device_delivers() {
    let fake = FakeCapture::new(48_000, 2, second_of_48k_stereo());
    let rec = Recorder::new(Arc::new(fake.clone()));
    assert_eq!(rec.status(), Status::Idle);
    rec.start().unwrap();
    assert!(fake.is_open(), "the device is open while recording");
    assert!(matches!(rec.status(), Status::Recording { .. }));
    assert_eq!(rec.start().unwrap_err(), RecordError::AlreadyRecording);
    let clip = rec.finish().unwrap();
    assert!(!fake.is_open(), "finishing closes the device");
    assert_eq!(clip.duration_ms(), 1000);
    assert!(clip.samples().iter().all(|&s| s == 8_192));
    assert_eq!(rec.status(), Status::Idle);
}

#[test]
fn cancel_closes_the_device_and_keeps_nothing() {
    let fake = FakeCapture::new(16_000, 1, vec![vec![0.5; 1600]]);
    let rec = Recorder::new(Arc::new(fake.clone()));
    rec.start().unwrap();
    rec.cancel();
    assert!(!fake.is_open());
    assert_eq!(rec.status(), Status::Idle);
    assert_eq!(rec.finish().unwrap_err(), RecordError::NotRecording);
}

#[test]
fn stops_the_device_by_itself_at_the_cap() {
    let fake = FakeCapture::new(16_000, 1, vec![vec![0.5; 1600]; 10]);
    let rec = Recorder::with_cap(Arc::new(fake.clone()), 8_000);
    rec.start().unwrap();
    fake.wait_closed();
    assert_eq!(rec.status(), Status::Full { recorded_ms: 500 });
    assert_eq!(rec.finish().unwrap().samples().len(), 8_000);
}

#[test]
fn a_device_that_cannot_open_is_reported_and_leaves_the_recorder_idle() {
    let fake = FakeCapture::failing(CaptureError::NoDevice);
    let rec = Recorder::new(Arc::new(fake));
    let e = rec.start().unwrap_err();
    assert_eq!(e, RecordError::Capture(CaptureError::NoDevice));
    assert!(e.to_string().contains("microphone"), "{e}");
    assert_eq!(rec.status(), Status::Idle);
}

#[test]
fn a_device_failure_while_recording_ends_the_recording_with_its_cause() {
    let fake = FakeCapture::new(16_000, 1, vec![vec![0.5; 160]]).then_fail(CaptureError::Device("unplugged".into()));
    let rec = Recorder::new(Arc::new(fake.clone()));
    rec.start().unwrap();
    fake.wait_closed();
    assert_eq!(rec.status(), Status::Failed(CaptureError::Device("unplugged".into())));
    let e = rec.finish().unwrap_err();
    assert!(e.to_string().contains("unplugged"), "{e}");
    assert_eq!(rec.status(), Status::Idle);
}
