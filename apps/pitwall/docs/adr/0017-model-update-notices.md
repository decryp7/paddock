# 17. Model update notices: ask, never fetch

Status: accepted (number provisional; it is allocated again when this branch merges, see `docs/suite-lessons.md`). The user approved
the feature in the roadmap; this ADR records the one new outbound request it adds, as ADR 0001 requires.

## Context

Models on Hugging Face change: a file is re-uploaded (a fixed chat template, a corrected quantisation), a better quantisation or an
importance-matrix build appears. People who downloaded a model have no way to learn that except by looking. Pitwall must not
phone home, and it must not fetch anything the user did not ask for (ADR 0001). The feature therefore has to be a *notice*: ask,
tell, and stop.

## Decision

- **One kind of outbound request, nothing else.** A check sends `GET /api/models/<repo>/tree/main?recursive=true` and
  `GET /api/models/<repo>` to `huggingface.co`, for repository ids the user already downloaded from: the same two requests the
  Discover tab makes when the user opens a repository. No body, no cookies, no identifier, no usage, no file names beyond the
  repository id in the address. The URLs are built only by `pitwall_core::hf` (`tree_url`, `info_url`, so an allow-listed host and a
  validated `RepoId`), the request is sent only by `pitwall_server::egress` (the one HTTP client), and `updates.rs` mentions
  neither `reqwest` nor a URL (a test reads the source).
- **Off until the user opts in.** `models.updateCheck` is `{ enabled: false, schedule: weekly }`. Nothing is asked at start-up or
  before the user turns the schedule on; turning it on runs the first check right away. "Check now" works whatever the setting
  (it is the user's click), and is rate limited to one a minute. The scheduled loop first looks a minute after start and then every
  five minutes; a check itself runs at most weekly or daily, and an attempt that failed or was refused is retried after an hour.
- **Off means zero requests.** Until the user turns the schedule on, nothing is asked. (1.0 also had an Offline mode that stopped
  checks; it was removed in 1.1.)
- **Bounded.** At most 40 repositories per check (a daily rotating window, so every repository gets its turn), 30 s per repository,
  a pause of 0.5 s between repositories, three failures in a row or one HTTP 429 end the check, answers are capped at 8 MB. A
  failing repository never loses the notices of the others; a check where nothing could be reached keeps the old notices and says
  so.
- **Where the truth lives.** The rules are pure (`pitwall_core::updates`): they take the installed models, the repositories'
  listings, the machine's `Room` and the clock. The schedule, the notices and the dismissals are pure too
  (`pitwall_core::update_state`). The server only fetches, scans and saves. The UI shows what the engine returned.
- **Provenance is recorded at download time.** When a download finishes, a sidecar `<file>.origin.json` records the repository, the
  file, the size, the SHA-256 of every part (the digest that was verified) and the repository's commit. It is versioned,
  size-bounded, and used only when it names the same repository and file and the size on disk still matches. Models without one
  (downloaded before this version, or copied in under `owner/repo/`) are *inferred* from their folder and compared by total size;
  models outside an `owner/repo` folder have no known origin and are never looked up. No migration is needed (nothing is released
  yet). A sidecar that is damaged or from a newer Pitwall reads as absent.
- **Rules**, documented in the module: (1) *newer revision of the same file*: the repository lists the file and its hash (or, for old
  entries, its size) differs; a new commit elsewhere in the repository is not a notice. (2) *better quantisation*: the same base model
  in the next higher-quality quantisation (ordered by `quality_rank`, up to `Q8_0`; 16 and 32-bit weights are never suggested) that
  fits this machine with room to spare at the default context, offered once for the best copy you already have. (3) *imatrix
  build* of the same quantisation, only when (2) found nothing.
- **Dismissals are per offer.** A notice has a stable id (`<model>#<kind>`) and a fingerprint (kind, file, size, hash). "Dismiss" hides
  that fingerprint until the repository offers something different; "Remind me in a week" snoozes it (1 to 90 days). State file
  `model-updates.json`: versioned, atomic with a backup, 1 MB cap, at most 200 notices and 500 dismissals, damage falls back to the
  backup and then to empty with an event.
- **Never automatic.** A notice only offers. "Get it" asks for confirmation with the size and the host, and then calls the same
  `download_model` as Discover; nothing in the checker can start a download (a test asserts the download list stays empty). A newer
  revision replaces the old file only after the new one is downloaded and verified, as any download does.
- **Management API** (all async in the Tauri shell): `update_check_status`, `update_notices`, `check_updates_now`,
  `set_update_check { enabled, schedule }`, `dismiss_update { id, snoozeDays }`. A finished check raises `modelUpdate` events (at
  most three, then one summary); a finished download of the offered file clears its notice.

## Consequences

- Hugging Face learns, from a user who turned this on, that their IP asked about repositories they downloaded: the same
  information the downloads themselves reveal. The settings page says so in words.
- The check cannot see updates to models that were not downloaded through Pitwall's library layout; that is the price of sending
  nothing but repository ids Pitwall itself fetched.
- The tests use a fake `RepoSource` (the port the checker asks through); production uses `HfSource` over egress. The store engine,
  the listing source and the clock are all replaceable without touching the rules.
