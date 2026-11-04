//! The `INetFwPolicy2` COM adapter (ADR 0027 WP-9). Every read runs on a fresh thread that enters a multithreaded apartment, so the
//! caller's apartment never matters; the thread leaves it before it ends. Property reads that fail leave that field out (the
//! rule is still listed); only failing to reach the policy object at all is an error. Nothing is logged but counts.

use crate::parse::firewall::RawWinRule;
use crate::windows_firewall::{WinFirewallPort, WinProfile, WinRules};
use dyno_core::firewall::{FirewallProfile, RuleAction, MAX_FIREWALL_RULES};
use dyno_core::ports::PlatformError;
use windows::core::{Interface, BSTR};
use windows::Win32::NetworkManagement::WindowsFirewall::{
    INetFwPolicy2, INetFwRule, NetFwPolicy2, NET_FW_ACTION, NET_FW_ACTION_BLOCK, NET_FW_PROFILE2_DOMAIN, NET_FW_PROFILE2_PRIVATE,
    NET_FW_PROFILE2_PUBLIC, NET_FW_PROFILE_TYPE2,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, IDispatch, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use windows::Win32::System::Ole::IEnumVARIANT;
use windows::Win32::System::Variant::VARIANT;

/// Keeps the thread in the apartment until dropped.
struct Apartment;

impl Apartment {
    fn enter() -> Result<Apartment, PlatformError> {
        // SAFETY: called once on a thread this adapter created; paired with `CoUninitialize` in `Drop` only when it succeeded.
        let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        hr.ok()
            .map_err(|e| PlatformError::Failed(format!("COM could not start on the firewall thread: {e}")))?;
        Ok(Apartment)
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        // SAFETY: balances the successful `CoInitializeEx` of `Apartment::enter` on this thread.
        unsafe { CoUninitialize() };
    }
}

fn text(b: windows::core::Result<BSTR>) -> Option<String> {
    b.ok().map(|b| b.to_string()).filter(|s| !s.is_empty())
}

fn action(a: NET_FW_ACTION) -> RuleAction {
    if a == NET_FW_ACTION_BLOCK {
        RuleAction::Block
    } else {
        RuleAction::Allow
    }
}

fn profile(policy: &INetFwPolicy2, kind: NET_FW_PROFILE_TYPE2, which: FirewallProfile, current: i32) -> WinProfile {
    // SAFETY: `policy` is a live COM interface owned by this thread; each call only reads a property.
    unsafe {
        WinProfile {
            profile: which,
            enabled: policy.get_FirewallEnabled(kind).map(|b| b.as_bool()).unwrap_or(false),
            block_all_inbound: policy.get_BlockAllInboundTraffic(kind).map(|b| b.as_bool()).unwrap_or(false),
            default_in: policy.get_DefaultInboundAction(kind).map(action).unwrap_or(RuleAction::Block),
            default_out: policy.get_DefaultOutboundAction(kind).map(action).unwrap_or(RuleAction::Allow),
            current: current & kind.0 != 0,
        }
    }
}

fn raw_rule(rule: &INetFwRule) -> RawWinRule {
    // SAFETY: `rule` is a live COM interface; each call only reads a property and a failure leaves the field out.
    unsafe {
        let ports = [("local", text(rule.LocalPorts())), ("remote", text(rule.RemotePorts()))]
            .into_iter()
            .filter_map(|(side, p)| p.filter(|p| p != "*").map(|p| format!("{side} {p}")))
            .collect::<Vec<_>>();
        RawWinRule {
            name: text(rule.Name()).unwrap_or_default(),
            direction: rule.Direction().map(|d| d.0).unwrap_or(0),
            action: rule.Action().map(|a| a.0).unwrap_or(1),
            enabled: rule.Enabled().map(|b| b.as_bool()).unwrap_or(false),
            profiles: rule.Profiles().unwrap_or(0x7fff_ffff),
            application: text(rule.ApplicationName()),
            ports: (!ports.is_empty()).then(|| ports.join("; ")),
            addresses: text(rule.RemoteAddresses()),
            group: text(rule.Grouping()),
            service: text(rule.ServiceName()),
            protocol: rule.Protocol().ok(),
        }
    }
}

fn read_in_apartment() -> Result<(Vec<WinProfile>, WinRules), PlatformError> {
    let _apartment = Apartment::enter()?;
    let unreachable = |e: windows::core::Error| PlatformError::Failed(format!("Windows Defender Firewall could not be reached: {e}"));
    // SAFETY: COM is initialised on this thread; every interface below lives and dies on it.
    unsafe {
        let policy: INetFwPolicy2 = CoCreateInstance(&NetFwPolicy2, None, CLSCTX_INPROC_SERVER).map_err(unreachable)?;
        let current = policy.CurrentProfileTypes().unwrap_or(0);
        let profiles = vec![
            profile(&policy, NET_FW_PROFILE2_DOMAIN, FirewallProfile::Domain, current),
            profile(&policy, NET_FW_PROFILE2_PRIVATE, FirewallProfile::Private, current),
            profile(&policy, NET_FW_PROFILE2_PUBLIC, FirewallProfile::Public, current),
        ];
        let rules = policy.Rules().map_err(unreachable)?;
        let total = rules.Count().ok().and_then(|n| u32::try_from(n).ok()).unwrap_or(0);
        let mut out = Vec::new();
        let en: IEnumVARIANT = rules._NewEnum().and_then(|u| u.cast()).map_err(unreachable)?;
        while out.len() < MAX_FIREWALL_RULES {
            let mut item = [VARIANT::default()];
            let mut got = 0u32;
            if en.Next(&mut item, &mut got).is_err() || got == 0 {
                break;
            }
            if let Ok(rule) = IDispatch::try_from(&item[0]).and_then(|d| d.cast::<INetFwRule>()) {
                out.push(raw_rule(&rule));
            }
        }
        let read = u32::try_from(out.len()).unwrap_or(u32::MAX);
        Ok((
            profiles,
            WinRules {
                rules: out,
                total: total.max(read),
            },
        ))
    }
}

/// The real firewall through COM.
pub struct ComFirewall;

impl WinFirewallPort for ComFirewall {
    fn read(&self) -> Result<(Vec<WinProfile>, WinRules), PlatformError> {
        std::thread::Builder::new()
            .name("firewall-com".into())
            .spawn(read_in_apartment)
            .map_err(|e| PlatformError::Failed(format!("The firewall thread could not start: {e}")))?
            .join()
            .map_err(|_| PlatformError::Failed("The firewall reader stopped unexpectedly.".into()))?
    }
}
