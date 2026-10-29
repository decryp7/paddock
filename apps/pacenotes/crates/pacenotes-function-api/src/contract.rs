//! The contract test of a [`Host`], for the engine's real host and for every fake: cancel ends a call, a budget limits a
//! reply, and a model call from background work is refused.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use pacenotes_core::ports::TokenBudget;
use paddock_jobs::CancelToken;

use crate::{Budget, BudgetKind, Host, HostCall, HostError, HostReply, HostRequest, Origin};

/// What [`host_contract`] needs from the host under test: at least two symbols match `query`.
pub struct HostFixture<'a> {
    /// The host.
    pub host: &'a dyn Host,
    /// A query that matches at least two symbols.
    pub query: &'a str,
}

/// Runs the contract of [`Host`].
pub fn host_contract(fx: &HostFixture<'_>) {
    let find = |budget: Budget| {
        HostRequest::new(
            1,
            HostCall::FindSymbols {
                query: fx.query.to_owned(),
            },
            budget,
        )
        .unwrap()
    };
    let roomy = Budget::new(30, 1000, 1 << 20).unwrap();

    // a call answers within its budget
    match fx.host.call(&find(roomy), &CancelToken::new()).unwrap() {
        HostReply::Symbols(s) => assert!(s.len() >= 2 && s.len() <= 1000),
        other => panic!("expected symbols, got {other:?}"),
    }

    // the budget has an effect: one item is room for fewer than the host found, so the host stops and says which limit
    let one = Budget::new(30, 1, 1 << 20).unwrap();
    match fx.host.call(&find(one), &CancelToken::new()) {
        Err(HostError::BudgetExceeded(b)) => assert_eq!((b.kind, b.limit), (BudgetKind::Items, 1)),
        other => panic!("a one-item budget must stop a two-item answer, got {other:?}"),
    }

    // cancel ends the call
    let dead = CancelToken::new();
    dead.cancel();
    assert_eq!(fx.host.call(&find(roomy), &dead), Err(HostError::Cancelled));

    // the model answers only a person who is waiting
    let ask = |origin| {
        let call = HostCall::Complete {
            prompt: "why?".into(),
            origin,
            tokens: TokenBudget::new(16).unwrap(),
        };
        HostRequest::new(2, call, roomy).unwrap()
    };
    assert!(
        matches!(
            fx.host.call(&ask(Origin::Background), &CancelToken::new()),
            Err(HostError::Refused(_))
        ),
        "a model call from background work is refused (family rule 46)"
    );
}
