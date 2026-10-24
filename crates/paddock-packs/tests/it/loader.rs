use crate::common;
use common::*;
use paddock_packs::{BuiltinPack, DirSource, Health, Limits, Origin, PackLoader};

fn loader(src: &MemSource) -> PackLoader<Cleanup> {
    PackLoader::new(
        vec![BuiltinPack {
            name: "builtin.toml",
            text: GOOD_CLEANUP,
        }],
        Box::new(src.clone()),
        Limits::default(),
    )
}

fn pack(id: &str) -> String {
    format!("format = 1\n[[category]]\nid='{id}'\nname='{id}'\n")
}

#[test]
fn built_in_first_then_user_packs_in_name_order() {
    let src = MemSource::default();
    src.put("b.toml", &pack("b"));
    src.put("a.toml", &pack("a"));
    let mut l = loader(&src);
    let s = l.reload().unwrap();
    let names: Vec<_> = s
        .packs
        .iter()
        .map(|p| (p.name.as_str(), p.origin))
        .collect();
    assert_eq!(
        names,
        [
            ("builtin.toml", Origin::Builtin),
            ("a.toml", Origin::User),
            ("b.toml", Origin::User)
        ]
    );
    assert_eq!(s.revision, 1);
    assert_eq!(s.health(), Health::Ok);
}

#[test]
fn one_broken_pack_never_takes_down_the_rest() {
    let src = MemSource::default();
    src.put("a.toml", &pack("a"));
    src.put("broken.toml", "format = 1\n[[category\n");
    src.put("c.toml", &pack("c"));
    let mut l = loader(&src);
    let s = l.reload().unwrap();
    let names: Vec<_> = s.packs.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["builtin.toml", "a.toml", "c.toml"]);
    assert_eq!(s.problems.len(), 1);
    assert_eq!(s.problems[0].file, "broken.toml");
    assert!(s.problems[0].message.contains("not valid TOML"));
    assert!(
        matches!(s.health(), Health::Degraded { message } if message.starts_with("broken.toml: "))
    );
}

#[test]
fn a_bad_built_in_pack_is_a_problem_and_the_user_packs_still_load() {
    let src = MemSource::default();
    src.put("a.toml", &pack("a"));
    let mut l = PackLoader::<Cleanup>::new(
        vec![BuiltinPack {
            name: "builtin.toml",
            text: "format = 9",
        }],
        Box::new(src.clone()),
        Limits::default(),
    );
    let s = l.reload().unwrap();
    assert_eq!(s.packs.len(), 1);
    assert_eq!(s.problems[0].file, "builtin.toml");
}

#[test]
fn an_oversized_user_pack_is_refused_and_only_bounded_bytes_are_read() {
    let src = MemSource::default();
    src.put_bytes("huge.toml", &vec![b'#'; 10_000]);
    src.put("ok.toml", &pack("ok"));
    let mut l = PackLoader::<Cleanup>::new(
        vec![],
        Box::new(src.clone()),
        Limits {
            max_file_bytes: 1024,
            ..Limits::default()
        },
    );
    let s = l.reload().unwrap();
    assert_eq!(s.packs.len(), 1);
    assert!(
        s.problems[0].message.contains("too large"),
        "{:?}",
        s.problems
    );
}

#[test]
fn a_pack_that_turns_broken_keeps_its_last_good_version_and_says_so() {
    let src = MemSource::default();
    src.put("a.toml", &pack("a"));
    let mut l = loader(&src);
    l.reload().unwrap();
    src.put("a.toml", "format = 1\n[[category]]\nid = 7\n");
    let s = l.reload().unwrap();
    assert_eq!(s.revision, 2);
    assert_eq!(s.packs.iter().filter(|p| p.name == "a.toml").count(), 1);
    assert!(
        s.problems[0]
            .message
            .ends_with("the previous version stays in use"),
        "{:?}",
        s.problems
    );
    // Fixed again: the problem clears.
    src.put("a.toml", &pack("a2"));
    let s = l.reload().unwrap();
    assert!(s.problems.is_empty());
    assert_eq!(s.packs[1].pack[0].id, "a2");
}

#[test]
fn a_pack_deleted_between_listing_and_reading_is_simply_gone() {
    let src = MemSource::default();
    src.put("a.toml", &pack("a"));
    src.put("gone.toml", &pack("g"));
    src.vanish_on_read("gone.toml");
    let mut l = loader(&src);
    let s = l.reload().unwrap();
    assert_eq!(
        s.packs.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
        ["builtin.toml", "a.toml"]
    );
    assert!(s.problems.is_empty());
}

#[test]
fn a_pack_deleted_after_it_loaded_leaves_the_set_even_if_it_was_good() {
    let src = MemSource::default();
    src.put("a.toml", &pack("a"));
    let mut l = loader(&src);
    l.reload().unwrap();
    src.remove("a.toml");
    let s = l.reload().unwrap();
    assert_eq!(s.packs.len(), 1);
    assert_eq!(s.revision, 2);
}

#[test]
fn an_unreadable_pack_is_a_problem_and_a_failed_listing_keeps_the_built_ins() {
    let src = MemSource::default();
    src.put("locked.toml", &pack("l"));
    src.fail_on_read("locked.toml");
    let mut l = loader(&src);
    let s = l.reload().unwrap();
    assert_eq!(s.packs.len(), 1);
    assert!(s.problems[0].message.contains("could not read locked.toml"));

    let src = MemSource::default();
    src.list_fails(true);
    let mut l = loader(&src);
    let s = l.reload().unwrap();
    assert_eq!(s.packs.len(), 1);
    assert_eq!(s.problems[0].file, "packs");
}

#[test]
fn files_over_the_pack_count_are_reported_not_read() {
    let src = MemSource::default();
    for i in 0..5 {
        src.put(&format!("p{i}.toml"), &pack("p"));
    }
    let mut l = PackLoader::<Cleanup>::new(
        vec![],
        Box::new(src.clone()),
        Limits {
            max_packs: 3,
            ..Limits::default()
        },
    );
    let s = l.reload().unwrap();
    assert_eq!(s.packs.len(), 3);
    assert_eq!(src.reads(), 3);
    assert!(
        s.problems[0]
            .message
            .contains("2 pack files are over the limit of 3")
    );
}

#[test]
fn unchanged_files_are_not_read_into_a_new_revision_and_not_parsed_again() {
    let src = MemSource::default();
    src.put("a.toml", &pack("a"));
    src.put("bad.toml", "nope");
    let mut l = loader(&src);
    assert_eq!(l.reload().unwrap().revision, 1);
    assert!(l.reload().is_none());
    assert!(l.reload().is_none());
    assert_eq!(l.current().revision, 1);
    src.put("a.toml", &pack("changed"));
    assert_eq!(l.reload().unwrap().revision, 2);
}

#[test]
fn warnings_come_with_the_file_and_clear_when_fixed() {
    let src = MemSource::default();
    src.put("a.toml", "format = 1\nzzz = 1\n");
    let mut l = loader(&src);
    let s = l.reload().unwrap();
    assert_eq!(s.warnings[0].file, "a.toml");
    assert!(matches!(s.health(), Health::Notes { message } if message.contains("zzz")));
    src.put("a.toml", "format = 1\nzzz = 1\nyyy = 2\n");
    assert!(
        matches!(l.reload().unwrap().health(), Health::Notes { message } if message.ends_with("(and 1 more)"))
    );
}

#[test]
fn health_names_the_first_problem_and_counts_the_rest() {
    let src = MemSource::default();
    src.put("a.toml", "x");
    src.put("b.toml", "y");
    let mut l = loader(&src);
    let s = l.reload().unwrap();
    assert!(
        matches!(s.health(), Health::Degraded { message } if message.starts_with("a.toml: ") && message.ends_with("(and 1 more)"))
    );
}

#[test]
fn a_real_folder_lists_only_toml_files_and_reads_a_bounded_amount() {
    let t = tempfile::tempdir().unwrap();
    let d = t.path();
    std::fs::write(d.join("a.toml"), pack("a")).unwrap();
    std::fs::write(d.join("UPPER.TOML"), pack("u")).unwrap();
    std::fs::write(d.join(".hidden.toml"), pack("h")).unwrap();
    std::fs::write(d.join("~tmp.toml"), pack("t")).unwrap();
    std::fs::write(d.join("notes.txt"), "x").unwrap();
    std::fs::write(d.join(".toml"), "x").unwrap();
    std::fs::create_dir(d.join("dir.toml")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(d.join("a.toml"), d.join("link.toml")).unwrap();
    let mut l = PackLoader::<Cleanup>::new(vec![], Box::new(DirSource::new(d)), Limits::default());
    let s = l.reload().unwrap();
    let mut names: Vec<_> = s.packs.iter().map(|p| p.name.as_str()).collect();
    names.sort();
    assert_eq!(names, ["UPPER.TOML", "a.toml"]);

    use paddock_packs::PackSource;
    let src = DirSource::new(d);
    std::fs::write(d.join("big.toml"), vec![b'x'; 5000]).unwrap();
    assert_eq!(src.read("big.toml", 100).unwrap().len(), 101);
    assert!(src.read("../a.toml", 100).is_err());
    assert_eq!(
        src.read("missing.toml", 10).unwrap_err().kind(),
        std::io::ErrorKind::NotFound
    );
}

#[test]
fn a_missing_folder_is_an_empty_one_and_a_file_in_its_place_is_a_problem() {
    let t = tempfile::tempdir().unwrap();
    let mut l = PackLoader::<Cleanup>::new(
        vec![],
        Box::new(DirSource::new(t.path().join("nope"))),
        Limits::default(),
    );
    let s = l.reload().unwrap();
    assert!(s.packs.is_empty() && s.problems.is_empty());

    std::fs::write(t.path().join("packs"), "x").unwrap();
    let mut l = PackLoader::<Cleanup>::new(
        vec![],
        Box::new(DirSource::new(t.path().join("packs"))),
        Limits::default(),
    );
    assert_eq!(l.reload().unwrap().problems[0].file, "packs");
}

#[test]
fn the_user_folder_is_inside_the_data_folder() {
    use paddock_env::{AppDirs, AppId, MapVars, Os};
    let app = AppId::new("demo", "Demo").unwrap();
    let vars = MapVars::new().with("DEMO_HOME", "/tmp/demo-home");
    let dirs = AppDirs::resolve(&app, Os::Linux, &vars, None, true).unwrap();
    assert_eq!(
        paddock_packs::user_packs_dir(&dirs),
        dirs.data().join("packs")
    );
}

#[test]
fn a_second_kind_with_a_different_format_loads_through_the_same_loader() {
    let src = MemSource::default();
    src.put(
        "s.toml",
        "format = 2\n[[scheme]]\nid='ubuntu'\npattern='(?<major>\\d{2})\\.(?<minor>\\d{2})'\n",
    );
    src.put("old.toml", "format = 1\n");
    let mut l = PackLoader::<Schemes>::new(vec![], Box::new(src.clone()), Limits::default());
    let s = l.reload().unwrap();
    assert_eq!(s.packs.len(), 1);
    assert_eq!(s.packs[0].pack[0].0, "ubuntu");
    assert!(s.problems[0].message.contains("older format"));
}
