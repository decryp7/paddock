//! The environment of a child that must not hold the queue's credentials (docs/plans/merge-queue.md): the queue's tokens, git
//! credentials and anything else named like a secret are removed, git reads no global or system configuration.

/// The variables and configuration a child must not see: the queue's tokens, git credentials, and any other variable whose name
/// says it is a secret. `parent` is the environment names of this process.
pub fn scrub(cmd: &mut std::process::Command, parent: &[String]) {
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
    const WORDS: [&str; 6] = ["TOKEN", "SECRET", "PASSWORD", "CREDENTIAL", "AUTH", "PRIVATE_KEY"];
    for name in parent {
        let up = name.to_ascii_uppercase();
        if NAMED.contains(&up.as_str())
            || up.starts_with("GIT_CONFIG_KEY_")
            || up.starts_with("GIT_CONFIG_VALUE_")
            || WORDS.iter().any(|w| up.contains(w))
        {
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
