//! The disk-health tool as Settings offers it (ADR 0027 section 7): what would be downloaded, from where, under which licence, and
//! why the button is or is not there. Pure: the agent passes in the pin, the helper's state and what the helper already holds.

use crate::helper::HelperHost;
use crate::helper_requests::ToolKind;
use serde::{Deserialize, Serialize};

/// Where the offer stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OfferState {
    /// Can be downloaded now.
    Available,
    /// The helper holds this version.
    Installed(String),
    /// Needs the helper first; the text says what to do.
    NeedsHelper(String),
    /// Not offered on this system; the text says what to do instead.
    NotHere(String),
}

/// Which copy of the tool Dyno uses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolSource {
    /// The copy the helper installed (read through the helper).
    Helper,
    /// Dyno's own copy in its tools folder.
    Dyno,
    /// A copy the person installed themselves, at this path.
    Own(String),
}

/// A tool that is installed, with the copy Dyno uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledTool {
    /// The version.
    pub version: String,
    /// Which copy.
    pub source: ToolSource,
}

/// What the Settings card shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolOffer {
    /// Which tool.
    pub tool: ToolKind,
    /// Its name.
    pub name: String,
    /// The pinned version.
    pub version: String,
    /// Download size in bytes (0 when nothing is offered).
    pub size: u64,
    /// The host it is fetched from.
    pub from: String,
    /// Its licence.
    pub licence: String,
    /// Where its source is.
    pub source_url: String,
    /// What the card can do.
    pub state: OfferState,
    /// Which copy Dyno uses, when it is installed.
    pub source: Option<ToolSource>,
}

/// What the pin says about the file (absent where the tool has no download for this system).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinFacts {
    /// Pinned version.
    pub version: String,
    /// Exact size.
    pub size: u64,
    /// Host.
    pub from: String,
    /// Licence.
    pub licence: String,
    /// Source address.
    pub source_url: String,
}

const NAME: &str = "smartmontools";
const SOURCE: &str = "https://www.smartmontools.org";

/// The offer for smartmontools on `host`. `helper_ready` is whether the helper answers; `installed` is the copy Dyno uses (the helper's, Dyno's own or the person's).
pub fn smartmontools_offer(host: HelperHost, pin: Option<PinFacts>, helper_ready: bool, installed: Option<InstalledTool>) -> ToolOffer {
    let base = |state: OfferState, pin: Option<PinFacts>| {
        let (version, size, from, licence, source_url) = pin.map_or(
            (String::new(), 0, String::new(), "GPL-2.0-or-later".to_string(), SOURCE.to_string()),
            |p| (p.version, p.size, p.from, p.licence, p.source_url),
        );
        ToolOffer {
            tool: ToolKind::Smartmontools,
            name: NAME.into(),
            version,
            size,
            from,
            licence,
            source_url,
            state,
            source: None,
        }
    };
    if let Some(t) = installed {
        return ToolOffer {
            source: Some(t.source),
            ..base(OfferState::Installed(t.version), pin)
        };
    }
    match (host, pin) {
        (HelperHost::Windows, Some(p)) if helper_ready => base(OfferState::Available, Some(p)),
        (HelperHost::Windows, Some(p)) => base(
            OfferState::NeedsHelper("Turn on administrator readings first (above): the helper installs the tool for all accounts.".into()),
            Some(p),
        ),
        (HelperHost::Windows, None) => base(OfferState::NotHere("This version of Dyno has no smartmontools download pinned.".into()), None),
        (HelperHost::Macos, _) => base(
            OfferState::NotHere("On macOS Dyno will run smartctl from its own copy; that is not in this version. You can install smartmontools yourself (for example with Homebrew).".into()),
            None,
        ),
        (HelperHost::Linux, _) => base(
            OfferState::NotHere("Install smartmontools with your distribution's package manager (for example: sudo apt install smartmontools).".into()),
            None,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pin() -> Option<PinFacts> {
        Some(PinFacts {
            version: "7.5".into(),
            size: 1_508_408,
            from: "github.com".into(),
            licence: "GPL-2.0-or-later".into(),
            source_url: "https://www.smartmontools.org".into(),
        })
    }

    #[test]
    fn windows_offers_the_download_when_the_helper_answers_and_asks_for_the_helper_first_otherwise() {
        let on = smartmontools_offer(HelperHost::Windows, pin(), true, None);
        assert_eq!(on.state, OfferState::Available);
        assert_eq!((on.version.as_str(), on.size, on.from.as_str()), ("7.5", 1_508_408, "github.com"));
        assert!(
            matches!(smartmontools_offer(HelperHost::Windows, pin(), false, None).state, OfferState::NeedsHelper(t) if t.contains("administrator readings"))
        );
    }

    #[test]
    fn an_installed_tool_shows_its_version_on_every_system() {
        for host in [HelperHost::Windows, HelperHost::Macos, HelperHost::Linux] {
            let o = smartmontools_offer(host, pin(), true, Some(installed("7.5", ToolSource::Helper)));
            assert_eq!(o.state, OfferState::Installed("7.5".into()));
            assert_eq!(o.source, Some(ToolSource::Helper));
        }
    }

    #[test]
    fn a_users_own_install_is_installed_too_and_says_where_it_is_even_without_the_helper() {
        let own = installed("7.5", ToolSource::Own(r"C:\Program Files\smartmontools\bin\smartctl.exe".into()));
        let o = smartmontools_offer(HelperHost::Windows, pin(), false, Some(own));
        assert_eq!(o.state, OfferState::Installed("7.5".into()));
        assert!(matches!(o.source, Some(ToolSource::Own(p)) if p.contains("Program Files")));
    }

    fn installed(version: &str, source: ToolSource) -> InstalledTool {
        InstalledTool {
            version: version.into(),
            source,
        }
    }

    #[test]
    fn macos_and_linux_say_what_to_do_instead_and_a_missing_pin_is_not_offered() {
        assert!(matches!(smartmontools_offer(HelperHost::Macos, None, true, None).state, OfferState::NotHere(t) if t.contains("Homebrew")));
        assert!(
            matches!(smartmontools_offer(HelperHost::Linux, None, true, None).state, OfferState::NotHere(t) if t.contains("package manager"))
        );
        let none = smartmontools_offer(HelperHost::Windows, None, true, None);
        assert!(matches!(none.state, OfferState::NotHere(_)));
        assert_eq!(none.size, 0);
        assert_eq!(none.licence, "GPL-2.0-or-later");
    }
}
