//! The `Extension` mechanism (ADR 0001, "Wire"): an app's own contract with its plugins.
//!
//! The app's contract crate implements [`Contract`] for its own request and reply enums (postcard, like every wire type);
//! a plugin registers a handler for it with [`crate::plugin::Contributions::add_extension`]; the platform routes
//! [`crate::protocol::PlatformRequest::Extension`] by `ns` and **never decodes the body**. The namespace carries its major
//! version in its name; [`Contract::MINOR`] is the newest minor of it the code knows, and a request for a newer minor is
//! refused with text that says the plugin must be updated.

use crate::tools::Host;
use paddock_ipc::{RemoteError, codes};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::sync::Arc;

/// An app's contract: a namespace, its newest known minor version and the request and reply types.
///
/// ```
/// use paddock_plugin_api::extension::Contract;
/// use serde::{Deserialize, Serialize};
///
/// #[derive(Serialize, Deserialize)]
/// enum Ask { Ping }
/// #[derive(Serialize, Deserialize)]
/// enum Answer { Pong }
///
/// struct Ping;
/// impl Contract for Ping {
///     const NS: &'static str = "example.ping.v1";
///     const MINOR: u16 = 0;
///     type Request = Ask;
///     type Reply = Answer;
/// }
/// assert_eq!(Ping::NS, "example.ping.v1");
/// ```
pub trait Contract: 'static {
    /// The namespace, major version included (`debrief.import.v1`).
    const NS: &'static str;
    /// The newest minor version this code knows: adding a variant at the end of an enum raises it.
    const MINOR: u16;
    /// What the app asks.
    type Request: Serialize + DeserializeOwned;
    /// What the plugin answers.
    type Reply: Serialize + DeserializeOwned;
}

/// What a contract handler sees of the call.
pub struct ExtCtx<'a> {
    pub(crate) cancelled: &'a dyn Fn() -> bool,
    pub(crate) host: Arc<dyn Host>,
}

impl ExtCtx<'_> {
    /// Whether the app cancelled the call; long work stops early when it is.
    pub fn is_cancelled(&self) -> bool {
        (self.cancelled)()
    }

    /// The app's services (host files, reports, output).
    pub fn host(&self) -> &Arc<dyn Host> {
        &self.host
    }
}

/// A plugin's handler of one contract.
pub trait ExtensionHandler<C: Contract>: Send + Sync + 'static {
    /// Answers one request. A typed failure is a [`RemoteError`] (the call fails, the plugin is fine).
    fn handle(&self, ctx: &ExtCtx, request: C::Request) -> Result<C::Reply, RemoteError>;
}

impl<C, F> ExtensionHandler<C> for F
where
    C: Contract,
    F: Fn(&ExtCtx, C::Request) -> Result<C::Reply, RemoteError> + Send + Sync + 'static,
{
    fn handle(&self, ctx: &ExtCtx, request: C::Request) -> Result<C::Reply, RemoteError> {
        self(ctx, request)
    }
}

type Run = dyn Fn(&ExtCtx, &[u8]) -> Result<Vec<u8>, RemoteError> + Send + Sync;

/// A registered contract handler with the types erased: bytes in, bytes out.
pub(crate) struct Registered {
    pub(crate) ns: &'static str,
    pub(crate) minor: u16,
    run: Box<Run>,
}

impl Registered {
    pub(crate) fn new<C: Contract>(handler: impl ExtensionHandler<C>) -> Registered {
        Registered {
            ns: C::NS,
            minor: C::MINOR,
            run: Box::new(move |ctx, body| {
                let request = paddock_ipc::decode_request::<C::Request>(body)?;
                paddock_ipc::encode_reply(&handler.handle(ctx, request)?)
            }),
        }
    }
}

/// Routes a request to the registered handler of its namespace, for plugin `plugin`.
pub(crate) fn route(
    registered: &[Registered],
    plugin: &str,
    ns: &str,
    minor: u16,
    body: &[u8],
    ctx: &ExtCtx,
) -> Result<Vec<u8>, RemoteError> {
    let Some(entry) = registered.iter().find(|r| r.ns == ns) else {
        return Err(RemoteError::new(codes::UNIMPLEMENTED, format!("plugin {plugin} does not speak \"{ns}\"")));
    };
    if minor > entry.minor {
        return Err(RemoteError::new(
            codes::VERSION,
            format!(
                "plugin {plugin} was built for an older contract (\"{ns}\" version {}, asked for {minor}); update it",
                entry.minor
            ),
        ));
    }
    (entry.run)(ctx, body)
}

/// A contract for the tests of this crate (u32 in, u32 out).
#[cfg(test)]
pub(crate) mod tests_support {
    use super::Contract;

    pub(crate) struct Calc;
    impl Contract for Calc {
        const NS: &'static str = "test.calc.v1";
        const MINOR: u16 = 1;
        type Request = u32;
        type Reply = u32;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::NoHost;
    use serde::Deserialize;

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    enum Ask {
        Add(u32, u32),
    }
    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    enum Answer {
        Sum(u32),
    }
    struct Calc;
    impl Contract for Calc {
        const NS: &'static str = "test.calc.v1";
        const MINOR: u16 = 1;
        type Request = Ask;
        type Reply = Answer;
    }

    fn entries() -> Vec<Registered> {
        vec![Registered::new::<Calc>(|ctx: &ExtCtx, Ask::Add(a, b): Ask| {
            if ctx.is_cancelled() {
                return Err(RemoteError::new(codes::CANCELLED, "stop"));
            }
            Ok(Answer::Sum(a + b))
        })]
    }

    fn ctx(cancelled: &dyn Fn() -> bool) -> ExtCtx<'_> {
        ExtCtx {
            cancelled,
            host: Arc::new(NoHost),
        }
    }

    #[test]
    fn requests_are_routed_by_namespace_and_never_decoded_by_the_platform() {
        let body = paddock_ipc::wire::encode(&Ask::Add(2, 3)).unwrap();
        let no = || false;
        let reply = route(&entries(), "p", "test.calc.v1", 1, &body, &ctx(&no)).unwrap();
        assert_eq!(paddock_ipc::wire::decode::<Answer>(&reply).unwrap(), Answer::Sum(5));
        // An older minor is served; the handler sees the same enum.
        assert!(route(&entries(), "p", "test.calc.v1", 0, &body, &ctx(&no)).is_ok());
        let yes = || true;
        assert_eq!(route(&entries(), "p", "test.calc.v1", 1, &body, &ctx(&yes)).unwrap_err().code, codes::CANCELLED);
        assert!(ctx(&yes).is_cancelled() && ctx(&yes).host().read_file(0, 0, 1).is_err());
    }

    #[test]
    fn an_unknown_namespace_a_newer_minor_and_garbage_are_refused() {
        let body = paddock_ipc::wire::encode(&Ask::Add(1, 1)).unwrap();
        let no = || false;
        let c = ctx(&no);
        let unknown = route(&entries(), "p", "other.v1", 0, &body, &c).unwrap_err();
        assert_eq!((unknown.code.as_str(), unknown.message.as_str()), (codes::UNIMPLEMENTED, "plugin p does not speak \"other.v1\""));
        let newer = route(&entries(), "p", "test.calc.v1", 2, &body, &c).unwrap_err();
        assert_eq!(newer.code, codes::VERSION);
        assert_eq!(
            newer.message,
            "plugin p was built for an older contract (\"test.calc.v1\" version 1, asked for 2); update it"
        );
        let garbled = route(&entries(), "p", "test.calc.v1", 1, &[0xff, 0xff, 0xff], &c).unwrap_err();
        assert_eq!(garbled.code, codes::INVALID);
    }
}
