# Pitcrew design

All of Pitcrew's design diagrams on one page, from the big picture down to individual flows. Each image is exported
from its editable draw.io source (linked under the image) with
Paddock's [`diagrams/scripts/export-diagrams.sh`](https://dev.decryptology.net/decryp7/paddock/src/branch/main/diagrams/scripts/export-diagrams.sh) (see [development.md](development.md)); edit the `.drawio`, then re-run the script. The SVGs are
re-exported from the sources (draw.io desktop); until then they may lag behind the `.drawio` files. The images follow your
light or dark setting. The style rules are in [diagrams/STYLE.md](diagrams/STYLE.md), the reasoning behind the design in the [architecture decision
records](adr/0001-architecture.md), and the prose overview in [architecture.md](architecture.md).

Contents: [Overview](#overview) · [C4 model](#c4-model) · [Deployment](#deployment) · [Use cases](#use-cases) ·
[Domain model](#domain-model) · [Flows](#flows) · [States](#states)

## Overview

The outside world, the service that keeps runners going, the app that shows and controls it, and the pure core that
holds every rule.

![Pitcrew at a glance](diagrams/pitcrew-overview.svg)

Source: [pitcrew-overview.svg](diagrams/pitcrew-overview.svg) (hand-written SVG that follows the light or dark setting;
the other diagrams are draw.io).

## C4 model

### Level 1 — system context

Who uses Pitcrew and which outside systems it talks to. Pitcrew keeps CI runners for Gitea and Forgejo going on a build
machine; the network is used only to reach the server, to fetch runner releases and, when the user opts in, Pitcrew's own
updates.

![C4 system context](diagrams/svg/c4-1-context.svg)

Source: [c4-1-context.drawio](diagrams/c4-1-context.drawio)

### Level 2 — containers

The runnable units: the Tauri app (Angular UI, Rust shell, tray) in the user's session, the Pitcrew service that owns
the runners, the runner processes it starts, and the files it keeps. The app can be closed or crash without touching the
runners. The service is a Windows service, a macOS launch agent or a Linux systemd user service. See ADR
[0001](adr/0001-architecture.md), [0003](adr/0003-service-model.md) and [0004](adr/0004-ipc-protocol-and-acl.md).

![C4 containers](diagrams/svg/c4-2-containers.svg)

Source: [c4-2-containers.drawio](diagrams/c4-2-containers.drawio)

### Level 3 — core components

Inside `pitcrew-core` (including the runner environment, self-restart decision and service paths): supervision of one runner (log classifier, job tracker, session rules, watchdog, restart policy,
controller, health), updates (release locations, checksums, versions, policy, update machine), housekeeping and metrics,
records and configuration, and the wire types the app shares. Everything here is pure: time is passed in, and decisions
go out as values (ADR [0002](adr/0002-runner-auto-update.md), [0006](adr/0006-supervision-and-housekeeping.md),
[0007](adr/0007-runner-scoped-metrics.md)).

![C4 core: supervising a runner](diagrams/svg/c4-3-core-supervision.svg)

Source: [c4-3-core-supervision.drawio](diagrams/c4-3-core-supervision.drawio)

Updates:

![C4 core: updates](diagrams/svg/c4-3-core-updates.svg)

Source: [c4-3-core-updates.drawio](diagrams/c4-3-core-updates.drawio)

Records, configuration and helpers:

![C4 core: records and helpers](diagrams/svg/c4-3-core-support.svg)

Source: [c4-3-core-support.drawio](diagrams/c4-3-core-support.drawio)

### Level 3 — service components

Inside `pitcrew-service`: the command line and host, the platform adapters, the IPC server, the supervisor at the
centre, and everything slow or OS-specific behind it (launcher and process control, probe, runner log file, store,
worker steps, HTTPS client, cleanup, login environment).

![C4 service: control path](diagrams/svg/c4-3-service-control.svg)

Source: [c4-3-service-control.drawio](diagrams/c4-3-service-control.drawio)

What the supervisor drives:

![C4 service: what the supervisor drives](diagrams/svg/c4-3-service-supervisor.svg)

Source: [c4-3-service-supervisor.drawio](diagrams/c4-3-service-supervisor.drawio)

## Deployment

Where each part runs on a build machine, per OS. Each OS has its own service manager, data folder and IPC endpoint; on Linux the service is a systemd user
service (ADR [0009](adr/0009-linux-service-and-runner-environment.md), [Linux guide](linux.md)).

Windows:

![Deployment on Windows](diagrams/svg/uml-deployment-windows.svg)

Source: [uml-deployment-windows.drawio](diagrams/uml-deployment-windows.drawio)

macOS:

![Deployment on macOS](diagrams/svg/uml-deployment-macos.svg)

Source: [uml-deployment-macos.drawio](diagrams/uml-deployment-macos.drawio)

Linux:

![Deployment on Linux](diagrams/svg/uml-deployment-linux.svg)

Source: [uml-deployment-linux.drawio](diagrams/uml-deployment-linux.drawio)

## Use cases

What the build-machine owner does with Pitcrew, and what Pitcrew does by itself when time passes or something fails.

Running runners:

![Use cases: running runners](diagrams/svg/uml-use-cases-runners.svg)

Source: [uml-use-cases-runners.drawio](diagrams/uml-use-cases-runners.drawio)

Updates and upkeep:

![Use cases: updates and upkeep](diagrams/svg/uml-use-cases-maintenance.svg)

Source: [uml-use-cases-maintenance.drawio](diagrams/uml-use-cases-maintenance.drawio)

On its own:

![Use cases: what Pitcrew does on its own](diagrams/svg/uml-use-cases-automatic.svg)

Source: [uml-use-cases-automatic.drawio](diagrams/uml-use-cases-automatic.drawio)

## Domain model

The core's main types as the service and the app use them: configuration (`Config`, `Defaults`, `RunnerConfig` and the
policies), the runner's live state (`Controller`, `UpdateMachine`, `RunnerSnapshot`), the records kept (`ServiceState`,
`RunnerState`, `History`) and the metrics series (`TieredStore`, `MachineSample`, `Usage`).

Configuration:

![Domain model: configuration](diagrams/svg/uml-class-config.svg)

Source: [uml-class-config.drawio](diagrams/uml-class-config.drawio)

Runtime:

![Domain model: runtime](diagrams/svg/uml-class-runtime.svg)

Source: [uml-class-runtime.drawio](diagrams/uml-class-runtime.drawio)

Records and metrics:

![Domain model: records and metrics](diagrams/svg/uml-class-records.svg)

Source: [uml-class-records.drawio](diagrams/uml-class-records.drawio)

## Flows

### Register or adopt a runner

The wizard adds the runner, then the service installs the runner binary if needed and runs its register command with the
token, or adopts a folder that is registered already. The token is passed on and never stored by Pitcrew, unless the user
ticks "remember" and the app keeps it in the OS credential store (ADR [0005](adr/0005-credential-storage.md)).

![Register or adopt a runner](diagrams/svg/uml-seq-register-runner.svg)

Source: [uml-seq-register-runner.drawio](diagrams/uml-seq-register-runner.drawio)

### Safe update with roll-back

Download, verify, wait for the running job, swap, restart, confirm; roll back automatically if the new runner does not
come up. A running job is never killed (ADR [0002](adr/0002-runner-auto-update.md)).

Part 1, find, download and verify:

![Safe update: find, download and verify](diagrams/svg/uml-seq-update-fetch.svg)

Source: [uml-seq-update-fetch.drawio](diagrams/uml-seq-update-fetch.drawio)

Part 2, swap, restart and confirm:

![Safe update: swap, restart and confirm](diagrams/svg/uml-seq-update-swap.svg)

Source: [uml-seq-update-swap.drawio](diagrams/uml-seq-update-swap.drawio)

### Stall detection and restart

At least six timed-out polls within 15 minutes while no job runs is a stall: the runner is stopped gracefully and
restarted with back-off (ADR [0006](adr/0006-supervision-and-housekeeping.md)).

![Stall detection and restart](diagrams/svg/uml-seq-stall-restart.svg)

Source: [uml-seq-stall-restart.drawio](diagrams/uml-seq-stall-restart.drawio)

### App to service push subscription

One long-lived subscription carries status and notifications as they happen; commands use short connections of their own
(ADR [0004](adr/0004-ipc-protocol-and-acl.md)).

![App to service push subscription](diagrams/svg/uml-seq-subscription.svg)

Source: [uml-seq-subscription.drawio](diagrams/uml-seq-subscription.drawio)

## States

### Runner lifecycle

One runner's life as the `Controller`: WaitingToStart, Starting, Running (with the Draining status while a request waits
for the job), Stopping, Stopped, Paused, Held by an update, WaitingForLogon and CrashLoop.

![Runner lifecycle](diagrams/svg/uml-state-runner.svg)

Source: [uml-state-runner.drawio](diagrams/uml-state-runner.drawio)

### Update machine

An update as the pure `UpdateMachine`: Downloading, Verifying, Draining, Stopping, Swapping, Confirming and RollingBack,
and how each can end (Updated, RolledBack, Postponed, Cancelled, Failed).

![Update machine](diagrams/svg/uml-state-update.svg)

Source: [uml-state-update.drawio](diagrams/uml-state-update.drawio)
