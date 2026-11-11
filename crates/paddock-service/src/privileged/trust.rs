//! The protected-chain rule for a privileged run (Dyno ADR 0027 §3 and paddock-tools ADR 0051 D4): a program or folder that
//! SYSTEM or root will run or write is trusted only when nobody but SYSTEM, Administrators (or root) can change it or
//! anything that leads to it.
//!
//! The rule is pure over a [`TrustPort`] (the facts of each path component, taken from an open object, never a path twice),
//! so every case is tested over fake security descriptors and stats. The real port is [`crate::privileged::trust_real`].
//!
//! What counts, per component of the chain from the drive root (or `/`) down to the target:
//!
//! - **Every component:** not a link or reparse point; owned by SYSTEM, Administrators or TrustedInstaller (root on Unix);
//!   no other account may hold `DELETE`, `WRITE_DAC` or `WRITE_OWNER` on it, nor `FILE_DELETE_CHILD` on its parent. Stock
//!   `C:\` and `C:\ProgramData` let ordinary users create files and folders, so an ancestor is *not* required to be locked
//!   against creation: a user can only add siblings there, never replace or rename a component.
//! - **Inside the protected root** (the folder the helper locked, and everything below it, and the target file): also no
//!   other account may hold `FILE_ADD_FILE`/`FILE_WRITE_DATA` or `FILE_ADD_SUBDIRECTORY`/`FILE_APPEND_DATA`.
//! - **Unix:** every component is owned by root, not group- or other-writable and carries no ACL entry that allows a write.
//!   (A POSIX ACL with named entries shows its mask in the group bits, so the mode check covers it.)
//!
//! Deny entries are ignored (an allow entry is judged on its own, which only makes the rule stricter).
#![cfg_attr(
    not(test),
    deny(
        clippy::let_underscore_must_use,
        clippy::let_underscore_future,
        clippy::let_underscore_untyped
    )
)]

use super::files::FileKind;
use std::fmt;
use std::io;

/// SYSTEM.
pub const SID_SYSTEM: &str = "S-1-5-18";
/// BUILTIN\Administrators.
pub const SID_ADMINISTRATORS: &str = "S-1-5-32-544";
/// BUILTIN\Users.
pub const SID_USERS: &str = "S-1-5-32-545";
/// NT SERVICE\TrustedInstaller.
pub const SID_TRUSTED_INSTALLER: &str =
    "S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464";
/// CREATOR OWNER.
pub const SID_CREATOR_OWNER: &str = "S-1-3-0";
/// Everyone.
pub const SID_EVERYONE: &str = "S-1-1-0";
/// NT AUTHORITY\Authenticated Users.
pub const SID_AUTHENTICATED_USERS: &str = "S-1-5-11";

/// The accounts allowed to change protected things.
const TRUSTED: [&str; 3] = [SID_SYSTEM, SID_ADMINISTRATORS, SID_TRUSTED_INSTALLER];

/// Windows access-mask bits that matter here.
pub mod right {
    /// `FILE_WRITE_DATA` (`FILE_ADD_FILE` on a folder).
    pub const WRITE_DATA: u32 = 0x0000_0002;
    /// `FILE_APPEND_DATA` (`FILE_ADD_SUBDIRECTORY` on a folder).
    pub const APPEND_DATA: u32 = 0x0000_0004;
    /// `FILE_DELETE_CHILD`.
    pub const DELETE_CHILD: u32 = 0x0000_0040;
    /// `DELETE`.
    pub const DELETE: u32 = 0x0001_0000;
    /// `WRITE_DAC`.
    pub const WRITE_DAC: u32 = 0x0004_0000;
    /// `WRITE_OWNER`.
    pub const WRITE_OWNER: u32 = 0x0008_0000;
    /// `FILE_WRITE_EA`.
    pub const WRITE_EA: u32 = 0x0000_0010;
    /// `FILE_WRITE_ATTRIBUTES`.
    pub const WRITE_ATTRIBUTES: u32 = 0x0000_0100;
    /// `GENERIC_WRITE`.
    pub const GENERIC_WRITE: u32 = 0x4000_0000;
    /// `GENERIC_ALL`.
    pub const GENERIC_ALL: u32 = 0x1000_0000;
}

/// One access-control entry of a DACL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ace {
    /// The account's SID in string form (`S-1-5-18`).
    pub sid: String,
    /// `true` for an allow entry, `false` for a deny entry.
    pub allow: bool,
    /// The access mask.
    pub mask: u32,
    /// Inherit-only: the entry applies to what the object holds, not to the object itself.
    pub inherit_only: bool,
}

impl Ace {
    /// An allow entry that applies to the object itself.
    pub fn allow(sid: &str, mask: u32) -> Self {
        Ace {
            sid: sid.into(),
            allow: true,
            mask,
            inherit_only: false,
        }
    }

    /// An allow entry that only its children inherit.
    #[must_use]
    pub fn inherit_only(mut self) -> Self {
        self.inherit_only = true;
        self
    }
}

/// The owner and DACL of one object (a Windows security descriptor, reduced to what the rule reads).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Descriptor {
    /// The owner's SID.
    pub owner: String,
    /// The DACL; `None` is a NULL DACL, which grants everyone everything.
    pub dacl: Option<Vec<Ace>>,
}

/// What a path component is, as read from the open object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Security {
    /// Windows: owner and DACL.
    Windows(Descriptor),
    /// Unix: owner, mode and whether an ACL entry allows a write.
    Unix {
        /// The owner's user id.
        owner: u32,
        /// The permission bits.
        mode: u32,
        /// An ACL entry (macOS) allows write, append, delete or a change of permissions.
        write_acl: bool,
    },
}

/// One path component.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    /// What it is (a link or reparse point is [`FileKind::Symlink`]).
    pub kind: FileKind,
    /// Its owner and access rules.
    pub security: Security,
}

/// Which file a path is, read from the open object without following a link: what stays the same when the path is renamed, and what a
/// swapped file does not have (round 5 of Pitcrew, P2-3 and P2-9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FileIdentity {
    /// The volume (Windows: the volume serial number; Unix: the device).
    pub volume: u64,
    /// The file on it (Windows: the 128-bit file id, which ReFS and Dev Drive need; Unix: the inode).
    pub index: u128,
    /// How many names the file has (hard links, directories count as 1 on Windows).
    pub links: u32,
}

/// What a port knows about which file a path is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Identity {
    /// Nothing is there.
    Missing,
    /// This file.
    Known(FileIdentity),
    /// Something is there but the port cannot say which file (a port that models no identities). A caller that decides on identity
    /// **refuses** on this, unless the port says it models none ([`TrustPort::models_identity`]).
    Unknown,
}

/// How paths split.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    /// `C:\a\b`, case-insensitive.
    Windows,
    /// `/a/b`.
    Unix,
}

/// What the rule needs from the machine.
pub trait TrustPort: Send {
    /// How this machine's paths split.
    fn style(&self) -> Style;
    /// The facts of `path` without following a link; `Ok(None)` when nothing is there.
    fn node(&mut self, path: &str) -> io::Result<Option<Node>>;
    /// The identity of `path` (see [`FileIdentity`]), without following a link. The real ports always answer [`Identity::Known`] for
    /// a path that exists; the default is [`Identity::Unknown`], never a guess.
    fn identity(&mut self, _path: &str) -> io::Result<Identity> {
        Ok(Identity::Unknown)
    }
    /// Whether this port can tell files apart. `false` only for a fake that models no identities (its callers then rely on the path
    /// alone, in tests); every other port is expected to, and a security decision refuses on [`Identity::Unknown`] from it.
    fn models_identity(&self) -> bool {
        true
    }
    /// The path with every link resolved (Unix; used for programs the OS ships, like `/usr/sbin/smartctl`).
    fn canonical(&mut self, path: &str) -> io::Result<String>;
    /// Every entry below `path`, never following a link (a link is listed, not entered), at most `limit`; an error when
    /// there are more.
    fn walk(&mut self, path: &str, limit: usize) -> io::Result<Vec<(String, Node)>>;
    /// Creates one folder (its parent must exist) and fails if it exists: on Windows with a protected security descriptor
    /// given at creation (`CreateDirectoryW` with `SECURITY_ATTRIBUTES`: SYSTEM and Administrators full, Users read and
    /// execute, no inheritance from above), elsewhere root-owned 0755.
    fn create_dir(&mut self, path: &str) -> io::Result<()>;
    /// Sets the owner and the protected DACL (Windows: `SetSecurityInfo` on a handle opened without following a link, after
    /// checking it is a real folder; never by path) or the mode 0755 (Unix) of an existing folder.
    fn lock_dir(&mut self, path: &str) -> io::Result<()>;
    /// Renames the entry next to itself to a name that is free, never following it, and returns the new path. Nothing in it
    /// is read or run again.
    fn rename_aside(&mut self, path: &str) -> io::Result<String>;
    /// Removes the oldest `<name>.untrusted-<secs>-<n>` entries in `parent` (exactly the folders [`rename_aside`](Self::rename_aside)
    /// makes for `name`) beyond the newest `keep`, and returns the names removed. Only a real folder (never a link) owned by
    /// SYSTEM, Administrators or root counts; anything else with a similar name is left alone.
    fn sweep_aside(&mut self, parent: &str, name: &str, keep: usize) -> io::Result<Vec<String>>;
    /// Removes a folder tree without following any link or reparse point: a link inside is removed, never its target.
    fn remove_tree(&mut self, path: &str) -> io::Result<()>;
}

/// Why a component is not trusted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reason {
    /// It is not there.
    Missing,
    /// It is a link or reparse point.
    Link,
    /// It is not the kind of thing it must be.
    WrongKind(FileKind),
    /// Its owner may change its permissions.
    UntrustedOwner(String),
    /// It has no DACL, which lets everyone do everything.
    NullDacl,
    /// An account other than SYSTEM and Administrators holds a right that lets it change this or what leads to it.
    Grants {
        /// The account's SID.
        sid: String,
        /// The right, by its Windows name.
        right: &'static str,
    },
    /// Its mode lets the group or others write.
    Writable(u32),
    /// An ACL entry allows a write.
    AclWrite,
    /// The target is not inside the root the helper locked.
    OutsideRoot,
    /// The path the open object really has differs from the path that was asked for (a junction or link in between).
    Redirected(String),
    /// It could not be examined.
    Unreadable(String),
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Reason::Missing => f.write_str("it is not there"),
            Reason::Link => f.write_str("it is a link or reparse point"),
            Reason::WrongKind(k) => write!(f, "it is a {k:?}, not what is expected"),
            Reason::UntrustedOwner(o) => {
                write!(f, "its owner ({o}) is not SYSTEM, Administrators or root")
            }
            Reason::NullDacl => f.write_str("it has no access list, so everyone may change it"),
            Reason::Grants { sid, right } => write!(f, "the account {sid} holds {right} on it"),
            Reason::Writable(m) => write!(f, "its mode {m:o} lets the group or others write"),
            Reason::AclWrite => f.write_str("an access-control entry allows a write"),
            Reason::OutsideRoot => f.write_str("it is not inside the helper's protected folder"),
            Reason::Redirected(to) => write!(
                f,
                "the path leads somewhere else ({to}) through a link or junction"
            ),
            Reason::Unreadable(e) => write!(f, "it could not be examined: {e}"),
        }
    }
}

/// A chain that is not protected: where, and why. Repairing the helper re-applies the lock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Untrusted {
    /// The component that failed.
    pub path: String,
    /// Why.
    pub reason: Reason,
}

impl fmt::Display for Untrusted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} is not a protected place: {}. Repair the helper (Settings > Helper > Repair) so it locks its folders again.",
            self.path, self.reason
        )
    }
}

impl Untrusted {
    /// The same words without the next step (for a message that is itself the result of a repair).
    pub fn describe(&self) -> String {
        format!("{} is not a protected place: {}", self.path, self.reason)
    }
}

impl std::error::Error for Untrusted {}

fn fail(path: &str, reason: Reason) -> Untrusted {
    Untrusted {
        path: path.to_string(),
        reason,
    }
}

/// The folder that holds `path`; `None` at the root.
pub fn parent_of(path: &str, style: Style) -> Option<String> {
    match style {
        Style::Windows => {
            let p = path.replace('/', "\\");
            let p = p.trim_end_matches('\\');
            let cut = p.rfind('\\')?;
            if p[..cut].is_empty() {
                return None;
            }
            // `C:` + `\` is the root; it has no parent.
            let head = &p[..cut];
            if head.ends_with(':') {
                Some(format!("{head}\\"))
            } else if p.ends_with(':') {
                None
            } else {
                Some(head.to_string())
            }
        }
        Style::Unix => {
            let p = path.trim_end_matches('/');
            if p.is_empty() {
                return None;
            }
            let cut = p.rfind('/')?;
            Some(if cut == 0 {
                "/".into()
            } else {
                p[..cut].to_string()
            })
        }
    }
}

fn norm(path: &str, style: Style) -> String {
    match style {
        Style::Windows => path
            .replace('/', "\\")
            .trim_end_matches('\\')
            .to_lowercase(),
        Style::Unix => path.trim_end_matches('/').to_string(),
    }
}

/// Whether `sid` is SYSTEM, Administrators or TrustedInstaller.
pub fn is_trusted_sid(sid: &str) -> bool {
    TRUSTED.contains(&sid)
}

fn is_trusted(sid: &str) -> bool {
    is_trusted_sid(sid)
}

/// The first right of `mask` that matters for a component, by name. `inside` adds the rights that create or write.
fn bad_right(mask: u32, inside: bool) -> Option<&'static str> {
    use right::*;
    if mask & GENERIC_ALL != 0 {
        return Some("full control");
    }
    if mask & DELETE != 0 {
        return Some("DELETE");
    }
    if mask & WRITE_DAC != 0 {
        return Some("WRITE_DAC");
    }
    if mask & WRITE_OWNER != 0 {
        return Some("WRITE_OWNER");
    }
    if inside {
        if mask & (WRITE_DATA | GENERIC_WRITE) != 0 {
            return Some("FILE_WRITE_DATA (add file)");
        }
        if mask & APPEND_DATA != 0 {
            return Some("FILE_APPEND_DATA (add subfolder)");
        }
        if mask & WRITE_ATTRIBUTES != 0 {
            return Some("FILE_WRITE_ATTRIBUTES");
        }
        if mask & WRITE_EA != 0 {
            return Some("FILE_WRITE_EA");
        }
    }
    None
}

fn check_windows(
    path: &str,
    d: &Descriptor,
    parent: Option<(&str, &Descriptor)>,
    inside: bool,
) -> Result<(), Untrusted> {
    if !is_trusted(&d.owner) {
        return Err(fail(path, Reason::UntrustedOwner(d.owner.clone())));
    }
    let dacl = d
        .dacl
        .as_ref()
        .ok_or_else(|| fail(path, Reason::NullDacl))?;
    for ace in dacl
        .iter()
        .filter(|a| a.allow && !a.inherit_only && !is_trusted(&a.sid))
    {
        // CREATOR OWNER on the object itself means its owner, who is trusted (checked above).
        if ace.sid == SID_CREATOR_OWNER {
            continue;
        }
        if let Some(right) = bad_right(ace.mask, inside) {
            return Err(fail(
                path,
                Reason::Grants {
                    sid: ace.sid.clone(),
                    right,
                },
            ));
        }
    }
    if let Some((parent_path, p)) = parent {
        let pdacl = p
            .dacl
            .as_ref()
            .ok_or_else(|| fail(parent_path, Reason::NullDacl))?;
        for ace in pdacl
            .iter()
            .filter(|a| a.allow && !a.inherit_only && !is_trusted(&a.sid))
        {
            if ace.sid == SID_CREATOR_OWNER {
                continue;
            }
            if ace.mask & (right::DELETE_CHILD | right::GENERIC_ALL) != 0 {
                return Err(fail(
                    path,
                    Reason::Grants {
                        sid: ace.sid.clone(),
                        right: "FILE_DELETE_CHILD on its parent folder",
                    },
                ));
            }
        }
    }
    Ok(())
}

fn check_unix(path: &str, owner: u32, mode: u32, write_acl: bool) -> Result<(), Untrusted> {
    if owner != 0 {
        return Err(fail(
            path,
            Reason::UntrustedOwner(format!("user id {owner}")),
        ));
    }
    if mode & 0o022 != 0 {
        return Err(fail(path, Reason::Writable(mode & 0o7777)));
    }
    if write_acl {
        return Err(fail(path, Reason::AclWrite));
    }
    Ok(())
}

/// Checks the chain from the root of the drive down to `target`, which must be a `want` (a file or a folder).
///
/// `root` is the protected folder the helper locked (`target` is inside it or is it); components inside it get the stricter
/// rule. `resolve` first resolves links (Unix, for programs the OS ships); without it a link anywhere is refused.
///
/// # Errors
/// The first component that is not protected, with the reason.
pub fn verify_chain(
    port: &mut dyn TrustPort,
    target: &str,
    want: FileKind,
    root: Option<&str>,
    resolve: bool,
) -> Result<(), Untrusted> {
    let style = port.style();
    let path = if resolve {
        port.canonical(target)
            .map_err(|e| fail(target, Reason::Unreadable(e.to_string())))?
    } else {
        target.to_string()
    };
    let mut chain = vec![path];
    while let Some(p) = chain.last().and_then(|c| parent_of(c, style)) {
        chain.push(p);
        if chain.len() > 64 {
            return Err(fail(
                target,
                Reason::Unreadable("the path is too deep".into()),
            ));
        }
    }
    // Index of the root in the chain: components at or below it are "inside".
    let root_index = match (root, resolve) {
        (Some(r), false) => {
            let r = norm(r, style);
            Some(
                chain
                    .iter()
                    .position(|c| norm(c, style) == r)
                    .ok_or_else(|| fail(target, Reason::OutsideRoot))?,
            )
        }
        _ => None,
    };
    // From the top of the chain down, so a link or junction on the way is named as one even when what lies behind it cannot
    // be examined at all (Windows refuses to traverse an untrusted mount point).
    let mut nodes = Vec::with_capacity(chain.len());
    for c in chain.iter().rev() {
        let node = port
            .node(c)
            .map_err(|e| fail(c, Reason::Unreadable(e.to_string())))?
            .ok_or_else(|| fail(c, Reason::Missing))?;
        if node.kind == FileKind::Symlink {
            return Err(fail(c, Reason::Link));
        }
        nodes.push(node);
    }
    nodes.reverse();
    for (i, (c, node)) in chain.iter().zip(&nodes).enumerate() {
        if node.kind == FileKind::Symlink {
            return Err(fail(c, Reason::Link));
        }
        let expected = if i == 0 { want } else { FileKind::Dir };
        if node.kind != expected {
            return Err(fail(c, Reason::WrongKind(node.kind)));
        }
    }
    // From the root of the drive down, so the reason names the highest component that is wrong.
    for i in (0..chain.len()).rev() {
        let inside = root_index.is_some_and(|r| i <= r);
        match &nodes[i].security {
            Security::Windows(d) => {
                let parent = match nodes.get(i + 1).map(|n| &n.security) {
                    Some(Security::Windows(p)) => Some((chain[i + 1].as_str(), p)),
                    _ => None,
                };
                check_windows(&chain[i], d, parent, inside)?;
            }
            Security::Unix {
                owner,
                mode,
                write_acl,
            } => check_unix(&chain[i], *owner, *mode, *write_acl)?,
        }
    }
    Ok(())
}

/// Whether `path` is `root` or below it (Windows paths compare case-insensitively).
pub fn is_within(path: &str, root: &str, style: Style) -> bool {
    let (p, r) = (norm(path, style), norm(root, style));
    let sep = if style == Style::Windows { '\\' } else { '/' };
    p == r || p.starts_with(&format!("{r}{sep}"))
}

/// Refuses a requested path whose open object has another final path (a junction or link on the way): the program must be
/// spawned by the checked final path, and only when it is the path that was asked for.
///
/// # Errors
/// [`Reason::Redirected`].
pub fn require_same_path(requested: &str, final_path: &str, style: Style) -> Result<(), Untrusted> {
    let strip = |p: &str| -> String {
        let p = p.strip_prefix("\\\\?\\").unwrap_or(p);
        norm(p, style)
    };
    if strip(requested) == strip(final_path) {
        Ok(())
    } else {
        Err(fail(requested, Reason::Redirected(final_path.to_string())))
    }
}

/// Checks `dir` as a place the helper will make folders in: the chain to it passes the ancestor rule and no other account may
/// delete what is made in it (`FILE_DELETE_CHILD`), so a user cannot swap a folder the helper made for their own.
///
/// # Errors
/// The first component that is not protected.
pub fn verify_container(port: &mut dyn TrustPort, dir: &str) -> Result<(), Untrusted> {
    verify_chain(port, dir, FileKind::Dir, None, false)?;
    let node = port
        .node(dir)
        .map_err(|e| fail(dir, Reason::Unreadable(e.to_string())))?
        .ok_or_else(|| fail(dir, Reason::Missing))?;
    if let Security::Windows(d) = &node.security {
        let dacl = d.dacl.as_ref().ok_or_else(|| fail(dir, Reason::NullDacl))?;
        for ace in dacl.iter().filter(|a| {
            a.allow && !a.inherit_only && !is_trusted(&a.sid) && a.sid != SID_CREATOR_OWNER
        }) {
            if ace.mask & (right::DELETE_CHILD | right::GENERIC_ALL) != 0 {
                return Err(fail(
                    dir,
                    Reason::Grants {
                        sid: ace.sid.clone(),
                        right: "FILE_DELETE_CHILD on what is made in it",
                    },
                ));
            }
        }
    }
    Ok(())
}

/// Checks everything below `dir` (a folder already shown to be protected) without following a link: no link or reparse
/// point, and every entry owned and guarded as the inside rule says. One entry that fails makes the whole folder untrusted.
/// `skip` is a path below `dir` (and what is in it) that is left out: the way on to a folder that is judged on its own.
///
/// # Errors
/// The first entry that is not protected.
pub fn verify_tree(
    port: &mut dyn TrustPort,
    dir: &str,
    skip: Option<&str>,
    limit: usize,
) -> Result<(), Untrusted> {
    let style = port.style();
    let top = port
        .node(dir)
        .map_err(|e| fail(dir, Reason::Unreadable(e.to_string())))?
        .ok_or_else(|| fail(dir, Reason::Missing))?;
    let entries = port
        .walk(dir, limit)
        .map_err(|e| fail(dir, Reason::Unreadable(e.to_string())))?;
    let mut by_path: std::collections::HashMap<String, &Node> = std::collections::HashMap::new();
    by_path.insert(norm(dir, style), &top);
    for (path, node) in &entries {
        by_path.insert(norm(path, style), node);
    }
    for (path, node) in &entries {
        if skip.is_some_and(|s| is_within(path, s, style)) || inside_aside(path, dir, style) {
            continue;
        }
        if node.kind == FileKind::Symlink {
            return Err(fail(path, Reason::Link));
        }
        match &node.security {
            Security::Windows(d) => {
                let parent = parent_of(path, style)
                    .and_then(|p| by_path.get(&norm(&p, style)).map(|n| (p, *n)))
                    .and_then(|(p, n)| match &n.security {
                        Security::Windows(pd) => Some((p, pd)),
                        Security::Unix { .. } => None,
                    });
                check_windows(
                    path,
                    d,
                    parent.as_ref().map(|(p, pd)| (p.as_str(), *pd)),
                    true,
                )?;
            }
            Security::Unix {
                owner,
                mode,
                write_acl,
            } => check_unix(path, *owner, *mode, *write_acl)?,
        }
    }
    Ok(())
}

/// Whether `path` is *inside* a folder the helper moved aside below `dir` (`<name>.untrusted-<secs>-<n>`): what such a folder
/// holds is the foreign tree it was moved for and is never read. The moved-aside folder itself is still checked (a user cannot
/// make a folder the trusted set owns, so a look-alike they made fails there).
fn inside_aside(path: &str, dir: &str, style: Style) -> bool {
    let sep = if style == Style::Windows { '\\' } else { '/' };
    let norm_path = path.replace('/', &sep.to_string());
    let rel = norm_path
        .get(dir.trim_end_matches(['\\', '/']).len()..)
        .unwrap_or("");
    let comps: Vec<&str> = rel.split(sep).filter(|c| !c.is_empty()).collect();
    let Some((_, ancestors)) = comps.split_last() else {
        return false;
    };
    ancestors.iter().any(|c| {
        c.to_lowercase()
            .rsplit_once(".untrusted-")
            .is_some_and(|(_, suffix)| {
                let parts: Vec<&str> = suffix.split('-').collect();
                (1..=2).contains(&parts.len())
                    && parts
                        .iter()
                        .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
            })
    })
}

/// The entry of `dir` that holds `path` (`dir\\a\\b` gives `dir\\a` for `dir\\a\\b\\c`); `None` when `path` is not below `dir`.
pub fn child_of(dir: &str, path: &str, style: Style) -> Option<String> {
    let mut cur = path.to_string();
    loop {
        let parent = parent_of(&cur, style)?;
        if norm(&parent, style) == norm(dir, style) {
            return Some(cur);
        }
        cur = parent;
    }
}

/// The folders strictly between `base` and `dir`, from the one under `base` down to `dir`'s parent. `None` when `dir` is
/// not below `base`.
pub fn between(base: &str, dir: &str, style: Style) -> Option<Vec<String>> {
    if !is_within(dir, base, style) || norm(dir, style) == norm(base, style) {
        return None;
    }
    let mut up = Vec::new();
    let mut cur = parent_of(dir, style);
    while let Some(c) = cur {
        if norm(&c, style) == norm(base, style) {
            up.reverse();
            return Some(up);
        }
        up.push(c.clone());
        cur = parent_of(&c, style);
    }
    None
}

/// Whether `ls -lde` output on macOS lists an ACL entry that allows a write, an append, a delete or a change of
/// permissions (to anyone: root does not need an entry).
pub fn acl_allows_write(listing: &str) -> bool {
    const WRITING: [&str; 10] = [
        "write",
        "append",
        "delete",
        "delete_child",
        "chown",
        "writeattr",
        "writeextattr",
        "writesecurity",
        "add_file",
        "add_subdirectory",
    ];
    listing
        .lines()
        .skip(1)
        .filter_map(|l| {
            let l = l.trim_start();
            let (index, rest) = l.split_once(": ")?;
            index.chars().all(|c| c.is_ascii_digit()).then_some(rest)
        })
        .filter_map(|rest| rest.split_once(" allow "))
        .any(|(_, rights)| rights.split(',').any(|r| WRITING.contains(&r.trim())))
}

#[cfg(test)]
#[path = "trust/tests.rs"]
pub(crate) mod tests;
