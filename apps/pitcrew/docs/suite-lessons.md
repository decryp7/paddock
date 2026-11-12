<!-- paddock:block suite-lessons rev=068c4d82 -->
# Suite lessons

The lessons that apply to every Paddock app (how to run tests, release, work with agents and what went wrong before)
live in one place: [paddock/docs/lessons.md](https://dev.decryptology.net/decryp7/paddock/src/branch/main/docs/lessons.md).
The binding rules distilled from them are in [paddock/docs/rules.md](https://dev.decryptology.net/decryp7/paddock/src/branch/main/docs/rules.md)
and in the "Paddock rules" section of `AGENTS.md`. Add a lesson there first; an app keeps only lessons about its own domain.
<!-- /paddock:block suite-lessons -->

# Lessons shared across the suite

- Ad-hoc signed macOS apps get a new cdhash on every build, and `SMAppService`/launchd keep the launch constraint (LWCR) of the build that registered the agent: after an update launchd loops on `job state = spawn failed`, exit 78 (EX_CONFIG), and the item can even be missing from Login Items. Registering over the old job does not replace the constraint. The fix is unregister, `launchctl bootout`, wait until `launchctl print` shows nothing, then register; do it proactively once per app version (record the version that registered it), retry with backoff, never touch what the user switched off, and parse real `launchctl print` captures in tests behind a fake ops seam. Apply it to every app with a login item (Pitwall 1.0.4, Pitcrew 1.0.2, Dyno).
- Controls for fixing the service must never live behind the service: anything the app itself owns (service state, repair, login items) renders and works with the service down, and every "service not running" message carries the working button.
