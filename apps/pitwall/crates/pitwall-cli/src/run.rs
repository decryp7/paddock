//! Running a command: the thin edge that connects the parsed command line, the server and the terminal. The decisions
//! that can be tested without a server are small pure functions at the top.

use crate::api::{chat_body, decode, Api};
use crate::args::{Command, Invocation, KeysCommand, PresetsCommand, DEFAULT_TIMEOUT, LONG_TIMEOUT};
use crate::discover::{self, Env, Os, RealSources, Sources};
use crate::exit::CliError;
use crate::format;
use crate::sse::{Event, Parser};
use pitwall_core::mgmt::{AppInfo, BenchmarkResult, CreatedKey, Download, DownloadState, LoadedModel, LocalModel, Overview, PresetEntry};
use serde_json::{json, Value};
use std::io::{IsTerminal, Write};
use std::time::Duration;

/// The longest prompt read from standard input.
pub const MAX_STDIN_BYTES: u64 = 1024 * 1024;
/// How often `pull` asks for progress.
pub const POLL_EVERY: Duration = Duration::from_millis(1000);

/// The one app whose id or name is `query` (ids first, then names ignoring case).
pub fn find_app<'a>(apps: &'a [AppInfo], query: &str) -> Result<&'a AppInfo, CliError> {
    if let Some(a) = apps.iter().find(|a| a.id == query) {
        return Ok(a);
    }
    let by_name: Vec<&AppInfo> = apps.iter().filter(|a| a.name.eq_ignore_ascii_case(query)).collect();
    match by_name.as_slice() {
        [one] => Ok(one),
        [] => Err(CliError::failure(format!(
            "no app \"{query}\". Known apps: {}. Create it with `pitwall keys create {query} --new`.",
            if apps.is_empty() {
                "none".to_string()
            } else {
                apps.iter().map(|a| a.name.clone()).collect::<Vec<_>>().join(", ")
            }
        ))),
        _ => Err(CliError::failure(format!(
            "more than one app is called \"{query}\": use its id (see `pitwall keys`)."
        ))),
    }
}

/// The preset with `alias`.
pub fn find_preset<'a>(list: &'a [PresetEntry], alias: &str) -> Result<&'a PresetEntry, CliError> {
    list.iter().find(|e| e.preset.alias.as_str() == alias).ok_or_else(|| {
        CliError::failure(format!(
            "no preset \"{alias}\". Known presets: {}.",
            if list.is_empty() {
                "none".to_string()
            } else {
                list.iter().map(|e| e.preset.alias.to_string()).collect::<Vec<_>>().join(", ")
            }
        ))
    })
}

/// What a download's state means for `pitwall pull` once it stopped changing: `None` while it still runs.
pub fn pull_outcome(d: &Download) -> Option<Result<(), CliError>> {
    match d.state {
        DownloadState::Done => Some(Ok(())),
        DownloadState::Failed => Some(Err(CliError::failure(format!(
            "the download of {} failed: {}. Run the same command again to resume.",
            d.name,
            d.error.as_deref().unwrap_or("unknown error")
        )))),
        DownloadState::Paused => Some(Err(CliError::failure(format!(
            "the download of {} is paused. Run the same command again to resume.",
            d.name
        )))),
        DownloadState::Cancelled => Some(Err(CliError::failure(format!("the download of {} was cancelled.", d.name)))),
        _ => None,
    }
}

fn print_json(v: &Value) {
    println!("{}", serde_json::to_string_pretty(v).unwrap_or_default());
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis().min(u128::from(u64::MAX)) as u64)
}

/// Runs the command. Output goes to stdout, progress and notes to stderr; the error carries the exit code.
pub async fn run(inv: Invocation, env: Env) -> Result<(), CliError> {
    let dir = discover::data_dir(&env, Os::current());
    let discovery = RealSources.read_file(&discover::discovery_path(&dir));
    let port = discover::port(inv.port, discovery.as_deref());
    if let Command::Chat {
        model,
        prompt,
        system,
        max_tokens,
    } = &inv.command
    {
        return chat(&inv, &env, port, model, prompt.as_deref(), system.as_deref(), *max_tokens).await;
    }
    let key = discover::admin_key(inv.key_file.as_deref(), &env, &dir, &RealSources).map_err(|e| CliError::failure(e.to_string()))?;
    let api = Api::new(port, Some(key))?;
    let short = inv.timeout_or(DEFAULT_TIMEOUT);
    let long = inv.timeout_or(LONG_TIMEOUT);
    match &inv.command {
        Command::Status => {
            let v = api.rpc_json("overview", Value::Null, short).await?;
            if inv.json {
                print_json(&v);
            } else {
                println!("{}", format::status(&decode::<Overview>(v)?, now_ms()));
            }
        }
        Command::Ps => {
            let o: Overview = api.rpc("overview", Value::Null, short).await?;
            if inv.json {
                print_json(&serde_json::to_value(&o.models).unwrap_or_default());
            } else {
                println!("{}", format::ps(&o.models as &[LoadedModel]));
            }
        }
        Command::Models => {
            let v = api.rpc_json("local_models", Value::Null, short).await?;
            if inv.json {
                print_json(&v);
            } else {
                println!("{}", format::models(&decode::<Vec<LocalModel>>(v)?));
            }
        }
        Command::Load { model } => {
            api.rpc_json("load_model", json!({"id": model}), long).await?;
            eprintln!("Loaded {model}.");
        }
        Command::Unload { model } => {
            api.rpc_json("unload_model", json!({"id": model}), short).await?;
            eprintln!("Unloaded {model}.");
        }
        Command::Pull { repo, file, wait } => pull(&api, &inv, repo, file, *wait).await?,
        Command::Keys(k) => keys(&api, &inv, k, short).await?,
        Command::Presets(p) => {
            let v = api.rpc_json("presets", Value::Null, short).await?;
            let list: Vec<PresetEntry> = decode(v.clone())?;
            match p {
                PresetsCommand::List if inv.json => print_json(&v),
                PresetsCommand::List => println!("{}", format::presets(&list)),
                PresetsCommand::Show { alias } => {
                    let e = find_preset(&list, alias)?;
                    if inv.json {
                        print_json(&serde_json::to_value(e).unwrap_or_default());
                    } else {
                        println!("{}", format::preset_detail(e));
                    }
                }
            }
        }
        Command::Bench { model } => {
            let v = api.rpc_json("run_benchmark", json!({"modelId": model}), long).await?;
            if inv.json {
                print_json(&v);
            } else {
                println!("{}", format::bench(&decode::<BenchmarkResult>(v)?));
            }
        }
        Command::Chat { .. } => {}
    }
    Ok(())
}

async fn keys(api: &Api, inv: &Invocation, cmd: &KeysCommand, t: Duration) -> Result<(), CliError> {
    match cmd {
        KeysCommand::List => {
            let v = api.rpc_json("apps", Value::Null, t).await?;
            if inv.json {
                print_json(&v);
            } else {
                println!("{}", format::keys(&decode::<Vec<AppInfo>>(v)?));
            }
        }
        KeysCommand::Create { app, new_app } => {
            let apps: Vec<AppInfo> = api.rpc("apps", Value::Null, t).await?;
            let id = if *new_app {
                if apps.iter().any(|a| a.name.eq_ignore_ascii_case(app) || a.id == *app) {
                    return Err(CliError::failure(format!(
                        "an app \"{app}\" already exists: leave out --new to add another key to it."
                    )));
                }
                api.rpc::<AppInfo>("create_app", json!({"name": app}), t).await?.id
            } else {
                find_app(&apps, app)?.id.clone()
            };
            let created: CreatedKey = api.rpc("create_key", json!({"appId": id}), t).await?;
            if inv.json {
                print_json(&serde_json::to_value(&created).unwrap_or_default());
            } else {
                // The secret alone on stdout (easy to capture), the explanation on stderr.
                println!("{}", created.secret);
                eprintln!("{}", format::created_key_note(&created));
            }
        }
        KeysCommand::Revoke { app, key } => {
            let apps: Vec<AppInfo> = api.rpc("apps", Value::Null, t).await?;
            let a = find_app(&apps, app)?;
            api.rpc_json("revoke_key", json!({"appId": a.id, "keyId": key}), t).await?;
            eprintln!("Revoked key {key} of {}.", a.name);
        }
    }
    Ok(())
}

async fn pull(api: &Api, inv: &Invocation, repo: &str, file: &str, wait: bool) -> Result<(), CliError> {
    let t = inv.timeout_or(Duration::from_secs(60));
    let started: Option<Download> = api
        .rpc("download_model", json!({"repo": repo, "file": file, "projector": null}), t)
        .await?;
    let Some(first) = started else {
        eprintln!("{repo}/{file} is already installed or finished.");
        return Ok(());
    };
    eprintln!(
        "Downloading {} from {} (resumable: run the same command again after an interruption).",
        first.name, first.host
    );
    if !wait {
        println!("{}", first.id);
        return Ok(());
    }
    let tty = std::io::stderr().is_terminal();
    let mut last_percent = u64::MAX;
    loop {
        let all: Vec<Download> = api.rpc("downloads", Value::Null, DEFAULT_TIMEOUT).await?;
        let d = all.into_iter().find(|d| d.id == first.id).unwrap_or_else(|| first.clone());
        let line = format::progress_line(&d);
        let percent = d.bytes_done.saturating_mul(100).checked_div(d.bytes_total).unwrap_or(0);
        if tty {
            eprint!("\r\x1b[K{line}");
        } else if percent / 10 != last_percent / 10 || pull_outcome(&d).is_some() {
            eprintln!("{line}");
        }
        last_percent = percent;
        if let Some(outcome) = pull_outcome(&d) {
            if tty {
                eprintln!();
            }
            outcome?;
            if inv.json {
                print_json(&serde_json::to_value(&d).unwrap_or_default());
            }
            return Ok(());
        }
        tokio::time::sleep(POLL_EVERY).await;
    }
}

async fn chat(
    inv: &Invocation,
    env: &Env,
    port: u16,
    model: &str,
    prompt: Option<&str>,
    system: Option<&str>,
    max: Option<u32>,
) -> Result<(), CliError> {
    let Some(key) = env.api_key.as_deref().and_then(discover::parse_key) else {
        return Err(CliError::failure(
            "chat needs a per-app key: set PITWALL_API_KEY. Make one with `pitwall keys create <app> --new` (it is shown once).",
        ));
    };
    let prompt = match prompt {
        Some(p) => p.to_string(),
        None => read_stdin().await?,
    };
    if prompt.trim().is_empty() {
        return Err(CliError {
            exit: crate::exit::Exit::Usage,
            message: "no prompt: pass it as an argument (pitwall chat <model> \"...\") or pipe text to standard input.".into(),
        });
    }
    let api = Api::new(port, Some(key))?;
    let body = chat_body(model, &prompt, system, max);
    let (mut parser, mut printed, mut failure) = (Parser::default(), false, None);
    let mut out = std::io::stdout();
    api.chat_stream(&body, inv.timeout_or(LONG_TIMEOUT), |chunk| {
        for e in parser.feed(chunk) {
            match e {
                Event::Delta(t) => {
                    printed = true;
                    let _ = out.write_all(t.as_bytes());
                    let _ = out.flush();
                }
                Event::Error(m) => {
                    failure = Some(m);
                    return false;
                }
                Event::Done => return false,
            }
        }
        true
    })
    .await?;
    if printed {
        println!();
    }
    match failure {
        Some(m) => Err(CliError::failure(format!("{}.", m.trim_end_matches('.')))),
        None => Ok(()),
    }
}

async fn read_stdin() -> Result<String, CliError> {
    use tokio::io::AsyncReadExt;
    if std::io::stdin().is_terminal() {
        return Err(CliError {
            exit: crate::exit::Exit::Usage,
            message: "no prompt: pass it as an argument (pitwall chat <model> \"...\") or pipe text to standard input.".into(),
        });
    }
    let mut buf = Vec::new();
    tokio::io::stdin()
        .take(MAX_STDIN_BYTES)
        .read_to_end(&mut buf)
        .await
        .map_err(|e| CliError::failure(format!("could not read standard input: {e}")))?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exit::Exit;

    fn app(id: &str, name: &str) -> AppInfo {
        serde_json::from_value(json!({
            "id": id, "name": name, "suite": false, "connected": false, "lastSeenAt": null, "keys": [],
            "permissions": {}, "today": {}
        }))
        .unwrap_or_else(|e| panic!("{e}"))
    }

    #[test]
    fn apps_are_found_by_id_or_name() {
        let apps = vec![app("a1", "Debrief"), app("a2", "Notes")];
        assert_eq!(find_app(&apps, "a2").unwrap().name, "Notes");
        assert_eq!(find_app(&apps, "debrief").unwrap().id, "a1");
        let e = find_app(&apps, "zzz").unwrap_err();
        assert!(e.message.contains("Debrief, Notes") && e.message.contains("--new"));
        assert!(find_app(&[], "x").unwrap_err().message.contains("none"));
        let dup = vec![app("a1", "x"), app("a2", "X")];
        assert!(find_app(&dup, "x").unwrap_err().message.contains("use its id"));
    }

    fn dl(state: DownloadState, error: Option<&str>) -> Download {
        Download {
            id: "d".into(),
            kind: pitwall_core::mgmt::DownloadKind::Model,
            name: "m.gguf".into(),
            host: "h".into(),
            bytes_done: 0,
            bytes_total: 0,
            speed_bps: 0,
            state,
            error: error.map(String::from),
            started_at: 0,
        }
    }

    #[test]
    fn pull_outcomes() {
        assert!(matches!(pull_outcome(&dl(DownloadState::Done, None)), Some(Ok(()))));
        assert!(pull_outcome(&dl(DownloadState::Downloading, None)).is_none());
        assert!(pull_outcome(&dl(DownloadState::Queued, None)).is_none());
        for (s, word) in [(DownloadState::Paused, "paused"), (DownloadState::Cancelled, "cancelled")] {
            let Some(Err(e)) = pull_outcome(&dl(s, None)) else { panic!() };
            assert!(e.message.contains(word) && e.exit == Exit::Error);
        }
        let Some(Err(e)) = pull_outcome(&dl(DownloadState::Failed, Some("disk full"))) else {
            panic!()
        };
        assert!(e.message.contains("disk full") && e.message.contains("resume"));
        let Some(Err(e)) = pull_outcome(&dl(DownloadState::Failed, None)) else {
            panic!()
        };
        assert!(e.message.contains("unknown error"));
    }

    #[test]
    fn presets_are_found_by_alias() {
        let e: PresetEntry = serde_json::from_value(json!({
            "preset": {"alias": "fast", "target": "auto:chat,fast"},
            "status": {"state": "noMatch", "message": "nothing installed"}
        }))
        .unwrap_or_else(|e| panic!("{e}"));
        let list = vec![e];
        assert_eq!(find_preset(&list, "fast").unwrap().preset.alias.as_str(), "fast");
        assert!(find_preset(&list, "slow").unwrap_err().message.contains("fast"));
        assert!(find_preset(&[], "slow").unwrap_err().message.contains("none"));
    }
}
