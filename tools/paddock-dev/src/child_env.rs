//! The environment of a child that must not hold the queue's credentials (docs/plans/merge-queue.md): the queue's tokens, git
//! credentials and anything else named like a secret are removed, git reads no global or system configuration.

/// The variables named as credentials, whatever their value.
const NAMED: [&str; 10] = [
    "GITEA_TOKEN",
    "QUEUE_BOT_TOKEN",
    "PLAN_TOKEN",
    "GITHUB_TOKEN",
    "GIT_ASKPASS",
    "SSH_ASKPASS",
    "GIT_CONFIG_PARAMETERS",
    "GIT_CONFIG_COUNT",
    "ACTIONS_RUNTIME_TOKEN",
    "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
];

/// Words in a variable's name that say it is a secret.
const WORDS: [&str; 6] = ["TOKEN", "SECRET", "PASSWORD", "CREDENTIAL", "AUTH", "PRIVATE_KEY"];

/// Whether a variable is a credential or names itself a secret (any case): the queue's tokens, git credentials and configuration
/// that can carry one, and any name holding one of [`WORDS`].
pub fn is_secret(name: &str) -> bool {
    let up = name.to_ascii_uppercase();
    NAMED.contains(&up.as_str())
        || up.starts_with("GIT_CONFIG_KEY_")
        || up.starts_with("GIT_CONFIG_VALUE_")
        || WORDS.iter().any(|w| up.contains(w))
}

/// The variables and configuration a child must not see: the queue's tokens, git credentials, and any other variable whose name
/// says it is a secret ([`is_secret`]). `parent` is the environment names of this process.
pub fn scrub(cmd: &mut std::process::Command, parent: &[String]) {
    for name in parent {
        if is_secret(name) {
            cmd.env_remove(name);
        }
    }
    for n in NAMED {
        cmd.env_remove(n);
    }
    // No credential helper or extra header from any git configuration the job brought.
    let null = if cfg!(windows) { "NUL" } else { "/dev/null" };
    cmd.env("GIT_CONFIG_GLOBAL", null)
        .env("GIT_CONFIG_SYSTEM", null)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0");
}

/// Variables a child of the push gate may see besides the base list (every gate but the rows of CI's own tests, which take CI's job
/// environment less the secrets instead): the caller's account (CI's runner has it, and tests read it: paddock-env's
/// macOS ACL test writes `user:$USER`; ER-47), the toolchain, Docker, caches and Paddock's own settings.
pub const PASS: &[&str] = &[
    "USER",
    "LOGNAME",
    "CARGO_*",
    "RUSTUP_*",
    "RUSTFLAGS",
    "RUSTC_WRAPPER",
    "DOCKER*",
    "PADDOCK_*",
    "HEAVY_*",
    "XWIN_*",
    "SCCACHE_*",
    "NODE_*",
    "CC",
    "CXX",
    "CLANG*",
    "LIBRARY_PATH",
    "SDKROOT",
    "TMPDIR",
    "NPM_CONFIG_*",
    "npm_config_*",
];
