# Amendment to ADR 0087 (2026-11-11): pointer to the line-ends control change

- Status: Accepted 2026-11-11 (user), with the amendment it points to. ADR 0087 (accepted 2026-11-10) is on
  `origin/docs/adr-windows-gate-ratchets` (`db7782e01`) only. This file only points; the text and the decision are in
  [0021-lf-checkouts-2026-11-11.md](0021-lf-checkouts-2026-11-11.md) item 4, and its register row is
  `docs/adr/register/R-2026-11-11-0021-lf-checkouts.toml` (not made; see [2026-11-11-windows-liveness-cut.md](2026-11-11-windows-liveness-cut.md)). It holds no work packages of its own.
- `tools/wintest-canary`'s `crlf-canary.cmd`, a declared CRLF type, must arrive with `\r\n`;
- an ordinary text fixture must arrive with `\n`.

Together they show the repository's attributes were applied on the guest. ADR 0087 WP-3, which builds the canary, uses
this form. §1.3's note on `include_str!` baking the host's LF bytes no longer describes a difference from the guest's
checkout.
