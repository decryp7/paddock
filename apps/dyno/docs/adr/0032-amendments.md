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
- **Off by default, per tool.** The tools use `Policy`: nothing is served until the user turns AI access on, each tool has its own
  switch on Settings > AI access, and every call is in the audit log and the app log (tool, client, duration, outcome, size; never
  the output).
- **Actions unchanged.** `wake_device` and `keep_awake` stay the only actions, still hidden and refused until allowed by name; sleep,
  shut down, kill and timed or scheduled actions are still never tools.
- `energy_report` is not built; the Power page still has it.
