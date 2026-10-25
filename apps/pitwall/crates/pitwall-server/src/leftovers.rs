//! Files that older versions left behind and nothing uses any more (ADR 0020). They are listed on the llama.cpp page and
//! removed only when the user presses the button: Pitwall never deletes them by itself.

use crate::paths::Dirs;
use pitwall_core::mgmt::Leftover;
use std::path::{Path, PathBuf};

/// The things that can be left over: `(id, label, folder name inside the engines folder)`.
const KNOWN: &[(&str, &str, &str)] = &[("pack-llamafile", "llamafile", "pack-llamafile")];

fn dir_size(p: &Path) -> u64 {
    let Ok(rd) = std::fs::read_dir(p) else {
        return std::fs::symlink_metadata(p).map(|m| m.len()).unwrap_or(0);
    };
    rd.filter_map(Result::ok)
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => dir_size(&e.path()),
            _ => e.metadata().map(|m| m.len()).unwrap_or(0),
        })
        .sum()
}

fn folder(dirs: &Dirs, name: &str) -> PathBuf {
    dirs.engines().join(name)
}

/// The leftovers present now.
pub fn find(dirs: &Dirs) -> Vec<Leftover> {
    KNOWN
        .iter()
        .filter_map(|(id, label, name)| {
            let p = folder(dirs, name);
            let is_real_dir = std::fs::symlink_metadata(&p).map(|m| m.is_dir()).unwrap_or(false);
            is_real_dir.then(|| Leftover {
                id: (*id).into(),
                label: (*label).into(),
                size_bytes: dir_size(&p),
            })
        })
        .collect()
}

/// Removes the leftover `id` (only the known ones; anything else is refused with a message).
pub fn remove(dirs: &Dirs, id: &str) -> Result<(), String> {
    let (_, _, name) = KNOWN
        .iter()
        .find(|(k, _, _)| *k == id)
        .ok_or_else(|| format!("\"{id}\" is not something Pitwall left on disk"))?;
    let p = folder(dirs, name);
    match std::fs::symlink_metadata(&p) {
        Ok(m) if m.is_dir() => {
            std::fs::remove_dir_all(&p).map_err(|e| format!("could not remove it: {e}. Close anything using it and try again."))
        }
        Ok(_) => Err("it is not a folder Pitwall created; remove it by hand if you want it gone".into()),
        Err(_) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_leftover_llamafile_is_listed_with_its_size_and_removed_only_on_request() {
        let d = tempfile::tempdir().unwrap();
        let dirs = Dirs::at(d.path());
        assert!(find(&dirs).is_empty());
        let p = dirs.engines().join("pack-llamafile");
        std::fs::create_dir_all(p.join("sub")).unwrap();
        std::fs::write(p.join("llamafile"), vec![0u8; 1000]).unwrap();
        std::fs::write(p.join("sub/x"), vec![0u8; 24]).unwrap();
        let l = find(&dirs);
        assert_eq!(l.len(), 1);
        assert_eq!(
            (l[0].id.as_str(), l[0].label.as_str(), l[0].size_bytes),
            ("pack-llamafile", "llamafile", 1024)
        );
        assert!(p.exists(), "listing never deletes");
        remove(&dirs, "pack-llamafile").unwrap();
        assert!(!p.exists() && find(&dirs).is_empty());
        remove(&dirs, "pack-llamafile").unwrap();
    }

    #[test]
    fn only_known_leftovers_can_be_removed() {
        let d = tempfile::tempdir().unwrap();
        let dirs = Dirs::at(d.path());
        std::fs::create_dir_all(dirs.engines().join("metal")).unwrap();
        for id in ["metal", "../models", "pack-other", ""] {
            assert!(remove(&dirs, id).unwrap_err().contains("not something"), "{id}");
        }
        assert!(dirs.engines().join("metal").exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_in_its_place_is_never_followed_or_deleted() {
        let d = tempfile::tempdir().unwrap();
        let dirs = Dirs::at(d.path());
        let target = d.path().join("precious");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::create_dir_all(dirs.engines()).unwrap();
        std::os::unix::fs::symlink(&target, dirs.engines().join("pack-llamafile")).unwrap();
        assert!(find(&dirs).is_empty());
        assert!(remove(&dirs, "pack-llamafile").is_err());
        assert!(target.exists());
    }
}
