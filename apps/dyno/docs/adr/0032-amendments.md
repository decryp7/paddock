# ADR 0032 amendments

## 2026-11-05: D7 grows eight read-only reading tools for 1.1.0 (user decision, an exception to the frozen 1.1.0 scope)

D7 named three read tools (`power_state`, `wol_devices`, `energy_report`). Dyno 1.1.0 also serves eight read-only reading tools, so
that Pitwall's coming MCP client and any other local MCP client can ask what the pages show: `system_summary`, `cpu_memory`,
`disks`, `top_processes`, `network_summary`, `alarms`, `history` and `sensors` (`app/src-tauri/src/mcp_readings.rs`). They follow the
family ADR 0007 as amended on the same day; the rules that matter here:

- **Same path as the pages.** A tool asks the agent with the pages' own requests (`AgentCalls`, so the agent's validation and logs
  apply) or reads the newest live update the window and the tray draw. No domain rule is computed in the tool; it shapes values
  (units in the names, rounding to what the pages show, buckets for history).
- **Reasons, never an empty success.** A reading that is not available returns its reason as the page words it ("Turn on
  Administrator readings in Settings so the helper can read this", "Not on this operating system"); a tool with nothing at all to
  say fails with those reasons. Agent down or no live update yet: the agent's own message.
- **Privacy (rule 56, the output goes to an AI client).** Programs: name, main process id, process count and numbers; never a
  command line, user name or path. No host name, no network address or port, no alarm detail or key (they carry paths).
- **Bounds.** At most 50 rows per list, 120 history points, 64 sensors or volumes, 256 cores, 120 characters per text, 32 KiB per
  answer, 10 s per call; arguments are validated against a closed schema (`additionalProperties: false`).
- **Off by default (the master switch); then per tool, opt-out.** The tools use `Policy`: nothing is served until the user turns AI
  access on; then every read tool is on until the user switches it off on Settings > AI access (a later change may add a note of the
  tools a client has seen when consent is given), and every call is in the audit log and the app log (tool, client, duration, outcome, size; never
  the output).
- **Actions unchanged.** `wake_device` and `keep_awake` stay the only actions, still hidden and refused until allowed by name; sleep,
  shut down, kill and timed or scheduled actions are still never tools.
- `energy_report` is not built; the Power page still has it.

## 2026-11-05: the listener (ADR 0007 rule 1), found missing by a design review

D7 gave Dyno tools, keys and a settings page but nothing served them: `serve_loopback` and `serve_stdio` had no production caller.
Decided and built for 1.1.0:

- **The app serves.** The tools ask the agent through the shell's link, which only the app process holds, so `paddock_mcp::listener::Listener`
  runs in the app (a window is not needed: the app stays in the tray, the listener lives with the process). When the app is quit the
  listener is gone and the agent keeps running; a client then gets "Dyno isn't open" (a refused connection, or `dyno mcp` saying so). The
  settings page cannot show that state because the page is closed with the app.
- **Loopback, ephemeral port, key handshake.** It binds 127.0.0.1 only, on a port the system picks (no allotted port exists for the family,
  and a fixed one could clash with a second instance or another program), serves while AI access is on and at least one unrevoked key exists,
  stops at once when either ends, at most 4 clients. The protocol is the family's line protocol after a `{"key": ...}` line; it is not
  HTTP, so there is no Origin or Host to check: a browser cannot complete the handshake.
- **Discovery file (rule 53).** `mcp-listener.json` in the config folder holds pid, process start time, port, app and version, never a key.
  It is a hint: `dyno mcp` and any client must check that the pid still has that start time before sending a key, and the listener removes
  the file only if it is still its own (same pid and start). A stale file from a crash is therefore harmless.
- **Stdio: `dyno mcp`, not `dyno --mcp`.** The family's stdio model has the host start a process that serves itself. Dyno's tools need the
  running app, so the host-started process is a bridge: the console program `dyno` (the CLI, a console subsystem program on Windows, where
  the GUI app has no stdio) reads the key from `DYNO_MCP_KEY` (not an argument, which shows in process lists), checks the discovery file and
  carries lines to the app's listener. Same keys, same policy, same audit; it needs the app open. ADR 0007's "the app's `--mcp` mode" is read
  as this.
- **Readings are current or refused.** `newest_live` refuses when the agent link is not connected and when the newest live update is older
  than 30 s (three times the slowest no-window pace); tool calls wait at most 8 s for the agent (under the 10 s call limit); an answer over
  its byte bound is shortened to valid JSON that says it was cut; a mount point is only `/` or a drive letter and the user-name segment of
  paths in labels is replaced; alarm titles carry no port.
- **Logging (ER-27).** Start, stop and failure of the listener are `paddock_mcp` log lines with the port, the discovery file and the duration.
