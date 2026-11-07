//! Login items and background tasks as `sfltool dumpbtm` lists them (the Background Task Management database), as typed
//! values. They cross the helper's wire (`HelperReply::LoginItems`), so they live here; the text parser is in `dyno-platform`.

use serde::{Deserialize, Serialize};

/// What kind of background item a record is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BtmKind {
    /// An app registered as a login item (`Type: login item`).
    LoginItem,
    /// A launch agent an app registered through `SMAppService` (`Type: agent`).
    Agent,
    /// A launch daemon, registered through `SMAppService` or as a plist file (`Type: daemon`, `legacy daemon`).
    Daemon,
}

/// One login item, agent or daemon.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BtmItem {
    /// The record's kind.
    pub kind: BtmKind,
    /// The name shown in System Settings (`Name`), or the identifier when it has none.
    pub name: String,
    /// BTM's identifier (`Identifier`), unique per kind.
    pub identifier: String,
    /// Whether the user has it switched on (`Disposition` has `enabled`).
    pub enabled: bool,
    /// The program, when BTM names one (`Executable Path`, else a file `URL`).
    pub exe: Option<String>,
    /// Whether it belongs to the whole machine (records of UID 0 and -2) and not to the asking account.
    pub system: bool,
}

/// The most items one `LoginItems` reply carries.
pub const MAX_BTM_ITEMS: usize = 4_096;

/// What `LoginItems` answers: the caller's items and the system-wide ones. Other accounts' records are only counted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginItemsReply {
    /// The items, at most [`MAX_BTM_ITEMS`].
    pub items: Vec<BtmItem>,
    /// Items left out because of the cap.
    pub truncated: u32,
    /// How many records belong to other accounts (never named).
    pub other_accounts: u32,
}

/// Where the agent gets the caller's login items: the helper link implements it (ADR 0027 WP-7/WP-11).
pub trait LoginItemsLink: Send {
    /// Asks the helper for `LoginItems`.
    fn login_items(&mut self) -> Result<LoginItemsReply, crate::ports::PlatformError>;

    /// Who read what the last [`LoginItemsLink::login_items`] returned, or why nobody could and what to do about it.
    fn last_via(&self) -> crate::read_via::ReadVia {
        crate::read_via::ReadVia::Helper
    }

    /// Moves whenever the helper comes or goes (a kept answer from before is stale).
    fn epoch(&self) -> u64 {
        0
    }

    /// A person asked to try again: a helper left alone after a hang is asked at once.
    fn retry_now(&mut self) {}
}

impl LoginItemsLink for Box<dyn LoginItemsLink> {
    fn login_items(&mut self) -> Result<LoginItemsReply, crate::ports::PlatformError> {
        (**self).login_items()
    }
    fn last_via(&self) -> crate::read_via::ReadVia {
        (**self).last_via()
    }
    fn epoch(&self) -> u64 {
        (**self).epoch()
    }
    fn retry_now(&mut self) {
        (**self).retry_now()
    }
}

/// The helper's side of `LoginItems`: reads the dump with the rights it has and keeps only what `uid` may see. The helper's
/// handler calls it with the uid of the connection's account (from the OS, never a pid) and plugs in the real implementation.
pub trait LoginItemsReader: Send + Sync {
    /// The items of `uid` and the machine-wide ones; other accounts' records only counted.
    fn read(&self, uid: u32) -> Result<LoginItemsReply, crate::ports::PlatformError>;
}
