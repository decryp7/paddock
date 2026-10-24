use paddock_testkit::{FakeRunner, Reply};

#[test]
fn replies_by_program_and_argument_prefix_first_rule_wins() {
    let r = FakeRunner::new();
    r.on("launchctl", &["print", "gui/501/net.x.agent"])
        .reply(Reply::ok("state = running"));
    r.on("launchctl", &["print"]).reply(Reply::fails("Could not find service"));
    r.on("launchctl", &[]).reply(Reply::ok(""));
    assert_eq!(r.run("launchctl", &["print", "gui/501/net.x.agent"]), Reply::ok("state = running"));
    assert_eq!(
        r.run("launchctl", &["print", "gui/501/other"]),
        Reply::fails("Could not find service")
    );
    assert_eq!(r.run("launchctl", &["bootout", "gui/501"]), Reply::ok(""));
    assert_eq!(r.count("launchctl", &["print"]), 2);
    assert_eq!(r.lines()[2], "launchctl bootout gui/501");
}

#[test]
fn queued_replies_are_used_in_order_and_the_last_repeats() {
    let r = FakeRunner::new();
    r.on("systemctl", &["--user", "is-active"])
        .reply(Reply::fails("inactive"))
        .then(Reply::ok("active"));
    let answers: Vec<_> = (0..3).map(|_| r.run("systemctl", &["--user", "is-active", "x.service"])).collect();
    assert_eq!(answers, [Reply::fails("inactive"), Reply::ok("active"), Reply::ok("active")]);
}

#[test]
fn an_unscripted_command_is_an_error_and_is_still_recorded() {
    let r = FakeRunner::new();
    r.on("reg", &["query"]).reply(Reply::ok("x"));
    match r.run("reg", &["delete", "HKCU\\Run"]) {
        Reply::Failed(m) => assert!(m.contains("unscripted command `reg delete HKCU\\Run`"), "{m}"),
        other => panic!("{other:?}"),
    }
    // A rule that never got a reply does not match either.
    r.on("net", &[]);
    assert!(matches!(r.run("net", &["use"]), Reply::Failed(_)));
    assert_eq!(r.calls().len(), 2);
    assert_eq!(r.calls()[0].program, "reg");
}

#[test]
fn a_command_that_cannot_start_and_clones_share_the_script() {
    let r = FakeRunner::new();
    let seen = r.clone();
    r.on("tool", &[]).reply(Reply::cannot_start("not found"));
    assert_eq!(r.run("tool", &[]), Reply::Failed("not found".into()));
    assert_eq!(seen.count("tool", &[]), 1);
}

#[test]
fn a_poisoned_script_still_answers() {
    let r = FakeRunner::new();
    let inner = r.clone();
    let _ = std::thread::spawn(move || {
        let _ = inner.run("x", &[]);
        panic!("a test thread failed");
    })
    .join();
    r.on("x", &[]).reply(Reply::ok("fine"));
    assert_eq!(r.run("x", &[]), Reply::ok("fine"));
}
