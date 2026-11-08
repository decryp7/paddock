//! A Linux card's own Wake-on-LAN mode, through the helper (`WolState`; helper-wiring plan section 7.1, decision D4). The
//! sysfs file `power/wakeup` says whether a card may wake the computer; which packets it wakes for is its `ethtool` mode, which only
//! root can read. Without the helper the readiness panel keeps its old sentence ("cannot be read without the helper").

use crate::helper_first::{HelperCalls, HelperFirst, HelperPort, HelperReading, ReadClass};
use crate::helper_link::CallError;
use dyno_core::metrics::Reading;
use dyno_core::ports::PlatformError;
use dyno_core::wake::{IfName, WolReply};
use dyno_core::wol::readiness::{with_modes, ItemState, ReadinessKind};
use dyno_core::wol::{ReadinessSource, ThisPcWol};
use std::sync::{Mutex, PoisonError};

/// The modes of one card; the card is set before each read.
pub struct WolModes {
    card: Option<IfName>,
}

impl HelperReading for WolModes {
    type Value = WolReply;

    fn name(&self) -> &'static str {
        "Wake-on-LAN mode"
    }

    fn class(&self) -> ReadClass {
        // A person opens the readiness panel and waits for it.
        ReadClass::Live
    }

    fn via_helper(&self, calls: &dyn HelperCalls) -> Result<WolReply, CallError> {
        let card = self
            .card
            .clone()
            .ok_or_else(|| CallError::from(PlatformError::Failed("no network card was named".into())))?;
        calls.wol_state(card)
    }

    fn local(&mut self) -> Reading<WolReply> {
        Reading::unavailable("the card's own Wake-on-LAN mode is readable only by an administrator")
    }
}

/// The platform's readiness with each card that may wake the computer judged by its own mode when the helper can read it.
pub struct HelperReadiness {
    inner: std::sync::Arc<dyn ReadinessSource>,
    modes: Mutex<HelperFirst<WolModes>>,
}

impl HelperReadiness {
    /// `inner` refined through `port`.
    pub fn new(inner: std::sync::Arc<dyn ReadinessSource>, port: Box<dyn HelperPort>) -> HelperReadiness {
        HelperReadiness {
            inner,
            modes: Mutex::new(HelperFirst::new(WolModes { card: None }, port)),
        }
    }
}

impl ReadinessSource for HelperReadiness {
    fn read(&self) -> ThisPcWol {
        let mut pc = self.inner.read();
        let mut modes = self.modes.lock().unwrap_or_else(PoisonError::into_inner);
        for item in &mut pc.items {
            if item.kind != ReadinessKind::NicWake || !matches!(item.state, ItemState::Ok { .. }) {
                continue;
            }
            let Some(card) = item.subject.as_deref().and_then(|s| IfName::parse(s).ok()) else {
                continue;
            };
            modes.reading_mut().card = Some(card);
            // Not read: the item keeps the old sentence, which already says the mode needs the helper.
            if let Reading::Available(reply) = modes.read().value {
                *item = with_modes(item.clone(), &reply);
            }
        }
        pc
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::helper_first::testkit::{centre, port, Net};
    use crate::helper_link::TransportError;
    use dyno_core::helper_wire::HelperReply;
    use dyno_core::wake::WolMode;
    use dyno_core::wol::readiness::{cant_tell, linux_wakeup_item, ReadinessItem};
    use paddock_jobs::time::FakeClock;
    use std::sync::Arc;

    struct Fixed(ThisPcWol);
    impl ReadinessSource for Fixed {
        fn read(&self) -> ThisPcWol {
            self.0.clone()
        }
    }

    fn pc() -> Arc<Fixed> {
        Arc::new(Fixed(ThisPcWol {
            items: vec![
                linux_wakeup_item("enp3s0", "enabled\n"),
                linux_wakeup_item("wlan0", "disabled\n"),
                ReadinessItem {
                    kind: ReadinessKind::Firmware,
                    subject: None,
                    state: cant_tell("firmware"),
                },
            ],
        }))
    }

    fn over(up: bool, answers: Vec<Result<HelperReply, TransportError>>) -> (HelperReadiness, Arc<Net>) {
        let net = Net::new(up, answers);
        let c = centre(&net, &FakeClock::new());
        (HelperReadiness::new(pc(), port(&c)), net)
    }

    #[test]
    fn a_card_that_may_wake_the_computer_is_asked_for_its_mode_and_only_that_card() {
        let (r, _) = over(
            true,
            vec![Ok(HelperReply::Wol(WolReply {
                enabled: vec![WolMode::Magic],
                supported: vec![WolMode::Magic],
            }))],
        );
        let got = r.read();
        assert!(
            matches!(&got.items[0].state, ItemState::Ok { note } if note.contains("magic packets")),
            "{:?}",
            got.items[0]
        );
        // The disabled card and the firmware row were not asked about (the script holds one answer; a second ask would panic).
        assert!(matches!(got.items[1].state, ItemState::Problem { .. }));
    }

    #[test]
    fn a_card_set_to_no_magic_is_a_problem_with_the_fix() {
        let (r, _) = over(
            true,
            vec![Ok(HelperReply::Wol(WolReply {
                enabled: vec![],
                supported: vec![WolMode::Magic],
            }))],
        );
        assert!(matches!(&r.read().items[0].state, ItemState::Problem { fix, .. } if fix.contains("ethtool -s enp3s0 wol g")));
    }

    #[test]
    fn without_the_helper_the_old_finding_stays() {
        let (r, net) = over(false, vec![]);
        let before = pc().read();
        assert_eq!(r.read(), before);
        assert_eq!(
            net.infos.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "an absent helper is not probed twice"
        );
    }

    #[test]
    fn a_helper_that_refuses_leaves_the_old_finding() {
        let (r, _) = over(
            true,
            vec![Err(TransportError::Remote {
                code: "failed".into(),
                message: "ethtool is not installed".into(),
            })],
        );
        assert_eq!(r.read(), pc().read());
    }
}
