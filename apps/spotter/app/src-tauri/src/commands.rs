//! The Tauri commands: the shell's implementation of the UI's `SpotterBackend` (`app/src/app/core/backend.ts`).
//! Every command is `async` (nothing runs on the main thread), validates its arguments (the webview is not trusted)
//! and maps errors to [`CommandError`]. Blocking work (the credential store, the native dialogs, `--version` of a
//! browser) goes to `spawn_blocking`. The command names are the snake_case of the TypeScript method names.

use crate::dto::*;
use crate::state::Shared;
use crate::{applog, lifecycle, tray, updater};
use spotter_core::limits::MAX_WATCHES;
use std::path::PathBuf;
use tauri::{AppHandle, Runtime, State};
use tauri_plugin_autostart::ManagerExt as AutostartExt;
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

type Out<T> = Result<T, CommandError>;

/// Longest id, address or version text accepted from the webview.
const MAX_TEXT: usize = 2048;
/// Most secret fields in one request.
const MAX_SECRETS: usize = 16;
/// Largest list text accepted for an import (the engine re-checks).
const MAX_IMPORT_TEXT: usize = 3 << 20;
/// Most events one request may ask for.
const MAX_EVENTS: usize = 500;

fn text_ok(name: &str, value: &str) -> Out<()> {
    if value.len() > MAX_TEXT || value.contains('\0') {
        return Err(CommandError::bad(format!("The {name} is not valid.")));
    }
    Ok(())
}

fn secrets_ok(secrets: &Secrets) -> Out<()> {
    if secrets.len() > MAX_SECRETS
        || secrets.keys().any(|k| k.len() > 64)
        || secrets.values().any(|v| v.len() > crate::credentials::MAX_SECRET_LEN)
    {
        return Err(CommandError::bad("Too many or too long secret values."));
    }
    Ok(())
}

/// The draft as a core spec, with the setting the form does not edit (`allow_local`) kept from the stored watch.
fn spec_of(state: &Shared, draft: WatchDraftDto) -> Out<spotter_core::watch::WatchSpec> {
    text_ok("watch name", &draft.name)?;
    let allow_local = match draft.id.as_deref().filter(|i| !i.is_empty()) {
        Some(id) => state.engine.get_watch(id).map(|d| d.spec.allow_local).unwrap_or(false),
        None => false,
    };
    Ok(draft.into_spec(allow_local))
}

// ---- app ----

/// `app_info`.
#[tauri::command]
pub async fn app_info<R: Runtime>(app: AppHandle<R>, state: State<'_, Shared>) -> Out<AppInfoDto> {
    Ok(AppInfoDto {
        version: app.package_info().version.to_string(),
        config_path: state.engine.config_path().display().to_string(),
        logs_dir: applog::dir().map(|d| d.display().to_string()).unwrap_or_default(),
        downloads_dir: downloads_dir(&state).display().to_string(),
        warnings: state.engine.warnings(),
        migrated_from_legacy: state.engine.migrated_from_legacy(),
        platform: platform_name(),
    })
}

fn downloads_dir(state: &Shared) -> PathBuf {
    state
        .engine
        .settings()
        .downloads_folder
        .map(PathBuf::from)
        .unwrap_or_else(|| state.default_downloads.clone())
}

// ---- watches ----

/// `list_watches`.
#[tauri::command]
pub async fn list_watches(state: State<'_, Shared>) -> Out<Vec<WatchSummaryDto>> {
    Ok(state.engine.list_watches().iter().map(Into::into).collect())
}

/// `get_watch`.
#[tauri::command]
pub async fn get_watch(state: State<'_, Shared>, id: String) -> Out<WatchDetailDto> {
    text_ok("watch id", &id)?;
    Ok((&state.engine.get_watch(&id)?).into())
}

/// `source_kinds`.
#[tauri::command]
pub async fn source_kinds(state: State<'_, Shared>) -> Out<Vec<SourceKindDto>> {
    Ok(state.engine.source_kinds().iter().map(Into::into).collect())
}

/// `test_source`: checks a draft without saving it.
#[tauri::command]
pub async fn test_source(state: State<'_, Shared>, draft: WatchDraftDto, secrets: Secrets) -> Out<SourcePreviewDto> {
    secrets_ok(&secrets)?;
    let spec = spec_of(&state, draft)?;
    Ok((&state.engine.test_source(&spec, &secrets).await).into())
}

/// `save_watch`. The credential store is touched by the engine, so this runs off the async threads.
#[tauri::command]
pub async fn save_watch(state: State<'_, Shared>, draft: WatchDraftDto, secrets: Secrets) -> Out<WatchSummaryDto> {
    secrets_ok(&secrets)?;
    let spec = spec_of(&state, draft)?;
    let engine = state.engine.clone();
    if spec.id.is_empty() && engine.list_watches().len() >= MAX_WATCHES {
        return Err(CommandError::from(spotter_engine::EngineError::Invalid(vec![format!(
            "Spotter keeps at most {MAX_WATCHES} watches."
        )])));
    }
    let saved = blocking(move || engine.save_watch(spec, &secrets).map_err(CommandError::from)).await?;
    Ok((&saved).into())
}

/// `delete_watch`.
#[tauri::command]
pub async fn delete_watch(state: State<'_, Shared>, id: String) -> Out<()> {
    text_ok("watch id", &id)?;
    let engine = state.engine.clone();
    blocking(move || engine.delete_watch(&id).map_err(CommandError::from)).await
}

/// `set_paused`.
#[tauri::command]
pub async fn set_paused(state: State<'_, Shared>, id: String, paused: bool) -> Out<()> {
    text_ok("watch id", &id)?;
    Ok(state.engine.set_paused(&id, paused)?)
}

/// `check_now`: one watch now (the answer comes when it is done), or every watch in the background (the watches
/// report through the pushed state, so a long run never trips the UI's timeout).
#[tauri::command]
pub async fn check_now(state: State<'_, Shared>, id: Option<String>) -> Out<()> {
    match id {
        Some(id) => {
            text_ok("watch id", &id)?;
            state.engine.check_now(Some(&id)).await?;
        }
        None => {
            let engine = state.engine.clone();
            tauri::async_runtime::spawn(async move {
                let _ = engine.check_now(None).await;
            });
        }
    }
    Ok(())
}

/// `acknowledge`.
#[tauri::command]
pub async fn acknowledge(state: State<'_, Shared>, id: String, version: Option<String>) -> Out<()> {
    text_ok("watch id", &id)?;
    text_ok("version", version.as_deref().unwrap_or(""))?;
    Ok(state.engine.acknowledge(&id, version.as_deref())?)
}

/// `acknowledge_product`: the latest release of one product of a watch has been seen.
#[tauri::command]
pub async fn acknowledge_product(state: State<'_, Shared>, id: String, product: String) -> Out<()> {
    text_ok("watch id", &id)?;
    text_ok("product", &product)?;
    Ok(state.engine.acknowledge_product(&id, &product)?)
}

/// `suggest_pattern`: a regular expression written from an example name and the selected parts.
#[tauri::command]
pub async fn suggest_pattern(state: State<'_, Shared>, example: String, selections: Vec<PatternSelectionDto>) -> Out<String> {
    text_ok("example", &example)?;
    if selections.len() > 16 {
        return Err(CommandError::bad("Select at most 16 parts."));
    }
    let selections: Vec<_> = selections.into_iter().map(Into::into).collect();
    Ok(state.engine.suggest_pattern(&example, &selections)?)
}

/// `test_pattern`: tries a version scheme on one name.
#[tauri::command]
pub async fn test_pattern(
    state: State<'_, Shared>,
    versions: String,
    version_pattern: Option<String>,
    missing_older: bool,
    text: String,
) -> Out<PatternTestDto> {
    text_ok("version scheme", &versions)?;
    text_ok("pattern", version_pattern.as_deref().unwrap_or(""))?;
    text_ok("name", &text)?;
    Ok((&state
        .engine
        .test_pattern(&versions, version_pattern.as_deref(), missing_older, &text))
        .into())
}

/// `version_schemes`: patterns to start from.
#[tauri::command]
pub async fn version_schemes(state: State<'_, Shared>) -> Out<Vec<SchemePresetDto>> {
    Ok(state.engine.version_schemes().iter().map(Into::into).collect())
}

/// `reset_acknowledgement`.
#[tauri::command]
pub async fn reset_acknowledgement(state: State<'_, Shared>, id: String) -> Out<()> {
    text_ok("watch id", &id)?;
    Ok(state.engine.reset_acknowledgement(&id)?)
}

/// `set_installed`.
#[tauri::command]
pub async fn set_installed(state: State<'_, Shared>, id: String, version: Option<String>) -> Out<()> {
    text_ok("watch id", &id)?;
    text_ok("version", version.as_deref().unwrap_or(""))?;
    Ok(state.engine.set_installed(&id, version.as_deref())?)
}

/// `open_release`: opens the release's location with the system's handler.
#[tauri::command]
pub async fn open_release<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, Shared>,
    id: String,
    version: String,
    product: Option<String>,
    artifact: Option<String>,
) -> Out<()> {
    text_ok("watch id", &id)?;
    text_ok("version", &version)?;
    let release = chosen_release(&state, &id, &version, product.as_deref(), artifact.as_deref())?;
    lifecycle::opener(&app).open(&release.location).map_err(CommandError::unavailable)
}

/// `download_release`: downloads or copies the release into the downloads folder, verifying its checksum.
#[tauri::command]
pub async fn download_release(
    state: State<'_, Shared>,
    id: String,
    version: String,
    product: Option<String>,
    artifact: Option<String>,
) -> Out<String> {
    text_ok("watch id", &id)?;
    text_ok("version", &version)?;
    let release = chosen_release(&state, &id, &version, product.as_deref(), artifact.as_deref())?;
    let folder = downloads_dir(&state);
    let result = spotter_sources::download::download(&state.http, &release, &folder, true).await;
    let outcome = result
        .as_ref()
        .map(|d| (d.path.display().to_string(), d.size, d.verified))
        .map_err(Clone::clone);
    let _ = state.engine.record_download(&id, &version, outcome);
    result.map(|d| d.path.display().to_string()).map_err(CommandError::unavailable)
}

/// The release a command acts on: the watch's own record of it (never a path from the webview), with the primary
/// file replaced by the named artifact when one is asked for.
fn chosen_release(
    state: &Shared,
    id: &str,
    version: &str,
    product: Option<&str>,
    artifact: Option<&str>,
) -> Result<spotter_core::release::Release, CommandError> {
    text_ok("product", product.unwrap_or(""))?;
    text_ok("artifact", artifact.unwrap_or(""))?;
    let release = state.engine.release_of_product(id, product, version)?;
    match artifact.filter(|a| !a.is_empty()) {
        None => Ok(release),
        Some(name) => release
            .with_artifact_named(name)
            .ok_or_else(|| CommandError::bad(format!("The release has no file called \"{name}\"."))),
    }
}

// ---- dashboard ----

/// `dashboard`.
#[tauri::command]
pub async fn dashboard(state: State<'_, Shared>, range: String) -> Out<DashboardDto> {
    text_ok("range", &range)?;
    Ok((&state.engine.dashboard(&range)?).into())
}

/// `events`.
#[tauri::command]
pub async fn events(state: State<'_, Shared>, limit: usize, before_ms: Option<i64>) -> Out<Vec<AppEventDto>> {
    Ok(state
        .engine
        .events(limit.clamp(1, MAX_EVENTS), before_ms)?
        .iter()
        .map(Into::into)
        .collect())
}

// ---- settings ----

/// `get_settings`.
#[tauri::command]
pub async fn get_settings(state: State<'_, Shared>) -> Out<SettingsDto> {
    Ok((&state.engine.settings()).into())
}

/// `save_settings`.
#[tauri::command]
pub async fn save_settings(state: State<'_, Shared>, settings: SettingsDto) -> Out<SettingsDto> {
    if let Some(f) = &settings.downloads_folder {
        text_ok("downloads folder", f)?;
    }
    Ok((&state.engine.save_settings(settings.into_settings())?).into())
}

/// `pause_notifications`.
#[tauri::command]
pub async fn pause_notifications(state: State<'_, Shared>, minutes: Option<u32>) -> Out<SettingsDto> {
    Ok((&state.engine.pause_notifications(minutes)?).into())
}

/// `get_autostart`.
#[tauri::command]
pub async fn get_autostart<R: Runtime>(app: AppHandle<R>) -> Out<bool> {
    app.autolaunch().is_enabled().map_err(|e| CommandError::internal(e.to_string()))
}

/// `set_autostart`: returns the state afterwards.
#[tauri::command]
pub async fn set_autostart<R: Runtime>(app: AppHandle<R>, enabled: bool) -> Out<bool> {
    let a = app.autolaunch();
    if enabled { a.enable() } else { a.disable() }
        .map_err(|e| CommandError::internal(format!("Start at login could not be changed: {e}")))?;
    a.is_enabled().map_err(|e| CommandError::internal(e.to_string()))
}

/// `open`: the configuration file, the logs or the downloads folder in the file manager.
#[tauri::command]
pub async fn open<R: Runtime>(app: AppHandle<R>, state: State<'_, Shared>, target: String) -> Out<()> {
    let opener = app.opener();
    let result = match target.as_str() {
        "config" => opener.reveal_item_in_dir(state.engine.config_path()),
        "logs" => {
            let dir = applog::dir().ok_or_else(|| CommandError::unavailable("The logs folder is not known on this machine."))?;
            let _ = std::fs::create_dir_all(&dir);
            opener.open_path(dir.display().to_string(), None::<&str>)
        }
        "downloads" => {
            let dir = downloads_dir(&state);
            let _ = std::fs::create_dir_all(&dir);
            opener.open_path(dir.display().to_string(), None::<&str>)
        }
        other => return Err(CommandError::bad(format!("\"{other}\" is not something Spotter can open."))),
    };
    result.map_err(|e| CommandError::unavailable(format!("It could not be opened: {e}")))
}

/// `choose_folder`: the native folder picker (`null` when cancelled).
#[tauri::command]
pub async fn choose_folder<R: Runtime>(app: AppHandle<R>) -> Out<Option<String>> {
    let picked = blocking(move || Ok(app.dialog().file().blocking_pick_folder())).await?;
    Ok(picked.and_then(|p| p.into_path().ok()).map(|p| p.display().to_string()))
}

/// `choose_program`: the native file picker for a program to run (`null` when cancelled).
#[tauri::command]
pub async fn choose_program<R: Runtime>(app: AppHandle<R>) -> Out<Option<String>> {
    let picked = blocking(move || Ok(app.dialog().file().blocking_pick_file())).await?;
    Ok(picked.and_then(|p| p.into_path().ok()).map(|p| p.display().to_string()))
}

// ---- presets, discovery, rendering, imports ----

/// `presets`.
#[tauri::command]
pub async fn presets(state: State<'_, Shared>) -> Out<Vec<PresetDto>> {
    Ok(state.engine.presets().iter().map(Into::into).collect())
}

/// `discover_feeds`.
#[tauri::command]
pub async fn discover_feeds(state: State<'_, Shared>, url: String) -> Out<Vec<DiscoveredFeedDto>> {
    text_ok("address", &url)?;
    Ok(state.engine.discover_feeds(&url).await?.iter().map(Into::into).collect())
}

/// `renderer_status`.
#[tauri::command]
pub async fn renderer_status(state: State<'_, Shared>) -> Out<RendererStatusDto> {
    let dir = state.data_dir.clone();
    blocking(move || Ok(spotter_render::renderer_status(&dir))).await
}

/// `install_renderer`: downloads the pinned, checksum-verified Chromium. Only ever runs because the user asked.
#[tauri::command]
pub async fn install_renderer(state: State<'_, Shared>) -> Out<RendererStatusDto> {
    spotter_render::install::install(&state.data_dir, &|_, _| {})
        .await
        .map_err(|e| CommandError::unavailable(e.to_string()))?;
    let dir = state.data_dir.clone();
    blocking(move || Ok(spotter_render::renderer_status(&dir))).await
}

/// `import_list`.
#[tauri::command]
pub async fn import_list(state: State<'_, Shared>, format: String, text: String) -> Out<ImportResultDto> {
    text_ok("format", &format)?;
    if text.len() > MAX_IMPORT_TEXT {
        return Err(CommandError::bad("The list is larger than 2 MiB."));
    }
    let imported = state.engine.import_list(&format, &text)?;
    Ok(ImportResultDto {
        drafts: imported.drafts.iter().map(Into::into).collect(),
        skipped: imported.skipped,
    })
}

// ---- Spotter's own updates ----

/// `app_update_status`.
#[tauri::command]
pub async fn app_update_status<R: Runtime>(app: AppHandle<R>) -> Out<AppUpdateStatusDto> {
    Ok(updater::status(&app))
}

/// `check_app_update`.
#[tauri::command]
pub async fn check_app_update<R: Runtime>(app: AppHandle<R>) -> Out<AppUpdateCheckDto> {
    updater::check(&app).await
}

/// `install_app_update`.
#[tauri::command]
pub async fn install_app_update<R: Runtime>(app: AppHandle<R>) -> Out<()> {
    updater::install(&app).await
}

// ---- windows ----

/// `show_main_window`: shows and focuses the main window, then asks it to show `route`.
#[tauri::command]
pub async fn show_main_window<R: Runtime>(app: AppHandle<R>, route: Option<String>) -> Out<()> {
    if let Some(r) = &route {
        if !tray::valid_route(r) {
            return Err(CommandError::bad("That page is not one Spotter can show."));
        }
    }
    let handle = app.clone();
    // Windows are created and shown on the main thread.
    app.run_on_main_thread(move || tray::show_main(&handle, route.as_deref()))
        .map_err(|e| CommandError::internal(e.to_string()))
}

/// `hide_tray`: closes the tray popup.
#[tauri::command]
pub async fn hide_tray<R: Runtime>(app: AppHandle<R>) -> Out<()> {
    let handle = app.clone();
    app.run_on_main_thread(move || tray::hide_popup(&handle))
        .map_err(|e| CommandError::internal(e.to_string()))
}

/// Runs blocking work off the async threads.
async fn blocking<T: Send + 'static>(work: impl FnOnce() -> Out<T> + Send + 'static) -> Out<T> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|e| CommandError::internal(format!("The work stopped unexpectedly: {e}")))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn webview_text_is_bounded() {
        assert!(text_ok("x", "fine").is_ok());
        assert!(text_ok("x", &"a".repeat(MAX_TEXT + 1)).is_err());
        assert!(text_ok("x", "nul\0byte").is_err());
    }

    #[test]
    fn secrets_are_bounded() {
        let mut s = Secrets::new();
        assert!(secrets_ok(&s).is_ok());
        s.insert("token".into(), "v".into());
        assert!(secrets_ok(&s).is_ok());
        s.insert("k".repeat(65), "v".into());
        assert!(secrets_ok(&s).is_err());
        let many: Secrets = (0..=MAX_SECRETS).map(|i| (format!("k{i}"), "v".into())).collect();
        assert!(secrets_ok(&many).is_err());
    }

    #[tokio::test]
    async fn blocking_work_returns_its_result_or_its_error() {
        assert_eq!(blocking(|| Ok(7)).await.unwrap(), 7);
        let e = blocking::<()>(|| Err(CommandError::bad("no"))).await.unwrap_err();
        assert_eq!(e.code, CommandErrorCode::BadRequest);
    }

    #[test]
    fn every_ui_command_has_a_handler() {
        let src = include_str!("lib.rs");
        let ts = include_str!("../../src/app/core/tauri-backend.ts");
        let mut seen = 0;
        for line in ts.lines() {
            if let Some(rest) = line.split("this.run").nth(1) {
                if let Some(name) = rest.split('\'').nth(1) {
                    assert!(src.contains(&format!("commands::{name},")), "no handler for {name}");
                    seen += 1;
                }
            }
        }
        assert!(seen >= 30, "found only {seen} commands");
    }
}
