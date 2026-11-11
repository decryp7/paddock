//! Memory modules (Linux): the slot table is root-only, so the helper reads it (`MemoryModules`); without the helper Dyno knows
//! the total and says why. The second instance of the seam, and the smallest (helper-wiring plan section 8.3).

use crate::helper_first::{HelperCalls, HelperReading, ReadClass};
use crate::helper_link::CallError;
use dyno_core::helper::HelperHost;
use dyno_core::helper_policy::RequestKind;
use dyno_core::inventory::{MemoryLayout, Unavailable};
use dyno_core::metrics::Reading;

/// Reads the layout on this computer for a total in bytes (the platform's reader in the agent, a fixed one in tests).
pub type LocalMemory = Box<dyn Fn(u64) -> MemoryLayout + Send + Sync>;

/// The memory layout: the helper's, else this computer's.
pub struct MemoryModules {
    host: HelperHost,
    total: u64,
    local: LocalMemory,
}

impl MemoryModules {
    /// A reading for `host` whose local reader is `local`.
    pub fn new(host: HelperHost, local: LocalMemory) -> MemoryModules {
        MemoryModules { host, total: 0, local }
    }

    /// The installed memory as the OS counts it, for the answers that carry only a total. Set before each read.
    pub fn set_total(&mut self, total: u64) {
        self.total = total;
    }
}

impl HelperReading for MemoryModules {
    type Value = MemoryLayout;
    const KINDS: &'static [RequestKind] = &[RequestKind::MemoryModules];

    fn name(&self) -> &'static str {
        "memory modules"
    }

    fn class(&self) -> ReadClass {
        ReadClass::Live
    }

    fn via_helper(&self, calls: &dyn HelperCalls) -> Result<MemoryLayout, CallError> {
        Ok(match calls.memory()? {
            // The helper knows the slots or says why not; its own total may be unknown to it.
            MemoryLayout::TotalOnly { total: 0, reason } => MemoryLayout::TotalOnly { total: self.total, reason },
            other => other,
        })
    }

    fn local(&mut self) -> Reading<MemoryLayout> {
        Reading::Available((self.local)(self.total))
    }

    fn partial(&self, local: &MemoryLayout) -> Option<String> {
        // Only Linux's table is root-only; elsewhere a total-only answer is as good as it gets.
        match local {
            MemoryLayout::TotalOnly { reason, .. } if self.host == HelperHost::Linux && *reason == Unavailable::linux_dmi_root_only() => {
                Some("The memory slots are readable only by an administrator.".into())
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::helper_first::testkit::{centre, port, Net};
    use crate::helper_first::HelperFirst;
    use dyno_core::helper_wire::HelperReply;
    use dyno_core::read_via::ReadVia;
    use paddock_jobs::time::FakeClock;

    fn root_only(total: u64) -> MemoryLayout {
        MemoryLayout::TotalOnly {
            total,
            reason: Unavailable::linux_dmi_root_only(),
        }
    }

    fn reading(
        host: HelperHost,
        up: bool,
        answers: Vec<Result<HelperReply, crate::helper_link::TransportError>>,
    ) -> HelperFirst<MemoryModules> {
        let c = centre(&Net::new(up, answers), &FakeClock::new());
        let mut r = HelperFirst::new(MemoryModules::new(host, Box::new(root_only)), port(&c));
        r.reading_mut().set_total(16 << 30);
        r
    }

    #[test]
    fn on_linux_without_the_helper_the_total_is_shown_and_the_helper_is_asked_for() {
        let got = reading(HelperHost::Linux, false, vec![]).read();
        assert_eq!(got.value.value().map(MemoryLayout::total), Some(16 << 30));
        assert_eq!(
            got.via,
            ReadVia::needs_admin("The memory slots are readable only by an administrator.")
        );
    }

    #[test]
    fn elsewhere_a_total_only_answer_is_plain_local() {
        assert_eq!(reading(HelperHost::Windows, false, vec![]).read().via, ReadVia::Local);
    }

    #[test]
    fn a_helper_answer_without_a_total_gets_the_agents_total_and_keeps_its_reason() {
        let none = MemoryLayout::TotalOnly {
            total: 0,
            reason: Unavailable::failed("This computer does not publish its memory slot table."),
        };
        let got = reading(HelperHost::Linux, true, vec![Ok(HelperReply::Memory(none))]).read();
        let Some(MemoryLayout::TotalOnly { total, reason }) = got.value.value().cloned() else {
            panic!("{got:?}")
        };
        assert_eq!(
            (total, reason.reason.as_str()),
            (16 << 30, "This computer does not publish its memory slot table.")
        );
        assert_eq!(got.via, ReadVia::Helper);
    }
}
