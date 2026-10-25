# 0015 Benchmarks on click, network observation and IP lookup through the browser

Status: accepted (2026-10-23). Related: 0001, 0006, 0009.

## Context

Three features read or stress the machine in ways the sampling loop does not: benchmarks (heavy, short, must never run
by themselves), the table of open sockets (cheap, but it names programs and remote hosts) and "who is this address?"
(which tempts a tool to call a web service). Dyno is observe-only by default and its core stays pure.

## Decision

- **Benchmarks run out of process, only on a click.** `dyno-bench` is a small executable next to the agent; the agent
  starts one per benchmark kind with a timeout (`dyno_platform::process`), reads one JSON status from its stdout and
  records the result. The disk test writes one capped file (16 to 512 MiB) into a scratch folder the agent created and
  removes, whatever the child did. A run stores the machine's load when it was started, whether it was on battery and
  the machine description, so two runs can be compared honestly (`dyno_core::bench::compare` warns when the conditions
  differed). Before/after a change is two runs sharing a group. History is a versioned, bounded JSON file that tolerates
  damage. GPU compute is reported unavailable until a graphics API is chosen (no new dependency for it now).
  *Rejected:* running kernels inside the agent (a hang or an allocation spike would hit sampling); a plugin (the plugin
  tier is for third-party or privileged code; this is first-party and trivial to spawn).
- **Sockets are read through a port.** `dyno_core::ports::NetworkSockets`, implemented with `netstat2`
  (`GetExtendedTcpTable`, libproc, `/proc/net`; MIT/Apache-2.0). The view, filters, exposure classification, owner
  naming and the tracker that turns new listeners and remote hosts into activity entries are pure functions in
  `dyno_core::network`. Programs of other users are "owner unknown" with the reason; Dyno never asks for elevation for
  the page. Nothing closes a connection or changes a firewall in v1.
- **"Look up IP" is a browser hand-off.** The user's https template (`{ip}`) is validated in Rust
  (`dyno_core::ip_lookup`), the canonical address is substituted (IPv6 compressed, zone stripped, encoded) and the shell
  opens the result with the opener plugin's Rust API after checking the scheme once more. The webview is granted no
  opener permission. Private, loopback, link-local, CGNAT, multicast, reserved and documentation addresses are refused
  with "Private address, nothing to look up." Dyno never fetches the page and puts nothing else in the URL. No country
  database is bundled.

## Consequences

- A benchmark cannot take down the agent, and its numbers can be trusted only as far as the recorded load says.
- Reverse DNS, per-process traffic rates, a process-details tab and rules on new listeners are not built yet
  (`docs/roadmap.md`).
- The preset list of lookup sites exists twice (engine constants and the settings dropdown); the engine validates every
  template, so a stale dropdown can never open something unsafe.
