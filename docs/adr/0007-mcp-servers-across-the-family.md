# 7. MCP servers across the family

- Status: accepted (user decision, 2026-10-23); implementation order below
- Related: ADR 0001 (`McpTool` contribution, `McpExpose` permission), ADR 0004 (`paddock-pitwall`, pairing), ADR 0006 (rollout);
  Pacenotes proposal section 13 (MCP surface); Pitwall's MCP client and tool router

## Context

Pacenotes already serves read-only code tools over MCP, Sighter drives a desktop through MCP, and Pitwall is the MCP client that
offers tools to local models. The other apps hold data a model could usefully ask about (logs, releases, runner jobs, machine
health), but each would invent its own server, limits and logging. One shared design keeps them consistent and keeps the safety
rules in one place.

## Decision

### Who serves what

| App | Serves MCP | Tools (all read-only unless noted) |
|---|---|---|
| Pacenotes | yes | code tools: symbols, references, callers/callees, search, dependency and requirement lookups |
| Sighter | yes, **acting** | desktop control tools with its own safety rules (below); the one exception to read-only |
| Debrief | yes | search events, errors around a time, anomalies and outliers, known issues, incident summary, exception groups, entity lookup |
| Spotter | yes | latest version of X, what changed since a version, watch status |
| Pitcrew | yes | runner status, job log, why a job is stuck, cleanup status |
| Dyno | yes | health, top processes, why slow, issues |
| Pitwall | **none** | it is the model server and the MCP client/router, not a data source |

### Who hosts (acts as MCP client)

Any app that has an agent loop and a model can be an MCP host. **Pitwall is the default host**: it offers the connected servers' tools to
local models and routes the calls. An app with its own agent loop (for example Debrief's assistant) may host directly, through
`paddock-pitwall` for the model and `paddock-mcp` for the client side.

### One shared crate: `paddock-mcp`

Protocol (server and client), tool schema types and validation, result limits, paging, the audit log, and the transport helpers
(stdio, loopback with a per-client key). Apps register tools; they do not implement the protocol, paging or logging themselves. It sits
beside `paddock-plugin-api`, whose `McpTool` contribution (ADR 0001) delivers a plugin's tools through the same registry, guarded and
quarantined like any plugin call.

### Rules for every server

1. **Local only.** Transport is stdio (the host starts the app's `--mcp` mode) or loopback with a **per-client key** issued like Pitwall pairing
   (ADR 0004): shown once, only its SHA-256 kept in an owner-only file in the app's config folder (amended 2026-11-05: not the OS credential store, whose prompt after an update of an ad-hoc signed build blocked Dyno before its first window; a hash is not a secret), revocable in the app. No remote listener, no tunnel.
2. **Off by default, per app.** The user turns it on in the app's settings; the setting says which tools are exposed and to whom.
3. **Every call is logged and visible in the app**: time, client, tool, arguments (redacted), result size, duration, outcome. The log
   holds no file contents beyond what the arguments and a size say, and the user can clear it.
4. **Bounded and paged.** Each tool has a result cap, a time limit and a cursor for more; huge results are never returned in one reply.
5. **Read-only by default.** Changes happen only through the app's own UI or CLI, never over MCP. `McpExpose` (ADR 0001) allows plugins
   to contribute read-only tools only.
6. **Sighter is the exception** because acting is its purpose: an **allow-list** of applications and actions, a **confirmation** for
   anything outside it, **budgets** (actions, time, spend per session), a global **stop key** that ends control immediately, and a
   **replay log** of every action for review. These rules live in Sighter and are not relaxed by the shared crate.
7. **Confidential data stays local.** Tool results go only to the local client that asked; nothing is sent to a remote model or service
   unless the user has pointed the host at one, and Pitwall's local-only mode refuses that.
8. **Tool output is untrusted data to the host** (prompt injection): hosts treat results as data, and servers redact secrets.

### Rollout

1. `paddock-mcp` is extracted when Debrief's server is built (rule of three is met across Pacenotes, Sighter and Debrief).
2. **Debrief's MCP server first, after Debrief 1.0.2.**
3. Then Spotter, Pitcrew and Dyno, each in its **next minor version** after that.
4. Pacenotes and Sighter move to `paddock-mcp` when it exists; their behaviour does not change.

### Relation to ADR 0001

ADR 0001's `McpTool` contribution and `McpExpose` permission describe how a plugin adds tools to an app that serves MCP. This ADR decides
which apps serve MCP and the rules they share; `paddock-mcp` is where `McpTool` calls are dispatched, limited and logged.

## Consequences

- One audit log shape and one pairing story for every server; a user can see all MCP activity per app.
- Each app owns what it exposes; the shared crate owns how.
- Pitwall stays a router, so no data app depends on a model being installed.

## Open questions

1. Stdio only for the first server (Debrief), with loopback added when Pitwall hosts remote-process servers? Proposed: yes.
2. Do the audit logs get a shared viewer in `@decryp7/paddock-ui`? Proposed: yes, after the second server.

## Amendments

- **2026-11-05, Dyno's read tools (user decision).** Dyno 1.1.0 serves eight read-only reading tools (system summary, CPU and
  memory, disks, top processes, network summary, alarms, history of one metric, sensors) beside its power tools, so that
  Pitwall's MCP client and any local MCP client can read what its pages show. Rules 1 to 8 apply unchanged: off by default, one
  switch per tool on Settings > AI access, every call audited and logged without its output, bounded rows and bytes. For any
  server whose answer goes to an AI client: process rows hold a name, a process id and numbers only (no command line, user name or
  path), and host names, network addresses and free-text details that can carry paths are left out unless a tool says otherwise.
  Details: Dyno ADR 0032 amendments.
