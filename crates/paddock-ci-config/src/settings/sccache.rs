//! The setup derived from the variables, returned as data: the compile cache's S3 level and the environment of its server, and the
//! test-result cache's store address. Nothing here starts a server, writes `$GITHUB_ENV` or calls the store; the adapters in
//! `tools/paddock-dev` apply what these return.

use super::declared::{
    PADDOCK_CI_RELEASE, PADDOCK_SCCACHE_S3_ACCESS_KEY_ID, PADDOCK_SCCACHE_S3_BUCKET,
    PADDOCK_SCCACHE_S3_ENDPOINT, PADDOCK_SCCACHE_S3_REGION, PADDOCK_SCCACHE_S3_SECRET_ACCESS_KEY,
    PADDOCK_SCCACHE_S3_SSL, PADDOCK_TAG_REHEARSAL, PADDOCK_TEST_CACHE_KEY_ID,
    PADDOCK_TEST_CACHE_SECRET,
};
use super::{SettingError, Settings};
use paddock_env::Vars;
use paddock_secrets::Secret;

/// The one reading of `PADDOCK_SCCACHE_S3_ENDPOINT` (ER-22): the repository variable holds `http://host:port` (or `https://`), never a
/// bare `host:port` with the scheme added again and never a path. Returns whether the endpoint speaks TLS and its `host:port` with no
/// scheme and no trailing slash, the form sccache's own settings take (`SCCACHE_ENDPOINT` with `SCCACHE_S3_USE_SSL`). A bare
/// `host:port` is plain HTTP. An endpoint with a user, a password or a path is an error that says what the variable should hold.
pub fn parse_endpoint(endpoint: &str) -> Result<(bool, String), String> {
    let (ssl, host_port) = if let Some(rest) = endpoint.strip_prefix("https://") {
        (true, rest)
    } else {
        (false, endpoint.strip_prefix("http://").unwrap_or(endpoint))
    };
    let host_port = host_port.trim_end_matches('/').to_string();
    if host_port.contains('@') {
        // `user:password@host` would put a credential into a variable that is logged and into the health check's URL.
        return Err(
            "PADDOCK_SCCACHE_S3_ENDPOINT holds `@`; it is http://host:port with no user or password (the key is the two secrets)".into(),
        );
    }
    if host_port.contains("://") || host_port.contains('/') || !host_port.contains(':') {
        return Err(format!(
            "PADDOCK_SCCACHE_S3_ENDPOINT is `{endpoint}`; it is http://host:port (or https://), with no path"
        ));
    }
    Ok((ssl, host_port))
}

/// The store's endpoint: the variable, else its declared default, through [`parse_endpoint`].
pub fn s3_endpoint(s: &Settings<'_>) -> Result<(bool, String), String> {
    let endpoint = s
        .get_or_default(PADDOCK_SCCACHE_S3_ENDPOINT)
        .unwrap_or_default();
    parse_endpoint(&endpoint)
}

/// The S3 key prefix of a runner class: `paddock-ci/<RUNNER_OS>-<arch>` (`os` as `RUNNER_OS` spells it, `arch` as Rust's `ARCH`). The
/// one definition: CI's [`S3Ci`] and the agents' local server both use it.
pub fn ci_prefix(os: &str, arch: &str) -> String {
    format!("paddock-ci/{os}-{arch}")
}

/// CI's S3 level of the compile cache, from the step's environment. Its key is two repository secrets (one key for every runner class,
/// its policy scoped to `<bucket>/paddock-ci/*`); they never appear in `Debug` output or an error.
#[derive(Clone, PartialEq, Eq)]
pub struct S3Ci {
    /// `PADDOCK_SCCACHE_S3_BUCKET`.
    pub bucket: String,
    /// The `host:port` of `PADDOCK_SCCACHE_S3_ENDPOINT`.
    pub endpoint: String,
    /// The endpoint is `https://`.
    pub ssl: bool,
    /// One key prefix per runner class: `paddock-ci/<RUNNER_OS>-<arch>`, so one OS's objects can be dropped or expired alone.
    pub prefix: String,
    /// The access key id and secret, for the sccache server's start only.
    pub key: (Secret, Secret),
}

impl std::fmt::Debug for S3Ci {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("S3Ci")
            .field("bucket", &self.bucket)
            .field("endpoint", &self.endpoint)
            .field("prefix", &self.prefix)
            .field("key", &"<hidden>")
            .finish()
    }
}

/// Why a job does not use S3, and how loudly: a job whose workflow passes no key is not wired yet (a notice); a key that is
/// passed but wrong, or a store that does not answer, is a warning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct S3Off {
    /// `::warning::` rather than `::notice::`.
    pub warn: bool,
    /// One sentence, never with a key in it.
    pub why: String,
}

impl S3Ci {
    /// The S3 level from the step's environment, or why it cannot be used. The endpoint is a URL, `http://host:port` or
    /// `https://host:port` (a bare `host:port` is read as http).
    pub fn from_vars(vars: &dyn Vars, os: &str, arch: &str) -> Result<S3Ci, S3Off> {
        let s = Settings::new(vars);
        let warn = |why: String| S3Off { warn: true, why };
        // A release or tag build never reads the shared store: one key serves every branch, so a branch could write the entries a
        // release build then trusts (review of 534918839, P2: cache poisoning). Release workflows hold no key either (C141's
        // NO_SECRET list); this is the second wall, for a key that reaches such a job by another way. The trigger is the release
        // condition itself: a tag ref, a tag rehearsal, or `PADDOCK_CI_RELEASE` (setup-rust sets it from `kit.sh release-path`, the one
        // definition). Never PADDOCK_DEV_DOWNLOAD: setup-rust sets that one for any job with no read token too.
        let git_ref = vars
            .get("GITHUB_REF")
            .and_then(|r| r.into_string().ok())
            .map(|r| r.trim().to_string());
        let trigger = if let Some(r) = git_ref.filter(|r| r.starts_with("refs/tags/")) {
            Some(format!("a tag run (GITHUB_REF is {r})"))
        } else if s.is_on(PADDOCK_TAG_REHEARSAL) {
            Some("a tag rehearsal (PADDOCK_TAG_REHEARSAL=1)".to_string())
        } else if s.is_on(PADDOCK_CI_RELEASE) {
            Some(format!(
                "a release or rule 54 run (PADDOCK_CI_RELEASE={})",
                s.get(PADDOCK_CI_RELEASE).unwrap_or_default()
            ))
        } else {
            None
        };
        if let Some(trigger) = trigger {
            return Err(S3Off {
                warn: false,
                why: format!(
                    "a release or tag build never reads the shared S3 cache (any branch could have written to it); this is {trigger}"
                ),
            });
        }
        let secret = |d| s.secret(d).map_err(|e| warn(e.0));
        let (id, key) = match (
            secret(PADDOCK_SCCACHE_S3_ACCESS_KEY_ID)?,
            secret(PADDOCK_SCCACHE_S3_SECRET_ACCESS_KEY)?,
        ) {
            (Some(i), Some(k)) => (i, k),
            (None, None) => {
                return Err(S3Off {
                    warn: false,
                    why: "this workflow passes no S3 key to setup-rust (`sccache-s3-access-key-id`, `sccache-s3-secret-access-key`)".into(),
                });
            }
            (None, Some(_)) => {
                return Err(warn(
                    "the secret PADDOCK_SCCACHE_S3_ACCESS_KEY_ID is empty or not passed".into(),
                ));
            }
            (Some(_), None) => {
                return Err(warn(
                    "the secret PADDOCK_SCCACHE_S3_SECRET_ACCESS_KEY is empty or not passed".into(),
                ));
            }
        };
        let need = |d: &super::Decl| {
            s.get(d)
                .ok_or_else(|| warn(format!("the variable {} is not set", d.name)))
        };
        let bucket = need(PADDOCK_SCCACHE_S3_BUCKET)?;
        let endpoint = need(PADDOCK_SCCACHE_S3_ENDPOINT)?;
        let (ssl, host_port) = parse_endpoint(&endpoint).map_err(warn)?;
        Ok(S3Ci {
            bucket,
            endpoint: host_port,
            ssl,
            prefix: ci_prefix(os, arch),
            key: (id, key),
        })
    }
}

/// Where the test-result cache's store is: `base` is `http://host:port` or `https://host:port` (the scheme comes from the endpoint
/// variable and is never added a second time, run 9135: `http://http://...` answered 308).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestCacheAddress {
    /// `http://host:port` or `https://host:port`.
    pub base: String,
    /// The region requests are signed for.
    pub region: String,
}

/// The test cache's store address: the endpoint exactly as the compile cache reads it ([`s3_endpoint`]), TLS when the endpoint says
/// `https://` or `PADDOCK_SCCACHE_S3_SSL` is `true`, and the region (default `auto`).
pub fn test_cache_address(s: &Settings<'_>) -> Result<TestCacheAddress, String> {
    let (tls, host_port) = s3_endpoint(s)?;
    let ssl = tls || s.is_on(PADDOCK_SCCACHE_S3_SSL);
    Ok(TestCacheAddress {
        base: format!("{}://{host_port}", if ssl { "https" } else { "http" }),
        region: s
            .get_or_default(PADDOCK_SCCACHE_S3_REGION)
            .unwrap_or_default(),
    })
}

/// CI's own key for the test cache (a separate key from the compile cache's: user decision 2026-11-26; C141 lets only `./dev ci
/// test-cache` steps name its secrets). `Ok(None)` unless both halves are set: the machine's usual key source is the reader's
/// fallback.
pub fn test_cache_key(s: &Settings<'_>) -> Result<Option<(Secret, Secret)>, SettingError> {
    Ok(
        match (
            s.secret(PADDOCK_TEST_CACHE_KEY_ID)?,
            s.secret(PADDOCK_TEST_CACHE_SECRET)?,
        ) {
            (Some(id), Some(secret)) => Some((id, secret)),
            _ => None,
        },
    )
}

/// The settings of the two-level chain the SERVER starts with: the local disk, then the S3 store, a failed S3 write never fatal
/// (`l0`; sccache v0.19.0 docs/MultiLevel.md). One definition for CI's `s3` backend and the agents' local server.
pub fn multilevel_settings(
    dir: &str,
    size: &str,
    bucket: &str,
    endpoint: &str,
    ssl: bool,
    prefix: &str,
) -> Vec<(String, String)> {
    [
        ("SCCACHE_DIR", dir),
        ("SCCACHE_CACHE_SIZE", size),
        ("SCCACHE_MULTILEVEL_CHAIN", "disk,s3"),
        ("SCCACHE_MULTILEVEL_WRITE_ERROR_POLICY", "l0"),
        ("SCCACHE_BUCKET", bucket),
        ("SCCACHE_ENDPOINT", endpoint),
        ("SCCACHE_REGION", "auto"),
        ("SCCACHE_S3_USE_SSL", if ssl { "true" } else { "false" }),
        ("SCCACHE_S3_KEY_PREFIX", prefix),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect()
}

/// The two settings that keep the server we start the only one: it never exits on its own (a build that finds none would start one
/// with the build's environment, and for S3 without the keys, and that one would replace ours and fail), and with S3 there is no
/// instance-metadata lookup (with no keys it hangs for about 75 s before it fails). Builds get these too, so a server a build starts
/// anyway behaves the same way. Never a credential.
pub fn survival_vars(s3: bool) -> Vec<(String, String)> {
    let mut v = vec![("SCCACHE_IDLE_TIMEOUT".to_string(), "0".to_string())];
    if s3 {
        v.push(("AWS_EC2_METADATA_DISABLED".to_string(), "true".to_string()));
    }
    v
}

/// The names of the variables whose values a Rust crate bakes into its outputs with `env!` and that sccache must hash verbatim, so a cache
/// entry is shared across checkouts only for crates that do not embed the checkout's path (sccache docs/Rust.md at v0.20.1).
pub const VERBATIM_ENV_DEPS: &str = "CARGO_MANIFEST_DIR,CARGO_MANIFEST_PATH";

/// `SCCACHE_BASEDIRS` and `SCCACHE_BASEDIRS_VERBATIM_ENV_DEPS` for the given checkout and build-folder roots (ER-22: the one place CI's
/// setup-rust and the agents' local server both get them). sccache 0.20 strips the longest matching base from every path in a Rust
/// compile's key, so the same source at another absolute path (a CI slot, a random work folder, a git worktree) hits the same entry.
/// Separator: `;` on Windows hosts, `:` elsewhere (docs/Configuration.md at v0.20.1); only absolute paths (a relative one stops the
/// server from starting), each once, in the given order. No usable root gives no variables. `CARGO_MANIFEST_DIR` and
/// `CARGO_MANIFEST_PATH` stay verbatim in the key: 31 crates read them with `env!`, and a cached binary must not carry another
/// checkout's path.
pub fn basedir_vars(roots: &[String], windows: bool) -> Vec<(String, String)> {
    let absolute = |r: &str| {
        if windows {
            let b = r.as_bytes();
            b.len() >= 3
                && b[0].is_ascii_alphabetic()
                && b[1] == b':'
                && (b[2] == b'\\' || b[2] == b'/')
        } else {
            r.starts_with('/')
        }
    };
    let mut seen: Vec<&str> = Vec::new();
    for r in roots
        .iter()
        .map(|r| r.trim())
        .filter(|r| !r.is_empty() && absolute(r))
    {
        let r = if r.len() > 1 {
            r.trim_end_matches(['/', '\\'])
        } else {
            r
        };
        if !seen.contains(&r) {
            seen.push(r);
        }
    }
    if seen.is_empty() {
        return Vec::new();
    }
    let sep = if windows { ";" } else { ":" };
    vec![
        ("SCCACHE_BASEDIRS".to_string(), seen.join(sep)),
        (
            "SCCACHE_BASEDIRS_VERBATIM_ENV_DEPS".to_string(),
            VERBATIM_ENV_DEPS.to_string(),
        ),
    ]
}

/// The `CARGO_*` variables CI's jobs have that sccache hashes into every Rust key (sccache 0.20.1 `generate_hash_key`: all of them but a
/// few): `CARGO_INCREMENTAL=0` and `CARGO_TERM_COLOR=always`, which setup-rust exports to `$GITHUB_ENV` for every job that has the cache.
/// The agents' shim exports them for a rustc compile, so a local key equals CI's and the two share entries; cargo's own environment is
/// left alone (no colour codes in piped output, incremental builds stay). docs/benchmarks.md, "Local keys against CI's".
pub const CI_KEY_ENV: [(&str, &str); 2] =
    [("CARGO_INCREMENTAL", "0"), ("CARGO_TERM_COLOR", "always")];
