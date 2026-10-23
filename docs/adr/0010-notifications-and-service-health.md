# 10. Notifications and service health

- Status: Proposed
- Date: 2026-10-24
- Related: rule 19 (a control that repairs X never depends on X), rule 39, rule 40, ADR 0008 (jobs report through events)
- First adopter: Debrief ADR 0055 (replaces its `StoreHealth`). Read it for the model, UI contract and migration.

## Context

In Debrief one store-worker failure produced three toasts and a red box in every panel; a one-off `StoreHealth` chip
fixed that single service by string-matching the error text. Job toasts, error toasts and banners were separate
channels. Every app has services that can stop (a local model, a background service, a worker, a plugin) and the same
failure shape will repeat.

Survey (read-only, 2026-10-24):

| App | Notifications and errors today | Service status today | Exposure |
|---|---|---|---|
| Pitcrew | a `ToastStore` + `pc-toasts` (`layout/toasts.ts`), `connection-banner.ts` | connection to its service shown as a banner | Medium: toasts and banner are separate; engine/service outages can double up |
| Pitwall | `core/toasts.ts` + `toast-host.ts`; many pages show their own errors | the local LLM service (`service-consent`, models and dashboard pages show state themselves) | High: the LLM service and model loads are the "store" of this app; per-page state |
| Spotter | `layout/toasts.ts`, `connection-banner.ts` | connection banner | Medium |
| Dyno | `shared/notifier.ts`, alarm notifications (domain) | collectors and plugins fail silently or per page (`health` appears in 80+ files) | Medium to high: many sources, so many opportunities for repeated reports |
| Sighter | none yet | will run a local server, trainers and a vision model | Adopt from day one |
| Pacenotes | none yet | will run an index, an LSP set and a vector DB | Adopt from day one |

## Decision

1. **Notifications**: one service. Every notification has a mandatory cause key; posting the same cause updates the
   existing one in place (count, time, details) and a cause that clears is resolved. Severities
   `info | success | warning | error | critical`. One display rule: one notification per failure, details collapsed,
   updated in place; persistent errors stay until resolved. Job outcomes, errors and offers are kinds of notification,
   not channels. Dedupe happens once, in Rust, and all windows receive the same event.
2. **Service health**: a general model in the engine, not in the UI. Each service (store, AI/LLM worker, plugin, browser
   runtime, downloads, collectors, a local server) is an entry with a closed state
   (`ok | starting | degraded | down | quarantined | repairing`), a plain cause with a next step and technical detail,
   and a repair action. The supervisor updates the registry; the registry emits one event to every window; new
   windows ask for a snapshot. Nothing derives health from error text or from which commands happened to succeed.
3. **Repair never depends on the broken thing** (rule 19): `health_repair` is served by the shell, outside the service.
4. **Shown once**: a status-bar chip with the message, Details and Repair / Try again; one notification on the
   transition. Panels that need a service show a calm "Waiting for X" state and load by themselves on recovery;
   a rejected command caused by an outage is cancelled quietly, not reported again.
5. **Shared parts** (rule 39): Rust crate `paddock-health` (state enum, registry, snapshot/repair shapes, event
   payload, cause-key notification core; no Tauri dependency, an event-sink adapter like `paddock-jobs`) and
   `@decryp7/paddock-ui` (`NotificationHost`, `ServiceHealthStore`, status-bar chip, `WaitingFor`, wire types). Each app
   keeps its list of services, their supervisors and repair actions, and its notification kinds.

## Consequences

- One failure is one chip and one notification in every window, in every app, with engine-written wording.
- Each app replaces its toast store and connection banner with the shared pieces; domain notifications (alarms,
  update offers) become notification kinds with actions.
- The wire contract gains a typed `cause` on command errors in each app (UI types and Rust DTO in the same change).

## Migration plan

1. Debrief adopts (ADR 0055; the cause key and the store service before 1.1).
2. Extract the Rust model and UI parts after Debrief 1.1.
3. Pitwall (LLM service) and Dyno (collectors) next, then Pitcrew and Spotter (replace the connection banner with a
   `service` health entry); Sighter and Pacenotes register their services as they are written.

## Alternatives considered

- **Generalise Debrief's `StoreHealth` in TypeScript.** It infers health from error strings per window; the engine
  already knows the typed cause.
- **Dedupe only at display time.** Fixes toasts, not panels, other windows or other channels.
- **A banner per service.** Reproduces the original bug as services are added.
