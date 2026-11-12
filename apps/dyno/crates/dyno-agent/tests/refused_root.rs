#![allow(clippy::unwrap_used, clippy::expect_used)]

//! A data folder the agent refuses (open to others, ADR 0092 W8) is not used at all: history is in memory, and nothing is read from or
//! written to that folder (ER-14: the folder is a temporary one).

#[cfg(unix)]
mod unix {
    use dyno_agent::agent::Shared;
    use dyno_agent::paths::Dirs;
    use dyno_platform::fake::Fake;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn a_refused_data_folder_leaves_dyno_db_and_every_other_file_untouched_and_the_history_in_memory() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("data");
        std::fs::create_dir(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o777)).unwrap();
        // Files another account planted: none of them may be opened, read or replaced.
        let planted: Vec<(&str, &[u8])> = vec![("dyno.db", b"not a database, planted"), ("settings.json", b"{\"planted\": true}")];
        for (name, body) in &planted {
            std::fs::write(root.join(name), body).unwrap();
        }
        let before: Vec<_> = {
            let mut v: Vec<_> = std::fs::read_dir(&root).unwrap().flatten().map(|e| e.file_name()).collect();
            v.sort();
            v
        };
        let fake = Fake::new();
        let built = Shared::build(Dirs::at(&root), fake.platform(), Box::new(fake), Box::new(|_| Ok(())));
        let (shared, ..) = built.expect("the agent still starts, on memory");
        // The refusal is said in the activity log and the history is not on disk.
        let notes: String = shared
            .core
            .lock()
            .unwrap()
            .store
            .activity(50)
            .unwrap()
            .iter()
            .map(|a| a.detail.clone())
            .collect();
        assert!(notes.contains("memory only") && notes.contains("chmod go-w"), "{notes}");
        assert!(!shared.dirs.root().starts_with(&root), "the stand-in folder is not the refused one");
        let after: Vec<_> = {
            let mut v: Vec<_> = std::fs::read_dir(&root).unwrap().flatten().map(|e| e.file_name()).collect();
            v.sort();
            v
        };
        assert_eq!(before, after, "nothing was created in the refused folder");
        for (name, body) in &planted {
            assert_eq!(
                &std::fs::read(root.join(name)).unwrap(),
                body,
                "{name} was not read back and rewritten"
            );
        }
        assert_eq!(
            std::fs::metadata(&root).unwrap().permissions().mode() & 0o777,
            0o777,
            "never tightened"
        );
        let scratch = shared.dirs.root().to_path_buf();
        drop(shared);
        assert!(!scratch.exists(), "the per-run stand-in folder is removed when the agent ends");
    }
}
