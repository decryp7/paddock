# 0006. Avatars, e-mail delivery and connection-time address pinning

Status: accepted (2026-10-23)

## Context
Watches showed a monogram only, e-mail channels were accepted but never delivered, and the guard that keeps watches
away from this computer checked a name once and let the HTTP client resolve it again (DNS rebinding).

## Decision
- **Avatars** (`spotter_core::avatar`, `spotter_engine::avatar`). A watch's picture is the owner's avatar on a forge
  (`github.com/<owner>.png`, `<gitea>/<owner>.png`) or the site's own `/favicon.ico`, fetched through the polite fetcher
  from the watched site only: no favicon service, no search, nothing about the user's watches goes to a third party.
  Sources without a site (folders, registries, containers) and every failure fall back to a **bundled mark** per source
  kind (original geometric glyphs in the accent colour, not brand logos), and the UI draws its monogram when there is
  neither. Downloaded images are untrusted: at most 128 KiB, PNG/JPEG/GIF/ICO read by their headers (at most 1024 px,
  never decoded in Rust) and SVG only through an element/attribute allow-list (no script, `foreignObject`, event
  handlers, entities or references outside the file). The UI shows them as `data:` images, which the CSP already allows.
  The cache is on disk (`cache/avatars`, atomic writes, re-checked on read, at most 500 entries): 7 days for a picture,
  6 hours for "this site has none", and an hour of grace after a failed refresh, during which the old picture stays.
  Offline checks are never remembered as "no picture".
- **E-mail** (`spotter_engine::mail`, lettre with rustls and the platform certificate verifier). The server (host, port,
  STARTTLS / TLS / none, account, sender) is a setting; the password is in the OS credential store (`app` /
  `smtp-password`), never in the configuration, the database or the logs. STARTTLS is required, not opportunistic: a
  server that does not offer it is refused rather than used in plain text. Every step has a timeout and every failure
  is a typed `MailError` whose message says what to check. "None" exists for trusted relays only and the UI says so.
  The engine delivers the channel itself (like webhooks); a test message command checks the settings.
- **Address pinning.** The HTTP client for watches that are not local resolves names through `GuardedResolver`, which
  applies the guard to the very addresses it connects to, and the pre-request check uses the same resolver. A watch
  marked local uses a second client with the system resolver. Behind a system proxy the proxy resolves the name, so only the
  pre-request check applies there.
- **Upgrade command.** `ReleaseView.upgrade_command` is derived from source data in the core (`docker pull` for
  container tags, `cargo`/`npm`/`pip`/`dotnet` for registry versions) after a strict character check; Spotter shows it
  for copying and never runs it.

## Consequences
New dependency `lettre` (MIT) and `base64`. Remote avatars add up to one request per site per week. The wire contract
gains `Avatar`, `MailStatus`, `SmtpSettings`, `Release.upgradeCommand` and the commands `watch_avatar`, `preset_avatar`,
`kind_avatar`, `mail_status`, `set_mail_password`, `send_test_email`.
