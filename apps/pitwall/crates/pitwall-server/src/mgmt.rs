//! The management API Pitwall's own window uses (admin key only): one JSON call per UI backend method
//! (`POST /mgmt/v1/rpc/{method}`), a server-sent live feed and the playground stream. The DTOs are
//! `pitwall_core::mgmt` (mirrored by `api.types.ts`).

use crate::app::{lock, AppState, Meta};
use crate::auth::Principal;
use crate::http::gate;
use crate::paths::now_ms;
use axum::body::{Body, Bytes};
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use pitwall_core::api::{ApiFamily, ErrorKind};
use pitwall_core::chat::{ChatEvent, ChatRequest, Message, ModelRef};
use pitwall_core::config::UiSettings;
use pitwall_core::events::{EventKind, Severity};
use pitwall_core::ids::ModelId;
use pitwall_core::mgmt::{
    self, CommandError, DiskUsage, Download, DownloadState, EngineKind, LiveUpdate, PlaygroundEvent, PlaygroundRequest,
};
use pitwall_core::presets::{Known, PresetError, Presets};
use pitwall_core::runtimes::builds;
use pitwall_core::scheduler::JobKind;
use pitwall_core::slots::{self, Slots};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::sync::Arc;

type S = State<Arc<AppState>>;
type R<T> = Result<T, CommandError>;

/// Routes under `/mgmt/v1`.
pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/mgmt/v1/rpc/{method}", post(rpc))
        .route("/mgmt/v1/live", get(live))
        .route("/mgmt/v1/chat", post(chat))
}

fn err(code: mgmt::ErrorCode, m: impl Into<String>) -> CommandError {
    CommandError::new(code, m)
}

fn admin(st: &AppState, h: &HeaderMap) -> Result<(), Box<Response>> {
    match gate(st, h) {
        Ok(Principal::Admin) => Ok(()),
        Ok(_) => Err(Box::new(command_response(&err(
            mgmt::ErrorCode::Forbidden,
            "only Pitwall's own window may use the management API",
        )))),
        Err(e) => Err(Box::new(command_response(&err(mgmt::ErrorCode::Unauthorized, e.message)))),
    }
}

fn command_response(e: &CommandError) -> Response {
    (
        StatusCode::from_u16(e.code.status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
        axum::Json(e),
    )
        .into_response()
}

fn arg<T: DeserializeOwned>(a: &Value, name: &str) -> R<T> {
    serde_json::from_value(a.get(name).cloned().unwrap_or(Value::Null))
        .map_err(|e| err(mgmt::ErrorCode::BadRequest, format!("argument \"{name}\": {e}")))
}

/// Runs `work` as an operation the caller can cancel with `cancel_op`, named by the optional `opId` argument. `None`
/// when it was cancelled (the work is dropped); without an `opId` it runs to the end as before.
async fn cancellable<T>(st: &AppState, a: &Value, work: impl std::future::Future<Output = R<T>>) -> R<Option<T>> {
    let Some(op) = arg::<Option<String>>(a, "opId")? else {
        return work.await.map(Some);
    };
    struct Forget<'a>(&'a AppState, String);
    impl Drop for Forget<'_> {
        fn drop(&mut self) {
            lock(&self.0.ops).end(&self.1);
        }
    }
    let notify = lock(&st.ops).begin(&op, now_ms());
    let _forget = Forget(st, op);
    tokio::select! {
        r = work => r.map(Some),
        () = notify.notified() => Ok(None),
    }
}

/// After a load was cancelled: the half-started engine went with the dropped load. A model this very call finished loading
/// in the same instant is unloaded, so a cancelled load never leaves behind a model it started; one that another request or
/// job loaded (or is using: `unload` refuses a busy model) is left alone.
pub fn undo_cancelled_load(st: &AppState, id: &ModelId, loaded_here: bool) {
    if loaded_here {
        st.serving.unload(id, pitwall_core::memory::ReleaseReason::Manual);
    }
}

fn model(a: &Value, name: &str) -> R<ModelId> {
    let s: String = arg(a, name)?;
    ModelId::parse(&s).map_err(|e| err(mgmt::ErrorCode::BadRequest, e.to_string()))
}

/// An optional slot count argument: absent or null is `None`; out of range is a bad request.
fn parse_slots_opt(a: &Value, name: &str) -> R<Option<Slots>> {
    let n: Option<u32> = arg(a, name)?;
    n.map(|n| Slots::new(n).map_err(|e| err(mgmt::ErrorCode::BadRequest, e.to_string())))
        .transpose()
}

fn ok<T: serde::Serialize>(v: T) -> R<Value> {
    serde_json::to_value(v).map_err(|e| err(mgmt::ErrorCode::Internal, e.to_string()))
}

async fn rpc(State(st): S, h: HeaderMap, Path(method): Path<String>, body: Bytes) -> Response {
    if let Err(r) = admin(&st, &h) {
        return *r;
    }
    let args: Value = if body.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&body).unwrap_or(Value::Null)
    };
    match call(&st, &method, &args).await {
        Ok(v) => axum::Json(v).into_response(),
        Err(e) => command_response(&e),
    }
}

async fn call(st: &Arc<AppState>, method: &str, a: &Value) -> R<Value> {
    let now = now_ms();
    match method {
        "overview" => ok(overview(st)),
        "history" => {
            let range: mgmt::HistoryRange = arg(a, "range")?;
            let (step_ms, points, by_app, by_model) = lock(&st.metrics).history(range, now, 240);
            let events = lock(&st.events).between(now.saturating_sub(range.span()), now);
            ok(mgmt::History {
                range,
                step_ms,
                points,
                requests_by_app: by_app,
                requests_by_model: by_model,
                events,
            })
        }
        "usage_series" => {
            let range: mgmt::UsageRange = arg(a, "range")?;
            let app: Option<String> = arg(a, "appId")?;
            ok(lock(&st.usage).series(range, now, app.as_deref()))
        }
        "app_detail" => {
            let id: String = arg(a, "appId")?;
            let Some(app) = apps(st).into_iter().find(|x| x.id == id) else {
                return Err(err(mgmt::ErrorCode::NotFound, "no such app"));
            };
            let month = lock(&st.usage).month(now, &id);
            let limits = st.limit_statuses(&id, &app.permissions.limits, now);
            let recent = lock(&st.requests).iter().filter(|r| r.app_id == id).take(20).cloned().collect();
            ok(mgmt::AppDetail {
                app,
                month,
                limits,
                recent,
            })
        }
        "events" => {
            let limit: usize = arg(a, "limit")?;
            ok(lock(&st.events).recent(limit.min(500)))
        }
        "stop_serving" => {
            let n = st.serving.stop_all(std::time::Duration::from_secs(10)).await;
            st.push(&LiveUpdate::Server {
                server: overview(st).server,
            });
            ok(json!({ "unloaded": n }))
        }
        "set_serving" => {
            let paused: bool = arg(a, "paused")?;
            st.serving.set_paused(paused);
            st.event(
                EventKind::ServingChanged,
                Severity::Info,
                if paused {
                    "Serving paused".into()
                } else {
                    "Serving resumed".into()
                },
            );
            ok(())
        }
        "release_memory" => ok(st.serving.release_all()),
        "cache_info" => ok(st.cache_info()),
        "clear_cache" => {
            let scope: pitwall_core::cache::ClearRequest = arg(a, "scope")?;
            let mut n = st.caches.clear(&scope.scope());
            if matches!(scope, pitwall_core::cache::ClearRequest::All) {
                n += crate::warm::drop_prefix(&st.dirs.slots(), "", None);
            }
            st.event(EventKind::PresetChanged, Severity::Info, format!("Cleared {n} cached item(s)."));
            ok(n)
        }
        "apply_preset" => {
            let p: mgmt::Preset = arg(a, "preset")?;
            let mut s = st.cfg();
            s.apply_preset(p);
            let saved = st.save_settings(s).map_err(|e| err(mgmt::ErrorCode::BadRequest, e))?;
            st.event(EventKind::PresetChanged, Severity::Info, format!("Memory preset: {p:?}"));
            ok(saved.ui())
        }
        "cancel_job" => {
            let id: String = arg(a, "id")?;
            let id: u64 = id.parse().map_err(|_| err(mgmt::ErrorCode::BadRequest, "not a job id"))?;
            match st.serving.cancel(id) {
                pitwall_core::scheduler::Cancelled::Unknown => Err(err(mgmt::ErrorCode::NotFound, "no such job")),
                _ => ok(()),
            }
        }
        "local_models" => {
            let s = st.cfg();
            let hot = st.serving.hot_ram().0;
            let room = crate::system::room_from(&st.serving.memory_now(), &st.hardware, hot);
            let v: Vec<mgmt::LocalModel> = st
                .library
                .local()
                .iter()
                .map(|f| {
                    crate::library::to_dto(
                        f,
                        st.serving.loaded_slots(&f.id),
                        &s.prefs(&f.id),
                        (s.models.default_context, s.models.default_slots),
                        &room,
                    )
                })
                .collect();
            ok(v)
        }
        "search_models" => {
            let q: String = arg(a, "query")?;
            ok(st.library.search(&q).await.map_err(download_failed)?)
        }
        "repo_files" => {
            let repo: String = arg(a, "repo")?;
            let s = st.cfg();
            let room = crate::system::room_from(&st.serving.memory_now(), &st.hardware, st.serving.hot_ram().0);
            ok(st
                .library
                .repo(&repo, s.models.default_context, &room)
                .await
                .map_err(download_failed)?)
        }
        "download_model" => {
            let repo: String = arg(a, "repo")?;
            let file: String = arg(a, "file")?;
            let projector: Option<String> = arg(a, "projector")?;
            download_model(st, &repo, &file, projector.as_deref()).await
        }
        "downloads" => ok(downloads(st)),
        "cancel_download" => {
            let id: String = arg(a, "id")?;
            let hit = st.library.cancel(&id) || lock(&st.install_cancel).get(&id).map(crate::egress::CancelFlag::cancel).is_some();
            if hit {
                ok(())
            } else {
                Err(err(mgmt::ErrorCode::NotFound, "no such download"))
            }
        }
        "delete_model" => {
            let id = model(a, "id")?;
            st.serving.unload(&id, pitwall_core::memory::ReleaseReason::Manual);
            st.serving.retry(&id);
            st.library.delete(&id).map_err(|e| err(mgmt::ErrorCode::NotFound, e.to_string()))?;
            st.invalidate_model(&id);
            ok(())
        }
        "load_model" => {
            let id = model(a, "id")?;
            st.serving.retry(&id);
            // Set only when this call's own `ensure` started the engine.
            let loaded_here = std::sync::atomic::AtomicBool::new(false);
            let load = async {
                st.serving
                    .ensure(&id)
                    .await
                    .map(|(_busy, cold)| loaded_here.store(cold.is_some(), std::sync::atomic::Ordering::SeqCst))
                    .map_err(|e| {
                        err(
                            if e.kind == ErrorKind::InsufficientMemory {
                                mgmt::ErrorCode::InsufficientMemory
                            } else {
                                mgmt::ErrorCode::NotAvailable
                            },
                            e.message,
                        )
                    })
            };
            match cancellable(st, a, load).await? {
                Some(()) => ok(()),
                None => {
                    undo_cancelled_load(st, &id, loaded_here.load(std::sync::atomic::Ordering::SeqCst));
                    ok(json!({ "cancelled": true }))
                }
            }
        }
        "cancel_op" => {
            let op: String = arg(a, "opId")?;
            // A cancel can arrive before its operation does: a short-lived permit stops it as it starts (see `Ops`).
            lock(&st.ops).cancel(&op, now_ms());
            ok(())
        }
        "unload_model" => {
            let id = model(a, "id")?;
            if st.serving.unload(&id, pitwall_core::memory::ReleaseReason::Manual) {
                ok(())
            } else {
                Err(err(
                    mgmt::ErrorCode::Conflict,
                    "the model is not loaded, or it is serving a request right now",
                ))
            }
        }
        "pin_model" => {
            let id = model(a, "id")?;
            let pinned: bool = arg(a, "pinned")?;
            update_prefs(st, &id, |p| p.pinned = pinned)?;
            ok(())
        }
        "set_model_idle" => {
            let id = model(a, "id")?;
            let secs: Option<u64> = arg(a, "idleSecs")?;
            update_prefs(st, &id, |p| p.idle_secs = secs)?;
            ok(())
        }
        "set_model_slots" => {
            let id = model(a, "id")?;
            let slots = parse_slots_opt(a, "slots")?;
            if !st.library.local().iter().any(|f| f.id == id) {
                return Err(err(mgmt::ErrorCode::NotFound, format!("the model {id} is not installed")));
            }
            update_prefs(st, &id, |p| p.slots = slots)?;
            let now = st.cfg().slots_for(&id);
            let loaded = st.serving.loaded_slots(&id);
            ok(mgmt::SlotsChange {
                slots: now.get(),
                loaded_slots: loaded.map(Slots::get),
                applies_on_next_load: loaded.is_some_and(|l| l != now),
            })
        }
        "model_fit" => {
            let id = model(a, "id")?;
            let s = st.cfg();
            let f = st
                .library
                .local()
                .into_iter()
                .find(|f| f.id == id)
                .ok_or_else(|| err(mgmt::ErrorCode::NotFound, format!("the model {id} is not installed")))?;
            let context: Option<u32> = arg(a, "context")?;
            let context = match context {
                Some(c) if !(slots::MIN_CONTEXT..=slots::MAX_CONTEXT).contains(&c) => {
                    return Err(err(
                        mgmt::ErrorCode::BadRequest,
                        format!(
                            "the context must be between {} and {} tokens",
                            slots::MIN_CONTEXT,
                            slots::MAX_CONTEXT
                        ),
                    ))
                }
                Some(c) => c,
                None => s.prefs(&id).context.unwrap_or(s.models.default_context),
            };
            let chosen = parse_slots_opt(a, "slots")?.unwrap_or_else(|| s.slots_for(&id));
            let room = crate::system::room_from(&st.serving.memory_now(), &st.hardware, st.serving.hot_ram().0);
            let trained = f.info.context_length.map(|c| c.min(u64::from(u32::MAX)) as u32);
            ok(slots::solve(
                Some(&crate::library::size_of(&f)),
                context,
                chosen,
                trained,
                f.info.kv_is_exact(),
                &room,
            ))
        }
        "disk_usage" => ok(disk_usage(st)),
        "engines" => ok(engines(st)),
        "retry_engine_setup" => {
            crate::engine_setup::auto_install(st, Arc::new(st.llama.clone()));
            ok(crate::engine_setup::dto(st))
        }
        "install_build" => {
            let id: String = arg(a, "buildId")?;
            install_build(st, &id)
        }
        "remove_build" => {
            let id: String = arg(a, "buildId")?;
            st.llama.remove(&id).map_err(|e| err(mgmt::ErrorCode::NotFound, e.to_string()))?;
            st.invalidate_engine();
            ok(())
        }
        "select_build" => {
            let c: String = arg(a, "choice")?;
            let before = crate::engine_pins::current(st);
            let s = st.cfg();
            let mut s = s;
            s.engines.build_choice = c;
            st.save_settings(s).map_err(|e| err(mgmt::ErrorCode::BadRequest, e))?;
            crate::engine_pins::record_change(st, before);
            ok(())
        }
        "engine_pins" => ok(crate::engine_pins::state(st)),
        "set_global_pin" => {
            let b: Option<String> = arg(a, "buildId")?;
            ok(crate::engine_pins::set_global(st, b)?)
        }
        "set_model_pin" => {
            let m = model(a, "modelId")?;
            let b: Option<String> = arg(a, "buildId")?;
            ok(crate::engine_pins::set_model(st, &m, b)?)
        }
        "rollback_build" => ok(crate::engine_pins::rollback(st)?),
        "build_capabilities" => ok(crate::engine_pins::capabilities(st)),
        "packs" => {
            let s = st.cfg();
            let room = crate::system::room_from(&st.serving.memory_now(), &st.hardware, st.serving.hot_ram().0);
            let tag: Option<String> = arg(a, "tag").unwrap_or(None);
            let rec = pitwall_core::packs::recommend(&st.packs.models, tag.as_deref(), &room);
            ok(json!({
                "problems": st.packs.problems,
                "warnings": st.packs.warnings,
                "recommendations": rec.iter().map(|r| json!({
                    "id": r.model.id, "name": r.model.name, "repo": r.model.repo, "file": r.variant.file, "quant": r.variant.quant,
                    "sizeBytes": r.variant.size_bytes, "tags": r.model.tags, "fit": r.fit, "contextTokens": r.model.context.min(s.models.default_context)
                })).collect::<Vec<_>>(),
            }))
        }
        "remove_leftover" => {
            let id: String = arg(a, "id")?;
            crate::leftovers::remove(&st.dirs, &id).map_err(|e| err(mgmt::ErrorCode::NotFound, e))?;
            ok(())
        }
        "apps" => ok(apps(st)),
        "create_app" => {
            let name: String = arg(a, "name")?;
            let k = st
                .auth
                .create_app(&name, Default::default(), now)
                .map_err(|e| err(mgmt::ErrorCode::BadRequest, e.to_string()))?;
            let info = apps(st)
                .into_iter()
                .find(|x| x.id == k.app_id)
                .ok_or_else(|| err(mgmt::ErrorCode::Internal, "app vanished"))?;
            // The UI shows the secret once through `create_key`; creating an app returns its info with the first key's prefix.
            lock(&st.new_secrets).insert(k.app_id.clone(), k.secret.clone());
            ok(info)
        }
        "delete_app" => {
            let id: String = arg(a, "id")?;
            if st.auth.remove_app(&id) {
                ok(())
            } else {
                Err(err(mgmt::ErrorCode::NotFound, "no such app"))
            }
        }
        "create_key" => {
            let id: String = arg(a, "appId")?;
            // A key created together with its app is handed over here, once.
            let pending = lock(&st.new_secrets).remove(&id);
            if let Some(secret) = pending {
                if let Some(k) = apps(st).into_iter().find(|x| x.id == id).and_then(|x| x.keys.first().cloned()) {
                    return ok(mgmt::CreatedKey {
                        app_id: id,
                        key: k,
                        secret,
                    });
                }
            }
            ok(st
                .auth
                .add_key(&id, now)
                .map_err(|e| err(mgmt::ErrorCode::NotFound, e.to_string()))?)
        }
        "revoke_key" => {
            let (app, key): (String, String) = (arg(a, "appId")?, arg(a, "keyId")?);
            if st.auth.revoke_key(&app, &key) {
                ok(())
            } else {
                Err(err(mgmt::ErrorCode::NotFound, "no such key"))
            }
        }
        "set_permissions" => {
            let app: String = arg(a, "appId")?;
            let p: mgmt::AppPermissions = arg(a, "permissions")?;
            p.limits.validate().map_err(|m| err(mgmt::ErrorCode::BadRequest, m))?;
            if !st.auth.set_permissions(&app, p) {
                return Err(err(mgmt::ErrorCode::NotFound, "no such app"));
            }
            apps(st)
                .into_iter()
                .find(|x| x.id == app)
                .map(ok)
                .unwrap_or_else(|| Err(err(mgmt::ErrorCode::NotFound, "no such app")))
        }
        "pairings" => ok(st.auth.pairings(now)),
        "approve_pairing" => {
            let id: String = arg(a, "id")?;
            let p: mgmt::AppPermissions = arg(a, "permissions")?;
            p.limits.validate().map_err(|m| err(mgmt::ErrorCode::BadRequest, m))?;
            let decided = st.auth.decide_pairing(&id, Some(p), now);
            st.push(&LiveUpdate::Pairing {
                pairings: st.auth.pairings(now),
            });
            if !decided.map_err(|e| err(mgmt::ErrorCode::Internal, e.to_string()))? {
                return Err(err(
                    mgmt::ErrorCode::NotFound,
                    "that pairing request expired or was already answered",
                ));
            }
            ok(())
        }
        "deny_pairing" => {
            let id: String = arg(a, "id")?;
            // A denial changes nothing on disk, so it cannot fail to save.
            let _ = st.auth.decide_pairing(&id, None, now);
            st.push(&LiveUpdate::Pairing {
                pairings: st.auth.pairings(now),
            });
            ok(())
        }
        "requests" => {
            let limit: usize = arg(a, "limit")?;
            ok(lock(&st.requests).iter().take(limit.min(500)).cloned().collect::<Vec<_>>())
        }
        "request_content" => {
            let id: String = arg(a, "id")?;
            lock(&st.contents).iter().find(|c| c.id == id).cloned().map_or_else(
                || {
                    Err(err(
                        mgmt::ErrorCode::NotFound,
                        "this request's content is not kept. Turn on \"Keep request content\" in Settings > Privacy to inspect prompts on this machine (only requests made afterwards are kept, the latest 50).",
                    ))
                },
                ok,
            )
        }
        "benchmarks" => ok(lock(&st.benchmarks).clone()),
        "run_benchmark" => {
            let id = model(a, "modelId")?;
            // Dropping the run cancels its generation; nothing is recorded, so the stored results stay as they were.
            match cancellable(st, a, run_benchmark(st, id)).await? {
                Some(result) => Ok(result),
                None => ok(json!({ "cancelled": true })),
            }
        }
        "update_check_status" => ok(st.update_status()),
        "update_notices" => ok(st.update_notices()),
        "check_updates_now" => {
            // Up to 40 repositories are asked one after another: a job on the app side, so it can be cancelled by `opId`.
            // Dropping the check releases its guard and writes nothing (the state is saved when it finishes).
            match cancellable(st, a, async { Ok(st.run_update_check().await.result) }).await? {
                Some(result) => ok(result),
                None => ok(json!({ "cancelled": true })),
            }
        }
        "set_update_check" => {
            let enabled: bool = arg(a, "enabled")?;
            let schedule: mgmt::UpdateSchedule = arg(a, "schedule")?;
            let mut s = st.cfg();
            s.models.update_check = mgmt::UpdateCheckSettings { enabled, schedule };
            st.save_settings(s).map_err(|e| err(mgmt::ErrorCode::BadRequest, e))?;
            // Turning it on starts the first check now instead of at the next look at the schedule.
            if enabled {
                let me = st.clone();
                tokio::spawn(async move {
                    let _ = me.scheduled_update_tick().await;
                });
            }
            ok(st.update_status())
        }
        "dismiss_update" => {
            let id: String = arg(a, "id")?;
            let days: Option<u32> = arg(a, "snoozeDays")?;
            st.updates.dismiss(&id, days, now).map_err(|e| {
                let code = match e {
                    pitwall_core::update_state::DismissError::NotFound(_) => mgmt::ErrorCode::NotFound,
                    pitwall_core::update_state::DismissError::BadSnooze(_) => mgmt::ErrorCode::BadRequest,
                };
                err(code, e.to_string())
            })?;
            ok(st.update_notices())
        }
        "presets" => ok(preset_entries(st)),
        "create_preset" => {
            let draft: mgmt::PresetDraft = arg(a, "preset")?;
            edit_presets(st, |ps, known| ps.create(draft.parse()?, known))
        }
        "update_preset" => {
            let alias: String = arg(a, "alias")?;
            let draft: mgmt::PresetDraft = arg(a, "preset")?;
            edit_presets(st, |ps, known| ps.update(&alias, draft.parse()?, known))
        }
        "delete_preset" => {
            let alias: String = arg(a, "alias")?;
            edit_presets(st, |ps, _| ps.delete(&alias))
        }
        "resolve_preset" => {
            let draft: mgmt::PresetDraft = arg(a, "preset")?;
            let editing: Option<String> = arg(a, "editing")?;
            ok(preview_preset(st, draft, editing.as_deref()))
        }
        "settings" => ok(st.cfg().ui()),
        "save_settings" => {
            let ui: UiSettings = arg(a, "settings")?;
            let s = st.cfg().with_ui(ui).map_err(|e| err(mgmt::ErrorCode::BadRequest, e))?;
            let saved = st.save_settings(s).map_err(|e| err(mgmt::ErrorCode::BadRequest, e))?.ui();
            ok(saved)
        }
        _ => Err(err(mgmt::ErrorCode::NotImplemented, format!("unknown call \"{method}\""))),
    }
}

/// A failed outbound call is never "the local server is not running": an unreachable or failing host is "upstream"
/// (the CLI exits 4), a blocked URL is a bad request, and a local write failure is ours.
fn download_failed(e: crate::egress::DownloadError) -> CommandError {
    use crate::egress::DownloadError as D;
    let code = match &e {
        D::Egress(_) => mgmt::ErrorCode::BadRequest,
        D::Network(_) | D::Status(..) | D::Checksum { .. } => mgmt::ErrorCode::Upstream,
        D::Io(_) => mgmt::ErrorCode::Internal,
        D::Cancelled => mgmt::ErrorCode::Conflict,
    };
    let hint = if matches!(e, D::Egress(_)) {
        " Use a huggingface.co or github.com address."
    } else {
        ""
    };
    err(code, format!("{}.{hint}", e.to_string().trim_end_matches('.')))
}

fn update_prefs(st: &AppState, id: &ModelId, f: impl FnOnce(&mut pitwall_core::config::ModelPrefs)) -> R<()> {
    let mut s = st.cfg();
    let mut p = s.prefs(id);
    f(&mut p);
    s.models.prefs.insert(id.clone(), p);
    st.save_settings(s).map(|_| ()).map_err(|e| err(mgmt::ErrorCode::BadRequest, e))
}

fn downloads(st: &AppState) -> Vec<Download> {
    let mut v = st.library.downloads();
    v.extend(lock(&st.installs).values().filter(|d| d.id.starts_with("eng-")).cloned());
    v.sort_by_key(|d| std::cmp::Reverse(d.started_at));
    v
}

async fn download_model(st: &Arc<AppState>, repo: &str, file: &str, projector: Option<&str>) -> R<Value> {
    let s = st.cfg();
    let room = crate::system::room_from(&st.serving.memory_now(), &st.hardware, st.serving.hot_ram().0);
    let r = st
        .library
        .repo(repo, s.models.default_context, &room)
        .await
        .map_err(download_failed)?;
    let q = r
        .variants
        .iter()
        .find(|v| v.file == file)
        .ok_or_else(|| err(mgmt::ErrorCode::NotFound, format!("{file} is not in {repo}")))?;
    let free = crate::system::free_disk(&st.dirs.root);
    if free > 0 && q.size_bytes + (1 << 30) > free {
        return Err(err(
            mgmt::ErrorCode::Conflict,
            format!(
                "not enough disk space: {:.1} GB needed, {:.1} GB free",
                q.size_bytes as f64 / 1e9,
                free as f64 / 1e9
            ),
        ));
    }
    // Rebuild the core variant (parts/sha) from the tree again: the DTO carries only the first file.
    let tree = {
        let id = pitwall_core::hf::RepoId::parse(repo).map_err(|e| err(mgmt::ErrorCode::BadRequest, e))?;
        let url = pitwall_core::hf::tree_url(&id).map_err(|e| err(mgmt::ErrorCode::BadRequest, e.to_string()))?;
        pitwall_core::hf::parse_tree(&st.egress.get_json(&url, 8 << 20).await.map_err(download_failed)?)
            .map_err(|e| err(mgmt::ErrorCode::Upstream, e))?
    };
    let mut want: Vec<&pitwall_core::hf::Variant> = tree.variants.iter().filter(|v| v.file == file).collect();
    if let Some(p) = projector {
        want.extend(tree.projectors.iter().filter(|v| v.file == p));
    }
    if want.is_empty() {
        return Err(err(mgmt::ErrorCode::NotFound, format!("{file} is not in {repo}")));
    }
    let mut first: Option<Download> = None;
    for v in want {
        let st2 = st.clone();
        let (repo_name, file_name) = (repo.to_string(), v.file.clone());
        // Models wait for llama.cpp while it installs itself ("Queued"); the engine in use says when it is ready.
        let engine = st.engine.clone();
        let gate: crate::engines::BoxFuture<'static, Result<(), String>> =
            Box::pin(async move { engine.ready().await.map_err(|e| e.to_string()) });
        let id = st
            .library
            .download_after(Some(gate), repo, v, move |d| {
                st2.push(&LiveUpdate::Download { download: d.clone() });
                if matches!(d.state, DownloadState::Done) {
                    st2.event(EventKind::Download, Severity::Info, format!("Downloaded {}", d.name));
                    st2.updates.downloaded(&repo_name, &file_name);
                }
            })
            .map_err(download_failed)?;
        if first.is_none() {
            first = st.library.downloads().into_iter().find(|d| d.id == id);
        }
    }
    ok(first)
}

fn disk_usage(st: &AppState) -> DiskUsage {
    fn size(p: &std::path::Path) -> u64 {
        let Ok(rd) = std::fs::read_dir(p) else {
            return std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
        };
        rd.filter_map(Result::ok)
            .map(|e| {
                if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    size(&e.path())
                } else {
                    e.metadata().map(|m| m.len()).unwrap_or(0)
                }
            })
            .sum()
    }
    DiskUsage {
        models_bytes: size(&st.dirs.models()),
        engines_bytes: size(&st.dirs.engines()),
        partial_bytes: size(&st.dirs.partial()),
        free_bytes: crate::system::free_disk(&st.dirs.root),
        unused: vec![],
        unused_after_days: 30,
    }
}

/// The llama.cpp page: its builds, the pinned catalog, running processes and leftovers of removed features.
fn engines(st: &AppState) -> Vec<mgmt::Engine> {
    let platform = st.llama.platform();
    let installed = st.llama.installed();
    let active = st.llama.active().map(|b| b.id);
    let suggested = st.llama.suggested();
    let loaded = st.serving.loaded();
    let mut catalog: Vec<mgmt::CatalogBuild> = st
        .llama
        .catalog()
        .builds(&platform)
        .iter()
        .map(|b| mgmt::CatalogBuild {
            id: b.id.clone(),
            label: builds::label(&b.id),
            tag: st.llama.catalog().tag.clone(),
            download_bytes: b.download_bytes(),
            suggested: suggested.as_deref() == Some(b.id.as_str()),
            installed: installed.iter().any(|i| i.id == b.id),
        })
        .collect();
    // The suggested build first (stable for the rest).
    catalog.sort_by_key(|c| !c.suggested);
    vec![mgmt::Engine {
        kind: EngineKind::LlamaCpp,
        name: "llama.cpp".into(),
        description: "Serves every model: CPU, Vulkan, CUDA and Metal builds chosen for this machine.".into(),
        availability: st.engine.availability(),
        build_choice: st.cfg().engines.build_choice.clone(),
        builds: installed
            .iter()
            .map(|b| mgmt::InstalledBuild {
                id: b.id.clone(),
                tag: b.tag.clone(),
                label: builds::label(&b.id),
                size_bytes: b.size,
                self_test: crate::engines::self_test_state(b),
                self_test_message: b.message.clone(),
                devices: b.devices.clone(),
                active: active.as_deref() == Some(b.id.as_str()),
                outdated: b.tag != st.llama.catalog().tag,
            })
            .collect(),
        catalog,
        processes: loaded
            .iter()
            .map(|m| mgmt::EngineProcess {
                pid: 0,
                model_id: m.model_id.clone(),
                health: mgmt::ProcessHealth::Healthy,
                restarts: 0,
                rss_bytes: m.ram_bytes,
                started_at: m.loaded_at,
            })
            .collect(),
        rollback_to: crate::engine_pins::state(st).rollback_to.map(|r| r.build_id),
        leftovers: crate::leftovers::find(&st.dirs),
    }]
}

fn install_build(st: &Arc<AppState>, id: &str) -> R<Value> {
    let platform = st.llama.platform();
    if st.llama.catalog().find(&platform, id).is_none() {
        return Err(err(mgmt::ErrorCode::NotFound, format!("there is no build \"{id}\" for {platform}")));
    }
    ok(crate::engine_setup::start_install(st, Arc::new(st.llama.clone()), id, false))
}

fn apps(st: &AppState) -> Vec<mgmt::AppInfo> {
    let now = now_ms();
    let usage = lock(&st.usage);
    st.auth.list(|app| usage.today(now, app), now)
}

fn overview(st: &Arc<AppState>) -> mgmt::Overview {
    let cfg = st.cfg();
    let port = st.port.load(std::sync::atomic::Ordering::SeqCst);
    let loaded = st.serving.loaded();
    mgmt::Overview {
        server: mgmt::ServerInfo {
            version: env!("CARGO_PKG_VERSION").into(),
            api_version: pitwall_core::API_VERSION,
            pid: std::process::id(),
            port: if port == 0 { cfg.server.port } else { port },
            started_at: st.started_at,
            serving: if st.serving.stopped() {
                mgmt::ServingState::Stopped
            } else if st.serving.paused() {
                mgmt::ServingState::Paused
            } else {
                mgmt::ServingState::Serving
            },
            lan: mgmt::LanStatus {
                enabled: false,
                url: None,
                cert_fingerprint: None,
            },
        },
        memory: st.memory_sample(),
        engines: engines(st)
            .iter()
            .map(|e| mgmt::EngineStatusTile {
                kind: e.kind,
                name: e.name.clone(),
                availability: e.availability.clone(),
                active_build: e.builds.iter().find(|b| b.active).map(|b| b.id.clone()),
                processes: loaded.len() as u32,
                unhealthy: 0,
            })
            .collect(),
        models: loaded,
        queue: st.queue_snapshot(),
        throughput: st.throughput(),
        apps: apps(st),
        preset: cfg.memory.preset,
        pairings: st.auth.pairings(now_ms()),
        issues: st.serving.issues(),
        engine_setup: crate::engine_setup::dto(st),
    }
}

async fn run_benchmark(st: &Arc<AppState>, id: ModelId) -> R<Value> {
    let req = ChatRequest {
        model: ModelRef::Named(id.to_string()),
        messages: vec![Message::text(
            pitwall_core::chat::Role::User,
            "Count from 1 to 60, separated by commas.",
        )],
        max_tokens: Some(96),
        temperature: Some(0.0),
        stream: true,
        ..Default::default()
    };
    let meta = Meta {
        family: ApiFamily::Extended,
        endpoint: "/mgmt/benchmark".into(),
        priority: Some(pitwall_core::scheduler::Priority::Background),
        extended: true,
        kind: JobKind::Benchmark,
        no_cache: true,
    };
    let mut run = st
        .start_chat(&Principal::Admin, meta, req)
        .await
        .map_err(|e| err(mgmt::ErrorCode::NotAvailable, e.message))?;
    let start = std::time::Instant::now();
    let (mut ttft, mut done): (
        Option<u64>,
        Option<(pitwall_core::chat::Usage, Option<pitwall_core::chat::Timings>)>,
    ) = (None, None);
    while let Some(ev) = run.rx.recv().await {
        let ev = ev.map_err(|e| err(mgmt::ErrorCode::Internal, e.0))?;
        run.rec.event(&ev);
        if ttft.is_none() && matches!(ev, ChatEvent::Delta(_)) {
            ttft = Some(start.elapsed().as_millis() as u64);
        }
        if let ChatEvent::Done { usage, timings, .. } = ev {
            run.rec.finish(usage, timings, None);
            done = Some((usage, timings));
        }
    }
    let (u, t) = done.ok_or_else(|| err(mgmt::ErrorCode::Internal, "the benchmark ended early"))?;
    let secs = start.elapsed().as_secs_f64().max(0.001);
    let result = mgmt::BenchmarkResult {
        model_id: id.to_string(),
        build: st.llama.active().map(|b| b.id).unwrap_or_default(),
        at: now_ms(),
        prompt_tps: t.map_or(0.0, |t| t.prompt_tps),
        gen_tps: t.map_or(f64::from(u.completion_tokens) / secs, |t| t.gen_tps),
        ttft_ms: ttft.unwrap_or(0),
        context_tokens: st
            .serving
            .loaded()
            .iter()
            .find(|m| m.model_id == id.as_str())
            .map_or(0, |m| m.context_tokens),
    };
    {
        let mut g = lock(&st.benchmarks);
        pitwall_core::benchmarks::upsert(&mut g, result.clone());
        st.serving.set_speeds(pitwall_core::benchmarks::speeds(&g));
        if let Err(e) = crate::paths::write_atomic(&st.dirs.benchmarks(), pitwall_core::benchmarks::to_json(&g).as_bytes()) {
            drop(g);
            st.event(
                EventKind::RequestFailed,
                Severity::Warn,
                format!("The benchmark result could not be saved ({e}); it is kept until Pitwall stops."),
            );
        }
    }
    ok(result)
}

async fn live(State(st): S, h: HeaderMap) -> Response {
    if let Err(r) = admin(&st, &h) {
        return *r;
    }
    let mut rx = st.live.subscribe();
    let stream = futures_util::stream::unfold(rx.resubscribe(), move |mut rx| async move {
        loop {
            match rx.recv().await {
                Ok(s) => return Some((Ok::<_, std::convert::Infallible>(Bytes::from(pitwall_core::sse::data(&s))), rx)),
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => return None,
            }
        }
    });
    let _ = &mut rx;
    Response::builder()
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from_stream(stream))
        .unwrap_or_default()
}

/// The neutral chat request for a playground request: text messages (with the tool calls and results of a tool loop), the
/// tools the caller offers and its tool choice.
fn playground_chat_request(req: &PlaygroundRequest) -> ChatRequest {
    use pitwall_core::chat::{Part, Role, Tool, ToolCall, ToolChoice};
    use pitwall_core::mgmt::PlaygroundToolChoice;
    ChatRequest {
        model: ModelRef::Named(req.model_id.clone()),
        messages: req
            .messages
            .iter()
            .map(|m| Message {
                role: m.role,
                parts: if m.content.is_empty() && m.role == Role::Assistant && !m.tool_calls.is_empty() {
                    vec![]
                } else {
                    vec![Part::Text(m.content.clone())]
                },
                tool_calls: m
                    .tool_calls
                    .iter()
                    .map(|c| ToolCall {
                        id: c.id.clone(),
                        name: c.name.clone(),
                        arguments: c.arguments.clone(),
                    })
                    .collect(),
                tool_call_id: m.tool_call_id.clone(),
            })
            .collect(),
        tools: req
            .tools
            .iter()
            .map(|t| Tool {
                name: t.name.clone(),
                description: t.description.clone(),
                parameters: t.parameters.clone(),
            })
            .collect(),
        tool_choice: match &req.tool_choice {
            PlaygroundToolChoice::Auto => ToolChoice::Auto,
            PlaygroundToolChoice::None => ToolChoice::None,
            PlaygroundToolChoice::Required => ToolChoice::Required,
            PlaygroundToolChoice::Named { name } => ToolChoice::Named(name.clone()),
        },
        temperature: Some(req.temperature),
        max_tokens: Some(req.max_tokens),
        stream: true,
        ..Default::default()
    }
}

fn playground_frame(e: &PlaygroundEvent) -> Bytes {
    Bytes::from(pitwall_core::sse::data(&serde_json::to_string(e).unwrap_or_default()))
}

/// How often the loading phase repeats while the model loads or the request waits for a slot.
const LOADING_TICK: std::time::Duration = std::time::Duration::from_secs(1);

/// Runs one playground chat and sends its frames: the response has already started, so the wait for a cold model load is a
/// labelled phase (ER-18) and not a silent one. Ends when the answer ends or when the client goes away (`tx` closed), which drops
/// the run and cancels the work.
async fn drive_playground_chat(st: Arc<AppState>, req: PlaygroundRequest, tx: tokio::sync::mpsc::Sender<Bytes>) {
    let started = std::time::Instant::now();
    log::info!(
        target: "pitwall_chat",
        "chat started: model {}, {} messages, {} tools",
        req.model_id,
        req.messages.len(),
        req.tools.len()
    );
    let model = req.model_id.clone();
    let mut outcome = ChatOutcome::ClientGone;
    run_playground_chat(st, req, tx, &mut outcome).await;
    let ms = started.elapsed().as_millis();
    match outcome {
        ChatOutcome::Ended {
            finish,
            prompt_tokens,
            completion_tokens,
            ttft_ms,
            cold_start_ms,
        } => log::info!(
            target: "pitwall_chat",
            "chat ended: model {model}, finish {finish:?}, {prompt_tokens} tokens in, {completion_tokens} out, ttft {ttft_ms} ms, cold start {cold_start_ms:?} ms, {ms} ms"
        ),
        ChatOutcome::Failed { code, cause } => {
            log::warn!(target: "pitwall_chat", "chat failed: model {model}, {code:?}, {ms} ms, cause: {cause}")
        }
        ChatOutcome::ClientGone => {
            log::info!(target: "pitwall_chat", "chat stopped: model {model}, the client went away, {ms} ms")
        }
    }
}

/// How a playground chat ended, for its log line (names, counts and times only; never the text).
enum ChatOutcome {
    Ended {
        finish: pitwall_core::mgmt::PlaygroundFinish,
        prompt_tokens: u32,
        completion_tokens: u32,
        ttft_ms: u64,
        cold_start_ms: Option<u64>,
    },
    Failed {
        code: mgmt::ErrorCode,
        cause: String,
    },
    /// The client closed the stream before the answer ended.
    ClientGone,
}

async fn run_playground_chat(st: Arc<AppState>, req: PlaygroundRequest, tx: tokio::sync::mpsc::Sender<Bytes>, outcome: &mut ChatOutcome) {
    use pitwall_core::chat::Finish;
    use pitwall_core::mgmt::{PlaygroundFinish, PlaygroundPhase};
    let started = std::time::Instant::now();
    let elapsed_ms = || u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let meta = Meta {
        family: ApiFamily::Extended,
        endpoint: "/mgmt/chat".into(),
        priority: Some(pitwall_core::scheduler::Priority::Interactive),
        extended: true,
        kind: JobKind::Chat,
        no_cache: true,
    };
    let send = |e: PlaygroundEvent| {
        let tx = tx.clone();
        async move { tx.send(playground_frame(&e)).await.is_ok() }
    };
    if !send(PlaygroundEvent::Phase {
        phase: PlaygroundPhase::Loading { elapsed_ms: 0 },
    })
    .await
    {
        return;
    }
    let start = st.start_chat(&Principal::Admin, meta, playground_chat_request(&req));
    tokio::pin!(start);
    let mut tick = tokio::time::interval(LOADING_TICK);
    tick.tick().await;
    let run = loop {
        tokio::select! {
            r = &mut start => break r,
            _ = tick.tick() => {
                if !send(PlaygroundEvent::Phase { phase: PlaygroundPhase::Loading { elapsed_ms: elapsed_ms() } }).await {
                    return;
                }
            }
            () = tx.closed() => return,
        }
    };
    let mut run = match run {
        Ok(r) => r,
        Err(e) => {
            let code = if e.kind == ErrorKind::InsufficientMemory {
                mgmt::ErrorCode::InsufficientMemory
            } else {
                mgmt::ErrorCode::NotAvailable
            };
            *outcome = ChatOutcome::Failed {
                code,
                cause: e.message.clone(),
            };
            send(PlaygroundEvent::Error {
                error: err(code, e.message),
            })
            .await;
            return;
        }
    };
    let cold = run.cold_start_ms;
    if !send(PlaygroundEvent::Phase {
        phase: PlaygroundPhase::Generating,
    })
    .await
    {
        return;
    }
    let mut ttft: Option<u64> = None;
    loop {
        let item = tokio::select! {
            item = run.rx.recv() => item,
            () = tx.closed() => return,
        };
        let event = match item {
            None => return,
            Some(Err(e)) => {
                run.rec.finish(Default::default(), None, Some(e.0.clone()));
                *outcome = ChatOutcome::Failed {
                    code: mgmt::ErrorCode::Internal,
                    cause: e.0.clone(),
                };
                send(PlaygroundEvent::Error {
                    error: err(mgmt::ErrorCode::Internal, e.0),
                })
                .await;
                return;
            }
            Some(Ok(e)) => e,
        };
        run.rec.event(&event);
        let out = match event {
            ChatEvent::Delta(text) => PlaygroundEvent::Delta { text },
            ChatEvent::Reasoning(text) => PlaygroundEvent::Reasoning { text },
            ChatEvent::ToolCall {
                index,
                id,
                name,
                arguments,
            } => PlaygroundEvent::ToolCall {
                index,
                id,
                name,
                arguments,
            },
            ChatEvent::Done { finish, usage, timings } => {
                run.rec.finish(usage, timings, None);
                let done = PlaygroundEvent::Done {
                    finish: match finish {
                        Finish::Stop => PlaygroundFinish::Stop,
                        Finish::Length => PlaygroundFinish::Length,
                        Finish::ToolCalls => PlaygroundFinish::ToolCalls,
                        Finish::Cancelled => PlaygroundFinish::Cancelled,
                    },
                    prompt_tokens: usage.prompt_tokens,
                    completion_tokens: usage.completion_tokens,
                    ttft_ms: ttft.unwrap_or_else(elapsed_ms),
                    gen_tps: timings.map_or(0.0, |t| t.gen_tps),
                    cold_start_ms: cold,
                };
                if let PlaygroundEvent::Done {
                    finish,
                    prompt_tokens,
                    completion_tokens,
                    ttft_ms,
                    cold_start_ms,
                    ..
                } = &done
                {
                    *outcome = ChatOutcome::Ended {
                        finish: *finish,
                        prompt_tokens: *prompt_tokens,
                        completion_tokens: *completion_tokens,
                        ttft_ms: *ttft_ms,
                        cold_start_ms: *cold_start_ms,
                    };
                }
                send(done).await;
                return;
            }
        };
        if ttft.is_none() {
            ttft = Some(elapsed_ms());
        }
        if !send(out).await {
            return;
        }
    }
}

async fn chat(State(st): S, h: HeaderMap, body: Bytes) -> Response {
    if let Err(r) = admin(&st, &h) {
        return *r;
    }
    let req: PlaygroundRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => return command_response(&err(mgmt::ErrorCode::BadRequest, e.to_string())),
    };
    let (tx, rx) = tokio::sync::mpsc::channel::<Bytes>(64);
    tokio::spawn(drive_playground_chat(st.clone(), req, tx));
    let stream = futures_util::stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|b| (Ok::<_, std::convert::Infallible>(b), rx))
    });
    Response::builder()
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from_stream(stream))
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------------------------------------------
// Presets (ADR 0014)

fn preset_error(e: PresetError) -> CommandError {
    let code = match e {
        PresetError::NotFound(_) => mgmt::ErrorCode::NotFound,
        PresetError::Duplicate(_) | PresetError::NameClash { .. } | PresetError::TooMany => mgmt::ErrorCode::Conflict,
        _ => mgmt::ErrorCode::BadRequest,
    };
    err(code, e.to_string())
}

fn preset_entries(st: &AppState) -> Vec<mgmt::PresetEntry> {
    st.serving
        .presets()
        .list()
        .iter()
        .map(|p| mgmt::PresetEntry {
            status: st.serving.preset_status(p),
            preset: p.clone(),
        })
        .collect()
}

/// Validates against the library, changes the presets, saves them (the change is kept only when the save worked) and
/// hands the new list to serving. Returns the saved preset (or the removed one) as JSON.
fn edit_presets(st: &AppState, change: impl FnOnce(&mut Presets, &Known) -> Result<mgmt::ModelPreset, PresetError>) -> R<Value> {
    let _gate = lock(&st.presets_gate);
    let before = st.serving.presets().list().to_vec();
    let models: Vec<ModelId> = st.library.local().iter().map(|f| f.id.clone()).collect();
    let mut next = st.serving.presets();
    let saved = change(&mut next, &Known { models: &models }).map_err(preset_error)?;
    st.save_presets(&next).map_err(|e| {
        err(
            mgmt::ErrorCode::Internal,
            format!("the presets could not be saved ({e}); check the disk and try again"),
        )
    })?;
    // Whatever a changed, renamed or deleted preset cached is dropped now.
    let after = next.list().to_vec();
    for p in before.iter().filter(|p| !after.contains(p)) {
        st.invalidate_preset(p.alias.as_str());
    }
    st.serving.set_presets(next);
    ok(saved)
}

/// The editor's live check: what is wrong with the draft, and what its target resolves to now.
fn preview_preset(st: &AppState, draft: mgmt::PresetDraft, editing: Option<&str>) -> mgmt::PresetPreview {
    let mut problem = None;
    // Parse the target alone first so a bad alias doesn't hide what the target resolves to.
    let target_only = mgmt::PresetDraft {
        alias: "preview".into(),
        ..draft.clone()
    };
    let status = match target_only.parse() {
        Ok(p) => Some(st.serving.preset_status(&p)),
        Err(e) => {
            problem = Some(e.to_string());
            None
        }
    };
    if problem.is_none() {
        let models: Vec<ModelId> = st.library.local().iter().map(|f| f.id.clone()).collect();
        let mut scratch = st.serving.presets();
        // The preset being edited is replaced by the draft, so it is not a duplicate of itself.
        if let Some(e) = editing {
            let _ = scratch.delete(e);
        }
        problem = draft
            .parse()
            .and_then(|p| scratch.create(p, &Known { models: &models }))
            .err()
            .map(|e| e.to_string());
    }
    mgmt::PresetPreview { problem, status }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::egress::DownloadError;
    use pitwall_core::mgmt::ErrorCode;

    #[test]
    fn download_failures_are_not_reported_as_a_stopped_server() {
        let net = download_failed(DownloadError::Network("cannot reach huggingface.co: down".into()));
        assert_eq!(net.code, ErrorCode::Upstream);
        assert!(net.message.contains("huggingface.co"));
        assert_eq!(download_failed(DownloadError::Status(503, "")).code, ErrorCode::Upstream);
        let sum = DownloadError::Checksum {
            expected: "a".into(),
            actual: "b".into(),
        };
        assert_eq!(download_failed(sum).code, ErrorCode::Upstream);
        assert_eq!(download_failed(DownloadError::Io("disk".into())).code, ErrorCode::Internal);
        assert_eq!(download_failed(DownloadError::Cancelled).code, ErrorCode::Conflict);
        let blocked = download_failed(DownloadError::Egress(pitwall_core::privacy::EgressError::NotAllowed("x".into())));
        assert_eq!(blocked.code, ErrorCode::BadRequest);
        assert!(
            blocked.message.contains("x is not allowed") && blocked.message.contains("huggingface.co"),
            "{}",
            blocked.message
        );
    }
}
