use paddock_power::{FakeEvents, PowerError, PowerEvent, SleepWakeSource};
use std::sync::{Arc, Mutex};

#[test]
fn events_reach_the_subscriber_until_it_is_dropped() {
    let source = FakeEvents::new();
    assert!(!source.emit(PowerEvent::Woke), "nobody subscribed");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let s = seen.clone();
    let sub = source
        .subscribe(Arc::new(move |e| s.lock().unwrap().push(e)))
        .unwrap();
    assert!(source.emit(PowerEvent::Sleeping));
    assert!(source.emit(PowerEvent::Woke));
    drop(sub);
    assert!(!source.emit(PowerEvent::Sleeping));
    assert_eq!(
        *seen.lock().unwrap(),
        [PowerEvent::Sleeping, PowerEvent::Woke]
    );
}

#[test]
fn a_source_can_refuse() {
    let source = FakeEvents::new();
    source.refuse(true);
    let err = source.subscribe(Arc::new(|_| {})).err().unwrap();
    assert!(matches!(err, PowerError::Unavailable { .. }));
    source.refuse(false);
    assert!(source.subscribe(Arc::new(|_| {})).is_ok());
}
