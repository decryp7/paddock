# notify-channels

Self-contained notification channels (webhook, ntfy, email) shared by the suite's apps. Copy the crate directory into an
app (Spotter, Dyno, Debrief); it depends on no other suite crate.

- Config: embed `Settings` in the app's own settings file (camelCase JSON). Off by default (no channels). Call
  `Settings::validate()` on save and show the message.
- Secrets never live in the config: a channel only names a `secretKey`. The app implements `SecretStore` over the OS
  keychain (`MemorySecrets` is for tests). Webhook/ntfy send it as `Authorization: Bearer`; email uses it as the SMTP
  password.
- Sending: `Dispatcher::new(settings).dispatch(&message, &secrets)` from a background thread. It sends only to enabled
  channels subscribed to `message.kind` with `severity >= minSeverity`, 15 s timeout per channel (`with_timeout`), no
  redirects followed, and returns one `Result` per channel. It never panics.
- Webhook: POST JSON of `Message`. ntfy: POST text with `Title`, `Priority` (3/4/5) and `Tags`. Email: `lettre` over
  rustls (STARTTLS or implicit TLS), subject = title, body = body.
- Limits: 8 channels, 10 recipients, bounded field lengths; topic charset `A-Za-z0-9_-`.
