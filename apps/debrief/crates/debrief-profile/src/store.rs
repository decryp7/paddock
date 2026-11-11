//! The profiling port the app shell talks to, split by purpose (interface
//! segregation): [`CaptureCatalog`] opens and lists captures,
//! [`ProfileQueries`] answers the CPU/allocation views and
//! [`HeapQueries`] the memory views. [`Captures`] is the in-memory
//! implementation; queries never mutate (command–query separation), only
//! `open` and `close` do.

use crate::api::*;
use crate::error::{ProfileError, Result};
use crate::heap::ops::{self as heap_ops, Heap};
use crate::heap::{HeapAnalysis, HeapSnapshot};
use crate::import::{import_with, sniff_with, Capture, CaptureImporter};
use crate::limits::query::TREE_CACHE;
use crate::limits::Limits;
use crate::model::{MetricId, Profile};
use crate::ops::diff::Side;
use crate::ops::tree::{CallTree, CompiledView};
use crate::ops::{butterfly, diff, findings, flame, hotspots, nav, timeline};
use debrief_budget::{MemoryBudget, Release, Ticket};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

/// Captures open at once (each can be large).
pub const MAX_OPEN: usize = 32;

/// Opening, listing and closing captures.
pub trait CaptureCatalog: Send + Sync {
    /// Imports a capture file (bytes read by the caller from a path the user chose).
    fn open(&mut self, file_name: &str, path: Option<&str>, bytes: &[u8]) -> Result<CaptureSummary>;
    fn list(&self) -> Vec<CaptureSummary>;
    /// The importer that recognises a file from its name and first bytes, to import it outside the store (an import can
    /// take minutes and must not hold the store: see [`CaptureCatalog::adopt`]).
    fn pick_importer(&self, file_name: &str, head: &[u8]) -> Option<Arc<dyn CaptureImporter>>;
    /// The error for a file no importer recognises (it names the formats that are supported).
    fn not_recognised(&self, file_name: &str) -> ProfileError;
    /// Whether another capture can be added now (checked before a long import, so it is not wasted).
    fn check_room(&self) -> Result<()>;
    /// Adds a capture imported outside the store (named like [`CaptureCatalog::open`] names its captures).
    fn adopt(&mut self, file_name: &str, capture: Capture, path: Option<&str>) -> Result<CaptureSummary>;
    /// What it takes to read capture `id` again when the memory budget released its body: `None` when it is in memory.
    /// Checks that the file is the one that was opened ([`ProfileError::Changed`]). Cheap; call it under a read lock, run
    /// [`Reload::load`] with no lock held and give the result to [`install_reload`](Self::install_reload).
    fn reload_plan(&self, id: u32) -> Result<Option<Reload>>;
    /// Puts a capture read by [`Reload::load`] back as the body of the capture it was made for (a short call).
    fn install_reload(&mut self, reload: &Reload, capture: Capture) -> Result<()>;
    /// Forgets a capture; false when it was not open.
    fn close(&mut self, id: u32) -> bool;
    /// Wall-clock spans of the captures, to line them up with logs.
    fn spans(&self) -> Vec<ProfileSpan>;
    /// What a file is (profile, heap snapshot or neither) from its name and
    /// first bytes, without importing it. No side effects.
    fn sniff(&self, file_name: &str, head: &[u8]) -> SniffKind;
}

/// The CPU / allocation / wall-clock views of profiles.
pub trait ProfileQueries: Send + Sync {
    fn hotspots(&self, req: &HotspotsRequest) -> Result<HotspotsPage>;
    fn tree(&self, req: &TreeRequest) -> Result<TreePage>;
    fn hot_path(&self, req: &HotPathRequest) -> Result<HotPath>;
    /// The hottest top-down paths to a function ("show in tree", "zoom flame graph to it").
    fn frame_paths(&self, req: &FramePathsRequest) -> Result<FramePaths>;
    fn butterfly(&self, req: &ButterflyRequest) -> Result<Butterfly>;
    fn flame(&self, req: &FlameRequest) -> Result<FlameGraph>;
    fn diff_flame(&self, req: &DiffFlameRequest) -> Result<FlameGraph>;
    fn function_diff(&self, req: &FunctionDiffRequest) -> Result<FunctionDiffPage>;
    fn thread_timeline(&self, req: &ThreadTimelineRequest) -> Result<ThreadTimeline>;
    fn findings(&self, req: &FindingsRequest) -> Result<Vec<ProfileFinding>>;
    /// Profile samples in a wall-clock window (the Moment view).
    fn moment(&self, req: &ProfileMomentRequest) -> Result<Vec<ProfileMoment>>;
}

/// The memory views of heap snapshots.
pub trait HeapQueries: Send + Sync {
    fn heap_types(&self, req: &HeapTypesRequest) -> Result<HeapTypesPage>;
    fn heap_diff(&self, req: &HeapDiffRequest) -> Result<HeapDiffPage>;
    fn heap_growth(&self, req: &HeapGrowthRequest) -> Result<HeapGrowthPage>;
    fn heap_dominators(&self, req: &HeapDominatorsRequest) -> Result<HeapObjectsPage>;
    fn heap_instances(&self, req: &HeapInstancesRequest) -> Result<HeapObjectsPage>;
    fn heap_paths(&self, req: &HeapPathsRequest) -> Result<HeapPaths>;
    /// Automatic findings over a snapshot (growth against an optional baseline).
    fn heap_findings(&self, req: &HeapFindingsRequest) -> Result<Vec<HeapFinding>>;
}

/// Everything the shell needs: one value per window.
pub trait ProfileStore: CaptureCatalog + ProfileQueries + HeapQueries {}
impl<T: CaptureCatalog + ProfileQueries + HeapQueries> ProfileStore for T {}

/// What an open capture holds in memory.
#[derive(Clone)]
enum Body {
    Profile(Arc<Profile>),
    Heap {
        snapshot: Arc<HeapSnapshot>,
        analysis: Option<Arc<HeapAnalysis>>,
    },
}

impl Body {
    fn approx_bytes(&self) -> u64 {
        match self {
            Body::Profile(p) => p.approx_bytes(),
            Body::Heap { snapshot, analysis } => snapshot.approx_bytes() + analysis.as_ref().map_or(0, |a| a.approx_bytes()),
        }
    }

    fn of(capture: Capture) -> Body {
        match capture {
            Capture::Profile(p) => Body::Profile(Arc::new(p)),
            Capture::Heap(h) => {
                let analysis = h.graph.as_ref().map(|g| Arc::new(HeapAnalysis::new(g, h.types.len())));
                Body::Heap {
                    snapshot: Arc::new(h),
                    analysis,
                }
            }
        }
    }
}

/// The body of one capture, which the memory budget can release (it is read again from the file on the next use).
struct Resident {
    slot: Mutex<Option<Body>>,
}

impl Resident {
    fn get(&self) -> Option<Body> {
        self.slot.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    fn set(&self, body: Body) {
        *self.slot.lock().unwrap_or_else(|p| p.into_inner()) = Some(body);
    }
}

impl Release for Resident {
    fn release(&self) {
        *self.slot.lock().unwrap_or_else(|p| p.into_inner()) = None;
    }
}

/// What identifies the file a capture was opened from, to see that a released capture is read again from the same file:
/// its length, its modification time and a hash of its first 64 KiB.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FileStamp {
    len: u64,
    modified: Option<std::time::SystemTime>,
    head: u64,
}

impl FileStamp {
    fn of(path: &std::path::Path) -> std::io::Result<FileStamp> {
        use std::hash::{Hash, Hasher};
        let meta = std::fs::metadata(path)?;
        let head = crate::import::read_head(path)?;
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        head.hash(&mut hasher);
        Ok(FileStamp {
            len: meta.len(),
            modified: meta.modified().ok(),
            head: hasher.finish(),
        })
    }
}

/// Reading a released capture again from its file: planned under a read lock ([`CaptureCatalog::reload_plan`]), loaded
/// with no lock held ([`Reload::load`]: an import can take minutes and the file is lent to the importer by ranges, never
/// read into the app), installed under a short write lock ([`CaptureCatalog::install_reload`]).
pub struct Reload {
    id: u32,
    name: String,
    path: std::path::PathBuf,
    importer: Arc<dyn CaptureImporter>,
    limits: Limits,
    stamp: FileStamp,
}

impl Reload {
    /// Imports the file again through the importer that opened it, and checks that the file did not change meanwhile.
    pub fn load(&self) -> Result<Capture> {
        let file_name = crate::frames::basename(&self.path.to_string_lossy()).to_string();
        let capture = self
            .importer
            .import_file(&file_name, &self.path, &self.limits)
            .map_err(|e| self.unreadable(&e.to_string()))?;
        self.check_unchanged()?;
        Ok(capture)
    }

    fn check_unchanged(&self) -> Result<()> {
        check_unchanged(&self.name, &self.path, &self.stamp)
    }

    fn unreadable(&self, why: &str) -> ProfileError {
        unreadable(&self.name, why)
    }
}

/// Fails with [`ProfileError::Changed`] unless the file at `path` is the one `stamp` was taken of.
fn check_unchanged(name: &str, path: &std::path::Path, stamp: &FileStamp) -> Result<()> {
    match FileStamp::of(path) {
        Ok(now) if now == *stamp => Ok(()),
        Ok(_) => Err(ProfileError::Changed { name: name.to_string() }),
        Err(e) => Err(unreadable(name, &e.to_string())),
    }
}

fn unreadable(name: &str, why: &str) -> ProfileError {
    ProfileError::Budget {
        message: format!(
            "\"{name}\" was released from memory to make room for other captures and could not be read again ({why}); open it again"
        ),
    }
}

/// The most reloads one view makes: its own capture, and one more for each other capture the budget released to make room
/// for it while the view held several (a diff of two captures needs both).
const MAX_RELOADS: usize = 6;

/// Runs a view on the store under a read lock, and when a capture it needs was released reads it again first, with no
/// lock held: the plan under a read lock, the import with none, the result under a short write lock. A reload of
/// minutes therefore never blocks the other views, opens or closes of the window.
pub fn read_reloading<T>(store: &std::sync::RwLock<Box<dyn ProfileStore>>, view: impl Fn(&dyn ProfileStore) -> Result<T>) -> Result<T> {
    let read = || store.read().unwrap_or_else(|p| p.into_inner());
    for _ in 0..MAX_RELOADS {
        let id = match view(read().as_ref()) {
            Err(ProfileError::Released(id)) => id,
            other => return other,
        };
        let plan = read().reload_plan(id)?;
        if let Some(plan) = plan {
            let started = std::time::Instant::now();
            let capture = plan.load()?;
            log::debug!("capture {id} imported again in {} ms", started.elapsed().as_millis());
            store.write().unwrap_or_else(|p| p.into_inner()).install_reload(&plan, capture)?;
        }
    }
    Err(ProfileError::Budget {
        message: "the captures this view needs do not fit in memory together; close some captures or raise the memory for open documents"
            .to_string(),
    })
}

/// An open capture: what the lists need without the body (so a released capture still lists), and the body.
struct Entry {
    summary: CaptureSummary,
    span: Option<ProfileSpan>,
    heap: bool,
    name: String,
    path: Option<String>,
    /// The file as it was when the capture was opened (none without a file, or when it could not be read).
    stamp: Option<FileStamp>,
    resident: Arc<Resident>,
    /// Its place in the memory budget, when the store has one.
    ticket: Option<Ticket>,
}

type Built = Arc<(CompiledView, CallTree)>;

/// The in-memory profile store.
pub struct Captures {
    importers: Vec<Arc<dyn CaptureImporter>>,
    limits: Limits,
    next_id: u32,
    entries: BTreeMap<u32, Entry>,
    /// The app-wide memory budget for open captures and documents (none: unlimited, as in tests).
    budget: Option<Arc<MemoryBudget>>,
    /// Recently built call trees, most recent last.
    cache: Mutex<Vec<(u32, ProfileView, Built)>>,
}

impl Captures {
    pub fn new(importers: Vec<Arc<dyn CaptureImporter>>, limits: Limits) -> Captures {
        Captures {
            importers,
            limits,
            next_id: 1,
            entries: BTreeMap::new(),
            budget: None,
            cache: Mutex::new(Vec::new()),
        }
    }

    /// Keeps the open captures within `budget`, shared with the other windows and the dumps: the least recently used
    /// captures that have a file are released and read again when used.
    pub fn with_budget(mut self, budget: Arc<MemoryBudget>) -> Captures {
        self.budget = Some(budget);
        self
    }

    /// Adds an already imported capture.
    pub fn insert(&mut self, capture: Capture, path: Option<&str>) -> Result<CaptureSummary> {
        self.check_room()?;
        let id = self.next_id;
        let body = Body::of(capture);
        let path = path.map(str::to_string);
        let (summary, span, heap, name) = describe(id, &body, path.as_deref());
        let stamp = path.as_deref().and_then(|p| FileStamp::of(std::path::Path::new(p)).ok());
        let resident = Arc::new(Resident {
            slot: Mutex::new(Some(body.clone())),
        });
        let ticket = match &self.budget {
            Some(budget) => {
                let release: Option<std::sync::Weak<dyn Release>> = path.as_ref().map(|_| {
                    let r: Arc<dyn Release> = resident.clone();
                    Arc::downgrade(&r)
                });
                let kind = if heap { "heap snapshot" } else { "profile" };
                Some(
                    budget
                        .admit(kind, &name, body.approx_bytes(), release)
                        .map_err(|e| ProfileError::Budget { message: e.to_string() })?,
                )
            }
            None => None,
        };
        self.next_id = self.next_id.wrapping_add(1).max(1);
        self.entries.insert(
            id,
            Entry {
                summary: summary.clone(),
                span,
                heap,
                name,
                path,
                stamp,
                resident,
                ticket,
            },
        );
        Ok(summary)
    }

    fn summary(&self, id: u32) -> Option<CaptureSummary> {
        self.entries.get(&id).map(|e| e.summary.clone())
    }

    /// The body of capture `id`, or [`ProfileError::Released`] when the budget released it: it is read again by the
    /// caller, with no lock held (see [`CaptureCatalog::reload_plan`]).
    fn body(&self, id: u32) -> Result<(&Entry, Body)> {
        let e = self.entries.get(&id).ok_or(ProfileError::UnknownCapture(id))?;
        if let Some(t) = &e.ticket {
            t.touch();
        }
        match e.resident.get() {
            Some(b) => Ok((e, b)),
            None => Err(ProfileError::Released(id)),
        }
    }

    fn profile(&self, id: u32) -> Result<Arc<Profile>> {
        match self.body(id)? {
            (_, Body::Profile(p)) => Ok(p),
            (_, Body::Heap { .. }) => Err(ProfileError::WrongCapture {
                id,
                expected: "profile",
                actual: "heap snapshot",
            }),
        }
    }

    fn heap(&self, id: u32) -> Result<(Arc<HeapSnapshot>, Option<Arc<HeapAnalysis>>)> {
        match self.body(id)? {
            (_, Body::Heap { snapshot, analysis }) => Ok((snapshot, analysis)),
            (_, Body::Profile(_)) => Err(ProfileError::WrongCapture {
                id,
                expected: "heap snapshot",
                actual: "profile",
            }),
        }
    }

    /// The profile and its call tree under `view` (cached).
    fn built(&self, id: u32, view: &ProfileView) -> Result<(Arc<Profile>, Built)> {
        let profile = self.profile(id)?;
        {
            let mut cache = self.cache.lock().unwrap_or_else(|p| p.into_inner());
            if let Some(i) = cache.iter().position(|(cid, v, _)| *cid == id && v == view) {
                let hit = cache.remove(i);
                let built = hit.2.clone();
                cache.push(hit);
                return Ok((profile, built));
            }
        }
        let compiled = CompiledView::compile(&profile, view)?;
        let tree = CallTree::build(&profile, &compiled);
        let built: Built = Arc::new((compiled, tree));
        let mut cache = self.cache.lock().unwrap_or_else(|p| p.into_inner());
        cache.push((id, view.clone(), built.clone()));
        if cache.len() > TREE_CACHE {
            cache.remove(0);
        }
        Ok((profile, built))
    }

    fn importer_refs(&self) -> Vec<&dyn CaptureImporter> {
        self.importers.iter().map(|i| i.as_ref()).collect()
    }
}

/// The summary, the wall-clock span and the name of a body, computed once when it is added.
fn describe(id: u32, body: &Body, path: Option<&str>) -> (CaptureSummary, Option<ProfileSpan>, bool, String) {
    let path = path.map(str::to_string);
    match body {
        Body::Profile(p) => {
            let mut kinds: Vec<_> = p.events.iter().map(|e| e.kind).collect();
            kinds.sort_by_key(|k| *k as u8);
            kinds.dedup();
            let summary = CaptureSummary::Profile(ProfileSummary {
                id,
                name: p.name.clone(),
                path,
                format: p.source.clone(),
                kind: p.kind,
                started_ms: p.started_ms,
                duration_ns: p.duration_ns,
                has_time: p.has_time,
                sample_count: p.samples.len() as u64,
                frame_count: p.frames.len() as u64,
                event_count: p.events.len() as u64,
                metrics: p.metrics.clone(),
                default_metric: p.default_metric(),
                threads: p.threads.clone(),
                event_kinds: kinds,
                notes: p.notes.clone(),
            });
            let span = p.started_ms.map(|s| ProfileSpan {
                id,
                name: p.name.clone(),
                capture: CaptureKind::Profile,
                kind: Some(p.kind),
                from_ms: s,
                to_ms: p.ended_ms().unwrap_or(s),
            });
            (summary, span, false, p.name.clone())
        }
        Body::Heap { snapshot: h, .. } => {
            let summary = CaptureSummary::Heap(HeapSummary {
                id,
                name: h.name.clone(),
                path,
                format: h.source.clone(),
                taken_ms: h.taken_ms,
                type_count: h.types.len() as u64,
                object_count: h.graph.as_ref().map(|g| g.len() as u64).unwrap_or_else(|| h.total_count()),
                total_bytes: h.total_bytes(),
                has_graph: h.graph.is_some(),
                notes: h.notes.clone(),
            });
            let span = h.taken_ms.map(|t| ProfileSpan {
                id,
                name: h.name.clone(),
                capture: CaptureKind::Heap,
                kind: None,
                from_ms: t,
                to_ms: t,
            });
            (summary, span, true, h.name.clone())
        }
    }
}

impl CaptureCatalog for Captures {
    fn open(&mut self, file_name: &str, path: Option<&str>, bytes: &[u8]) -> Result<CaptureSummary> {
        if self.entries.len() >= MAX_OPEN {
            return Err(ProfileError::LimitExceeded {
                what: "open captures",
                limit: MAX_OPEN as u64,
                hint: "close a capture first",
            });
        }
        let capture = import_with(&self.importer_refs(), file_name, bytes, &self.limits)?;
        self.adopt(file_name, capture, path)
    }

    fn not_recognised(&self, file_name: &str) -> ProfileError {
        ProfileError::Unrecognised(
            file_name.to_string(),
            self.importers.iter().map(|i| i.name()).collect::<Vec<_>>().join(", "),
        )
    }

    fn check_room(&self) -> Result<()> {
        if self.entries.len() >= MAX_OPEN {
            return Err(ProfileError::LimitExceeded {
                what: "open captures",
                limit: MAX_OPEN as u64,
                hint: "close a capture first",
            });
        }
        Ok(())
    }

    fn pick_importer(&self, file_name: &str, head: &[u8]) -> Option<Arc<dyn CaptureImporter>> {
        let head = &head[..head.len().min(crate::import::SNIFF_BYTES)];
        self.importers
            .iter()
            .map(|i| (i.sniff(file_name, head).min(100), i))
            .filter(|(s, _)| *s > 0)
            .fold(None, |best: Option<(u8, &Arc<dyn CaptureImporter>)>, (s, i)| match best {
                Some((bs, _)) if bs >= s => best,
                _ => Some((s, i)),
            })
            .map(|(_, i)| i.clone())
    }

    fn adopt(&mut self, file_name: &str, mut capture: Capture, path: Option<&str>) -> Result<CaptureSummary> {
        let base = crate::frames::basename(file_name).to_string();
        match &mut capture {
            Capture::Profile(p) if p.name.is_empty() || p.name == p.source => p.name = base,
            Capture::Heap(h) if h.name.is_empty() || h.name == h.source => h.name = base,
            _ => {}
        }
        self.insert(capture, path)
    }

    fn list(&self) -> Vec<CaptureSummary> {
        self.entries.keys().filter_map(|&id| self.summary(id)).collect()
    }

    fn reload_plan(&self, id: u32) -> Result<Option<Reload>> {
        let e = self.entries.get(&id).ok_or(ProfileError::UnknownCapture(id))?;
        if e.resident.get().is_some() {
            return Ok(None);
        }
        let (Some(path), Some(stamp)) = (e.path.as_deref(), e.stamp.clone()) else {
            return Err(ProfileError::UnknownCapture(id));
        };
        let file = std::path::PathBuf::from(path);
        let unreadable = |why: &str| unreadable(&e.name, why);
        check_unchanged(&e.name, &file, &stamp)?;
        let head = crate::import::read_head(&file).map_err(|er| unreadable(&er.to_string()))?;
        let file_name = crate::frames::basename(path).to_string();
        let importer = self
            .pick_importer(&file_name, &head)
            .ok_or_else(|| unreadable("no importer recognises the file any more"))?;
        Ok(Some(Reload {
            id,
            name: e.name.clone(),
            path: file,
            importer,
            limits: self.limits,
            stamp,
        }))
    }

    fn install_reload(&mut self, reload: &Reload, capture: Capture) -> Result<()> {
        let e = self.entries.get(&reload.id).ok_or(ProfileError::UnknownCapture(reload.id))?;
        let body = Body::of(capture);
        if let Some(t) = &e.ticket {
            t.readmit(body.approx_bytes())
                .map_err(|o| ProfileError::Budget { message: o.to_string() })?;
        }
        e.resident.set(body.clone());
        log::info!(
            "capture {} read again from its file after it was released ({} MB)",
            reload.id,
            body.approx_bytes() >> 20
        );
        Ok(())
    }

    fn close(&mut self, id: u32) -> bool {
        let removed = self.entries.remove(&id).is_some();
        if removed {
            self.cache
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .retain(|(cid, _, _)| *cid != id);
        }
        removed
    }

    fn sniff(&self, file_name: &str, head: &[u8]) -> SniffKind {
        sniff_with(&self.importer_refs(), file_name, head)
    }

    fn spans(&self) -> Vec<ProfileSpan> {
        self.entries.values().filter_map(|e| e.span.clone()).collect()
    }
}

impl ProfileQueries for Captures {
    fn hotspots(&self, req: &HotspotsRequest) -> Result<HotspotsPage> {
        let (p, b) = self.built(req.profile_id, &req.view)?;
        hotspots::hotspots(&p, &b.1, &b.0, req)
    }

    fn tree(&self, req: &TreeRequest) -> Result<TreePage> {
        let (p, b) = self.built(req.profile_id, &req.view)?;
        nav::tree_page(&p, &b.1, req)
    }

    fn hot_path(&self, req: &HotPathRequest) -> Result<HotPath> {
        let (p, b) = self.built(req.profile_id, &req.view)?;
        nav::hot_path(&p, &b.1, req)
    }

    fn frame_paths(&self, req: &FramePathsRequest) -> Result<FramePaths> {
        let (p, b) = self.built(req.profile_id, &req.view)?;
        nav::frame_paths(&p, &b.1, req)
    }

    fn butterfly(&self, req: &ButterflyRequest) -> Result<Butterfly> {
        let (p, b) = self.built(req.profile_id, &req.view)?;
        butterfly::butterfly(&p, &b.1, &b.0, req)
    }

    fn flame(&self, req: &FlameRequest) -> Result<FlameGraph> {
        let (p, b) = self.built(req.profile_id, &req.view)?;
        flame::flame(&p, &b.1, req)
    }

    fn diff_flame(&self, req: &DiffFlameRequest) -> Result<FlameGraph> {
        let (bp, bb) = self.built(req.baseline_id, &req.view)?;
        let (tp, tb) = self.built(req.target_id, &req.view)?;
        let base = Side {
            profile: &bp,
            tree: &bb.1,
            view: &bb.0,
        };
        let target = Side {
            profile: &tp,
            tree: &tb.1,
            view: &tb.0,
        };
        diff::diff_flame(&base, &target, req)
    }

    fn function_diff(&self, req: &FunctionDiffRequest) -> Result<FunctionDiffPage> {
        let (bp, bb) = self.built(req.baseline_id, &req.view)?;
        let (tp, tb) = self.built(req.target_id, &req.view)?;
        let base = Side {
            profile: &bp,
            tree: &bb.1,
            view: &bb.0,
        };
        let target = Side {
            profile: &tp,
            tree: &tb.1,
            view: &tb.0,
        };
        diff::function_diff(&base, &target, req)
    }

    fn thread_timeline(&self, req: &ThreadTimelineRequest) -> Result<ThreadTimeline> {
        let p = self.profile(req.profile_id)?;
        let view = CompiledView::compile(&p, &req.view)?;
        timeline::thread_timeline(&p, &view, req)
    }

    fn findings(&self, req: &FindingsRequest) -> Result<Vec<ProfileFinding>> {
        let (tp, tb) = self.built(req.profile_id, &req.view)?;
        let target = Side {
            profile: &tp,
            tree: &tb.1,
            view: &tb.0,
        };
        let base_built = match req.baseline_id {
            Some(id) => Some(self.built(id, &ProfileView::default())?),
            None => None,
        };
        let base = base_built.as_ref().map(|(p, b)| Side {
            profile: p,
            tree: &b.1,
            view: &b.0,
        });
        let ctx = findings::Context {
            profile_id: req.profile_id,
            baseline_id: req.baseline_id,
            view: &req.view,
            thresholds: &req.thresholds,
        };
        findings::findings(&target, base.as_ref(), &ctx)
    }

    fn moment(&self, req: &ProfileMomentRequest) -> Result<Vec<ProfileMoment>> {
        if req.from_ms > req.to_ms {
            return Err(ProfileError::invalid("the window ends before it starts"));
        }
        let top = req.top.unwrap_or(5).min(50);
        let mut out = Vec::new();
        for (&id, e) in &self.entries {
            if e.heap {
                continue;
            }
            // From the summary, so a capture outside the window is not read back into memory.
            let (Some(start), Some(span)) = (e.span.as_ref().map(|s| s.from_ms), e.span.as_ref()) else {
                continue;
            };
            let end = span.to_ms;
            let CaptureSummary::Profile(summary) = &e.summary else { continue };
            if !summary.has_time || req.to_ms < start || req.from_ms > end {
                continue;
            }
            let profile = self.profile(id)?;
            let p = &*profile;
            let to_ns = |ms: i64| u64::try_from(ms.saturating_sub(start)).unwrap_or(0).saturating_mul(1_000_000);
            let range = TimeRange {
                from_ns: to_ns(req.from_ms),
                to_ns: to_ns(req.to_ms).min(p.duration_ns),
            };
            let view = ProfileView {
                range: Some(range),
                ..ProfileView::default()
            };
            let (_, b) = self.built(id, &view)?;
            let metric = p.default_metric();
            let k = b.1.metric_index(metric)?;
            let page = hotspots::hotspots(
                p,
                &b.1,
                &b.0,
                &HotspotsRequest {
                    profile_id: id,
                    view: view.clone(),
                    group_by: GroupBy::Function,
                    metric: Some(metric),
                    sort: None,
                    search: None,
                    offset: 0,
                    limit: top.max(1),
                },
            )?;
            out.push(ProfileMoment {
                profile_id: id,
                name: p.name.clone(),
                range,
                samples: b.1.samples_in_view,
                metric,
                measurement: p.metrics[k].measurement,
                value: b.1.grand[k],
                top: if top == 0 { Vec::new() } else { page.rows },
            });
        }
        Ok(out)
    }
}

impl HeapQueries for Captures {
    fn heap_types(&self, req: &HeapTypesRequest) -> Result<HeapTypesPage> {
        let (s, a) = self.heap(req.snapshot_id)?;
        heap_ops::types(
            &Heap {
                snapshot: &s,
                analysis: a.as_deref(),
            },
            req,
        )
    }

    fn heap_diff(&self, req: &HeapDiffRequest) -> Result<HeapDiffPage> {
        let (b, _) = self.heap(req.baseline_id)?;
        let (t, _) = self.heap(req.target_id)?;
        heap_ops::diff(&b, &t, req)
    }

    fn heap_growth(&self, req: &HeapGrowthRequest) -> Result<HeapGrowthPage> {
        let snaps = req
            .snapshot_ids
            .iter()
            .map(|&id| self.heap(id).map(|h| h.0))
            .collect::<Result<Vec<_>>>()?;
        let refs: Vec<&HeapSnapshot> = snaps.iter().map(|s| s.as_ref()).collect();
        heap_ops::growth(&refs, req)
    }

    fn heap_dominators(&self, req: &HeapDominatorsRequest) -> Result<HeapObjectsPage> {
        let (s, a) = self.heap(req.snapshot_id)?;
        heap_ops::dominators(
            &Heap {
                snapshot: &s,
                analysis: a.as_deref(),
            },
            req,
        )
    }

    fn heap_instances(&self, req: &HeapInstancesRequest) -> Result<HeapObjectsPage> {
        let (s, a) = self.heap(req.snapshot_id)?;
        heap_ops::instances(
            &Heap {
                snapshot: &s,
                analysis: a.as_deref(),
            },
            req,
        )
    }

    fn heap_paths(&self, req: &HeapPathsRequest) -> Result<HeapPaths> {
        let (s, a) = self.heap(req.snapshot_id)?;
        heap_ops::paths(
            &Heap {
                snapshot: &s,
                analysis: a.as_deref(),
            },
            req,
        )
    }

    fn heap_findings(&self, req: &HeapFindingsRequest) -> Result<Vec<HeapFinding>> {
        let (s, a) = self.heap(req.snapshot_id)?;
        let baseline = match req.baseline_id.filter(|&b| b != req.snapshot_id) {
            Some(b) => Some(self.heap(b)?.0),
            None => None,
        };
        Ok(crate::heap::findings::findings(
            &Heap {
                snapshot: &s,
                analysis: a.as_deref(),
            },
            baseline.as_deref(),
        ))
    }
}

/// Runs every view with ordinary arguments over a capture and discards the
/// results: used by fuzz targets and property tests to check that whatever
/// an importer produced is safe to query. Errors are fine; panics are bugs.
#[doc(hidden)]
pub fn fuzz_views(capture: Capture) {
    let mut s = Captures::new(Vec::new(), Limits::default());
    let Ok(summary) = s.insert(capture, None) else { return };
    let id = summary.id();
    match summary {
        CaptureSummary::Profile(p) => {
            let views = [
                ProfileView::default(),
                ProfileView {
                    just_my_code: Some(JustMyCode::default()),
                    recursion: RecursionMode::All,
                    range: Some(TimeRange {
                        from_ns: p.duration_ns / 4,
                        to_ns: p.duration_ns / 2,
                    }),
                    threads: (!p.threads.is_empty()).then(|| vec![0]),
                },
            ];
            for view in views {
                for group_by in [
                    GroupBy::Function,
                    GroupBy::Type,
                    GroupBy::Namespace,
                    GroupBy::Module,
                    GroupBy::Thread,
                ] {
                    let _ = s.hotspots(&HotspotsRequest {
                        profile_id: id,
                        view: view.clone(),
                        group_by,
                        metric: None,
                        sort: None,
                        search: Some("a".into()),
                        offset: 0,
                        limit: 50,
                    });
                }
                for direction in [TreeDirection::TopDown, TreeDirection::BottomUp] {
                    if let Ok(h) = s.hot_path(&HotPathRequest {
                        profile_id: id,
                        view: view.clone(),
                        direction,
                        path: Vec::new(),
                        metric: None,
                        min_fraction: Some(0.0),
                    }) {
                        let _ = s.tree(&TreeRequest {
                            profile_id: id,
                            view: view.clone(),
                            direction,
                            path: h.path.iter().take(3).copied().collect(),
                            metric: None,
                            offset: 0,
                            limit: 50,
                        });
                        if let Some(&f) = h.path.last() {
                            let _ = s.butterfly(&ButterflyRequest {
                                profile_id: id,
                                view: view.clone(),
                                frame: f,
                                metric: None,
                                limit: 20,
                            });
                            let _ = s.frame_paths(&FramePathsRequest {
                                profile_id: id,
                                view: view.clone(),
                                frame: f,
                                metric: None,
                                limit: Some(3),
                            });
                        }
                    }
                    let _ = s.flame(&FlameRequest {
                        profile_id: id,
                        view: view.clone(),
                        direction,
                        focus: Vec::new(),
                        viewport: Viewport { start: 0.1, end: 0.9 },
                        width_px: 1200.0,
                        min_width_px: None,
                        max_depth: None,
                        metric: None,
                        order: FlameOrder::Value,
                        search: Some("e".into()),
                        search_regex: false,
                        max_rects: Some(5000),
                    });
                }
                let _ = s.thread_timeline(&ThreadTimelineRequest {
                    profile_id: id,
                    view: view.clone(),
                    buckets: 100,
                    metric: None,
                    event_kinds: None,
                    max_events: Some(100),
                });
                let _ = s.findings(&FindingsRequest {
                    profile_id: id,
                    baseline_id: Some(id),
                    view: view.clone(),
                    thresholds: FindingThresholds::default(),
                });
                let _ = s.function_diff(&FunctionDiffRequest {
                    baseline_id: id,
                    target_id: id,
                    view: view.clone(),
                    group_by: GroupBy::Function,
                    metric: Some(MetricId::Samples),
                    sort: DiffSort::AbsDeltaSelf,
                    search: None,
                    offset: 0,
                    limit: 20,
                });
                let _ = s.diff_flame(&DiffFlameRequest {
                    baseline_id: id,
                    target_id: id,
                    view,
                    focus: Vec::new(),
                    viewport: Viewport::default(),
                    width_px: 800.0,
                    min_width_px: None,
                    max_depth: Some(64),
                    metric: None,
                    order: FlameOrder::Name,
                    search: None,
                    search_regex: false,
                    max_rects: Some(2000),
                });
            }
            if let Some(start) = p.started_ms {
                let _ = s.moment(&ProfileMomentRequest {
                    from_ms: start,
                    to_ms: start.saturating_add(10),
                    top: None,
                });
            }
        }
        CaptureSummary::Heap(h) => {
            for sort in [HeapSort::Name, HeapSort::Count, HeapSort::Bytes, HeapSort::Retained] {
                let _ = s.heap_types(&HeapTypesRequest {
                    snapshot_id: id,
                    sort,
                    search: None,
                    offset: 0,
                    limit: 50,
                });
            }
            let _ = s.heap_diff(&HeapDiffRequest {
                baseline_id: id,
                target_id: id,
                search: None,
                sort: HeapDiffSort::BytesDelta,
                offset: 0,
                limit: 50,
            });
            let _ = s.heap_growth(&HeapGrowthRequest {
                snapshot_ids: vec![id, id, id],
                search: None,
                steady_only: false,
                offset: 0,
                limit: 50,
            });
            let _ = s.heap_findings(&HeapFindingsRequest {
                snapshot_id: id,
                baseline_id: Some(id),
            });
            if let Ok(top) = s.heap_dominators(&HeapDominatorsRequest {
                snapshot_id: id,
                parent: None,
                offset: 0,
                limit: 20,
            }) {
                for r in top.rows.iter().take(3) {
                    let _ = s.heap_paths(&HeapPathsRequest {
                        snapshot_id: id,
                        object: r.object,
                        max_paths: Some(3),
                        max_depth: Some(64),
                    });
                    let _ = s.heap_instances(&HeapInstancesRequest {
                        snapshot_id: id,
                        type_name: r.type_name.clone(),
                        offset: 0,
                        limit: 10,
                    });
                }
            }
            let _ = h;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heap::dominators::tests::graph;
    use crate::import::tests::Lines;
    use crate::model::MetricId;

    fn store() -> Captures {
        Captures::new(vec![Arc::new(Lines(90))], Limits::default())
    }

    /// A folder with capture files of `Lines`' format, removed on drop.
    struct Dir(std::path::PathBuf, #[allow(dead_code)] tempfile::TempDir);
    impl Dir {
        fn new(tag: &str) -> Dir {
            let t = tempfile::Builder::new().prefix(&format!("debrief-profile-budget-{tag}-")).tempdir().unwrap();
            Dir(t.path().to_path_buf(), t)
        }
        fn file(&self, name: &str, stacks: usize) -> String {
            let text: String = std::iter::once("#t\n".to_string())
                .chain((0..stacks).map(|i| format!("main;f{i};g{i} {}\n", i + 1)))
                .collect();
            let path = self.0.join(name);
            std::fs::write(&path, text).unwrap();
            path.to_string_lossy().into_owned()
        }
    }

    fn open_file(s: &mut Captures, path: &str) -> Result<CaptureSummary> {
        let name = crate::frames::basename(path).to_string();
        s.open(&name, Some(path), &std::fs::read(path).unwrap())
    }

    fn summary_id(c: &CaptureSummary) -> u32 {
        match c {
            CaptureSummary::Profile(p) => p.id,
            CaptureSummary::Heap(h) => h.id,
        }
    }

    /// MEM-2: captures are kept within the app-wide budget; the least recently used one that has a file is released
    /// and read again from it when a view asks, and the other windows' captures count in the same budget.
    #[test]
    fn captures_over_the_budget_release_the_oldest_and_read_it_again() {
        let dir = Dir::new("lru");
        let (a, b, c) = (dir.file("a.folded", 200), dir.file("b.folded", 200), dir.file("c.folded", 200));
        // The size of one capture, from a store without a budget.
        let one = {
            let mut s = store();
            open_file(&mut s, &a).unwrap();
            s.profile(1).unwrap().approx_bytes()
        };
        assert!(one > 1_000, "a capture of 200 stacks has a size: {one}");
        let budget = debrief_budget::MemoryBudget::new(one * 5 / 2);
        let mut s = Captures::new(vec![Arc::new(Lines(90))], Limits::default()).with_budget(budget.clone());
        let ids: Vec<u32> = [&a, &b].iter().map(|p| summary_id(&open_file(&mut s, p).unwrap())).collect();
        assert_eq!(budget.used(), 2 * one);
        // The third does not fit beside two: the first (least recently used) is released.
        let third = summary_id(&open_file(&mut s, &c).unwrap());
        assert_eq!(budget.used(), 2 * one, "one capture was released");
        assert_eq!(s.list().len(), 3, "a released capture is still listed, with its summary");
        assert!(s.entries[&ids[0]].resident.get().is_none() && s.entries[&ids[1]].resident.get().is_some());
        // Asking for the released capture reads it from its file again and releases the next oldest.
        let hotspots = |id: u32| {
            move |s: &dyn ProfileStore| {
                s.hotspots(&HotspotsRequest {
                    profile_id: id,
                    view: ProfileView::default(),
                    group_by: GroupBy::Function,
                    metric: None,
                    sort: None,
                    search: None,
                    offset: 0,
                    limit: 5,
                })
            }
        };
        assert!(
            matches!(hotspots(ids[0])(&s), Err(ProfileError::Released(id)) if id == ids[0]),
            "a view without a reload says the capture was released"
        );
        reload(&mut s, ids[0]);
        assert!(!hotspots(ids[0])(&s).unwrap().rows.is_empty());
        assert_eq!(s.list().len(), 3);
        assert!(s.entries[&ids[0]].resident.get().is_some());
        assert!(s.entries[&ids[1]].resident.get().is_none(), "b was the least recently used then");
        assert!(s.entries[&third].resident.get().is_some());
        assert_eq!(budget.used(), 2 * one);
        // A released capture whose file is gone says which one and what to do.
        std::fs::remove_file(&b).unwrap();
        let e = s.reload_plan(ids[1]).err().unwrap();
        let text = e.to_string();
        assert!(text.contains("b.folded") && text.contains("open it again"), "{text}");
        assert_eq!(e.code(), crate::error::ErrorCode::Limit);
    }

    /// What [`read_reloading`] does for a released capture, without a lock: plan, load, install.
    fn reload(s: &mut Captures, id: u32) {
        let plan = s.reload_plan(id).unwrap().expect("the capture was released");
        let capture = plan.load().unwrap();
        s.install_reload(&plan, capture).unwrap();
    }

    /// `Lines` that counts how the file reached it, and can stop in `import_file` (the reload under test) or change the
    /// file there.
    struct Spy {
        inner: Lines,
        /// Calls of `import` (the whole file as bytes) and of `import_file` (the path).
        bytes: std::sync::atomic::AtomicUsize,
        files: std::sync::atomic::AtomicUsize,
        /// Armed: `import_file` says it is inside (on `entered`), then waits for `release`.
        stop: Mutex<Option<Gate>>,
        /// Armed: `import_file` runs this on the path before it returns (a change made while the file is read).
        meanwhile: Mutex<Option<Meanwhile>>,
    }

    /// The two ends of a stop: `entered` is sent when the importer is inside, `release` is waited for.
    type Gate = (std::sync::mpsc::Sender<()>, std::sync::mpsc::Receiver<()>);
    type Meanwhile = Box<dyn FnOnce(&std::path::Path) + Send>;

    impl Spy {
        fn new() -> Arc<Spy> {
            Arc::new(Spy {
                inner: Lines(90),
                bytes: Default::default(),
                files: Default::default(),
                stop: Mutex::new(None),
                meanwhile: Mutex::new(None),
            })
        }
        fn counts(&self) -> (usize, usize) {
            use std::sync::atomic::Ordering::SeqCst;
            (self.bytes.load(SeqCst), self.files.load(SeqCst))
        }
    }

    struct SpyHandle(Arc<Spy>);

    impl CaptureImporter for SpyHandle {
        fn id(&self) -> &str {
            self.0.inner.id()
        }
        fn name(&self) -> &str {
            self.0.inner.name()
        }
        fn sniff(&self, file_name: &str, head: &[u8]) -> u8 {
            self.0.inner.sniff(file_name, head)
        }
        fn import(&self, file_name: &str, bytes: &[u8], limits: &Limits) -> Result<Capture> {
            self.0.bytes.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.0.inner.import(file_name, bytes, limits)
        }
        fn import_file(&self, file_name: &str, path: &std::path::Path, limits: &Limits) -> Result<Capture> {
            self.0.files.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if let Some((entered, release)) = self.0.stop.lock().unwrap().take() {
                entered.send(()).unwrap();
                release.recv().unwrap();
            }
            let bytes = std::fs::read(path).unwrap();
            let capture = self.0.inner.import(file_name, &bytes, limits);
            if let Some(f) = self.0.meanwhile.lock().unwrap().take() {
                f(path);
            }
            capture
        }
    }

    /// Two captures where only one fits in the budget: the first is released when the second opens.
    fn one_released(dir: &Dir, spy: &Arc<Spy>) -> (Captures, [String; 2]) {
        let (a, b) = (dir.file("a.folded", 200), dir.file("b.folded", 200));
        let one = {
            let mut s = store();
            open_file(&mut s, &a).unwrap();
            s.profile(1).unwrap().approx_bytes()
        };
        let budget = debrief_budget::MemoryBudget::new(one * 3 / 2);
        let mut s = Captures::new(vec![Arc::new(SpyHandle(spy.clone()))], Limits::default()).with_budget(budget);
        open_file(&mut s, &a).unwrap();
        open_file(&mut s, &b).unwrap();
        assert!(s.entries[&1].resident.get().is_none(), "the first capture was released");
        (s, [a, b])
    }

    fn hotspots_of(id: u32) -> impl Fn(&dyn ProfileStore) -> Result<HotspotsPage> {
        move |s| {
            s.hotspots(&HotspotsRequest {
                profile_id: id,
                view: ProfileView::default(),
                group_by: GroupBy::Function,
                metric: None,
                sort: None,
                search: None,
                offset: 0,
                limit: 5,
            })
        }
    }

    /// MEM-2: a released capture is read again through the importer's `import_file` (a plugin's process is lent the file by
    /// ranges, the app reads none of it) and with no lock on the window's captures held meanwhile.
    #[test]
    fn a_released_capture_is_read_again_by_path_and_with_no_lock_held() {
        let dir = Dir::new("reload-lock");
        let spy = Spy::new();
        let (s, _) = one_released(&dir, &spy);
        let (entered_tx, entered) = std::sync::mpsc::channel();
        let (release, release_rx) = std::sync::mpsc::channel();
        *spy.stop.lock().unwrap() = Some((entered_tx, release_rx));
        let before = spy.counts();
        let lock: std::sync::RwLock<Box<dyn ProfileStore>> = std::sync::RwLock::new(Box::new(s));
        let hot = std::thread::scope(|scope| {
            let reader = scope.spawn(|| read_reloading(&lock, hotspots_of(1)));
            // The reload is inside the importer now: the window's captures are free for everyone else.
            entered.recv().unwrap();
            assert!(lock.try_write().is_ok(), "no lock is held while the capture is imported again");
            assert_eq!(lock.try_read().unwrap().list().len(), 2, "the other views carry on");
            release.send(()).unwrap();
            reader.join().unwrap()
        })
        .unwrap();
        assert!(!hot.rows.is_empty());
        let after = spy.counts();
        assert_eq!(after.1 - before.1, 1, "read again once, by its path");
        assert_eq!(after.0, before.0, "the app did not hand the importer the whole file");
    }

    /// MEM-2: a released capture is not read again from a file that is not the one that was opened.
    #[test]
    fn a_released_capture_whose_file_changed_is_not_read_again() {
        let dir = Dir::new("reload-changed");
        // Longer.
        let spy = Spy::new();
        let (s, [a, _]) = one_released(&dir, &spy);
        let mut longer = std::fs::read_to_string(&a).unwrap();
        longer.push_str("main;extra 1\n");
        std::fs::write(&a, longer).unwrap();
        let e = s.reload_plan(1).err().unwrap();
        assert!(matches!(e, ProfileError::Changed { .. }), "{e}");
        assert!(e.to_string().ends_with("changed since it was opened; open it again"), "{e}");
        assert_eq!(e.code(), crate::error::ErrorCode::Invalid);
        let lock: std::sync::RwLock<Box<dyn ProfileStore>> = std::sync::RwLock::new(Box::new(s));
        let failed = read_reloading(&lock, hotspots_of(1)).err().unwrap();
        assert!(matches!(failed, ProfileError::Changed { .. }), "the view fails with it: {failed}");
        assert_eq!(spy.counts().1, 0, "the changed file was not imported");
        // The same length and the same modification time, other bytes: the head hash sees it.
        let dir = Dir::new("reload-changed-head");
        let spy = Spy::new();
        let (s, [a, _]) = one_released(&dir, &spy);
        let modified = std::fs::metadata(&a).unwrap().modified().unwrap();
        let mut bytes = std::fs::read(&a).unwrap();
        let at = bytes.len() / 2;
        bytes[at] = if bytes[at] == b'x' { b'y' } else { b'x' };
        let (head_len, total) = (bytes.len().min(crate::import::SNIFF_BYTES), bytes.len());
        assert!(
            at < head_len && total == std::fs::metadata(&a).unwrap().len() as usize,
            "the change is inside the head"
        );
        std::fs::write(&a, bytes).unwrap();
        std::fs::OpenOptions::new()
            .write(true)
            .open(&a)
            .unwrap()
            .set_modified(modified)
            .unwrap();
        assert!(matches!(s.reload_plan(1).err(), Some(ProfileError::Changed { .. })));
        // Untouched: planned.
        let dir = Dir::new("reload-same");
        let (s, _) = one_released(&dir, &Spy::new());
        assert!(s.reload_plan(1).unwrap().is_some());
        assert!(s.reload_plan(2).unwrap().is_none(), "a capture in memory needs no reload");
    }

    /// MEM-2: a file that changes while it is being read again is not installed.
    #[test]
    fn a_file_that_changes_while_it_is_read_again_is_not_installed() {
        let dir = Dir::new("reload-meanwhile");
        let spy = Spy::new();
        let (s, _) = one_released(&dir, &spy);
        *spy.meanwhile.lock().unwrap() = Some(Box::new(|p| {
            let mut t = std::fs::read_to_string(p).unwrap();
            t.push_str("main;late 1\n");
            std::fs::write(p, t).unwrap();
        }));
        let lock: std::sync::RwLock<Box<dyn ProfileStore>> = std::sync::RwLock::new(Box::new(s));
        let e = read_reloading(&lock, hotspots_of(1)).err().unwrap();
        assert!(matches!(e, ProfileError::Changed { .. }), "{e}");
        assert_eq!(lock.read().unwrap().list().len(), 2);
    }

    /// A capture that has no file cannot be released: when it and the others cannot fit, the open fails and the message
    /// names the document and what to do (MEM-2).
    #[test]
    fn a_capture_that_does_not_fit_says_which_one_and_what_to_close() {
        let dir = Dir::new("pinned");
        let big = dir.file("big.folded", 300);
        let bytes = std::fs::read(&big).unwrap();
        let one = {
            let mut s = store();
            s.open("big.folded", None, &bytes).unwrap();
            s.profile(1).unwrap().approx_bytes()
        };
        let budget = debrief_budget::MemoryBudget::new(one * 3 / 2);
        let mut s = Captures::new(vec![Arc::new(Lines(90))], Limits::default()).with_budget(budget.clone());
        s.open("pasted.folded", None, &bytes).unwrap();
        let e = s.open("second.folded", None, &bytes).unwrap_err();
        let text = e.to_string();
        assert!(text.contains("second.folded") && text.contains("pasted.folded"), "{text}");
        assert!(
            text.contains("Close one of them") && text.contains("Memory for open documents"),
            "{text}"
        );
        assert_eq!(s.list().len(), 1, "the failed open added nothing");
        assert_eq!(budget.used(), one);
        // Closing frees the room.
        assert!(s.close(1));
        assert!(s.open("second.folded", None, &bytes).is_ok());
    }

    #[test]
    fn sniffs_without_opening_and_finds_frame_paths() {
        let mut s = Captures::new(vec![Arc::new(Lines(90)), Arc::new(crate::import::tests::Heaps)], Limits::default());
        assert_eq!(s.sniff("a", b"#t\nmain 1\n"), SniffKind::Capture);
        assert_eq!(s.sniff("a", b"#h"), SniffKind::Heap);
        assert_eq!(s.sniff("a.log", b"INFO hi"), SniffKind::None);
        assert!(s.list().is_empty(), "sniffing opens nothing");
        let id = s.open("p", None, b"#t\nmain;a 3\nmain;b;a 1\n").unwrap().id();
        let tree = s
            .tree(&TreeRequest {
                profile_id: id,
                view: ProfileView::default(),
                direction: TreeDirection::TopDown,
                path: Vec::new(),
                metric: None,
                offset: 0,
                limit: 10,
            })
            .unwrap();
        let main = tree.nodes[0].frame.id;
        let kids = s
            .tree(&TreeRequest {
                profile_id: id,
                view: ProfileView::default(),
                direction: TreeDirection::TopDown,
                path: vec![main],
                metric: None,
                offset: 0,
                limit: 10,
            })
            .unwrap();
        let a = kids.nodes.iter().find(|n| n.frame.name == "a").unwrap().frame.id;
        let paths = s
            .frame_paths(&FramePathsRequest {
                profile_id: id,
                view: ProfileView::default(),
                frame: a,
                metric: None,
                limit: None,
            })
            .unwrap();
        assert_eq!(paths.total_paths, 2);
        assert_eq!(paths.paths[0].path, vec![main, a]);
        assert!(matches!(
            s.frame_paths(&FramePathsRequest {
                profile_id: 99,
                view: ProfileView::default(),
                frame: a,
                metric: None,
                limit: None,
            }),
            Err(ProfileError::UnknownCapture(99))
        ));
    }

    #[test]
    fn open_list_query_close() {
        let mut s = store();
        let sum = s.open("/tmp/x.lines", Some("/tmp/x.lines"), b"#t\nmain;a 3\nmain;b 1\n").unwrap();
        let id = sum.id();
        let CaptureSummary::Profile(p) = &sum else { panic!() };
        assert_eq!(p.name, "x.lines");
        assert_eq!(p.sample_count, 2);
        assert_eq!(p.default_metric, MetricId::Samples);
        assert_eq!(s.list().len(), 1);
        assert!(s.spans().is_empty(), "no wall-clock start");
        let req = HotspotsRequest {
            profile_id: id,
            view: ProfileView::default(),
            group_by: GroupBy::Function,
            metric: None,
            sort: None,
            search: None,
            offset: 0,
            limit: 10,
        };
        let h = s.hotspots(&req).unwrap();
        assert_eq!(h.rows[0].name, "a");
        assert_eq!(s.hotspots(&req).unwrap(), h, "cached tree, same answer");
        for i in 0..TREE_CACHE + 1 {
            let mut r = req.clone();
            r.view.threads = Some(vec![0; i + 1]);
            s.hotspots(&r).unwrap();
        }
        assert!(s.cache.lock().unwrap().len() <= TREE_CACHE);
        assert!(s.close(id));
        assert!(!s.close(id));
        assert!(s.cache.lock().unwrap().is_empty());
        assert!(matches!(s.hotspots(&req), Err(ProfileError::UnknownCapture(_))));
        assert!(s.open("y.bin", None, b"nope").is_err());
    }

    #[test]
    fn wrong_kinds_and_limits() {
        let mut s = store();
        let heap = s.insert(Capture::Heap(graph(&[("A", 1, &[])], &[0])), None).unwrap();
        let prof = s.open("p", None, b"#t\na 1\n").unwrap();
        let e = s
            .hotspots(&HotspotsRequest {
                profile_id: heap.id(),
                view: ProfileView::default(),
                group_by: GroupBy::Function,
                metric: None,
                sort: None,
                search: None,
                offset: 0,
                limit: 10,
            })
            .unwrap_err();
        assert!(e.to_string().contains("is a heap snapshot, not a profile"), "{e}");
        let e = s
            .heap_types(&HeapTypesRequest {
                snapshot_id: prof.id(),
                sort: HeapSort::Bytes,
                search: None,
                offset: 0,
                limit: 10,
            })
            .unwrap_err();
        assert!(e.to_string().contains("not a heap snapshot"), "{e}");
        assert!(s
            .heap_diff(&HeapDiffRequest {
                baseline_id: 77,
                target_id: heap.id(),
                search: None,
                sort: HeapDiffSort::Name,
                offset: 0,
                limit: 1
            })
            .is_err());
        for _ in 0..MAX_OPEN - 2 {
            s.open("p", None, b"#t\na 1\n").unwrap();
        }
        assert!(matches!(s.open("p", None, b"#t\na 1\n"), Err(ProfileError::LimitExceeded { .. })));
        assert!(s.insert(Capture::Heap(graph(&[("A", 1, &[])], &[0])), None).is_err());
    }

    #[test]
    fn spans_and_moment_line_up_with_wall_clock() {
        let mut p = crate::ops::tree::tests::profile(&[(0, 0, "main;a", 1), (0, 5_000_000, "main;b", 2), (1, 9_000_000, "main;b", 1)]);
        p.started_ms = Some(1_000);
        let mut h = graph(&[("A", 1, &[])], &[0]);
        h.taken_ms = Some(1_500);
        let mut s = store();
        let id = s.insert(Capture::Profile(p), None).unwrap().id();
        s.insert(Capture::Heap(h), None).unwrap();
        let spans = s.spans();
        assert_eq!(spans.len(), 2);
        assert_eq!(
            (spans[0].from_ms, spans[0].to_ms, spans[0].capture),
            (1_000, 1_009, CaptureKind::Profile)
        );
        assert_eq!((spans[1].from_ms, spans[1].to_ms), (1_500, 1_500));
        let m = s
            .moment(&ProfileMomentRequest {
                from_ms: 1_004,
                to_ms: 1_006,
                top: Some(3),
            })
            .unwrap();
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].profile_id, id);
        assert_eq!(m[0].samples, 1);
        assert_eq!(m[0].top[0].name, "b");
        assert_eq!(m[0].range.from_ns, 4_000_000);
        assert!(s
            .moment(&ProfileMomentRequest {
                from_ms: 2_000,
                to_ms: 3_000,
                top: None
            })
            .unwrap()
            .is_empty());
        assert!(s
            .moment(&ProfileMomentRequest {
                from_ms: 5,
                to_ms: 1,
                top: None
            })
            .is_err());
        let none = s
            .moment(&ProfileMomentRequest {
                from_ms: 0,
                to_ms: 1_001,
                top: Some(0),
            })
            .unwrap();
        assert!(none[0].top.is_empty());
    }

    #[test]
    fn every_view_survives_odd_captures() {
        fuzz_views(Capture::Profile(crate::ops::tree::tests::profile(&[
            (0, 0, "a;a;b", 1),
            (2, 7, "fw.x;fw.y;c", 3),
        ])));
        fuzz_views(Capture::Heap(graph(&[("A", 1, &[1]), ("B", 2, &[0])], &[0])));
        let mut b = crate::heap::HeapBuilder::new("t", Limits::default());
        b.type_stat("X", 1, 1).unwrap();
        fuzz_views(Capture::Heap(b.finish().unwrap()));
    }
}
