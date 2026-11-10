//! Two commands started together: exactly one claim passes (ADR 0083 R8, "the flock test"), and one path is made once.
use super::Fixture;
use std::process::{Command, Stdio};

fn spawn(fx: &Fixture, args: &[&str]) -> std::process::Child {
    Command::new(env!("CARGO_BIN_EXE_paddock-dev"))
        .arg("--json")
        .args(args)
        .current_dir(&fx.primary)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env_remove("PADDOCK_AGENT_ID")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
}

#[test]
fn two_claims_started_together_give_exactly_one_winner() {
    let fx = Fixture::new();
    let mut rounds_with_one_winner = 0;
    for round in 0..8 {
        let path = fx.add(&format!("w{round}"));
        let p = path.to_str().unwrap();
        let a = spawn(&fx, &["worktree", "claim", p, "--agent", "agent-A"]);
        let b = spawn(&fx, &["worktree", "claim", p, "--agent", "agent-B"]);
        let codes: Vec<i32> = [a, b]
            .into_iter()
            .map(|c| c.wait_with_output().unwrap().status.code().unwrap())
            .collect();
        assert_eq!(codes.iter().filter(|c| **c == 0).count(), 1, "round {round}: {codes:?}");
        assert_eq!(codes.iter().filter(|c| **c == 4).count(), 1, "round {round}: {codes:?}");
        let winner = fx.owner_of(&path).unwrap();
        assert!(winner == "agent-A" || winner == "agent-B");
        rounds_with_one_winner += 1;
    }
    assert_eq!(rounds_with_one_winner, 8, "the test ran its rounds");
    assert_eq!(
        fx.owners()["owners"].as_object().unwrap().len(),
        8,
        "one entry per worktree, none lost to a racing write"
    );
}

#[test]
fn claims_of_different_worktrees_started_together_all_land() {
    let fx = Fixture::new();
    let paths: Vec<_> = (0..6).map(|i| fx.add(&format!("w{i}"))).collect();
    let children: Vec<_> = paths
        .iter()
        .enumerate()
        .map(|(i, path)| {
            spawn(
                &fx,
                &["worktree", "claim", path.to_str().unwrap(), "--agent", &format!("agent-{i}")],
            )
        })
        .collect();
    for c in children {
        assert!(c.wait_with_output().unwrap().status.success());
    }
    for (i, path) in paths.iter().enumerate() {
        assert_eq!(fx.owner_of(path).unwrap(), format!("agent-{i}"));
    }
}

#[test]
fn two_adds_of_one_path_make_it_once() {
    let fx = Fixture::new();
    for round in 0..4 {
        let path = fx.path(&format!("same{round}"));
        let p = path.to_str().unwrap();
        let a = spawn(
            &fx,
            &[
                "worktree",
                "add",
                p,
                "-b",
                &format!("a{round}"),
                "origin/main",
                "--owner",
                "x:y",
                "--agent",
                "agent-A",
            ],
        );
        let b = spawn(
            &fx,
            &[
                "worktree",
                "add",
                p,
                "-b",
                &format!("b{round}"),
                "origin/main",
                "--owner",
                "x:y",
                "--agent",
                "agent-B",
            ],
        );
        let codes: Vec<i32> = [a, b]
            .into_iter()
            .map(|c| c.wait_with_output().unwrap().status.code().unwrap())
            .collect();
        assert_eq!(codes.iter().filter(|c| **c == 0).count(), 1, "round {round}: {codes:?}");
        assert_eq!(codes.iter().filter(|c| **c == 4).count(), 1, "round {round}: {codes:?}");
        let winner = fx.owner_of(&path).unwrap();
        let loser_branch = if winner == "agent-A" {
            format!("b{round}")
        } else {
            format!("a{round}")
        };
        assert!(
            !fx.git_ok(&fx.primary, &["rev-parse", "--verify", "-q", &format!("refs/heads/{loser_branch}")]),
            "the loser made no branch"
        );
    }
}
