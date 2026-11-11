//! Login items and background tasks on macOS (System Settings > Login Items & Extensions): `sfltool dumpbtm` asks for an
//! administrator's password, so Dyno never runs it itself (measured in a VM, ADR 0027 WP-0); the helper reads the dump as root and
//! sends back the asking account's items. Built on the seam ([`crate::helper_first`]), with no local fallback. `LoginItems` is a
//! slow kind, so the reading is Bulk.

use crate::helper_first::{HelperCalls, HelperFirst, HelperPort, HelperReading, ReadClass};
use crate::helper_link::CallError;
use dyno_core::btm::{LoginItemsLink, LoginItemsReply};
use dyno_core::helper_policy::RequestKind;
use dyno_core::metrics::Reading;
use dyno_core::ports::PlatformError;
use dyno_core::read_via::ReadVia;

/// The caller's login items: the helper's, else the reason.
pub struct LoginItems;

impl HelperReading for LoginItems {
    type Value = LoginItemsReply;
    const KINDS: &'static [RequestKind] = &[RequestKind::LoginItems];

    fn name(&self) -> &'static str {
        "login items"
    }

    fn class(&self) -> ReadClass {
        ReadClass::Bulk
    }

    fn via_helper(&self, calls: &dyn HelperCalls) -> Result<LoginItemsReply, CallError> {
        calls.login_items()
    }

    fn local(&mut self) -> Reading<LoginItemsReply> {
        // The tool needs an administrator's password as a user: nothing is run.
        Reading::Unavailable("Login items (System Settings > Login Items & Extensions) need administrator rights to read".into())
    }
}

/// The macOS startup list's link to the helper, kept between lists (a hung helper is left alone per reading).
pub struct HelperLoginItems {
    reading: HelperFirst<LoginItems>,
    via: ReadVia,
}

impl HelperLoginItems {
    /// Login items over `port`.
    pub fn new(port: Box<dyn HelperPort>) -> HelperLoginItems {
        HelperLoginItems {
            reading: HelperFirst::new(LoginItems, port),
            via: ReadVia::Local,
        }
    }
}

impl LoginItemsLink for HelperLoginItems {
    fn login_items(&mut self) -> Result<LoginItemsReply, PlatformError> {
        let got = self.reading.read();
        self.via = got.via.clone();
        match got.value {
            Reading::Available(reply) => Ok(reply),
            // The reason is the one the page shows next to the button; the list's own note says the same words.
            Reading::Unavailable(text) => Err(PlatformError::AccessDenied(got.via.reason().map_or(text, str::to_string))),
        }
    }

    fn last_via(&self) -> ReadVia {
        self.via.clone()
    }

    fn epoch(&self) -> u64 {
        self.reading.epoch()
    }

    fn retry_now(&mut self) {
        self.reading.retry_now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::helper_first::testkit::{centre, port, Net};
    use crate::helper_link::TransportError;
    use dyno_core::btm::{BtmItem, BtmKind};
    use dyno_core::helper_wire::HelperReply;
    use paddock_jobs::time::FakeClock;

    fn reply() -> LoginItemsReply {
        LoginItemsReply {
            items: vec![BtmItem {
                kind: BtmKind::LoginItem,
                name: "Editor".into(),
                identifier: "4.editor".into(),
                enabled: true,
                exe: None,
                system: false,
            }],
            truncated: 0,
            other_accounts: 2,
        }
    }

    fn link(up: bool, answers: Vec<Result<HelperReply, TransportError>>) -> HelperLoginItems {
        let c = centre(&Net::new(up, answers), &FakeClock::new());
        HelperLoginItems::new(port(&c))
    }

    #[test]
    fn the_helpers_items_are_returned_and_say_so() {
        let mut l = link(true, vec![Ok(HelperReply::LoginItems(reply()))]);
        assert_eq!(l.login_items().unwrap(), reply());
        assert_eq!(l.last_via(), ReadVia::Helper);
    }

    #[test]
    fn without_the_helper_nothing_is_run_and_the_reason_is_the_one_the_page_shows() {
        let mut l = link(false, vec![]);
        let Err(PlatformError::AccessDenied(why)) = l.login_items() else {
            panic!("expected a refusal")
        };
        assert!(why.contains("administrator rights"), "{why}");
        assert_eq!(
            l.last_via().reason(),
            Some(why.as_str()),
            "the list's note and the source say the same words"
        );
        assert!(matches!(l.last_via(), ReadVia::NeedsHelper { .. }));
    }

    #[test]
    fn a_login_items_request_is_asked_with_the_slow_kinds_own_wait() {
        let net = Net::new(true, vec![Ok(HelperReply::LoginItems(reply()))]);
        let c = centre(&net, &FakeClock::new());
        let mut l = HelperLoginItems::new(port(&c));
        l.login_items().unwrap();
        let maxes = net.maxes.lock().unwrap();
        assert!(
            maxes.last().is_some_and(Option::is_none),
            "a Bulk reading gets no live deadline: {maxes:?}"
        );
    }
}
