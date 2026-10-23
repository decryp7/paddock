//! Avatars for watches: the owner's picture on a forge or the site's favicon, fetched politely from the watched site
//! itself, checked ([`spotter_core::avatar::check_image`]) and kept in an on-disk cache with an expiry. Offline, or
//! when a site has no picture, the mark bundled for the source kind is used, and the UI draws its monogram when there
//! is neither.
//!
//! Nothing here asks a third party about the user's watches: no favicon service, no search.
use crate::engine::Engine;
use crate::error::{EngineError, Result};
use crate::fetch::{FetchOptions, Fetched, PoliteFetcher};
use serde::{Deserialize, Serialize};
use spotter_core::avatar::{avatar_url, bundled_mark, cache_key, check_image, ImageKind, MAX_AVATAR_BYTES};
use spotter_core::source::{SourceConfig, SourceError};
use std::path::{Path, PathBuf};

/// How long a fetched picture is used before it is asked for again (7 days).
pub const FRESH_MS: i64 = 7 * 24 * 3600 * 1000;
/// How long "this site has no picture" is believed (6 hours).
pub const ABSENT_MS: i64 = 6 * 3600 * 1000;
/// After a failed refresh, the old picture is used for this long before the next try (1 hour).
pub const RETRY_MS: i64 = 3600 * 1000;
/// Most pictures kept on disk.
pub const MAX_ENTRIES: usize = 500;

/// Where a picture came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AvatarSource {
    /// Fetched from the watched site (possibly from the cache).
    Remote,
    /// Bundled with Spotter for the source kind.
    Bundled,
}

/// A picture to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvatarView {
    /// Where it came from.
    pub source: AvatarSource,
    /// Its media type.
    pub mime: &'static str,
    /// The checked image bytes.
    pub bytes: Vec<u8>,
}

impl AvatarView {
    /// The mark bundled for a source kind.
    pub fn bundled(kind: &str) -> Option<AvatarView> {
        bundled_mark(kind).map(|svg| AvatarView {
            source: AvatarSource::Bundled,
            mime: ImageKind::Svg.mime(),
            bytes: svg.as_bytes().to_vec(),
        })
    }

    /// The image as a `data:` address, which the UI's content security policy allows.
    pub fn data_url(&self) -> String {
        use base64::Engine as _;
        format!(
            "data:{};base64,{}",
            self.mime,
            base64::engine::general_purpose::STANDARD.encode(&self.bytes)
        )
    }
}

/// What the cache remembers about one address.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Meta {
    fetched_ms: i64,
    /// The image's format; `None` when the site had no usable picture.
    kind: Option<String>,
}

/// One cached answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// When it was fetched.
    pub fetched_ms: i64,
    /// The image, or `None` for "no usable picture".
    pub image: Option<(ImageKind, Vec<u8>)>,
}

impl Entry {
    /// Whether the answer can be used without asking again.
    pub fn fresh(&self, now_ms: i64) -> bool {
        let limit = if self.image.is_some() { FRESH_MS } else { ABSENT_MS };
        let age = now_ms.saturating_sub(self.fetched_ms);
        // A clock that went backwards makes the entry look newer than now: ask again rather than trust it.
        (0..limit).contains(&age)
    }
}

fn kind_name(k: ImageKind) -> &'static str {
    k.mime()
}

fn kind_of(name: &str) -> Option<ImageKind> {
    [ImageKind::Png, ImageKind::Jpeg, ImageKind::Gif, ImageKind::Ico, ImageKind::Svg]
        .into_iter()
        .find(|k| k.mime() == name)
}

/// The pictures on disk: `<key>.json` (when and what) next to `<key>.img` (the bytes), both written atomically.
/// Anything missing, unreadable or no longer a valid image is a miss, never an error.
#[derive(Debug, Clone)]
pub struct AvatarCache {
    dir: PathBuf,
}

impl AvatarCache {
    /// A cache in `dir` (created on the first write).
    pub fn new(dir: impl Into<PathBuf>) -> AvatarCache {
        AvatarCache { dir: dir.into() }
    }

    fn meta_path(&self, key: &str) -> PathBuf {
        self.dir.join(format!("{key}.json"))
    }

    fn image_path(&self, key: &str) -> PathBuf {
        self.dir.join(format!("{key}.img"))
    }

    /// The remembered answer for `key`.
    pub fn load(&self, key: &str) -> Option<Entry> {
        let meta: Meta = serde_json::from_slice(&std::fs::read(self.meta_path(key)).ok()?).ok()?;
        let Some(kind) = meta.kind.as_deref() else {
            return Some(Entry {
                fetched_ms: meta.fetched_ms,
                image: None,
            });
        };
        let bytes = std::fs::read(self.image_path(key)).ok()?;
        // The file may have been changed behind our back: it is checked again before it is ever shown.
        let checked = check_image(&bytes).ok()?;
        (kind_of(kind) == Some(checked)).then_some(Entry {
            fetched_ms: meta.fetched_ms,
            image: Some((checked, bytes)),
        })
    }

    /// Remembers `entry` for `key`, then trims the cache to [`MAX_ENTRIES`]. A disk that cannot be written to only
    /// means the picture is fetched again next time.
    pub fn store(&self, key: &str, entry: &Entry) {
        if std::fs::create_dir_all(&self.dir).is_err() {
            return;
        }
        let meta = Meta {
            fetched_ms: entry.fetched_ms,
            kind: entry.image.as_ref().map(|(k, _)| kind_name(*k).to_string()),
        };
        if let Some((_, bytes)) = &entry.image {
            if write_atomic(&self.image_path(key), bytes).is_err() {
                return;
            }
        } else {
            let _ = std::fs::remove_file(self.image_path(key));
        }
        if let Ok(json) = serde_json::to_vec(&meta) {
            let _ = write_atomic(&self.meta_path(key), &json);
        }
        self.trim();
    }

    /// Deletes the oldest entries beyond [`MAX_ENTRIES`].
    fn trim(&self) {
        let Ok(read) = std::fs::read_dir(&self.dir) else { return };
        let mut metas: Vec<(std::time::SystemTime, PathBuf)> = read
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
            .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
            .collect();
        if metas.len() <= MAX_ENTRIES {
            return;
        }
        metas.sort();
        for (_, path) in &metas[..metas.len() - MAX_ENTRIES] {
            let _ = std::fs::remove_file(path.with_extension("img"));
            let _ = std::fs::remove_file(path);
        }
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// The picture for a source: the cached or freshly fetched one from its site, else the bundled mark, else `None`
/// (the UI draws a monogram). Never fails: every problem means "use the fallback".
pub async fn avatar_for(
    cache: &AvatarCache,
    fetcher: &PoliteFetcher,
    source: &SourceConfig,
    allow_local: bool,
    now_ms: i64,
) -> Option<AvatarView> {
    let fallback = || AvatarView::bundled(&source.kind);
    let Some(address) = avatar_url(source) else { return fallback() };
    let key = cache_key(&address);
    let cached = cache.load(&key);
    if let Some(entry) = cached.as_ref().filter(|e| e.fresh(now_ms)) {
        return entry.image.as_ref().map(remote).or_else(fallback);
    }
    match fetch_image(fetcher, &address, allow_local).await {
        Outcome::Image(image) => {
            let entry = Entry {
                fetched_ms: now_ms,
                image: Some(image),
            };
            cache.store(&key, &entry);
            entry.image.as_ref().map(remote)
        }
        Outcome::Absent => {
            cache.store(
                &key,
                &Entry {
                    fetched_ms: now_ms,
                    image: None,
                },
            );
            fallback()
        }
        // Offline: nothing is remembered about the site, so the next request tries again. An old picture keeps being
        // shown meanwhile, and is not asked for again for an hour.
        Outcome::Offline => match cached.and_then(|e| e.image) {
            Some(old) => {
                let shown = remote(&old);
                cache.store(
                    &key,
                    &Entry {
                        fetched_ms: now_ms.saturating_sub(FRESH_MS - RETRY_MS),
                        image: Some(old),
                    },
                );
                Some(shown)
            }
            None => fallback(),
        },
    }
}

fn remote((kind, bytes): &(ImageKind, Vec<u8>)) -> AvatarView {
    AvatarView {
        source: AvatarSource::Remote,
        mime: kind.mime(),
        bytes: bytes.clone(),
    }
}

/// What asking the site produced.
enum Outcome {
    Image((ImageKind, Vec<u8>)),
    /// The site answered, but has no usable picture (not found, not an image, too large, refused).
    Absent,
    /// The site could not be reached.
    Offline,
}

/// Fetches and checks one picture.
async fn fetch_image(fetcher: &PoliteFetcher, address: &str, allow_local: bool) -> Outcome {
    let request = spotter_core::source::HttpRequest::get(address).header("Accept", "image/png, image/*;q=0.8");
    let opts = FetchOptions {
        allow_local,
        max_body: Some(MAX_AVATAR_BYTES),
        ..FetchOptions::default()
    };
    match fetcher.get(&request, &opts).await {
        Ok(Fetched::Body { bytes, .. }) => match check_image(&bytes) {
            Ok(kind) => Outcome::Image((kind, bytes)),
            Err(_) => Outcome::Absent,
        },
        Ok(Fetched::NotModified) | Err(SourceError::NotFound(_) | SourceError::InvalidResponse(_) | SourceError::InvalidConfig(_)) => {
            Outcome::Absent
        }
        Err(_) => Outcome::Offline,
    }
}

impl Engine {
    fn avatar_cache(&self) -> AvatarCache {
        AvatarCache::new(self.inner.cfg.data_dir.join("cache").join("avatars"))
    }

    /// The picture of a watch: from its site (cached), else the mark bundled for its source kind, else `None` (the UI
    /// draws a monogram). Asking has no effect on the watch; only the picture cache changes.
    pub async fn watch_avatar(&self, id: &str) -> Result<Option<AvatarView>> {
        let spec = self.find_spec(id)?;
        Ok(avatar_for(
            &self.avatar_cache(),
            &self.inner.fetcher,
            &spec.source,
            spec.allow_local,
            self.now().0,
        )
        .await)
    }

    /// The picture of a catalogue preset, like [`Engine::watch_avatar`].
    pub async fn preset_avatar(&self, id: &str) -> Result<Option<AvatarView>> {
        let preset = self
            .inner
            .pack
            .presets
            .iter()
            .find(|p| p.id == id)
            .ok_or_else(|| EngineError::NotFound(format!("There is no preset \"{id}\".")))?;
        Ok(avatar_for(&self.avatar_cache(), &self.inner.fetcher, &preset.spec.source, false, self.now().0).await)
    }

    /// The mark bundled for a source kind; it needs no network.
    pub fn kind_avatar(&self, kind: &str) -> Option<AvatarView> {
        AvatarView::bundled(kind)
    }
}
