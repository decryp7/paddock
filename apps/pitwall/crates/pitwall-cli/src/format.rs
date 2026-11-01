//! Output formatting: pure functions from the engine's DTOs to text. Nothing here prints or reads a clock; the
//! caller passes "now". Sizes are decimal (GB), as the Pitwall window shows them.

use pitwall_core::mgmt::{
    ApiKeyInfo, AppInfo, BenchmarkResult, CreatedKey, Download, DownloadState, LoadedModel, LocalModel, Overview, PresetEntry, PresetStatus,
};
use pitwall_core::time::{rfc3339, Millis};
use serde::Serialize;

/// The wire name of an enum value (`"ready"`, `"vulkan"`), so the CLI never keeps its own copy of the names.
pub fn label<T: Serialize>(v: &T) -> String {
    match serde_json::to_value(v) {
        Ok(serde_json::Value::String(s)) => s,
        Ok(other) => other
            .get("state")
            .and_then(|s| s.as_str())
            .map_or_else(|| other.to_string(), str::to_string),
        Err(_) => "?".into(),
    }
}

/// `1.5 GB`, `340 MB`, `12 KB`, `7 B`.
pub fn human_bytes(n: u64) -> String {
    const UNITS: [(&str, f64); 4] = [("TB", 1e12), ("GB", 1e9), ("MB", 1e6), ("KB", 1e3)];
    for (name, size) in UNITS {
        if n as f64 >= size {
            let v = n as f64 / size;
            return if v >= 100.0 {
                format!("{v:.0} {name}")
            } else {
                format!("{v:.1} {name}")
            };
        }
    }
    format!("{n} B")
}

/// `2h 5m`, `3m 04s`, `12s`.
pub fn human_duration(ms: u64) -> String {
    let s = ms / 1000;
    match (s / 3600, s % 3600 / 60, s % 60) {
        (0, 0, s) => format!("{s}s"),
        (0, m, s) => format!("{m}m {s:02}s"),
        (h, m, _) => format!("{h}h {m}m"),
    }
}

/// `2026-10-22 09:15:03Z` (UTC, so output does not depend on the machine's time zone).
pub fn timestamp(ms: Millis) -> String {
    let t = rfc3339(ms);
    format!("{} {}Z", t.get(..10).unwrap_or(""), t.get(11..19).unwrap_or(""))
}

/// A left-aligned table with a header row; short rows are padded, trailing spaces trimmed.
pub fn table(headers: &[&str], rows: &[Vec<String>]) -> String {
    let cols = headers.len();
    let mut widths: Vec<usize> = headers.iter().map(|h| h.chars().count()).collect();
    for r in rows {
        for (i, c) in r.iter().take(cols).enumerate() {
            widths[i] = widths[i].max(c.chars().count());
        }
    }
    let line = |cells: Vec<&str>| {
        let mut s = String::new();
        for (i, w) in widths.iter().enumerate() {
            let c = cells.get(i).copied().unwrap_or("");
            s.push_str(c);
            if i + 1 < cols {
                s.push_str(&" ".repeat(w - c.chars().count() + 2));
            }
        }
        s.trim_end().to_string()
    };
    let mut out = line(headers.to_vec());
    for r in rows {
        out.push('\n');
        out.push_str(&line(r.iter().map(String::as_str).collect()));
    }
    out
}

/// A percentage of `part` in `whole` (0 when `whole` is 0).
fn percent(part: u64, whole: u64) -> u64 {
    if whole == 0 {
        0
    } else {
        (part.min(whole) as u128 * 100 / whole as u128) as u64
    }
}

/// `pitwall status`.
pub fn status(o: &Overview, now: Millis) -> String {
    let s = &o.server;
    let m = &o.memory;
    let mut out = format!(
        "Pitwall {} on 127.0.0.1:{} (pid {}, up {})\nServing:   {}\nMemory:    {} free of {} ({} pressure)\nLoaded:    {}\nQueue:     {} running, {} waiting\nSpeed:     {:.0} tokens/s generated, {:.1} requests/min\nPreset:    {}",
        s.version,
        s.port,
        s.pid,
        human_duration(now.saturating_sub(s.started_at)),
        label(&s.serving),
        human_bytes(m.ram_available),
        human_bytes(m.ram_total),
        label(&m.pressure),
        if o.models.is_empty() {
            "no models".to_string()
        } else {
            o.models.iter().map(|x| x.model_id.clone()).collect::<Vec<_>>().join(", ")
        },
        o.queue.running.len(),
        o.queue.waiting.len(),
        o.throughput.gen_tps,
        o.throughput.requests_per_min,
        label(&o.preset),
    );
    for g in &m.gpus {
        let vram = match (g.vram_used, g.vram_total) {
            (Some(u), Some(t)) => format!(", {} of {} video memory used", human_bytes(u), human_bytes(t)),
            _ => String::new(),
        };
        out.push_str(&format!("\nGPU:       {}{vram}", g.name));
    }
    for i in &o.issues {
        out.push_str(&format!("\nIssue:     {} ({})", i.model_id, label(&i.state)));
    }
    if !o.pairings.is_empty() {
        out.push_str(&format!(
            "\nPairing:   {} app(s) waiting for approval in the Pitwall window",
            o.pairings.len()
        ));
    }
    out
}

/// `pitwall ps`.
pub fn ps(models: &[LoadedModel]) -> String {
    if models.is_empty() {
        return "No models are loaded. Load one with: pitwall load <model>".into();
    }
    let rows = models
        .iter()
        .map(|m| {
            vec![
                m.model_id.clone(),
                label(&m.state),
                label(&m.engine),
                human_bytes(m.ram_bytes),
                human_bytes(m.vram_bytes),
                m.context_tokens.to_string(),
                m.slots.to_string(),
                if m.pinned { "pinned".into() } else { String::new() },
            ]
        })
        .collect::<Vec<_>>();
    table(&["MODEL", "STATE", "ENGINE", "RAM", "VRAM", "CONTEXT", "SLOTS", ""], &rows)
}

/// `pitwall models`.
pub fn models(models: &[LocalModel]) -> String {
    if models.is_empty() {
        return "No models are installed. Download one with: pitwall pull <owner>/<repo>/<file.gguf>".into();
    }
    let rows = models
        .iter()
        .map(|m| {
            vec![
                m.id.clone(),
                m.quant.clone().unwrap_or_default(),
                human_bytes(m.size_bytes),
                if m.loaded { "loaded".into() } else { String::new() },
                label(&m.fit.verdict),
            ]
        })
        .collect::<Vec<_>>();
    table(&["MODEL", "QUANT", "SIZE", "", "FIT"], &rows)
}

/// `pitwall keys`.
pub fn keys(apps: &[AppInfo]) -> String {
    if apps.is_empty() {
        return "No apps yet. Create a key with: pitwall keys create <app> --new".into();
    }
    let mut rows = Vec::new();
    for a in apps {
        if a.keys.is_empty() {
            rows.push(vec![a.name.clone(), a.id.clone(), "(no keys)".into(), String::new(), String::new()]);
        }
        for k in &a.keys {
            rows.push(vec![
                a.name.clone(),
                a.id.clone(),
                key_id(k),
                k.created_at_text(),
                k.last_used_text(),
            ]);
        }
    }
    table(&["APP", "APP ID", "KEY ID (prefix)", "CREATED", "LAST USED"], &rows)
}

fn key_id(k: &ApiKeyInfo) -> String {
    format!("{} ({}...)", k.id, k.prefix)
}

/// Text for the two key timestamps.
trait KeyTimes {
    fn created_at_text(&self) -> String;
    fn last_used_text(&self) -> String;
}

impl KeyTimes for ApiKeyInfo {
    fn created_at_text(&self) -> String {
        timestamp(self.created_at)
    }
    fn last_used_text(&self) -> String {
        self.last_used_at.map_or_else(|| "never".into(), timestamp)
    }
}

/// What `pitwall keys create` prints on stderr beside the secret on stdout.
pub fn created_key_note(k: &CreatedKey) -> String {
    format!(
        "Created key {} for app {}. This is the only time it is shown: store it now (for example as PITWALL_API_KEY).",
        k.key.id, k.app_id
    )
}

fn status_text(s: &PresetStatus) -> String {
    match s {
        PresetStatus::Ready {
            model_id,
            chosen_by,
            loaded,
            ..
        } => {
            format!("ready: {model_id} ({}{})", label(chosen_by), if *loaded { ", loaded" } else { "" })
        }
        PresetStatus::Stale { message } | PresetStatus::NoMatch { message } | PresetStatus::Shadowed { message } => {
            format!("{}: {message}", label(s))
        }
    }
}

/// `pitwall presets`.
pub fn presets(list: &[PresetEntry]) -> String {
    if list.is_empty() {
        return "No presets. Create one in Pitwall > Models > Presets.".into();
    }
    let rows = list
        .iter()
        .map(|e| vec![e.preset.alias.to_string(), e.preset.target.to_text(), status_text(&e.status)])
        .collect::<Vec<_>>();
    table(&["ALIAS", "TARGET", "STATUS"], &rows)
}

/// `pitwall presets show <alias>`.
pub fn preset_detail(e: &PresetEntry) -> String {
    let p = &e.preset;
    let mut out = format!(
        "Alias:         {}\nTarget:        {}\nStatus:        {}",
        p.alias,
        p.target.to_text(),
        status_text(&e.status)
    );
    let mut add = |name: &str, v: Option<String>| {
        if let Some(v) = v {
            out.push_str(&format!("\n{name:<15}{v}"));
        }
    };
    add("Context:", p.context.map(|c| c.to_string()));
    add("Temperature:", p.temperature.map(|t| t.to_string()));
    add("Top-p:", p.top_p.map(|t| t.to_string()));
    add("Max tokens:", p.max_tokens.map(|t| t.to_string()));
    add(
        "System prompt:",
        p.system_prompt.as_ref().map(|s| format!("{} characters", s.chars().count())),
    );
    out
}

/// One progress line for a download (`pitwall pull`).
pub fn progress_line(d: &Download) -> String {
    let state = label(&d.state);
    match d.state {
        DownloadState::Downloading if d.bytes_total > 0 => format!(
            "{} {:>3}%  {} / {}  {}/s",
            d.name,
            percent(d.bytes_done, d.bytes_total),
            human_bytes(d.bytes_done),
            human_bytes(d.bytes_total),
            human_bytes(d.speed_bps)
        ),
        DownloadState::Failed => format!("{} failed: {}", d.name, d.error.as_deref().unwrap_or("unknown error")),
        _ => format!("{} {state}", d.name),
    }
}

/// Whether a download will not change any more without the user's action.
pub fn download_finished(s: DownloadState) -> bool {
    matches!(
        s,
        DownloadState::Done | DownloadState::Failed | DownloadState::Cancelled | DownloadState::Paused
    )
}

/// `pitwall bench`.
pub fn bench(b: &BenchmarkResult) -> String {
    format!(
        "{} ({}, {} tokens context)\nPrompt:      {:.1} tokens/s\nGeneration:  {:.1} tokens/s\nFirst token: {} ms",
        b.model_id, b.build, b.context_tokens, b.prompt_tps, b.gen_tps, b.ttft_ms
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use pitwall_core::mgmt::*;

    #[test]
    fn sizes_and_durations() {
        assert_eq!(human_bytes(0), "0 B");
        assert_eq!(human_bytes(999), "999 B");
        assert_eq!(human_bytes(12_000), "12.0 KB");
        assert_eq!(human_bytes(340_000_000), "340 MB");
        assert_eq!(human_bytes(1_500_000_000), "1.5 GB");
        assert_eq!(human_bytes(2_000_000_000_000), "2.0 TB");
        assert_eq!(human_duration(12_000), "12s");
        assert_eq!(human_duration(184_000), "3m 04s");
        assert_eq!(human_duration(7_500_000), "2h 5m");
        assert_eq!(timestamp(0), "1970-01-01 00:00:00Z");
    }

    #[test]
    fn tables_align_and_pad() {
        let t = table(&["A", "BB"], &[vec!["xxx".into(), "1".into()], vec!["y".into()]]);
        assert_eq!(t, "A    BB\nxxx  1\ny");
        assert_eq!(table(&["A"], &[]), "A");
    }

    #[test]
    fn labels_use_wire_names() {
        assert_eq!(label(&HotState::Ready), "ready");
        assert_eq!(label(&PresetStatus::Stale { message: "m".into() }), "stale");
        assert_eq!(label(&1u32), "1");
    }

    fn download(state: DownloadState, done: u64) -> Download {
        Download {
            id: "d1".into(),
            kind: DownloadKind::Model,
            name: "m.gguf".into(),
            host: "huggingface.co".into(),
            bytes_done: done,
            bytes_total: 1_000_000_000,
            speed_bps: 5_000_000,
            state,
            error: None,
            started_at: 0,
        }
    }

    #[test]
    fn download_progress() {
        let l = progress_line(&download(DownloadState::Downloading, 250_000_000));
        assert!(l.contains(" 25%") && l.contains("250 MB / 1.0 GB") && l.contains("5.0 MB/s"), "{l}");
        let mut f = download(DownloadState::Failed, 0);
        assert!(progress_line(&f).contains("unknown error"));
        f.error = Some("disk full".into());
        assert_eq!(progress_line(&f), "m.gguf failed: disk full");
        assert_eq!(progress_line(&download(DownloadState::Verifying, 0)), "m.gguf verifying");
        let mut z = download(DownloadState::Downloading, 5);
        z.bytes_total = 0;
        assert_eq!(progress_line(&z), "m.gguf downloading");
        assert!(download_finished(DownloadState::Done) && download_finished(DownloadState::Paused));
        assert!(!download_finished(DownloadState::Downloading) && !download_finished(DownloadState::Queued));
        assert_eq!(percent(5, 0), 0);
        assert_eq!(percent(20, 10), 100);
    }

    #[test]
    fn bench_text() {
        let b = BenchmarkResult {
            model_id: "m".into(),
            build: "vulkan".into(),
            at: 0,
            prompt_tps: 400.0,
            gen_tps: 52.34,
            ttft_ms: 180,
            context_tokens: 4096,
        };
        let t = bench(&b);
        assert!(t.contains("52.3 tokens/s") && t.contains("180 ms") && t.contains("vulkan"));
    }

    fn from_json<T: serde::de::DeserializeOwned>(v: serde_json::Value) -> T {
        serde_json::from_value(v).unwrap_or_else(|e| panic!("{e}"))
    }

    fn loaded_model() -> LoadedModel {
        from_json(
            serde_json::json!({"modelId": "acme--tiny--q4", "name": "Tiny", "engine": "llama.cpp", "build": "vulkan", "state": "ready",
            "ramBytes": 1_500_000_000u64, "vramBytes": 0, "contextTokens": 4096, "slots": 2, "pinned": true, "activeRequests": 0,
            "loadedAt": 1, "lastUsedAt": 1, "idleReleaseAt": null, "coldStartMs": 900, "capabilities": ["chat"]}),
        )
    }

    #[test]
    fn ps_table() {
        let t = ps(&[loaded_model()]);
        let lines: Vec<&str> = t.lines().collect();
        assert!(
            lines[0].starts_with("MODEL") && lines[1].contains("acme--tiny--q4") && lines[1].contains("1.5 GB"),
            "{t}"
        );
        assert!(lines[1].contains("llama.cpp") && lines[1].ends_with("pinned"), "{t}");
    }

    #[test]
    fn models_table() {
        let m: LocalModel = from_json(
            serde_json::json!({"id": "a--b--q4", "repo": "a/b", "file": "f.gguf", "name": "B", "quant": null,
            "sizeBytes": 400_000_000u64, "paramsB": null, "contextMax": null, "architecture": null, "capabilities": [], "projector": null,
            "addedAt": 1, "lastUsedAt": null, "loaded": false, "pinned": false, "idleSecs": null,
            "fit": {"verdict": "wontFit", "needRam": 1, "needVram": 0, "haveRam": 2, "haveVram": 0, "offload": "none", "message": "x"}}),
        );
        let t = models(&[m]);
        assert!(t.contains("400 MB") && t.contains("wontFit") && !t.contains("loaded"), "{t}");
    }

    #[test]
    fn keys_table_and_secret_note() {
        let app = |keys: serde_json::Value| -> AppInfo {
            from_json(
                serde_json::json!({"id": "app-1", "name": "Debrief", "suite": true, "connected": false, "lastSeenAt": null,
                "keys": keys, "permissions": {}, "today": {}}),
            )
        };
        let t = keys(&[
            app(serde_json::json!([{"id": "k1", "prefix": "pw_ab", "createdAt": 0, "lastUsedAt": 86_400_000u64}])),
            app(serde_json::json!([])),
        ]);
        assert!(
            t.contains("k1 (pw_ab...)") && t.contains("1970-01-01 00:00:00Z") && t.contains("1970-01-02") && t.contains("(no keys)"),
            "{t}"
        );
        let c = CreatedKey {
            app_id: "app-1".into(),
            key: from_json(serde_json::json!({"id": "k9", "prefix": "pw_x", "createdAt": 0, "lastUsedAt": null})),
            secret: "SECRET".into(),
        };
        let note = created_key_note(&c);
        assert!(note.contains("k9") && note.contains("only time") && !note.contains("SECRET"));
    }

    fn entry(status: serde_json::Value) -> PresetEntry {
        from_json(
            serde_json::json!({"preset": {"alias": "fast", "target": "auto:chat,fast", "context": 8192, "temperature": 0.2, "topP": 0.9,
            "maxTokens": 256, "systemPrompt": "Be brief."}, "status": status}),
        )
    }

    #[test]
    fn preset_listing_and_detail() {
        let ready = entry(serde_json::json!({"state": "ready", "modelId": "m", "chosenBy": "capabilities", "reason": "r", "loaded": true}));
        assert!(presets(std::slice::from_ref(&ready)).contains("ready: m (capabilities, loaded)"));
        let d = preset_detail(&ready);
        for want in ["fast", "Context:", "8192", "Top-p:", "0.9", "Max tokens:", "256", "9 characters"] {
            assert!(d.contains(want), "{want} in {d}");
        }
        for (state, word) in [("stale", "stale"), ("noMatch", "noMatch"), ("shadowed", "shadowed")] {
            let e = entry(serde_json::json!({"state": state, "message": "fix it"}));
            assert!(preset_detail(&e).contains(&format!("{word}: fix it")));
        }
    }

    #[test]
    fn status_text() {
        let o: Overview = from_json(serde_json::json!({
            "server": {"version": "0.1.0", "apiVersion": 1, "pid": 7, "port": 11435, "startedAt": 0, "serving": "paused",
                       "lan": {"enabled": false, "url": null, "certFingerprint": null}},
            "memory": {"at": 1, "ramTotal": 32_000_000_000u64, "ramAvailable": 8_000_000_000u64, "commitUsed": null, "commitLimit": null,
                       "swapUsed": 0, "pressure": "elevated", "cpuPercent": 3.0,
                       "gpus": [{"name": "G1", "vramTotal": 8_000_000_000u64, "vramUsed": 2_000_000_000u64, "utilisation": null, "unified": false},
                                {"name": "G2", "vramTotal": null, "vramUsed": null, "utilisation": null, "unified": true}],
                       "pitwallRss": 1, "foregroundBusy": false},
            "models": [], "engines": [],
            "queue": {"running": [], "waiting": [], "depth": {"interactive": 0, "background": 0, "batch": 0}},
            "throughput": {"promptTps": 0.0, "genTps": 41.6, "ttftP50Ms": 0.0, "requestsPerMin": 2.0, "activeRequests": 0},
            "apps": [], "preset": "balanced",
            "pairings": [{"id": "p", "appId": "a", "appName": "X", "code": "123456", "requestedAt": 1, "permissions": {}}],
            "issues": [{"modelId": "m", "state": "failed", "crashes": 3, "restartAt": null, "message": "crashed"}]
        }));
        let t = status(&o, 3_661_000);
        for want in [
            "up 1h 1m",
            "paused",
            "8.0 GB free of 32.0 GB (elevated pressure)",
            "no models",
            "42 tokens/s",
            "2.0 GB of 8.0 GB video memory",
            "GPU:       G2",
            "Issue:     m (failed)",
            "1 app(s) waiting",
        ] {
            assert!(t.contains(want), "{want} in {t}");
        }
    }

    #[test]
    fn empty_listings_say_what_to_do() {
        assert!(ps(&[]).contains("pitwall load"));
        assert!(models(&[]).contains("pitwall pull"));
        assert!(keys(&[]).contains("pitwall keys create"));
        assert!(presets(&[]).contains("Presets"));
    }
}
