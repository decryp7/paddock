//! The management API Pitwall's own window uses (admin key only): one JSON call per UI backend method
//! (`POST /mgmt/v1/rpc/{method}`), a server-sent live feed and the playground stream. The DTOs are
//! `pitwall_core::mgmt` (mirrored by `api.types.ts`).

use crate::app::{lock, AppState, Meta};
use crate::auth::Principal;
use crate::engines::Engine as _;
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
    self, CommandError, DiskUsage, Download, DownloadKind, DownloadState, EngineKind, LiveUpdate, PlaygroundEvent, PlaygroundRequest,
};
use pitwall_core::runtimes::builds;
use pitwall_core::scheduler::JobKind;
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

fn model(a: &Value, name: &str) -> R<ModelId> {
    let s: String = arg(a, name)?;
    ModelId::parse(&s).map_err(|e| err(mgmt::ErrorCode::BadRequest, e.to_string()))
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
        "events" => {
            let limit: usize = arg(a, "limit")?;
            ok(lock(&st.events).recent(limit.min(500)))
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
            let room = crate::system::room(&st.hardware, hot);
            let v: Vec<mgmt::LocalModel> = st
                .library
                .local()
                .iter()
                .map(|f| crate::library::to_dto(f, st.serving.is_loaded(&f.id), &s.prefs(&f.id), s.models.default_context, &room))
                .collect();
            ok(v)
        }
        "search_models" => {
            let q: String = arg(a, "query")?;
            ok(st.library.search(&q).await.map_err(offline_or)?)
        }
        "repo_files" => {
            let repo: String = arg(a, "repo")?;
            let s = st.cfg();
            let room = crate::system::room(&st.hardware, st.serving.hot_ram().0);
            ok(st.library.repo(&repo, s.models.default_context, &room).await.map_err(offline_or)?)
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
            st.library.delete(&id).map_err(|e| err(mgmt::ErrorCode::NotFound, e.to_string()))?;
            ok(())
        }
        "load_model" => {
            let id = model(a, "id")?;
            st.serving.ensure(&id).await.map_err(|e| {
                err(
                    if e.kind == ErrorKind::InsufficientMemory {
                        mgmt::ErrorCode::InsufficientMemory
                    } else {
                        mgmt::ErrorCode::NotAvailable
                    },
                    e.message,
                )
            })?;
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
        "disk_usage" => ok(disk_usage(st)),
        "engines" => ok(engines(st)),
        "install_build" => {
            let id: String = arg(a, "buildId")?;
            install_build(st, &id)
        }
        "remove_build" => {
            let id: String = arg(a, "buildId")?;
            st.llama.remove(&id).map_err(|e| err(mgmt::ErrorCode::NotFound, e.to_string()))?;
            ok(())
        }
        "select_build" => {
            let c: String = arg(a, "choice")?;
            let mut s = st.cfg();
            s.engines.previous_build = st.llama.active().map(|b| b.id);
            s.engines.build_choice = c;
            st.save_settings(s).map_err(|e| err(mgmt::ErrorCode::BadRequest, e))?;
            ok(())
        }
        "rollback_build" => {
            let mut s = st.cfg();
            let prev = s
                .engines
                .previous_build
                .take()
                .ok_or_else(|| err(mgmt::ErrorCode::NotFound, "there is no previous build to go back to"))?;
            s.engines.build_choice = prev;
            st.save_settings(s).map_err(|e| err(mgmt::ErrorCode::BadRequest, e))?;
            ok(())
        }
        "packs" => {
            let s = st.cfg();
            let room = crate::system::room(&st.hardware, st.serving.hot_ram().0);
            let tag: Option<String> = arg(a, "tag").unwrap_or(None);
            let rec = pitwall_core::packs::recommend(&st.packs.models, tag.as_deref(), &room);
            ok(json!({
                "engines": st.packs.engines.iter().map(|e| json!({"id": e.id, "name": e.name, "version": e.version})).collect::<Vec<_>>(),
                "problems": st.packs.problems,
                "warnings": st.packs.warnings,
                "recommendations": rec.iter().map(|r| json!({
                    "id": r.model.id, "name": r.model.name, "repo": r.model.repo, "file": r.variant.file, "quant": r.variant.quant,
                    "sizeBytes": r.variant.size_bytes, "tags": r.model.tags, "fit": r.fit, "contextTokens": r.model.context.min(s.models.default_context)
                })).collect::<Vec<_>>(),
            }))
        }
        "set_model_engine" => {
            let id = model(a, "id")?;
            let engine: Option<String> = arg(a, "engine")?;
            if let Some(e) = &engine {
                if e != "llama.cpp" && !st.pack_engines.iter().any(|p| &p.pack.id == e) {
                    return Err(err(mgmt::ErrorCode::NotFound, format!("there is no engine pack \"{e}\"")));
                }
            }
            update_prefs(st, &id, |p| p.engine = engine.filter(|e| e != "llama.cpp"))?;
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
            if !st.auth.decide_pairing(&id, Some(p), now) {
                return Err(err(
                    mgmt::ErrorCode::NotFound,
                    "that pairing request expired or was already answered",
                ));
            }
            st.push(&LiveUpdate::Pairing {
                pairings: st.auth.pairings(now),
            });
            ok(())
        }
        "deny_pairing" => {
            let id: String = arg(a, "id")?;
            st.auth.decide_pairing(&id, None, now);
            st.push(&LiveUpdate::Pairing {
                pairings: st.auth.pairings(now),
            });
            ok(())
        }
        "requests" => {
            let limit: usize = arg(a, "limit")?;
            ok(lock(&st.requests).iter().take(limit.min(500)).cloned().collect::<Vec<_>>())
        }
        "request_content" => Err(err(
            mgmt::ErrorCode::NotFound,
            "request content is not kept. Turn on \"Keep request content\" in Settings > Privacy to inspect prompts on this machine.",
        )),
        "benchmarks" => ok(lock(&st.benchmarks).clone()),
        "run_benchmark" => {
            let id = model(a, "modelId")?;
            run_benchmark(st, id).await
        }
        "settings" => ok(st.cfg().ui()),
        "save_settings" => {
            let ui: UiSettings = arg(a, "settings")?;
            let s = st.cfg().with_ui(ui).map_err(|e| err(mgmt::ErrorCode::BadRequest, e))?;
            ok(st.save_settings(s).map_err(|e| err(mgmt::ErrorCode::BadRequest, e))?.ui())
        }
        _ => Err(err(mgmt::ErrorCode::NotImplemented, format!("unknown call \"{method}\""))),
    }
}

fn offline_or(e: crate::egress::DownloadError) -> CommandError {
    match e {
        crate::egress::DownloadError::Egress(pitwall_core::privacy::EgressError::Offline) => err(mgmt::ErrorCode::Offline, e.to_string()),
        other => err(mgmt::ErrorCode::ServerUnavailable, other.to_string()),
    }
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
    let room = crate::system::room(&st.hardware, st.serving.hot_ram().0);
    let r = st.library.repo(repo, s.models.default_context, &room).await.map_err(offline_or)?;
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
        let url = pitwall_core::hf::tree_url(&id).map_err(|e| err(mgmt::ErrorCode::Offline, e.to_string()))?;
        pitwall_core::hf::parse_tree(&st.egress.get_json(&url, 8 << 20).await.map_err(offline_or)?)
            .map_err(|e| err(mgmt::ErrorCode::ServerUnavailable, e))?
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
        let id = st
            .library
            .download(repo, v, move |d| {
                st2.push(&LiveUpdate::Download { download: d.clone() });
                if matches!(d.state, DownloadState::Done) {
                    st2.event(EventKind::Download, Severity::Info, format!("Downloaded {}", d.name));
                }
            })
            .map_err(offline_or)?;
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

fn engines(st: &AppState) -> Vec<mgmt::Engine> {
    let platform = st.llama.platform();
    let installed = st.llama.installed();
    let active = st.llama.active().map(|b| b.id);
    let suggested = st.llama.suggested();
    let cfg = st.cfg();
    let mut list: Vec<mgmt::Engine> = st
        .engines
        .iter()
        .map(|e| {
            let llama = e.kind() == EngineKind::LlamaCpp;
            let (name, description) = match e.kind() {
                EngineKind::LlamaCpp => (
                    "llama.cpp",
                    "The default engine: CPU, Vulkan, CUDA and Metal builds chosen for this machine.",
                ),
                EngineKind::Mlx => ("MLX", "Apple's array framework for Apple Silicon."),
                EngineKind::Vllm => ("vLLM", "High-throughput serving on Linux with NVIDIA GPUs."),
                EngineKind::WhisperCpp => ("whisper.cpp", "Speech-to-text."),
                EngineKind::Pack => ("Engine pack", "An engine declared by a pack."),
            };
            mgmt::Engine {
                kind: e.kind(),
                name: name.into(),
                description: description.into(),
                availability: e.availability(),
                build_choice: if llama { cfg.engines.build_choice.clone() } else { "auto".into() },
                builds: if llama {
                    installed
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
                        .collect()
                } else {
                    vec![]
                },
                catalog: if llama {
                    st.llama
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
                        .collect()
                } else {
                    vec![]
                },
                processes: if llama {
                    st.serving
                        .loaded()
                        .iter()
                        .map(|m| mgmt::EngineProcess {
                            pid: 0,
                            model_id: m.model_id.clone(),
                            health: mgmt::ProcessHealth::Healthy,
                            restarts: 0,
                            rss_bytes: m.ram_bytes,
                            started_at: m.loaded_at,
                        })
                        .collect()
                } else {
                    vec![]
                },
                rollback_to: if llama { cfg.engines.previous_build.clone() } else { None },
            }
        })
        .collect();
    list.extend(st.pack_engines.iter().map(|pe| {
        let installed = pe.binary().is_some();
        let dl = pe.pack.downloads.iter().find(|d| d.platform == platform);
        mgmt::Engine {
            kind: EngineKind::Pack,
            name: pe.pack.name.clone(),
            description: if pe.pack.description.is_empty() { "An engine pack.".into() } else { pe.pack.description.clone() },
            availability: pe.availability(),
            build_choice: "auto".into(),
            builds: if installed {
                vec![mgmt::InstalledBuild { id: pe.pack.id.clone(), tag: pe.pack.version.clone(), label: pe.pack.name.clone(), size_bytes: dl.map_or(0, |d| d.size), self_test: mgmt::SelfTest::NotRun, self_test_message: None, devices: vec![], active: true, outdated: false }]
            } else {
                vec![]
            },
            catalog: dl.map(|d| vec![mgmt::CatalogBuild { id: pe.pack.id.clone(), label: pe.pack.name.clone(), tag: pe.pack.version.clone(), download_bytes: d.size, suggested: false, installed }]).unwrap_or_default(),
            processes: vec![],
            rollback_to: None,
        }
    }));
    list
}

fn install_build(st: &Arc<AppState>, id: &str) -> R<Value> {
    if st.offline.load(std::sync::atomic::Ordering::SeqCst) {
        return Err(err(
            mgmt::ErrorCode::Offline,
            pitwall_core::privacy::EgressError::Offline.to_string(),
        ));
    }
    if let Some(pe) = st.pack_engines.iter().find(|p| p.pack.id == id).cloned() {
        return install_pack(st, pe);
    }
    let platform = st.llama.platform();
    let build = st
        .llama
        .catalog()
        .find(&platform, id)
        .cloned()
        .ok_or_else(|| err(mgmt::ErrorCode::NotFound, format!("there is no build \"{id}\" for {platform}")))?;
    let n = lock(&st.installs).len();
    let dl_id = format!("eng-{n}");
    let cancel = crate::egress::CancelFlag::default();
    let host = build
        .assets
        .first()
        .and_then(|a| pitwall_core::privacy::DownloadUrl::parse(&a.url).ok())
        .map(|u| u.host().to_string())
        .unwrap_or_default();
    let dl = Download {
        id: dl_id.clone(),
        kind: DownloadKind::Engine,
        name: format!("llama.cpp {} ({})", builds::label(id), st.llama.catalog().tag),
        host,
        bytes_done: 0,
        bytes_total: build.download_bytes(),
        speed_bps: 0,
        state: DownloadState::Downloading,
        error: None,
        started_at: now_ms(),
    };
    lock(&st.installs).insert(dl_id.clone(), dl.clone());
    lock(&st.install_cancel).insert(dl_id.clone(), cancel.clone());
    let (st2, bid) = (st.clone(), id.to_string());
    tokio::spawn(async move {
        let upd = |f: &dyn Fn(&mut Download)| {
            let mut g = lock(&st2.installs);
            if let Some(d) = g.get_mut(&dl_id) {
                f(d);
                st2.push(&LiveUpdate::Download { download: d.clone() });
            }
        };
        let t0 = std::time::Instant::now();
        let mut last = std::time::Instant::now();
        let res = st2
            .llama
            .install(&bid, &cancel, |done, total| {
                if last.elapsed().as_millis() > 400 || done == total {
                    last = std::time::Instant::now();
                    let bps = (done as f64 / t0.elapsed().as_secs_f64().max(0.001)) as u64;
                    let mut g = lock(&st2.installs);
                    if let Some(d) = g.get_mut(&dl_id) {
                        d.bytes_done = done;
                        d.speed_bps = bps;
                        if done >= total {
                            d.state = DownloadState::SelfTest;
                        }
                    }
                }
            })
            .await;
        match res {
            Ok(info) => {
                upd(&|d| {
                    d.state = DownloadState::Done;
                    d.bytes_done = d.bytes_total;
                });
                st2.event(
                    EventKind::EngineStarted,
                    Severity::Info,
                    format!("Installed llama.cpp {} ({})", bid, info.tag),
                );
            }
            Err(e) => {
                let cancelled = matches!(e, crate::engines::EngineError::Download(crate::egress::DownloadError::Cancelled));
                let msg = e.to_string();
                upd(&|d| {
                    d.state = if cancelled {
                        DownloadState::Cancelled
                    } else {
                        DownloadState::Failed
                    };
                    d.error = Some(msg.clone());
                });
                st2.event(
                    EventKind::EngineFailed,
                    Severity::Error,
                    format!("Installing llama.cpp {bid} failed: {msg}"),
                );
            }
        }
        lock(&st2.install_cancel).remove(&dl_id);
    });
    ok(dl)
}

fn install_pack(st: &Arc<AppState>, pe: crate::pack_engine::PackEngine) -> R<Value> {
    let dl = pe.pack.downloads.iter().find(|d| d.platform == st.llama.platform()).ok_or_else(|| err(mgmt::ErrorCode::NotAvailable, format!("the {} pack has no build for this platform", pe.pack.name)))?;
    let dl_id = format!("eng-{}", lock(&st.installs).len());
    let cancel = crate::egress::CancelFlag::default();
    let entry = Download { id: dl_id.clone(), kind: DownloadKind::Engine, name: format!("{} {}", pe.pack.name, pe.pack.version), host: dl.url.host().to_string(), bytes_done: 0, bytes_total: dl.size, speed_bps: 0, state: DownloadState::Downloading, error: None, started_at: now_ms() };
    lock(&st.installs).insert(dl_id.clone(), entry.clone());
    lock(&st.install_cancel).insert(dl_id.clone(), cancel.clone());
    let st2 = st.clone();
    tokio::spawn(async move {
        let res = pe
            .install(&cancel, |done, _| {
                if let Some(d) = lock(&st2.installs).get_mut(&dl_id) {
                    d.bytes_done = done;
                }
            })
            .await;
        let mut g = lock(&st2.installs);
        if let Some(d) = g.get_mut(&dl_id) {
            match &res {
                Ok(()) => {
                    d.state = DownloadState::Done;
                    d.bytes_done = d.bytes_total;
                }
                Err(e) => {
                    d.state = if matches!(e, crate::engines::EngineError::Download(crate::egress::DownloadError::Cancelled)) { DownloadState::Cancelled } else { DownloadState::Failed };
                    d.error = Some(e.to_string());
                }
            }
            st2.push(&LiveUpdate::Download { download: d.clone() });
        }
        lock(&st2.install_cancel).remove(&dl_id);
    });
    ok(entry)
}

fn apps(st: &AppState) -> Vec<mgmt::AppInfo> {
    let now = now_ms();
    let usage = lock(&st.usage).clone();
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
            serving: if st.serving.paused() {
                mgmt::ServingState::Paused
            } else {
                mgmt::ServingState::Serving
            },
            offline: cfg.network.offline,
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
                processes: if e.kind == EngineKind::LlamaCpp { loaded.len() as u32 } else { 0 },
                unhealthy: 0,
            })
            .collect(),
        models: loaded,
        queue: st.queue_snapshot(),
        throughput: st.throughput(),
        apps: apps(st),
        preset: cfg.memory.preset,
        pairings: st.auth.pairings(now_ms()),
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
        g.retain(|b| b.model_id != result.model_id);
        g.push(result.clone());
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

async fn chat(State(st): S, h: HeaderMap, body: Bytes) -> Response {
    if let Err(r) = admin(&st, &h) {
        return *r;
    }
    let req: PlaygroundRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => return command_response(&err(mgmt::ErrorCode::BadRequest, e.to_string())),
    };
    let chat = ChatRequest {
        model: ModelRef::Named(req.model_id.clone()),
        messages: req.messages.iter().map(|m| Message::text(m.role, m.content.clone())).collect(),
        temperature: Some(req.temperature),
        max_tokens: Some(req.max_tokens),
        stream: true,
        ..Default::default()
    };
    let meta = Meta {
        family: ApiFamily::Extended,
        endpoint: "/mgmt/chat".into(),
        priority: Some(pitwall_core::scheduler::Priority::Interactive),
        extended: true,
        kind: JobKind::Chat,
    };
    let ev = |e: &PlaygroundEvent| pitwall_core::sse::data(&serde_json::to_string(e).unwrap_or_default());
    let run = st.start_chat(&Principal::Admin, meta, chat).await;
    let run = match run {
        Ok(r) => r,
        Err(e) => {
            let code = if e.kind == ErrorKind::InsufficientMemory {
                mgmt::ErrorCode::InsufficientMemory
            } else {
                mgmt::ErrorCode::NotAvailable
            };
            let body = ev(&PlaygroundEvent::Error {
                error: err(code, e.message),
            });
            return Response::builder()
                .header(header::CONTENT_TYPE, "text/event-stream")
                .body(Body::from(body))
                .unwrap_or_default();
        }
    };
    let cold = run.cold_start_ms;
    let started = std::time::Instant::now();
    let stream = futures_util::stream::unfold((run, false), move |(mut r, ended)| async move {
        if ended {
            return None;
        }
        let item = r.rx.recv().await;
        let (frame, end) = match item {
            None => return None,
            Some(Err(e)) => {
                r.rec.finish(Default::default(), None, Some(e.0.clone()));
                (
                    ev(&PlaygroundEvent::Error {
                        error: err(mgmt::ErrorCode::Internal, e.0),
                    }),
                    true,
                )
            }
            Some(Ok(e)) => {
                r.rec.event(&e);
                match e {
                    ChatEvent::Delta(t) => (ev(&PlaygroundEvent::Delta { text: t }), false),
                    ChatEvent::Done { usage, timings, .. } => {
                        r.rec.finish(usage, timings, None);
                        let total = started.elapsed().as_millis() as u64;
                        (
                            ev(&PlaygroundEvent::Done {
                                prompt_tokens: usage.prompt_tokens,
                                completion_tokens: usage.completion_tokens,
                                ttft_ms: total.min(timings.map_or(total, |_| total)),
                                gen_tps: timings.map_or(0.0, |t| t.gen_tps),
                                cold_start_ms: cold,
                            }),
                            true,
                        )
                    }
                    _ => (String::new(), false),
                }
            }
        };
        Some((Ok::<_, std::convert::Infallible>(Bytes::from(frame)), (r, end)))
    });
    Response::builder()
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from_stream(stream))
        .unwrap_or_default()
}
