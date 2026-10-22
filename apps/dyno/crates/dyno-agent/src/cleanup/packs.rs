//! Loads the categories: the built-in pack plus the packs a user dropped into the data folder's `packs/` folder.
//! A pack that is too big, unreadable, invalid or clashes with an existing category id is ignored whole, with a message
//! naming the file; the others still load.

use dyno_core::cleanup::pack::{parse, Trust, MAX_PACK_BYTES};
use dyno_core::cleanup::{catalog, CategorySpec};
use std::path::Path;

/// The most pack files read.
pub const MAX_FILES: usize = 32;

/// Loads the built-in categories and the user's packs. Returns them with one message per ignored pack.
pub fn load(dir: &Path) -> (Vec<CategorySpec>, Vec<String>) {
    let mut specs = catalog();
    let mut notes = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else { return (specs, notes) };
    let mut files: Vec<_> = rd
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("toml")))
        .collect();
    files.sort_by_key(|e| e.file_name());
    if files.len() > MAX_FILES {
        notes.push(format!("Only the first {MAX_FILES} cleanup packs are read; the rest are ignored."));
        files.truncate(MAX_FILES);
    }
    for e in files {
        let name = e.file_name().to_string_lossy().into_owned();
        let ignored = |why: String| format!("The cleanup pack {name} was ignored: {why}");
        let Ok(meta) = e.path().symlink_metadata() else { continue };
        if !meta.is_file() {
            notes.push(ignored("it is not a plain file.".into()));
            continue;
        }
        if meta.len() > MAX_PACK_BYTES as u64 {
            notes.push(ignored(format!("it is larger than {} KiB.", MAX_PACK_BYTES / 1024)));
            continue;
        }
        let text = match std::fs::read_to_string(e.path()) {
            Ok(t) => t,
            Err(err) => {
                notes.push(ignored(format!("it cannot be read ({err}).")));
                continue;
            }
        };
        match parse(&text, Trust::User) {
            Ok(found) => {
                if let Some(clash) = found.iter().find(|c| specs.iter().any(|s| s.id == c.id)) {
                    notes.push(ignored(format!("the category id `{}` is already taken.", clash.id)));
                } else {
                    specs.extend(found);
                }
            }
            Err(why) => notes.push(ignored(why.replace('\n', " "))),
        }
    }
    (specs, notes)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = "format = 1\n[[category]]\nid = \"gradle\"\nname = \"Gradle\"\ndescription = \"d\"\ngroup = \"developer\"\nrisk = \"review\"\npaths = [\"{home}/.gradle/caches\"]\n";

    #[test]
    fn user_packs_add_categories_and_bad_ones_are_reported_and_ignored() {
        let t = tempfile::tempdir().unwrap();
        let builtin = catalog().len();
        assert_eq!(load(&t.path().join("none")), (catalog(), vec![]), "no folder is fine");
        std::fs::write(t.path().join("a-good.toml"), GOOD).unwrap();
        std::fs::write(t.path().join("b-broken.toml"), "format = [").unwrap();
        std::fs::write(t.path().join("c-clash.toml"), GOOD.replace("gradle", "userTemp")).unwrap();
        std::fs::write(t.path().join("d-huge.toml"), vec![b'#'; MAX_PACK_BYTES + 1]).unwrap();
        std::fs::write(t.path().join("e-bad-utf8.toml"), [0xff, 0xfe, 0xfd]).unwrap();
        std::fs::write(t.path().join("readme.txt"), "not a pack").unwrap();
        std::fs::create_dir(t.path().join("f-folder.toml")).unwrap();
        let (specs, notes) = load(t.path());
        assert_eq!(specs.len(), builtin + 1);
        assert!(specs.iter().any(|s| s.id == "gradle" && !s.builtin));
        assert_eq!(notes.len(), 5, "{notes:?}");
        assert!(notes[0].contains("b-broken.toml") && notes[0].contains("was ignored"));
        assert!(notes.iter().any(|n| n.contains("already taken")));
        assert!(notes.iter().any(|n| n.contains("larger than")));
        assert!(notes.iter().any(|n| n.contains("cannot be read")));
        assert!(notes.iter().any(|n| n.contains("not a plain file")));
    }

    #[test]
    fn too_many_files_are_capped() {
        let t = tempfile::tempdir().unwrap();
        for i in 0..MAX_FILES + 2 {
            std::fs::write(t.path().join(format!("p{i:03}.toml")), GOOD.replace("gradle", &format!("g{i}"))).unwrap();
        }
        let (specs, notes) = load(t.path());
        assert_eq!(specs.len(), catalog().len() + MAX_FILES);
        assert!(notes[0].contains("Only the first"));
    }
}
