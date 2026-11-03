//! One echo through the OS's own `ping` program (no raw socket, no elevated right), with a short timeout. Only IPv4 addresses are
//! pinged; the address is parsed first so nothing but an address ever reaches the program's arguments.

use crate::runner::{tool, ToolRunner, TOOL_TIMEOUT};
use dyno_core::wol::wake::Prober;
use std::net::IpAddr;

/// Pings through a [`ToolRunner`].
pub struct OsProber {
    runner: Box<dyn ToolRunner>,
}

impl OsProber {
    /// A prober over `runner`.
    pub fn new(runner: Box<dyn ToolRunner>) -> OsProber {
        OsProber {
            runner,
        }
    }
}

/// The program and its arguments for one echo to `ip`, waiting about two seconds.
#[cfg(windows)]
fn echo_tool(ip: &str) -> crate::runner::Tool {
    tool("ping", &["-n", "1", "-w", "2000", ip])
}

/// The program and its arguments for one echo to `ip`, waiting about two seconds.
#[cfg(target_os = "macos")]
fn echo_tool(ip: &str) -> crate::runner::Tool {
    tool("/sbin/ping", &["-c", "1", "-t", "2", ip])
}

/// The program and its arguments for one echo to `ip`, waiting about two seconds.
#[cfg(not(any(windows, target_os = "macos")))]
fn echo_tool(ip: &str) -> crate::runner::Tool {
    tool("ping", &["-c", "1", "-W", "2", ip])
}

impl Prober for OsProber {
    fn ping(&self, ip: &str) -> Result<bool, String> {
        let addr: IpAddr = ip.parse().map_err(|_| "not an IP address".to_string())?;
        if !addr.is_ipv4() {
            return Err("ping of an IPv6 address is not supported".into());
        }
        match self.runner.run_tool(&echo_tool(&addr.to_string()), TOOL_TIMEOUT) {
            // On Windows an unreachable host can exit zero, with "Destination host unreachable" in the output.
            Ok(out) => Ok(out.ok && !out.stdout.contains("unreachable") && !out.stdout.contains("timed out")),
            Err(e) => Err(format!("ping is not available: {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::script::{fail, out, Script};
    use super::*;
    use dyno_core::ports::PlatformError;
    use std::sync::Mutex;

    fn prober(
        answers: Vec<(&'static str, Result<crate::runner::ToolOutput, PlatformError>)>,
    ) -> (OsProber, std::sync::Arc<Mutex<Vec<crate::runner::Tool>>>) {
        let calls = std::sync::Arc::new(Mutex::new(Vec::new()));
        (
            OsProber::new(Box::new(Script {
                answers,
                calls: calls.clone(),
            })),
            calls,
        )
    }

    #[test]
    fn an_answer_a_silence_and_a_missing_program() {
        let (p, calls) = prober(vec![("ping", out("1 packets transmitted, 1 received"))]);
        assert_eq!(p.ping("192.0.2.40"), Ok(true));
        assert!(calls.lock().unwrap()[0].args.contains(&"192.0.2.40".to_string()));
        let (p, _) = prober(vec![("ping", fail("100% packet loss"))]);
        assert_eq!(p.ping("192.0.2.40"), Ok(false));
        let (p, _) = prober(vec![("ping", out("Reply from 192.0.2.1: Destination host unreachable."))]);
        assert_eq!(p.ping("192.0.2.40"), Ok(false));
        let (p, _) = prober(vec![]);
        assert!(p.ping("192.0.2.40").unwrap_err().starts_with("ping is not available"));
    }

    #[test]
    fn only_ipv4_addresses_reach_the_program() {
        let (p, calls) = prober(vec![("ping", out("ok"))]);
        assert!(p.ping("-f 192.0.2.1").is_err());
        assert!(p.ping("2001:db8::1").unwrap_err().contains("IPv6"));
        assert!(p.ping("host.example").is_err());
        assert!(calls.lock().unwrap().is_empty());
    }
}
