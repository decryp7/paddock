use paddock_power::{FakePower, HoldRequest, KeepAwake, PowerError, Scope};
use std::sync::Arc;

fn req(why: &str) -> HoldRequest {
    HoldRequest::new(Scope::System, "Dyno", why)
}

fn guard() -> (Arc<FakePower>, KeepAwake) {
    let os = Arc::new(FakePower::new());
    (os.clone(), KeepAwake::new(os))
}

#[test]
fn holds_until_released() {
    let (os, g) = guard();
    assert!(g.current().is_none());
    let info = g.hold(&req("for 1 h")).unwrap();
    assert_eq!(info.os_ref, "fake#1");
    assert_eq!(g.current(), Some(info));
    assert_eq!(os.live(), 1);
    assert!(g.release());
    assert!(g.current().is_none());
    assert_eq!(os.live(), 0);
    assert!(!g.release(), "nothing left to release");
}

#[test]
fn a_new_hold_is_taken_before_the_old_one_is_released() {
    let (os, g) = guard();
    g.hold(&req("one")).unwrap();
    g.hold(&req("two")).unwrap();
    assert_eq!(
        os.log(),
        ["acquire#1 System one", "acquire#2 System two", "release#1"]
    );
    assert_eq!(os.live(), 1);
}

#[test]
fn a_failed_hold_keeps_the_old_one() {
    let (os, g) = guard();
    let first = g.hold(&req("one")).unwrap();
    os.fail_with(PowerError::Unavailable {
        reason: "no logind".into(),
    });
    let err = g.hold(&req("two")).unwrap_err();
    assert!(matches!(err, PowerError::Unavailable { .. }));
    assert_eq!(g.current(), Some(first));
    assert_eq!(os.live(), 1);
    os.succeed();
    assert!(g.hold(&req("three")).is_ok());
}

#[test]
fn an_empty_name_or_reason_is_refused_before_the_os() {
    let (os, g) = guard();
    assert!(matches!(
        g.hold(&HoldRequest::new(Scope::System, "", "why")),
        Err(PowerError::Invalid(_))
    ));
    assert!(matches!(
        g.hold(&HoldRequest::new(Scope::System, "Dyno", " \n")),
        Err(PowerError::Invalid(_))
    ));
    assert!(os.log().is_empty());
}

#[test]
fn dropping_the_guard_releases() {
    let (os, g) = guard();
    g.hold(&req("one")).unwrap();
    drop(g);
    assert_eq!(os.live(), 0);
}
