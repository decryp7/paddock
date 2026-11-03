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

/// How the tool gets onto the machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Route {
    /// The helper installs it for every account (Windows).
    Helper,
    /// Dyno keeps its own copy in its own folder, no administrator rights (macOS).
    OwnCopy,
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
    /// How it is installed.
    pub route: Route,
    /// Whether the installed tool can be removed from the card (Dyno's own copy; the helper's tools are not removed here).
    pub removable: bool,
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

/// The offer for smartmontools on `host`. `helper_ready` is whether the helper answers; `installed` is the version it holds.
pub fn smartmontools_offer(host: HelperHost, pin: Option<PinFacts>, helper_ready: bool, installed: Option<String>) -> ToolOffer {
    let route = if host == HelperHost::Macos { Route::OwnCopy } else { Route::Helper };
    let removable = route == Route::OwnCopy;
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
            route,
            removable,
        }
    };
    if let Some(v) = installed {
        return base(OfferState::Installed(v), pin);
    }
    match (host, pin) {
        (HelperHost::Windows, Some(p)) if helper_ready => base(OfferState::Available, Some(p)),
        (HelperHost::Windows, Some(p)) => base(
            OfferState::NeedsHelper("Turn on administrator readings first (above): the helper installs the tool for all accounts.".into()),
            Some(p),
        ),
        (HelperHost::Windows, None) => base(OfferState::NotHere("This version of Dyno has no smartmontools download pinned.".into()), None),
        (HelperHost::Macos, Some(p)) => base(OfferState::Available, Some(p)),
        (HelperHost::Macos, None) => base(
            OfferState::NotHere("This version of Dyno has no smartmontools download pinned. You can install smartmontools yourself (for example with Homebrew).".into()),
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
            assert_eq!(
                smartmontools_offer(host, pin(), true, Some("7.5".into())).state,
                OfferState::Installed("7.5".into())
            );
        }
    }

    #[test]
    fn macos_offers_its_own_copy_without_the_helper_and_can_remove_it() {
        let a = smartmontools_offer(HelperHost::Macos, pin(), false, None);
        assert_eq!((a.state, a.route, a.removable), (OfferState::Available, Route::OwnCopy, true));
        let i = smartmontools_offer(HelperHost::Macos, pin(), false, Some("7.5".into()));
        assert_eq!((i.state, i.removable), (OfferState::Installed("7.5".into()), true));
        let w = smartmontools_offer(HelperHost::Windows, pin(), true, Some("7.5".into()));
        assert_eq!((w.route, w.removable), (Route::Helper, false), "the helper's tools are not removed from this card");
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
