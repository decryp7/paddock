use crate::{HoldRequest, PowerError, PowerEvent};
use std::sync::Arc;

/// One OS sleep-prevention object. Dropping it releases it; the OS also releases it when the process ends.
pub trait Hold: Send {
    /// The OS's own reference for logs (assertion ids, request handle, inhibitor), no user content.
    fn os_ref(&self) -> String;
}

/// Takes OS sleep-prevention objects. The real adapters call the OS; tests use [`FakePower`](crate::FakePower).
///
/// Contract: [`acquire`](Self::acquire) either returns a hold that keeps the system awake until it is dropped or returns a
/// typed error and holds nothing; it never blocks for long (an OS or D-Bus call) and never panics.
pub trait PowerOs: Send + Sync {
    /// Takes a hold.
    fn acquire(&self, request: &HoldRequest) -> Result<Box<dyn Hold>, PowerError>;
}

/// Where sleep and wake notifications go. Called on the adapter's own thread, so it must be quick and must not block.
pub type EventSink = Arc<dyn Fn(PowerEvent) + Send + Sync>;

/// A live subscription; dropping it stops the notifications and ends the adapter's thread.
pub trait Subscription: Send {}

/// Sleep and wake notifications from the OS.
pub trait SleepWakeSource: Send + Sync {
    /// Starts delivering [`PowerEvent`]s to `sink` until the subscription is dropped.
    fn subscribe(&self, sink: EventSink) -> Result<Box<dyn Subscription>, PowerError>;
}
