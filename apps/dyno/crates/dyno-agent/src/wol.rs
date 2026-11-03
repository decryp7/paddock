//! Wake-on-LAN in the agent (ADR 0032 section 3, D6, D8). The saved computers live in `dyno.db`; a SecureOn password lives in the
//! OS credential store under the device's id and never anywhere else. A wake is a job (phases, Cancel, one result): the pure run
//! is `dyno_core::wol::wake::run_wake` over the platform's ports, so a test never sends a datagram or pings a host (ER-14).
//!
//! Logs (ER-27): each wake logs its outcome and duration, and per planned datagram the card name, destination, size, rounds sent
//! and error; the hardware address appears only as its last two bytes and a device only by its id. Never a name, never a
//! password. Relaying through another Dyno is not built (D8).

use crate::agent::{lock, now, Shared};
use dyno_core::power::Capability;
use dyno_core::protocol::{AgentError, ErrorCode, Message};
use dyno_core::wol::plan::plan_sends;
use dyno_core::wol::wake::{run_wake, Pacer, WakePhase, WakePorts, WakeRequest, WolOutcome};
use dyno_core::wol::WolClock;
use dyno_core::wol::{
    candidates, validate_input, LastWake, Neighbour, SecureChange, SecureOn, ThisPcWol, WolDevice, WolDeviceInput, WolView,
};
use dyno_platform::wol::WolPorts;
use paddock_jobs::{JobContext, JobError, Phase};
use paddock_secrets::{Name, Secret};
use std::time::{Duration, Instant};

/// How finely a pause looks for Cancel.
const SLICE: Duration = Duration::from_millis(100);

/// The agent's Wake-on-LAN service.
pub struct WolCentre {
    ports: WolPorts,
}

fn bad(m: impl Into<String>) -> AgentError {
    AgentError::new(ErrorCode::BadRequest, m)
}

fn internal(m: impl Into<String>) -> AgentError {
    AgentError::new(ErrorCode::Internal, m)
}

fn secret_name(id: u64) -> Option<Name> {
    Name::new("wol", &format!("secure-on-{id}")).ok()
}

/// A job's clock and Cancel: pauses in slices so Cancel is noticed within a tenth of a second.
struct JobPacer<'a> {
    ctx: &'a JobContext,
    clock: &'a dyn WolClock,
    started: Duration,
    phase: Option<WakePhase>,
}

impl Pacer for JobPacer<'_> {
    fn pause(&mut self, secs: u32) -> bool {
        let until = self.clock.now() + Duration::from_secs(u64::from(secs));
        while self.clock.now() < until {
            if self.ctx.cancel_token().is_cancelled() {
                return false;
            }
            self.clock.sleep(SLICE.min(until.saturating_sub(self.clock.now())));
        }
        !self.ctx.cancel_token().is_cancelled()
    }

    fn elapsed_secs(&self) -> u32 {
        u32::try_from(self.clock.now().saturating_sub(self.started).as_secs()).unwrap_or(u32::MAX)
    }

    fn progress(&mut self, phase: WakePhase, done: u32, total: Option<u32>) {
        if self.phase != Some(phase) {
            self.phase = Some(phase);
            self.ctx.phase(match total {
                Some(t) => Phase::items(phase.id(), phase.label(), u64::from(t)),
                // Seconds waited, growing: no total (ER-18).
                None => Phase::discovering(phase.id(), phase.label()),
            });
        }
        self.ctx.set(u64::from(done));
    }
}

impl WolCentre {
    /// A service over `ports`.
    pub fn new(ports: WolPorts) -> WolCentre {
        WolCentre { ports }
    }

    fn devices(&self, shared: &Shared) -> Result<Vec<WolDevice>, AgentError> {
        let mut all = lock(&shared.core)
            .store
            .wol_devices()
            .map_err(|e| internal(format!("The saved computers could not be read: {e}")))?;
        all.sort_by_key(|d| (d.name.to_lowercase(), d.id));
        Ok(all)
    }

    fn enabled(shared: &Shared) -> bool {
        lock(&shared.core).settings.power.wol.enabled
    }

    /// The list, with whether this computer can send at all.
    pub fn view(&self, shared: &Shared) -> Result<WolView, AgentError> {
        let can_send = if self.ports.interfaces.ipv4().is_empty() {
            Capability::CantTell {
                reason: "No network card with an IPv4 address is connected.".into(),
            }
        } else {
            Capability::Known
        };
        Ok(WolView {
            enabled: Self::enabled(shared),
            devices: self.devices(shared)?,
            can_send,
        })
    }

    /// Neighbour-table entries that are not saved yet. Read on demand; refused when discovery is off.
    pub fn candidates(&self, shared: &Shared) -> Result<Vec<Neighbour>, AgentError> {
        let s = lock(&shared.core).settings.power.wol.clone();
        if !s.enabled || !s.discovery {
            return Err(bad("Add from network is turned off in Settings."));
        }
        let t = Instant::now();
        let all = self
            .ports
            .neighbours
            .neighbours()
            .map_err(|e| internal(format!("The neighbour table could not be read: {e}")))?;
        let found = all.len();
        let mut out = candidates(all, &self.devices(shared)?);
        // Names come from the Network page's resolver (bounded, cached, with its own deadline); an address that has none stays blank.
        let ips: Vec<String> = out
            .iter()
            .map(|n| n.ip.clone())
            .take(dyno_core::network_names::MAX_RESOLVE)
            .collect();
        if let Ok(names) = shared.network.resolve(&ips) {
            for n in out.iter_mut() {
                n.name = names.iter().find(|h| h.address == n.ip).and_then(|h| h.name.clone());
            }
        }
        log::info!(target: "dyno_agent::wol", "neighbour table read: {found} entries, {} unsaved, in {} ms", out.len(), t.elapsed().as_millis());
        Ok(out)
    }

    /// Adds or changes a computer. A password goes to the credential store before the row is written, so a refused store leaves
    /// nothing half-saved.
    pub fn save(&self, shared: &Shared, input: WolDeviceInput) -> Result<WolView, AgentError> {
        if !Self::enabled(shared) {
            return Err(bad("Wake-on-LAN is turned off in Settings."));
        }
        let saved = self.devices(shared)?;
        let valid = validate_input(&input, &saved).map_err(bad)?;
        let mut device = match valid.id.and_then(|id| saved.iter().find(|d| d.id == id)) {
            Some(d) => d.clone(),
            None => WolDevice::default(),
        };
        device.name = valid.name;
        device.mac = valid.mac;
        device.last_ip = valid.last_ip;
        device.interface = valid.interface;
        // The row first when new: the password's name needs the id.
        let adding = device.id == 0;
        let store = |d: &mut WolDevice| {
            lock(&shared.core)
                .store
                .put_wol_device(d)
                .map_err(|e| internal(format!("The computer could not be saved: {e}")))
        };
        if adding {
            store(&mut device)?;
        }
        match valid.secure_on {
            SecureChange::Keep => {}
            SecureChange::Clear => {
                self.forget_password(device.id)?;
                device.secure_on = false;
            }
            SecureChange::Set(p) => match self.keep_password(device.id, &p) {
                Ok(()) => device.secure_on = true,
                Err(e) => {
                    if adding {
                        let _ = lock(&shared.core).store.delete_wol_device(device.id);
                    }
                    return Err(e);
                }
            },
        }
        store(&mut device)?;
        log::info!(target: "dyno_agent::wol", "device {} {} (hardware address {}, SecureOn {})", device.id, if adding { "added" } else { "changed" }, device.mac.short(), if device.secure_on { "set" } else { "none" });
        self.changed(shared)
    }

    /// Removes a computer and its password.
    pub fn remove(&self, shared: &Shared, id: u64) -> Result<WolView, AgentError> {
        self.forget_password(id)?;
        lock(&shared.core)
            .store
            .delete_wol_device(id)
            .map_err(|e| internal(format!("The computer could not be removed: {e}")))?;
        log::info!(target: "dyno_agent::wol", "device {id} removed");
        self.changed(shared)
    }

    /// The list after a change, pushed to every connected app (the tray's Wake submenu follows it).
    fn changed(&self, shared: &Shared) -> Result<WolView, AgentError> {
        let view = self.view(shared)?;
        shared.broadcast(&Message::WolChanged(Box::new(view.clone())));
        Ok(view)
    }

    fn keep_password(&self, id: u64, p: &SecureOn) -> Result<(), AgentError> {
        let name = secret_name(id).ok_or_else(|| internal("The password could not be named."))?;
        let secret = Secret::new(p.to_text()).map_err(|e| bad(e.to_string()))?;
        self.ports.secrets.set(&name, &secret).map_err(|e| {
            AgentError::new(
                ErrorCode::AccessDenied,
                format!("The password could not be stored in the system's credential store: {e}"),
            )
        })
    }

    fn forget_password(&self, id: u64) -> Result<(), AgentError> {
        let Some(name) = secret_name(id) else { return Ok(()) };
        self.ports.secrets.delete(&name).map_err(|e| {
            AgentError::new(
                ErrorCode::AccessDenied,
                format!("The password could not be removed from the system's credential store: {e}"),
            )
        })
    }

    fn password(&self, id: u64) -> Result<Option<SecureOn>, String> {
        let name = secret_name(id).ok_or("The password could not be named.")?;
        match self.ports.secrets.get(&name) {
            Ok(Some(s)) => SecureOn::parse(s.expose())
                .map(Some)
                .map_err(|_| "The stored SecureOn password is damaged; set it again.".to_string()),
            Ok(None) => Ok(None),
            Err(e) => Err(format!("The SecureOn password could not be read from the credential store: {e}")),
        }
    }

    /// A saved computer's name, for a job's subject.
    pub fn name_of(&self, shared: &Shared, id: u64) -> Option<String> {
        self.devices(shared).ok()?.into_iter().find(|d| d.id == id).map(|d| d.name)
    }

    /// This PC's readiness, read now.
    pub fn readiness(&self) -> ThisPcWol {
        let t = Instant::now();
        let r = self.ports.readiness.read();
        log::info!(target: "dyno_agent::wol", "this PC's wake readiness read: {} items in {} ms", r.items.len(), t.elapsed().as_millis());
        r
    }

    /// What a wake request needs before a job starts: the feature is on and the computer exists.
    pub fn check_wake(&self, shared: &Shared, id: u64) -> Result<(), AgentError> {
        if !Self::enabled(shared) {
            return Err(bad("Wake-on-LAN is turned off in Settings."));
        }
        if self.devices(shared)?.iter().any(|d| d.id == id) {
            Ok(())
        } else {
            Err(AgentError::new(ErrorCode::NotFound, "That computer is no longer in the list."))
        }
    }

    /// The wake job: plans the sends, runs them, saves how it ended (and a moved address), and logs the outcome.
    pub fn run(&self, shared: &Shared, ctx: &JobContext, id: u64, test: bool) -> Result<WolOutcome, JobError> {
        let started = Instant::now();
        // A phase label from the first moment (ER-18), before anything is read.
        ctx.phase(Phase::discovering("preparing", "Looking up the computer"));
        let fail = |e: AgentError| JobError::Failed(e.message);
        self.check_wake(shared, id).map_err(fail)?;
        let device = self
            .devices(shared)
            .map_err(fail)?
            .into_iter()
            .find(|d| d.id == id)
            .ok_or_else(|| JobError::Failed("That computer is no longer in the list.".into()))?;
        let settings = lock(&shared.core).settings.power.wol.clone();
        let secure_on = if device.secure_on {
            self.password(id).map_err(JobError::Failed)?
        } else {
            None
        };
        let plan = plan_sends(
            &self.ports.interfaces.ipv4(),
            device.interface.as_deref(),
            settings.directed_broadcast,
        );
        let request = WakeRequest {
            mac: device.mac,
            secure_on,
            last_ip: device.last_ip.clone(),
            plan,
            port: device.port,
            test_timeout_secs: test.then_some(settings.test_timeout_secs),
        };
        log::info!(target: "dyno_agent::wol", "wake of device {id} ({}) starting: {} datagrams planned, test={test}", device.mac.short(), request.plan.len());
        let mut pacer = JobPacer {
            ctx,
            clock: &*self.ports.clock,
            started: self.ports.clock.now(),
            phase: None,
        };
        let outcome = run_wake(
            &request,
            &WakePorts {
                udp: &*self.ports.udp,
                neighbours: &*self.ports.neighbours,
                prober: &*self.ports.prober,
            },
            &mut pacer,
        );
        for r in &outcome.sent {
            log::info!(
                target: "dyno_agent::wol",
                "device {id}: card {} to {}: {} bytes, {} of {} rounds sent{}",
                r.interface, r.dest, r.bytes, r.sent, dyno_core::wol::wake::ROUNDS,
                r.error.as_deref().map(|e| format!(", error: {e}")).unwrap_or_default()
            );
        }
        log::info!(
            target: "dyno_agent::wol",
            "wake of device {id} ended in {} ms: answered={} timed_out={} cancelled={}",
            started.elapsed().as_millis(), outcome.answered.is_some(), outcome.timed_out, outcome.cancelled
        );
        if outcome.cancelled {
            ctx.check()?;
            return Err(JobError::Failed("Cancelled.".into()));
        }
        self.record(shared, id, &outcome);
        Ok(outcome)
    }

    /// Saves how the wake ended on the device; a changed address is saved too.
    fn record(&self, shared: &Shared, id: u64, o: &WolOutcome) {
        let sent_ok = o.sent.iter().any(|r| r.sent > 0) && o.sent.iter().all(|r| r.error.is_none());
        let ok = if o.tested { o.answered.is_some() } else { sent_ok };
        let core = lock(&shared.core);
        let Ok(devices) = core.store.wol_devices() else { return };
        let Some(mut d) = devices.into_iter().find(|d| d.id == id) else {
            return;
        };
        d.last_wake = Some(LastWake {
            at: now(),
            ok,
            tested: o.tested,
            summary: o.message.clone(),
        });
        if let Some(ip) = o.answered.as_ref().and_then(|a| a.new_ip.clone()) {
            d.last_ip = Some(ip);
        }
        let saved = core.store.put_wol_device(&mut d);
        drop(core);
        match saved {
            Ok(_) => {
                let _ = self.changed(shared);
            }
            Err(e) => log::warn!(target: "dyno_agent::wol", "could not save how the wake of device {id} ended: {e}"),
        }
    }
}
