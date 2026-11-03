//! What a privileged [`ServiceSpec`](crate::ServiceSpec) gives up (ADR 0027 §1): the Windows privileges the SCM keeps in
//! the service's token and who may start it, and the systemd sandbox of a unit. Declared once next to the spec, rendered
//! by [`crate::render`] (`sc privs`, `sc sdset`, the `[Service]` keys) and checked after registering, so the helper's
//! strength cannot drift from what the ADR measured.

use crate::error::SpecError;
use crate::spec::MAX_NAME;

/// The service DACL that lets interactive users *start* the service and read its state, and nothing else: `SY` and `BA`
/// keep their usual rights, `IU` gets query status, start, interrogate and read control. Network logons get nothing.
pub const SDDL_USERS_MAY_START: &str =
    "D:(A;;CCLCSWRPWPDTLOCRRC;;;SY)(A;;CCDCLCSWRPWPDTLOCRSDRCWDWO;;;BA)(A;;LCRPLORC;;;IU)";

/// The systemd sandbox of a system unit (systemd.exec(5)). The fixed keys are the ADR 0027 list; the variable parts are
/// the capabilities, the socket families and the state and runtime folders.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Sandbox {
    capabilities: Vec<String>,
    address_families: Vec<String>,
    state_dir: Option<String>,
    runtime_dir: Option<String>,
}

impl Sandbox {
    /// A sandbox with no capability, no socket family and no folders yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// `CapabilityBoundingSet=` (`CAP_SYS_PTRACE`, …).
    #[must_use]
    pub fn capabilities<I, S>(mut self, caps: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.capabilities = caps.into_iter().map(Into::into).collect();
        self
    }

    /// `RestrictAddressFamilies=` (`AF_UNIX`, …).
    #[must_use]
    pub fn address_families<I, S>(mut self, families: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.address_families = families.into_iter().map(Into::into).collect();
        self
    }

    /// `StateDirectory=` (0700, under `/var/lib`).
    #[must_use]
    pub fn state_dir(mut self, name: &str) -> Self {
        self.state_dir = Some(name.to_string());
        self
    }

    /// `RuntimeDirectory=` (0755, under `/run`: the socket's root-owned folder).
    #[must_use]
    pub fn runtime_dir(mut self, name: &str) -> Self {
        self.runtime_dir = Some(name.to_string());
        self
    }

    /// The capabilities.
    pub fn capability_list(&self) -> &[String] {
        &self.capabilities
    }
    /// The socket families.
    pub fn family_list(&self) -> &[String] {
        &self.address_families
    }
    /// The state folder name.
    pub fn state_directory(&self) -> Option<&str> {
        self.state_dir.as_deref()
    }
    /// The runtime folder name.
    pub fn runtime_directory(&self) -> Option<&str> {
        self.runtime_dir.as_deref()
    }

    fn check(&self) -> Result<(), SpecError> {
        let word = |prefix: &str, v: &str| {
            v.len() > prefix.len()
                && v.len() <= MAX_NAME
                && v.starts_with(prefix)
                && v.chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        };
        if self.capabilities.iter().any(|c| !word("CAP_", c)) {
            return Err(SpecError::new(
                "sandbox",
                "has a capability that is not CAP_ followed by A-Z, 0-9 or _",
            ));
        }
        if self.address_families.is_empty() || self.address_families.iter().any(|f| !word("AF_", f))
        {
            return Err(SpecError::new(
                "sandbox",
                "needs at least one socket family, each AF_ followed by A-Z, 0-9 or _",
            ));
        }
        for d in [&self.state_dir, &self.runtime_dir].into_iter().flatten() {
            let ok = !d.is_empty()
                && d.len() <= 64
                && !d.starts_with(['-', '.'])
                && d.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
            if !ok {
                return Err(SpecError::new(
                    "sandbox",
                    "has a state or runtime folder that is not 1 to 64 lower-case letters, digits, '-' or '_'",
                ));
            }
        }
        Ok(())
    }
}

/// Everything a privileged service gives up, per OS.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Hardening {
    windows_privileges: Vec<String>,
    users_may_start: bool,
    sandbox: Option<Sandbox>,
}

impl Hardening {
    /// No hardening yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// The only privileges the SCM keeps in the service's token (`sc privs`); an empty list leaves the SCM default.
    #[must_use]
    pub fn windows_privileges<I, S>(mut self, privileges: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.windows_privileges = privileges.into_iter().map(Into::into).collect();
        self
    }

    /// Interactive users may start the service ([`SDDL_USERS_MAY_START`], `sc sdset`).
    #[must_use]
    pub fn users_may_start(mut self) -> Self {
        self.users_may_start = true;
        self
    }

    /// The systemd sandbox.
    #[must_use]
    pub fn sandbox(mut self, sandbox: Sandbox) -> Self {
        self.sandbox = Some(sandbox);
        self
    }

    /// The privileges the token keeps.
    pub fn privileges(&self) -> &[String] {
        &self.windows_privileges
    }
    /// Whether interactive users may start it.
    pub fn users_start(&self) -> bool {
        self.users_may_start
    }
    /// The systemd sandbox.
    pub fn systemd_sandbox(&self) -> Option<&Sandbox> {
        self.sandbox.as_ref()
    }

    pub(crate) fn check(&self) -> Result<(), SpecError> {
        for p in &self.windows_privileges {
            let ok = p.len() <= 64
                && p.starts_with("Se")
                && p.ends_with("Privilege")
                && p.len() > "SePrivilege".len()
                && p.chars().all(|c| c.is_ascii_alphanumeric());
            if !ok {
                return Err(SpecError::new(
                    "privileges",
                    "must be Windows privilege names like SeChangeNotifyPrivilege",
                ));
            }
        }
        if let Some(s) = &self.sandbox {
            s.check()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sandbox() -> Sandbox {
        Sandbox::new()
            .capabilities(["CAP_SYS_PTRACE"])
            .address_families(["AF_UNIX"])
            .state_dir("dyno-helper")
    }

    #[test]
    fn a_good_hardening_is_kept_and_read_back() {
        let h = Hardening::new()
            .windows_privileges(["SeChangeNotifyPrivilege"])
            .users_may_start()
            .sandbox(sandbox().runtime_dir("dyno-helper"));
        h.check().unwrap();
        assert_eq!(h.privileges(), ["SeChangeNotifyPrivilege"]);
        assert!(h.users_start());
        let s = h.systemd_sandbox().unwrap();
        assert_eq!(s.capability_list(), ["CAP_SYS_PTRACE"]);
        assert_eq!(s.family_list(), ["AF_UNIX"]);
        assert_eq!(
            (s.state_directory(), s.runtime_directory()),
            (Some("dyno-helper"), Some("dyno-helper"))
        );
        assert!(!Hardening::new().users_start() && Hardening::new().systemd_sandbox().is_none());
    }

    #[test]
    fn a_part_that_would_break_a_file_or_command_is_refused() {
        let bad = |h: Hardening| h.check().unwrap_err().to_string();
        assert!(bad(Hardening::new().windows_privileges(["debug"])).contains("privileges"));
        assert!(bad(Hardening::new().windows_privileges(["SeX Privilege"])).contains("privileges"));
        assert!(bad(Hardening::new().windows_privileges(["SePrivilege"])).contains("privileges"));
        assert!(
            bad(Hardening::new().sandbox(sandbox().capabilities(["cap_x"]))).contains("capability")
        );
        assert!(
            bad(Hardening::new().sandbox(sandbox().capabilities(["CAP_"]))).contains("capability")
        );
        assert!(bad(Hardening::new().sandbox(Sandbox::new())).contains("socket family"));
        assert!(
            bad(Hardening::new().sandbox(sandbox().address_families(["AF_UNIX\nExecStart=x"])))
                .contains("socket family")
        );
        assert!(bad(Hardening::new().sandbox(sandbox().state_dir("../x"))).contains("folder"));
        assert!(bad(Hardening::new().sandbox(sandbox().runtime_dir(""))).contains("folder"));
    }
}
