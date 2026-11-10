//! The errors of the private-files type: a refusal names why a folder or file was not trusted, an I/O error names the step.

use crate::folder::Refusal;
use std::fmt;
use std::path::PathBuf;

/// What can go wrong creating, opening or using a private folder or file. Messages say what to fix and never hold file content.
///
/// The text of an error **names the path**. Adopters show it to the user and log only [`PrivateError::reason`], which names no
/// path (ER-27).
#[derive(Debug)]
pub enum PrivateError {
    /// The folder or file exists but cannot be trusted (another owner, open to other accounts, a link, a bad parent). Nothing was
    /// written there and nothing was changed: tightening an existing folder cannot evict what was planted while it was open.
    Refused {
        /// The folder or file.
        path: PathBuf,
        /// Why.
        why: Refusal,
    },
    /// An operation failed.
    Io {
        /// What was being done ("create folder", "write", ...).
        op: &'static str,
        /// The folder or file.
        path: PathBuf,
        /// The cause.
        source: std::io::Error,
    },
    /// The name is not one plain file or folder name (empty, `.`, `..`, a separator or a NUL).
    BadName(String),
    /// A path that must be absolute is not.
    NotAbsolute(PathBuf),
    /// The file is larger than the caller's limit; it was not read.
    TooLarge {
        /// The file.
        path: PathBuf,
        /// The limit in bytes.
        limit: u64,
    },
}

/// The next step for a refusal (ADR 0092 section 2): each kind of refusal has its own, and tightening is never done for the user.
pub(super) fn fix_it(why: &Refusal) -> &'static str {
    match why {
        Refusal::Symlink | Refusal::ReparsePoint => LINK,
        Refusal::NotAFolder | Refusal::NotAFile => IN_THE_WAY,
        Refusal::OtherOwner { .. } | Refusal::OtherOwnerAccount { .. } => OWNER,
        Refusal::Parent(_) => PARENT,
        Refusal::Unreadable { .. } => UNREADABLE,
        Refusal::OpenToOthers { .. } => OPEN,
        Refusal::OpenToOthersAcl { .. } => ACL,
    }
}

const LINK: &str = "replace the link with a real folder you own (or remove it), or point the app's data-folder variable at a folder of your own";
const IN_THE_WAY: &str = "move the file or other item that is in the way, or point the app's data-folder variable at a folder of your own";
const UNREADABLE: &str = "check that the folder exists and that your account may open it, or point the app's data-folder variable at a folder of your own";
#[cfg(windows)]
const OWNER: &str = "take ownership of the folder (Properties, Security, Advanced, Owner) or use a folder your account created, or point the app's data-folder variable at a folder of your own";
#[cfg(not(windows))]
const OWNER: &str = "use a folder your account owns (`chown` it to your account, or make a new one), or point the app's data-folder variable at a folder of your own";
#[cfg(windows)]
const PARENT: &str = "remove the rights of other users to delete items in the parent folder or to change its permissions (Security tab), or point the app's data-folder variable at a folder under your own profile";
#[cfg(not(windows))]
const PARENT: &str = "run `chmod go-w` on the parent folder (or make it sticky and yours), or point the app's data-folder variable at a folder under your own home";
#[cfg(windows)]
const ACL: &str = "remove the entries for other users in the folder's Security tab (including entries that new files and folders inherit), or point the app's data-folder variable at a folder of your own";
#[cfg(not(windows))]
const ACL: &str = "remove the extra access-list entry (list it with `ls -lde`, remove it with `chmod -a# <index>`; an entry that new files inherit counts), or point the app's data-folder variable at a folder of your own";
#[cfg(windows)]
const OPEN: &str = "remove the entries for other users in the folder's Security tab, or point the app's data-folder variable at a folder of your own";
#[cfg(not(windows))]
const OPEN: &str = "run `chmod go-w` on the folder, or point the app's data-folder variable at a folder of your own";

impl fmt::Display for PrivateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused { path, why } => write!(
                f,
                "{} is not used for private data: {why}; {}",
                path.display(),
                fix_it(why)
            ),
            Self::Io { op, path, source } => {
                write!(f, "could not {op} {}: {source}", path.display())
            }
            Self::BadName(name) => write!(f, "{name:?} is not a plain file or folder name"),
            Self::NotAbsolute(path) => write!(f, "{} is not an absolute path", path.display()),
            Self::TooLarge { path, limit } => {
                write!(
                    f,
                    "{} is larger than {limit} bytes and was not read",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for PrivateError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Refused { why, .. } => Some(why),
            _ => None,
        }
    }
}

impl PrivateError {
    /// A short phrase naming the cause and no path, for logs.
    pub fn reason(&self) -> &'static str {
        match self {
            Self::Refused { why, .. } => why.reason(),
            Self::Io { .. } => "an operation on the folder or file failed",
            Self::BadName(_) => "the name is not a plain file or folder name",
            Self::NotAbsolute(_) => "the path is not absolute",
            Self::TooLarge { .. } => "the file is larger than the limit",
        }
    }
}

/// `Result` with [`PrivateError`].
pub type PrivateResult<T> = Result<T, PrivateError>;

/// A failure inside the platform layer, before a path is attached to it.
#[derive(Debug)]
pub(super) enum Fail {
    /// The OS said no.
    Io(std::io::Error),
    /// The object is not trusted.
    Refused(Refusal),
}

impl From<std::io::Error> for Fail {
    fn from(e: std::io::Error) -> Self {
        Fail::Io(e)
    }
}

impl From<Refusal> for Fail {
    fn from(r: Refusal) -> Self {
        Fail::Refused(r)
    }
}

/// Attaches the operation and path to a platform failure.
pub(super) fn at(op: &'static str, path: &std::path::Path) -> impl FnOnce(Fail) -> PrivateError {
    let path = path.to_path_buf();
    move |f| match f {
        Fail::Io(source) => PrivateError::Io { op, path, source },
        Fail::Refused(why) => PrivateError::Refused { path, why },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_refusal_names_its_own_next_step() {
        let acl = Refusal::OpenToOthersAcl {
            who: "S-1-1-0".into(),
        };
        let cases = [
            (Refusal::Symlink, "link"),
            (Refusal::ReparsePoint, "link"),
            (Refusal::NotAFolder, "in the way"),
            (Refusal::NotAFile, "in the way"),
            (Refusal::OtherOwner { uid: 9 }, "own"),
            (
                Refusal::OtherOwnerAccount {
                    who: "S-1-5-18".into(),
                },
                "own",
            ),
            (Refusal::Parent(Box::new(acl.clone())), "parent folder"),
            (
                Refusal::Unreadable {
                    what: "open",
                    cause: String::new(),
                },
                "exists",
            ),
            (Refusal::OpenToOthers { mode: 0o777 }, "folder"),
            (acl, "entr"),
        ];
        for (why, word) in cases {
            let step = fix_it(&why);
            assert!(step.contains(word), "{why:?}: {step}");
        }
        let steps: std::collections::HashSet<_> = [
            fix_it(&Refusal::Symlink),
            fix_it(&Refusal::NotAFolder),
            fix_it(&Refusal::OtherOwner { uid: 1 }),
            fix_it(&Refusal::Parent(Box::new(Refusal::NotAFile))),
            fix_it(&Refusal::OpenToOthers { mode: 0 }),
            fix_it(&Refusal::Unreadable {
                what: "x",
                cause: String::new(),
            }),
        ]
        .into();
        assert_eq!(steps.len(), 6, "six kinds, six different sentences");
    }

    #[test]
    fn the_reason_names_no_path() {
        let path = PathBuf::from("/secret/place");
        let errors = [
            PrivateError::Refused {
                path: path.clone(),
                why: Refusal::Symlink,
            },
            PrivateError::Io {
                op: "read",
                path: path.clone(),
                source: std::io::ErrorKind::Other.into(),
            },
            PrivateError::BadName("a/b".into()),
            PrivateError::NotAbsolute(path.clone()),
            PrivateError::TooLarge { path, limit: 1 },
        ];
        for e in errors {
            assert!(!e.reason().is_empty());
            assert!(
                !e.reason().contains("secret") && !e.reason().contains("a/b"),
                "{}",
                e.reason()
            );
        }
    }
}
