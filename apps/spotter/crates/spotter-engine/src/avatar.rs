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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{EngineConfig, Services};
    use crate::secrets::MemorySecrets;
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn png() -> Vec<u8> {
        let mut b = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13];
        b.extend_from_slice(b"IHDR");
        b.extend_from_slice(&16u32.to_be_bytes());
        b.extend_from_slice(&16u32.to_be_bytes());
        b.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
        b
    }

    async fn serve(body: Vec<u8>) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = listener.accept().await else { return };
                let body = body.clone();
                tokio::spawn(async move {
                    let mut buf = [0u8; 2048];
                    let _ = s.read(&mut buf).await;
                    let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                    let _ = s.write_all(head.as_bytes()).await;
                    let _ = s.write_all(&body).await;
                });
            }
        });
        base
    }

    fn fetcher() -> PoliteFetcher {
        let mut cfg = crate::fetch::FetchConfig::new("test");
        cfg.min_host_interval = std::time::Duration::ZERO;
        PoliteFetcher::new(cfg).unwrap()
    }

    #[tokio::test]
    async fn a_picture_is_fetched_cached_reused_and_replaced_by_the_mark_when_unusable() {
        let dir = tempfile::tempdir().unwrap();
        let cache = AvatarCache::new(dir.path().join("avatars"));
        let source = |base: &str| SourceConfig::new("feed", &[("url", &format!("{base}/feed.xml"))]);
        let good = serve(png()).await;
        let first = avatar_for(&cache, &fetcher(), &source(&good), true, 1000).await.unwrap();
        assert_eq!((first.source, first.mime), (AvatarSource::Remote, "image/png"));
        assert!(first.data_url().starts_with("data:image/png;base64,"));
        // Fresh: served from disk even when the site is gone.
        let again = avatar_for(
            &cache,
            &fetcher(),
            &SourceConfig::new("feed", &[("url", &format!("{good}/other"))]),
            true,
            2000,
        )
        .await;
        assert_eq!(again.unwrap(), first);

        let junk = serve(b"<html>not an image</html>".to_vec()).await;
        let mark = avatar_for(&cache, &fetcher(), &source(&junk), true, 1000).await.unwrap();
        assert_eq!(mark.source, AvatarSource::Bundled);
        // A watch that may not reach local addresses never asks this computer.
        let empty = AvatarCache::new(dir.path().join("elsewhere"));
        let refused = avatar_for(&empty, &fetcher(), &source(&good), false, 1000).await.unwrap();
        assert_eq!(refused.source, AvatarSource::Bundled);
        // A folder has no site, only the mark; an unknown kind has nothing.
        let folder = SourceConfig::new("folder", &[("path", "/srv")]);
        assert_eq!(
            avatar_for(&cache, &fetcher(), &folder, true, 0).await.unwrap().source,
            AvatarSource::Bundled
        );
        assert!(avatar_for(&cache, &fetcher(), &SourceConfig::new("nope", &[]), true, 0)
            .await
            .is_none());
    }

    #[test]
    fn the_cache_remembers_images_and_absence_and_forgets_damage() {
        let dir = tempfile::tempdir().unwrap();
        let cache = AvatarCache::new(dir.path());
        let image = Entry {
            fetched_ms: 10,
            image: Some((ImageKind::Png, png())),
        };
        cache.store("a", &image);
        assert_eq!(cache.load("a"), Some(image.clone()));
        assert!(image.fresh(10) && !image.fresh(10 + FRESH_MS) && !image.fresh(5));
        let absent = Entry {
            fetched_ms: 10,
            image: None,
        };
        cache.store("a", &absent);
        assert_eq!(cache.load("a"), Some(absent.clone()));
        assert!(absent.fresh(10 + ABSENT_MS - 1) && !absent.fresh(10 + ABSENT_MS));
        std::fs::write(dir.path().join("b.json"), b"{broken").unwrap();
        assert_eq!(cache.load("b"), None);
        cache.store("c", &image);
        std::fs::write(dir.path().join("c.img"), b"<svg onload='x()'/>").unwrap();
        assert_eq!(cache.load("c"), None, "a changed file is checked again and refused");
        std::fs::create_dir_all(dir.path().join("d.img")).unwrap();
        cache.store("d", &image);
        assert_eq!(cache.load("d"), None);
    }

    #[tokio::test]
    async fn the_engine_answers_for_kinds_unknown_ids_and_registry_presets() {
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::open(EngineConfig::new(dir.path(), "test"), Services::new(Arc::new(MemorySecrets::new()))).unwrap();
        assert_eq!(engine.kind_avatar("github").unwrap().source, AvatarSource::Bundled);
        assert!(engine.kind_avatar("nope").is_none());
        assert!(matches!(engine.watch_avatar("missing").await, Err(EngineError::NotFound(_))));
        assert!(matches!(engine.preset_avatar("missing").await, Err(EngineError::NotFound(_))));
        let mark = engine.preset_avatar("serde").await.unwrap().unwrap();
        assert_eq!(mark.source, AvatarSource::Bundled);
        let spec = spotter_core::watch::WatchSpec {
            id: String::new(),
            name: "Feed".into(),
            source: SourceConfig::new("folder", &[("path", &dir.path().display().to_string())]),
            ..engine.presets().iter().find(|p| p.id == "serde").unwrap().spec.clone()
        };
        let id = engine.save_watch(spec, &Default::default()).unwrap().id;
        assert_eq!(engine.watch_avatar(&id).await.unwrap().unwrap().source, AvatarSource::Bundled);
    }
}
