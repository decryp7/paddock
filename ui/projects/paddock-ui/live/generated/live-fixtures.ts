// Generated from paddock-live's wire model by its tests (Rust round trip). Do not edit: PADDOCK_WRITE_TS=1 cargo test -p paddock-live --features tauri --test it generated.

/** A conversation as Rust sent it (snapshot, merged and plain deltas, link changes) with the state, sequence and link
 * Rust ended with. The state is the test overview `{ memory, events, count }`. */
export const LIVE_CONVERSATION: { frames: readonly unknown[]; finalSeq: number; finalState: unknown; finalLink: unknown } = {
  "finalLink": {
    "state": "stopped"
  },
  "finalSeq": 6,
  "finalState": {
    "count": 1,
    "events": [
      "first",
      "second"
    ],
    "memory": 200
  },
  "frames": [
    {
      "kind": "snapshot",
      "link": {
        "state": "connected"
      },
      "seq": 1,
      "state": {
        "count": 0,
        "events": [],
        "memory": 0
      }
    },
    {
      "delta": {
        "kind": "event",
        "text": "first"
      },
      "from": 2,
      "kind": "delta",
      "seq": 2
    },
    {
      "delta": {
        "bytes": 200,
        "kind": "memory"
      },
      "from": 3,
      "kind": "delta",
      "seq": 4
    },
    {
      "delta": {
        "kind": "bump"
      },
      "from": 5,
      "kind": "delta",
      "seq": 5
    },
    {
      "delta": {
        "kind": "event",
        "text": "second"
      },
      "from": 6,
      "kind": "delta",
      "seq": 6
    },
    {
      "kind": "link",
      "link": {
        "attempt": 1,
        "reason": "The connection was closed",
        "retryInMs": 1000,
        "state": "reconnecting"
      }
    },
    {
      "kind": "link",
      "link": {
        "reason": "The agent is not installed",
        "state": "down"
      }
    },
    {
      "kind": "link",
      "link": {
        "state": "stopped"
      }
    }
  ]
};
