# Amendment to ADR 0051 (2026-11-22): WP-14 (`paddock_proc::login_env`) is due before the Dyno 1.2 tag

- Status: Accepted (user, 2026-11-22), recorded by the ADR audit of 2026-11-22.
- Amends: WP-14, the only work package the trim of 2026-11-11 kept.

`login_env` replaces the three app copies (`pitcrew-service/src/login_env.rs`, Dyno's `toolpath.rs`, Pitwall's `login_path.rs`). It is built
and landed **before the Dyno 1.2 tag**, which is the `delete_by` (`dyno-v1.2.0`) that 0051 already gives Dyno's copy. The date does not move.
The copies are to be listed in `docs/known-copies.toml` with that line when WP-14 starts.
