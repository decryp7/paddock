# Features added in the engine round

What each feature does for a user, and what Spotter does not do.

## Pictures for watches
Each watch shows the picture of the thing it watches: the owner's avatar for GitHub and Gitea/Forgejo repositories, the
site's favicon for feeds, pages, directory listings and JSON APIs. Spotter asks the watched site only, at most once a
week per picture, and never a favicon service. Folders, package registries and container images use Spotter's own mark
for their kind. Offline, or when a site has no usable picture, the last picture (or the mark, or the monogram) stays.
Only small, plain images are accepted; scripts and external references in SVG files are refused.

## E-mail notifications
Add an e-mail channel to a watch (an address) and set up the mail server once under Settings: server, port, security
(STARTTLS for port 587, TLS for port 465, "None" only for a relay on a trusted network), account and sender. The password
is kept in the system credential store. "Send test" sends a message through the saved settings and, when it fails, says
what to check (server unreachable, secure connection refused, account not accepted, address refused, no answer in
time). Mail is sent for new releases of watches that have the channel; failures appear in the journal as "notify" failures.

![Settings: the e-mail form](screenshots/settings-email-light.png)

## Safer address checks
A watch that is not marked local cannot reach this computer or its local link even if a name changes its answer between
the check and the connection (DNS rebinding).

## Install commands
For container tags and package versions (crates.io, npm, PyPI, NuGet) each release carries a command you can copy
(`docker pull image:tag`, `npm install name@version`, ...). Spotter never runs it. Other sources have none, because what
to install depends on the platform. The watch's detail page shows a Copy command button on the upgrade line.

![A package watch with the upgrade line and Copy command](screenshots/watch-detail-upgrade-light.png)

## Downloads
Spotter's downloads (a release file you download, the file an "after a new release" action fetches, the browser for
JavaScript pages) all go through the Paddock downloader. A host that serves byte ranges is read over up to four
connections at once (never more than four to one host, however many downloads run); one that does not is read in one
stream. When a host answers "too many requests" Spotter waits as long as it asks and tries again with fewer connections.
A download that is cancelled or cut keeps its partial file and the next download of the same file carries on from it.
The checksum is computed while the file is written, so nothing is read a second time, and it is compared with the
publisher's SHA-256 (the release's own, or the `.sha256` file next to it); a file that does not match is deleted, never
left looking finished. Before it starts Spotter checks that the disk has room for the file. Every download is a job in the
status bar (Connecting, Resuming, Downloading, Verifying) with Cancel at any moment; Spotter never gives up on one on its own.
