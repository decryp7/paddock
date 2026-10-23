# Dyno design

The design diagrams, one page. The sources are generated from [`docs/diagrams/src`](diagrams/src) into draw.io files in
[`../diagrams/`](../diagrams) and exported to SVG there; they follow [`diagrams/STYLE.md`](../diagrams/STYLE.md). Dashed boxes and ellipses are
planned (not built). The text companion is [architecture.md](architecture.md).

## Overview

Dyno is a per-user **agent** that measures the machine and keeps its history, an **app** (Tauri shell plus Angular UI)
that shows it, a pure Rust **core** that holds every decision, OS **adapters** behind the core's ports, and (planned) a
privileged **helper** installed on the first admin action. The user changes nothing by accident: rules ship disabled,
and every action is logged with an undo (ADR 0006).

![Dyno at a glance](../diagrams/dyno-overview.svg)

## C4 model

### Level 1 — system context

![System context](../diagrams/svg/c4-1-context.svg)

The PC owner uses Dyno; Dyno reads the operating system through ports and checks Dyno's own release host for opt-in
updates. Debrief (open a time window), Pitwall (explain on click) and backup tools (health checks) are planned
integrations and never get data unless the user clicks.

### Level 2 — containers

![Containers](../diagrams/svg/c4-2-containers.svg)

The app (Angular UI in a Tauri shell) and the agent run as the user; every arrow between Dyno processes is the one
`dyno-ipc` crate. The agent is the single writer of the data folder. Plugins run out of process; the helper is a separate system service installed on a click (ADR 0016).

### Level 3 — core components

![dyno-core components](../diagrams/svg/c4-3-components-core.svg)

Pure modules over values; time is a parameter. `dyno-platform` implements the ports.

### Level 3 — agent components

![dyno-agent components](../diagrams/svg/c4-3-components-agent.svg)

A sampling thread feeding history, alarms and the flight recorder, an ipc server with a thread per connection, request
handlers and the SQLite store with its rollups.

### Level 3 — agent support modules

![dyno-agent support modules](../diagrams/svg/c4-3-components-agent-support.svg)

The CLI, the service registration, the shared state, the plugin host and the benchmark runner that starts `dyno-bench`.

## Deployment

![Deployment on macOS](../diagrams/svg/uml-deployment-macos.svg)

![Deployment on Windows](../diagrams/svg/uml-deployment-windows.svg)

![Deployment on Linux](../diagrams/svg/uml-deployment-linux.svg)

Per OS: the app bundle, the OS's way of starting the agent at login (launchd through `SMAppService`, a systemd user
unit, a per-user `Run` entry on Windows), the agent process, the data folder, and the helper, a system service that exists only after the user installed it.

## Use cases

![Use cases: see and understand](../diagrams/svg/uml-use-cases-observe.svg)

![Use cases: change and tune](../diagrams/svg/uml-use-cases-act.svg)

Everything that changes the machine needs an enabled rule or an explicit click. Dashed use cases are planned.

## Domain model

![Domain model: measurements](../diagrams/svg/uml-class-domain.svg)

![Domain model: rules and actions](../diagrams/svg/uml-class-rules.svg)

![Domain model: diagnosis and changes](../diagrams/svg/uml-class-insight.svg)

The core's main types. `Reading<T>` makes a missing sensor explicit; `ProcessRef` (pid plus start time) is the only way to
name a process in an action; `ActivityEntry` carries an `UndoRecord`.

## Flows

### Live update push

![Live update push](../diagrams/svg/uml-seq-live-update.svg)

The app subscribes once; the sampler measures at adaptive intervals; the agent pushes each `LiveUpdate` to every
subscriber at its detail level, dropping frames for a slow subscriber.

### Act on a process, then undo

![Act and undo](../diagrams/svg/uml-seq-act-undo.svg)

Validate, check the protected list, apply to a `ProcessRef`, record the inverse; `Undo` applies it.

### First admin action installs the helper

![Helper install](../diagrams/svg/uml-seq-helper-install.svg)

Review, the helper's explanation, the OS's own prompt (the install runs `dyno-helper install` elevated), then the change: a second validation inside the helper, the setting read back, the helper's audit line and the activity entry with its inverse (ADR 0016). The picture predates the one-step install and still shows the same order.

### Agent restart after an update

![Agent restart](../diagrams/svg/uml-seq-agent-restart.svg)

A newer app asks the older agent to exit; the service manager starts the new one; `release_all` first undoes what rules
applied. On Windows no manager restarts the agent; the app does.

### History rollup

![History rollup](../diagrams/svg/uml-seq-history-rollup.svg)

Raw points every 5 s; closed buckets roll up to minute, 15-minute and hour rows with min, average and max; a range
query uses the finest tier that covers it.

## States

### Window lifecycle

![Window lifecycle](../diagrams/svg/uml-state-window.svg)

Closing the window destroys it; the tray and the agent stay; opening builds a fresh window; with no tray, closing quits.

### Service registration

![Service registration](../diagrams/svg/uml-state-service.svg)

Not installed, or installed with two flags: running and pointing at this executable. `ensure` repairs a stale
registration and never installs one unasked.

### Rule engine per (rule, app)

![Rule engine](../diagrams/svg/uml-state-rule.svg)

Watching, Acting at step n, Releasing, Snoozed; steps add up and are undone in reverse order.
