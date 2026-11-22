//! The merge driver of docs/lessons.md (`./dev merge lessons-driver`, `merge=lessons`): a real three-way merge of two branches that each
//! append a section. Git's union merge drops the blank line before the second `## ` heading (9ec43be3c, b9bf5fc4b, 75ce0d2c8, 5c241d93f);
//! the control runs the same merge with the plain union attribute and shows the drop, so the fixture is known to reproduce it (ER-33).
// windows-twin: none: the driver is run by git through `sh`, with a Unix path of the binary; the pure part is unit-tested on every OS (lessons_driver.rs)
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::process::Output;

const BASE: &str = "# Lessons\n\nIntro.\n\n## Old\n- x\n";

#[allow(clippy::disallowed_methods, reason = "a test runs git in a throw-away repository")]
fn git(cwd: &Path, args: &[&str]) -> Output {
    std::process::Command::new("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(cwd)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap()
}

fn ok(cwd: &Path, args: &[&str]) {
    let out = git(cwd, args);
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

/// Two branches that each append a section to `docs/lessons.md`, merged one after the other into main; returns the second merge's
/// status and the file.
fn merged(attribute: &str, driver: bool) -> (bool, String) {
    let d = tempfile::tempdir().unwrap();
    let root = d.path();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join("docs/lessons.md"), BASE).unwrap();
    std::fs::write(root.join(".gitattributes"), format!("/docs/lessons.md merge={attribute}\n")).unwrap();
    ok(root, &["init", "-q", "-b", "main"]);
    ok(root, &["add", "-A"]);
    ok(root, &["commit", "-q", "-m", "base"]);
    if driver {
        let command = format!("{} merge lessons-driver %O %A %B", crate::dev_bin::PATH);
        ok(root, &["config", "merge.lessons.driver", &command]);
    }
    for (name, tag) in [("a", "A"), ("b", "B")] {
        ok(root, &["checkout", "-q", "-b", name, "main"]);
        std::fs::write(
            root.join("docs/lessons.md"),
            format!("{BASE}\n## New {tag}\n- {}\n", tag.to_lowercase()),
        )
        .unwrap();
        ok(root, &["commit", "-q", "-am", name]);
    }
    ok(root, &["checkout", "-q", "main"]);
    ok(root, &["merge", "-q", "--no-edit", "a"]);
    let second = git(root, &["merge", "-q", "--no-edit", "b"]);
    (
        second.status.success(),
        std::fs::read_to_string(root.join("docs/lessons.md")).unwrap(),
    )
}

fn glued(text: &str) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    lines
        .iter()
        .enumerate()
        .any(|(i, l)| l.starts_with("## ") && i > 0 && !lines[i - 1].is_empty())
}

#[test]
fn control_gits_plain_union_merge_glues_the_second_heading() {
    let (clean, text) = merged("union", false);
    assert!(clean);
    assert!(text.contains("## New A") && text.contains("## New B"), "both sections are kept");
    assert!(glued(&text), "the union dropped the blank line:\n{text}");
}

#[test]
fn the_lessons_driver_keeps_both_sections_and_the_blank_line_before_each_heading() {
    let (clean, text) = merged("lessons", true);
    assert!(clean, "the merge conflicted:\n{text}");
    assert!(!glued(&text), "a heading is glued:\n{text}");
    assert_eq!(text, format!("{BASE}\n## New A\n- a\n\n## New B\n- b\n"));
}

#[test]
fn a_clone_without_the_driver_gets_a_conflict_not_a_silent_drop() {
    let (clean, text) = merged("lessons", false);
    assert!(!clean, "a missing driver must not merge silently:\n{text}");
    assert!(text.contains("<<<<<<<"), "{text}");
}

#[test]
fn the_driver_fails_loudly_when_git_cannot_merge() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("base"), "x\n").unwrap();
    std::fs::write(d.path().join("theirs"), "x\n").unwrap();
    let out = crate::dev_bin::command()
        .args(["merge", "lessons-driver"])
        .arg(d.path().join("base"))
        .arg(d.path().join("missing"))
        .arg(d.path().join("theirs"))
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("::error::lessons-driver"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
