# 17. Notification channels

- Status: accepted (2026-10-23)
- Related: ADR 0005 (credential storage); code: `crates/notify-channels` (self-contained), `app/src-tauri/src/channels.rs`,
  Settings > Notification channels

## Context

OS notifications miss a headless Mac or Linux box and a team. Webhook, ntfy and email reach people elsewhere. Spotter,
Dyno and Debrief need the same.

## Decision

**A small shareable module.** `crates/notify-channels` depends on nothing of Pitcrew. It has a `Message`, a `Channel`
trait, a `SecretStore` trait and a `Dispatcher`; the transports are a webhook (JSON POST), ntfy (title, priority and
tags as headers) and email (SMTP through `lettre` with rustls, STARTTLS or implicit TLS only). Another app copies the
folder and implements `SecretStore` over its keychain.

**Off by default, per-event choice.** The settings (`notification-channels.json` in the app's data folder, versioned,
a damaged or newer file gives no channels) hold channels, each disabled until the owner enables it, with the list of
notification kinds it wants and a minimum severity. A message goes only to enabled channels that list its kind and
severity. There is a "Send test message" button.

**Secrets only in the OS keychain.** The settings name a keychain entry (`secretKey`); the secret itself is write-only in
the UI and is read by the sender at the moment of sending. Debug output and the settings file never contain it.

**The app sends.** The app holds the keychain access (the service never sees a secret, like the Gitea read token) and
dispatches from a background thread when the service pushes a notification. Consequence: a machine where the Pitcrew
app is not running (no tray) gets no channel messages, only what the service logs. HTTP calls do not follow redirects, have
a timeout and a panic in a channel is contained.

## Consequences

- Failures are logged by channel id and reason, never with the message content.
- A later "service sends" mode would need the service to read the keychain (Windows service account!) or a secret
  hand-over; deliberately not done now.
