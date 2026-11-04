//! `$INSTDIR\updates\` on disk (native glue; the decisions are `checkdir::plan_updates_dir` and `aside_name`).

use super::facts::inspect_updates;
use super::handle::Descriptor;
use super::wide::to_wide;
use crate::checkdir::{UPDATES_SDDL, UpdatesAction, aside_name, plan_updates_dir};
use std::path::Path;
use windows_sys::Win32::Storage::FileSystem::CreateDirectoryW;

/// Creates `path` with the explicit descriptor of section 2.5.
fn create_with_descriptor(path: &str) -> Result<(), String> {
    let sd = Descriptor::from_sddl(UPDATES_SDDL).map_err(|e| format!("the folder descriptor could not be built (error {e})"))?;
    let attrs = sd.attributes();
    let wide = to_wide(path);
    // SAFETY: NUL-terminated path; the attributes point at a live descriptor.
    if unsafe { CreateDirectoryW(wide.as_ptr(), &attrs) } == 0 {
        return Err(format!("the folder could not be made (error {})", super::handle::last_error()));
    }
    Ok(())
}

/// Makes or checks `path`; a wrong owner, a reparse point or a non-folder is renamed aside (a link is renamed, never
/// followed) and a new one made. Returns the action taken.
pub fn ensure(path: &str) -> Result<UpdatesAction, String> {
    let found = inspect_updates(path);
    let action = plan_updates_dir(&found);
    match action {
        UpdatesAction::Use => {}
        UpdatesAction::Create => create_with_descriptor(path)?,
        UpdatesAction::RenameAsideAndCreate => {
            let parent = Path::new(path).parent().ok_or("the folder has no parent")?;
            let taken: Vec<String> = std::fs::read_dir(parent)
                .map(|d| d.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect())
                .unwrap_or_default();
            let aside = parent.join(aside_name(&taken));
            std::fs::rename(path, &aside).map_err(|e| format!("the folder could not be renamed aside ({e})"))?;
            create_with_descriptor(path)?;
        }
    }
    Ok(action)
}
