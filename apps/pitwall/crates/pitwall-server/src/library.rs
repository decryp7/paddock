//! The model library (feature 2): the GGUF files on disk, Hugging Face search and file lists, the fit check, and
//! resumable verified downloads. Hugging Face answers are parsed by `pitwall_core::hf`; this module does the I/O.

use crate::egress::{CancelFlag, DownloadError, Egress};
use crate::paths::{now_ms, Dirs};
use pitwall_core::fit::{self, ModelSize, Room};
use pitwall_core::gguf::{self, GgufInfo};
use pitwall_core::hf::{self, RepoId};
use pitwall_core::ids::ModelId;
use pitwall_core::mgmt::{Download, DownloadKind, DownloadState, HfRepo, LocalModel, QuantVariant};
use pitwall_core::privacy::DownloadUrl;
use pitwall_core::slots::{self, Slots};
use pitwall_core::updates::{self, Installed, Provenance};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const MAX_SCAN_DEPTH: usize = 4;
const MAX_JSON: usize = 8 * 1024 * 1024;

/// A model file found on disk.
#[derive(Debug, Clone)]
pub struct LocalFile {
    /// The id clients use.
    pub id: ModelId,
    /// First (or only) file.
    pub path: PathBuf,
    /// Total bytes of all parts.
    pub size: u64,
    /// Parsed header.
    pub info: GgufInfo,
    /// Matching vision projector, if one sits next to it.
    pub projector: Option<PathBuf>,
    /// Most pixels of one image its projector takes (read from the projector's header once, at the scan; the default budget
    /// when the header does not say); `None` without a projector.
    pub image_pixels: Option<u64>,
    /// Modified time.
    pub modified: u64,
    /// `owner/repo` when it was downloaded from there.
    pub repo: String,
}

fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.filter_map(Result::ok) {
        let p = e.path();
        let Ok(t) = e.file_type() else { continue };
        if t.is_dir() && depth < MAX_SCAN_DEPTH {
            walk(&p, depth + 1, out);
        } else if t.is_file() && p.extension().is_some_and(|x| x == "gguf") {
            out.push(p);
        }
    }
}

/// Scan `dir` for models. Damaged files are skipped (they never stop the scan); split models appear once.
pub fn scan(dir: &Path) -> Vec<LocalFile> {
    let mut files = vec![];
    walk(dir, 0, &mut files);
    files.sort();
    let mut out: Vec<LocalFile> = vec![];
    for p in &files {
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let stem = name.trim_end_matches(".gguf");
        if let Some((_, part, _)) = pitwall_core::ids::split_part(stem) {
            if part != 1 {
                continue;
            }
        }
        let Ok(info) = gguf::inspect(p) else { continue };
        if gguf::is_projector_architecture(&info.architecture) {
            continue;
        }
        let Ok(id) = ModelId::from_file_name(&name) else { continue };
        let size = split_total(p, &files);
        let projector = p
            .parent()
            .and_then(|d| {
                files.iter().find(|f| {
                    f.parent() == Some(d)
                        && f.file_name()
                            .is_some_and(|n| n.to_string_lossy().to_ascii_lowercase().contains("mmproj"))
                })
            })
            .cloned();
        let image_pixels = projector.as_deref().map(image_budget);
        let modified = std::fs::metadata(p)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_millis() as u64);
        let repo = repo_of(dir, p);
        if out.iter().any(|o| o.id == id) {
            continue;
        }
        out.push(LocalFile {
            id,
            path: p.clone(),
            size,
            info,
            projector,
            image_pixels,
            modified,
            repo,
        });
    }
    out
}

/// The pixel budget of the projector at `path`; [`gguf::FALLBACK_IMAGE_PIXELS`] when its header cannot be read.
fn image_budget(path: &Path) -> u64 {
    gguf::inspect_projector(path).map_or(gguf::FALLBACK_IMAGE_PIXELS, |p| p.max_image_pixels())
}

fn split_total(first: &Path, all: &[PathBuf]) -> u64 {
    let name = first.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let base = pitwall_core::ids::strip_split_suffix(name.trim_end_matches(".gguf")).to_string();
    all.iter()
        .filter(|f| f.parent() == first.parent())
        .filter(|f| {
            let n = f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            n.trim_end_matches(".gguf") == name.trim_end_matches(".gguf")
                || (pitwall_core::ids::split_part(n.trim_end_matches(".gguf")).is_some()
                    && pitwall_core::ids::strip_split_suffix(n.trim_end_matches(".gguf")) == base)
        })
        .filter_map(|f| std::fs::metadata(f).ok())
        .map(|m| m.len())
        .sum()
}

fn repo_of(root: &Path, p: &Path) -> String {
    let rel = p.strip_prefix(root).unwrap_or(p);
    let parts: Vec<_> = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    if parts.len() >= 3 {
        format!("{}/{}", parts[0], parts[1])
    } else {
        String::new()
    }
}

/// Where the provenance of a model file is recorded (next to it, see `pitwall_core::updates`).
pub fn sidecar_path(model: &Path) -> PathBuf {
    let name = model.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    model.with_file_name(updates::sidecar_name(&name))
}

/// Size facts for the fit check from a scanned file.
pub fn size_of(f: &LocalFile) -> ModelSize {
    let proj = f.projector.as_ref().and_then(|p| std::fs::metadata(p).ok()).map_or(0, |m| m.len());
    ModelSize::from_header(&f.info, proj)
}

/// The library over a models folder.
#[derive(Clone)]
pub struct Library {
    dirs: Dirs,
    egress: Egress,
    downloads: Arc<Mutex<Downloads>>,
}

struct Slot {
    dl: Download,
    cancel: CancelFlag,
}

#[derive(Default)]
struct Downloads {
    next: u64,
    items: HashMap<String, Slot>,
}

impl Library {
    /// A library.
    pub fn new(dirs: Dirs, egress: Egress) -> Library {
        Library {
            dirs,
            egress,
            downloads: Arc::new(Mutex::new(Downloads::default())),
        }
    }

    /// Models on disk.
    pub fn local(&self) -> Vec<LocalFile> {
        scan(&self.dirs.models())
    }

    /// The downloaded models that came from a Hugging Face repository, with what was recorded when they were
    /// downloaded (models from before that carry no record and are compared by size). Reads files; call it off the
    /// async threads.
    pub fn installed(&self) -> Vec<Installed> {
        let root = self.dirs.models();
        self.local()
            .iter()
            .filter_map(|f| {
                let rel = f.path.strip_prefix(root.join(&f.repo)).ok()?;
                let file = rel
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/");
                let sidecar = std::fs::read_to_string(sidecar_path(&f.path))
                    .ok()
                    .filter(|t| t.len() <= updates::MAX_PROVENANCE_BYTES);
                Installed::from_disk(
                    &f.id.to_string(),
                    &f.repo,
                    &file,
                    f.info.quant.as_deref(),
                    f.size,
                    sidecar.as_deref(),
                )
            })
            .collect()
    }

    /// Delete a model (all parts and its folder when empty).
    pub fn delete(&self, id: &ModelId) -> std::io::Result<()> {
        let all = self.local();
        let f = all
            .iter()
            .find(|f| &f.id == id)
            .ok_or_else(|| std::io::Error::other("no such model"))?;
        let dir = f.path.parent().map(Path::to_path_buf).unwrap_or_default();
        let name = f.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let base = pitwall_core::ids::strip_split_suffix(name.trim_end_matches(".gguf")).to_string();
        for e in std::fs::read_dir(&dir)?.filter_map(Result::ok) {
            let n = e.file_name().to_string_lossy().into_owned();
            let stem = n.trim_end_matches(".gguf");
            if n.ends_with(".gguf")
                && (stem == name.trim_end_matches(".gguf")
                    || (pitwall_core::ids::split_part(stem).is_some() && pitwall_core::ids::strip_split_suffix(stem) == base))
            {
                std::fs::remove_file(e.path())?;
            }
        }
        let _ = std::fs::remove_file(sidecar_path(&f.path));
        let _ = std::fs::remove_dir(&dir);
        Ok(())
    }

    /// Hugging Face search (GGUF models).
    pub async fn search(&self, query: &str) -> Result<Vec<hf::HfModel>, DownloadError> {
        let url = hf::search_url(query)?;
        let v = self.egress.get_json(&url, MAX_JSON).await?;
        hf::parse_search(&v).map_err(DownloadError::Network)
    }

    /// A repository's quantisation variants with a fit verdict for each.
    pub async fn repo(&self, repo: &str, context: u32, room: &Room) -> Result<HfRepo, DownloadError> {
        let id = RepoId::parse(repo).map_err(DownloadError::Network)?;
        let tree = self.egress.get_json(&hf::tree_url(&id)?, MAX_JSON).await?;
        let files = hf::parse_tree(&tree).map_err(DownloadError::Network)?;
        let (license, gated) = match self.egress.get_json(&hf::info_url(&id)?, MAX_JSON).await {
            Ok(v) => hf::parse_info(&v),
            Err(_) => (None, false),
        };
        let have = self.local();
        // The fit is for one slot at `context`; `max_slots` says how many parallel requests that context allows.
        let variant = |v: &hf::Variant| {
            let size = ModelSize::from_file(v.size, v.quant.as_deref());
            QuantVariant {
                file: v.file.clone(),
                quant: v.quant.clone(),
                size_bytes: v.size,
                parts: v.parts.len() as u32,
                sha256: if v.sha256.len() == 1 { v.sha256.first().cloned() } else { None },
                fit: fit::assess(Some(&size), context, room),
                max_slots: slots::max_slots(&size, context, room, false),
                downloaded: have.iter().any(|h| h.repo == repo && h.path.ends_with(&v.file)),
            }
        };
        Ok(HfRepo {
            repo: repo.into(),
            variants: files.variants.iter().map(variant).collect(),
            projectors: files.projectors.iter().map(variant).collect(),
            license,
            gated,
        })
    }

    /// The listing of downloads (newest first).
    pub fn downloads(&self) -> Vec<Download> {
        let g = self.downloads.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut v: Vec<Download> = g.items.values().map(|s| s.dl.clone()).collect();
        v.sort_by(|a, b| b.started_at.cmp(&a.started_at).then(b.id.cmp(&a.id)));
        v
    }

    /// Cancel a download (a partial file is kept for resuming).
    pub fn cancel(&self, id: &str) -> bool {
        let g = self.downloads.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        g.items.get(id).map(|s| s.cancel.cancel()).is_some()
    }

    fn update(&self, id: &str, f: impl FnOnce(&mut Download)) {
        let mut g = self.downloads.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(s) = g.items.get_mut(id) {
            f(&mut s.dl);
        }
    }

    /// Start downloading a model file (all parts of a split model) into the library. Returns the download id; the
    /// work runs in a background task and `on_change` is called with every state change.
    pub fn download(
        &self,
        repo: &str,
        variant: &hf::Variant,
        on_change: impl Fn(Download) + Send + Sync + 'static,
    ) -> Result<String, DownloadError> {
        self.download_after(None, repo, variant, on_change)
    }

    /// [`Library::download`] that stays queued until `gate` resolves (llama.cpp finishing its own install); a gate
    /// that fails fails the download with that message.
    pub fn download_after(
        &self,
        gate: Option<crate::engines::BoxFuture<'static, Result<(), String>>>,
        repo: &str,
        variant: &hf::Variant,
        on_change: impl Fn(Download) + Send + Sync + 'static,
    ) -> Result<String, DownloadError> {
        let repo_id = RepoId::parse(repo).map_err(DownloadError::Network)?;
        let urls: Vec<(DownloadUrl, String)> = variant
            .parts
            .iter()
            .map(|p| hf::file_url(&repo_id, p).map(|u| (u, p.clone())).map_err(DownloadError::Network))
            .collect::<Result<_, _>>()?;
        let host = urls.first().map(|(u, _)| u.host().to_string()).unwrap_or_default();
        let (id, cancel) = {
            let mut g = self.downloads.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            g.next += 1;
            let id = format!("dl{}", g.next);
            let cancel = CancelFlag::default();
            let dl = Download {
                id: id.clone(),
                kind: DownloadKind::Model,
                name: variant.file.rsplit('/').next().unwrap_or(&variant.file).to_string(),
                host,
                bytes_done: 0,
                bytes_total: variant.size,
                speed_bps: 0,
                state: DownloadState::Queued,
                error: None,
                started_at: now_ms(),
            };
            g.items.insert(
                id.clone(),
                Slot {
                    dl,
                    cancel: cancel.clone(),
                },
            );
            (id, cancel)
        };
        let this = self.clone();
        let on_change = Arc::new(on_change);
        let (id2, repo, shas, total) = (id.clone(), repo.to_string(), variant.sha256.clone(), variant.size);
        let first_file = variant.file.clone();
        tokio::spawn(async move {
            let notify = |lib: &Library| {
                if let Some(s) = lib
                    .downloads
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .items
                    .get(&id2)
                {
                    on_change(s.dl.clone());
                }
            };
            if let Some(gate) = gate {
                if let Err(m) = gate.await {
                    this.update(&id2, |d| {
                        d.state = DownloadState::Failed;
                        d.error = Some(m);
                    });
                    notify(&this);
                    return;
                }
            }
            this.update(&id2, |d| d.state = DownloadState::Downloading);
            notify(&this);
            // The repository's commit now, for the provenance record (best effort: a download never waits on it).
            let revision = match hf::info_url(&repo_id) {
                Ok(u) => this
                    .egress
                    .get_json(&u, 1 << 20)
                    .await
                    .ok()
                    .and_then(|v| updates::parse_revision(&v)),
                Err(_) => None,
            };
            let mut digests: Vec<Option<String>> = vec![];
            let mut base = 0u64;
            let started = std::time::Instant::now();
            let mut result: Result<(), DownloadError> = Ok(());
            for (i, (url, rel)) in urls.iter().enumerate() {
                let dest = this.dirs.models().join(&repo).join(rel);
                let part = dest.with_extension("gguf.part");
                if dest.is_file() && std::fs::metadata(&dest).map(|m| m.len()).unwrap_or(0) > 0 && shas.get(i).is_none() {
                    digests.push(None);
                    continue;
                }
                let sha = shas.get(i).map(String::as_str);
                let (lib, id3, b) = (this.clone(), id2.clone(), base);
                let mut last = std::time::Instant::now();
                let r = this
                    .egress
                    .download(url, &part, sha, &cancel, |done, t| {
                        if last.elapsed() > std::time::Duration::from_millis(400) {
                            last = std::time::Instant::now();
                            let secs = started.elapsed().as_secs_f64().max(0.001);
                            lib.update(&id3, |d| {
                                d.bytes_done = b + done;
                                d.bytes_total = total.max(b + t.unwrap_or(0));
                                d.speed_bps = ((b + done) as f64 / secs) as u64;
                            });
                            if let Some(s) = lib
                                .downloads
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner)
                                .items
                                .get(&id3)
                            {
                                on_change(s.dl.clone());
                            }
                        }
                    })
                    .await;
                match r {
                    Ok(digest) => {
                        digests.push(Some(digest));
                        this.update(&id2, |d| d.state = DownloadState::Verifying);
                        if let Err(e) = verify_gguf(&part) {
                            let _ = std::fs::remove_file(&part);
                            result = Err(DownloadError::Io(e));
                            break;
                        }
                        if let Err(e) = std::fs::rename(&part, &dest) {
                            result = Err(DownloadError::Io(e.to_string()));
                            break;
                        }
                        base += std::fs::metadata(&dest).map(|m| m.len()).unwrap_or(0);
                    }
                    Err(e) => {
                        result = Err(e);
                        break;
                    }
                }
            }
            if result.is_ok() {
                let parts: Vec<&str> = urls.iter().map(|(_, rel)| rel.as_str()).collect();
                record_provenance(&this.dirs.models(), &repo, &first_file, &parts, digests, revision);
            }
            this.update(&id2, |d| match &result {
                Ok(()) => {
                    d.state = DownloadState::Done;
                    d.bytes_done = d.bytes_total;
                }
                Err(DownloadError::Cancelled) => d.state = DownloadState::Cancelled,
                Err(e) => {
                    d.state = DownloadState::Failed;
                    d.error = Some(e.to_string());
                }
            });
            notify(&this);
        });
        Ok(id)
    }
}

/// Writes the provenance record next to a finished download: the repository, the file, the total size of its parts on
/// disk, the SHA-256 of every part (only when every one is known) and the repository commit. Best effort: without
/// the record the model is compared by size at the next update check.
fn record_provenance(models: &Path, repo: &str, first: &str, parts: &[&str], digests: Vec<Option<String>>, revision: Option<String>) {
    let size: u64 = parts
        .iter()
        .filter_map(|rel| std::fs::metadata(models.join(repo).join(rel)).ok())
        .map(|m| m.len())
        .sum();
    let sha256 = if digests.iter().all(Option::is_some) {
        digests.into_iter().flatten().collect()
    } else {
        vec![]
    };
    let record = Provenance::new(repo, first, size, sha256, revision, now_ms());
    let _ = std::fs::write(sidecar_path(&models.join(repo).join(first)), record.to_json());
}

/// A downloaded model must parse as GGUF before it enters the library.
fn verify_gguf(p: &Path) -> Result<(), String> {
    gguf::inspect(p)
        .map(|_| ())
        .map_err(|e| format!("the file is not a usable GGUF model ({}); it was discarded", e.0))
}

/// The `LocalModel` DTO for the UI.
pub fn to_dto(
    f: &LocalFile,
    loaded_slots: Option<Slots>,
    context_per_slot: Option<u32>,
    prefs: &pitwall_core::config::ModelPrefs,
    defaults: (u32, Slots),
    room: &Room,
) -> LocalModel {
    let (context, default_slots) = defaults;
    let size = size_of(f);
    let slots = prefs.slots.unwrap_or(default_slots);
    let loaded = loaded_slots.is_some();
    LocalModel {
        id: f.id.to_string(),
        repo: f.repo.clone(),
        file: f.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        name: f.info.name.clone().unwrap_or_else(|| f.id.to_string()),
        quant: f.info.quant.clone(),
        size_bytes: f.size,
        params_b: Some(f.info.params_b).filter(|p| *p > 0.0),
        context_max: f.info.context_length,
        architecture: Some(f.info.architecture.clone()),
        capabilities: capabilities(f),
        projector: f
            .projector
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned()),
        image_pixels: f.image_pixels,
        added_at: f.modified,
        last_used_at: None,
        loaded,
        pinned: prefs.pinned,
        idle_secs: prefs.idle_secs,
        slots: slots.get(),
        slots_custom: prefs.slots.is_some(),
        loaded_slots: loaded_slots.map(Slots::get),
        context_per_slot,
        fit: slots::assess_slots(Some(&size), prefs.context.unwrap_or(context), slots, room),
    }
}

/// What a model can do, from its header and projector.
pub fn capabilities(f: &LocalFile) -> Vec<pitwall_core::select::Capability> {
    pitwall_core::select::capabilities_of(
        f.info.kind,
        f.projector.is_some(),
        f.info.chat_template.as_deref(),
        &f.id.to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_skips_damaged_and_non_gguf_files() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("o/r")).unwrap();
        std::fs::write(d.path().join("o/r/bad.gguf"), b"not a gguf").unwrap();
        std::fs::write(d.path().join("o/r/notes.txt"), b"x").unwrap();
        assert!(scan(d.path()).is_empty());
        assert_eq!(repo_of(d.path(), &d.path().join("o/r/a.gguf")), "o/r");
        assert_eq!(repo_of(d.path(), &d.path().join("a.gguf")), "");
    }

    #[test]
    fn a_finished_download_records_where_it_came_from() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join("o/r");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("m-00001-of-00002.gguf"), [0u8; 10]).unwrap();
        std::fs::write(dir.join("m-00002-of-00002.gguf"), [0u8; 5]).unwrap();
        let sha = "a".repeat(64);
        let parts = ["m-00001-of-00002.gguf", "m-00002-of-00002.gguf"];
        record_provenance(
            d.path(),
            "o/r",
            parts[0],
            &parts,
            vec![Some(sha.clone()), Some(sha.clone())],
            Some("1".repeat(40)),
        );
        let p = Provenance::parse(&std::fs::read_to_string(sidecar_path(&dir.join(parts[0]))).unwrap()).unwrap();
        assert_eq!(
            (p.size, p.sha256.len(), p.file.as_str(), p.revision.is_some()),
            (15, 2, parts[0], true)
        );
        // A part whose digest is unknown (resumed earlier): no hashes recorded rather than a partial list.
        record_provenance(d.path(), "o/r", parts[0], &parts, vec![Some(sha), None], None);
        let p = Provenance::parse(&std::fs::read_to_string(sidecar_path(&dir.join(parts[0]))).unwrap()).unwrap();
        assert!(p.sha256.is_empty() && p.revision.is_none());
        assert_eq!(sidecar_path(Path::new("/x/m.gguf")), Path::new("/x/m.gguf.origin.json"));
    }

    /// A GGUF header with `kvs` (a string architecture, then u32 and bool values), padded to look like a real file.
    fn header(arch: &str, u32s: &[(&str, u32)], bools: &[(&str, bool)]) -> Vec<u8> {
        let mut b = b"GGUF".to_vec();
        b.extend(3u32.to_le_bytes());
        b.extend(0u64.to_le_bytes());
        b.extend(((1 + u32s.len() + bools.len()) as u64).to_le_bytes());
        let key = |b: &mut Vec<u8>, k: &str| {
            b.extend((k.len() as u64).to_le_bytes());
            b.extend(k.as_bytes());
        };
        key(&mut b, "general.architecture");
        b.extend(8u32.to_le_bytes());
        b.extend((arch.len() as u64).to_le_bytes());
        b.extend(arch.as_bytes());
        for (k, v) in u32s {
            key(&mut b, k);
            b.extend(4u32.to_le_bytes());
            b.extend(v.to_le_bytes());
        }
        for (k, v) in bools {
            key(&mut b, k);
            b.extend(7u32.to_le_bytes());
            b.push(u8::from(*v));
        }
        b.extend(vec![0u8; 70_000]);
        b
    }

    /// ADR 0023 WP-5: the projector's pixel budget (`ProjectorInfo::max_image_pixels`, which nothing called before) reaches
    /// the model list, so the app downscales an image to what this model's encoder takes. A projector whose header cannot be
    /// read still gets the default budget; a model without one has none.
    #[test]
    fn a_vision_model_lists_its_projectors_image_budget() {
        let d = tempfile::tempdir().unwrap();
        let vl = d.path().join("o/vl");
        let plain = d.path().join("o/plain");
        let odd = d.path().join("o/odd");
        for (dir, name) in [(&vl, "vl"), (&plain, "plain"), (&odd, "odd")] {
            std::fs::create_dir_all(dir).unwrap();
            std::fs::write(dir.join(format!("{name}-Q4_K_M.gguf")), header("llama", &[], &[])).unwrap();
        }
        let proj = header(
            "clip",
            &[
                ("clip.vision.image_size", 768),
                ("clip.vision.patch_size", 16),
                ("clip.vision.spatial_merge_size", 2),
            ],
            &[("clip.has_vision_encoder", true)],
        );
        std::fs::write(vl.join("mmproj-vl-f16.gguf"), proj).unwrap();
        std::fs::write(odd.join("mmproj-odd-f16.gguf"), header("clip", &[], &[])).unwrap();
        let files = scan(d.path());
        let budget = |repo: &str| files.iter().find(|f| f.repo == repo).map(|f| f.image_pixels);
        assert_eq!(budget("o/vl"), Some(Some(gguf::MAX_IMAGE_TOKENS * 32 * 32)));
        assert_eq!(budget("o/plain"), Some(None));
        assert_eq!(budget("o/odd"), Some(Some(gguf::FALLBACK_IMAGE_PIXELS)));
        let f = files.iter().find(|f| f.repo == "o/vl").unwrap();
        let dto = to_dto(
            f,
            None,
            None,
            &pitwall_core::config::ModelPrefs::default(),
            (4096, Slots::default()),
            &Room::default(),
        );
        assert_eq!(dto.image_pixels, Some(gguf::MAX_IMAGE_TOKENS * 32 * 32));
    }

    #[test]
    fn a_download_must_be_a_gguf() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("x.part");
        std::fs::write(&p, b"<html>gated</html>").unwrap();
        assert!(verify_gguf(&p).unwrap_err().contains("not a usable GGUF"));
    }

    #[tokio::test]
    async fn unreachable_search_and_repo_fail_and_downloads_fail_cleanly() {
        let d = tempfile::tempdir().unwrap();
        let lib = Library::new(Dirs::at(d.path()), Egress::unreachable());
        assert!(lib.search("qwen").await.is_err());
        assert!(lib.repo("o/r", 4096, &Room::default()).await.is_err());
        let v = hf::Variant {
            file: "m-Q4_K_M.gguf".into(),
            parts: vec!["m-Q4_K_M.gguf".into()],
            quant: Some("Q4_K_M".into()),
            size: 10,
            sha256: vec![],
        };
        let id = lib.download("o/r", &v, |_| {}).unwrap();
        for _ in 0..100 {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            if lib.downloads()[0].state == DownloadState::Failed {
                break;
            }
        }
        let dl = &lib.downloads()[0];
        assert_eq!((dl.id.as_str(), dl.state), (id.as_str(), DownloadState::Failed));
        assert!(!dl.error.as_ref().unwrap().is_empty());
        assert!(lib.cancel(&id) && !lib.cancel("nope"));
        assert!(lib.download("../x", &v, |_| {}).is_err());
    }

    #[tokio::test]
    async fn a_download_waits_queued_for_its_gate_and_fails_with_the_gates_reason() {
        let d = tempfile::tempdir().unwrap();
        let lib = Library::new(Dirs::at(d.path()), Egress::unreachable());
        let v = hf::Variant {
            file: "m-Q4_K_M.gguf".into(),
            parts: vec!["m-Q4_K_M.gguf".into()],
            quant: Some("Q4_K_M".into()),
            size: 10,
            sha256: vec![],
        };
        let (tx, rx) = tokio::sync::oneshot::channel::<Result<(), String>>();
        let gate: crate::engines::BoxFuture<'static, Result<(), String>> = Box::pin(async move { rx.await.unwrap_or(Ok(())) });
        lib.download_after(Some(gate), "o/r", &v, |_| {}).unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert_eq!(lib.downloads()[0].state, DownloadState::Queued);
        tx.send(Err("llama.cpp could not be installed. Press Retry.".into())).unwrap();
        for _ in 0..100 {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            if lib.downloads()[0].state == DownloadState::Failed {
                break;
            }
        }
        let dl = &lib.downloads()[0];
        assert_eq!(dl.state, DownloadState::Failed);
        assert!(dl.error.as_ref().unwrap().contains("Retry"));
    }
}
