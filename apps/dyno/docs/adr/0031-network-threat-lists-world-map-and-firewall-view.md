# 31. Network: threat lists, the connections world map and the firewall view, in the core with downloaded data

- Status: Proposed (2026-11-03)
- Date: 2026-11-03
- Revision 2 (2026-11-03): answers the independent review of `54c861678` (P1: hostname allows could be spoofed through
  reverse names, the data folder contradicted ADR 0029, the DROP fetch floor, the source record; P2: Feodo dropped, more
  special-purpose ranges, map truncation and small countries, MMDB verification, privacy text, D5, the estimate). The
  reviewer's answers to D1 to D10 are recorded with each decision.
- Related:
  - ADR [0007](0007-extensibility-three-tiers.md) (three tiers: data is a pack, behaviour every user gets is core; plugins
    only for native or third-party code) and its amendment by ADR [0021](0021-monitoring-only.md) (monitoring only: Dyno
    changes nothing on the system).
  - ADR [0015](0015-benchmarks-network-and-ip-lookup.md) (sockets through the `NetworkSockets` port, "Look up IP" as a
    browser hand-off, "no country database is bundled": this record keeps that sentence and adds an opt-in *download*).
  - ADR 0027 revision 4 (`38ceddf56` on `docs/dyno-helper-wp0`, read 2026-11-03): C10 and WP-9 (firewall readers), the
    WP-0 measurements of `INetFwPolicy2` and `socketfilterfw`, the helper's `Firewall` request for `pf` and `nft`.
  - ADR 0029 (`5aed6c641` on `docs/dyno-cached-views`, accepted 2026-11-03): rebuildable data goes to the OS cache folder
    (`Dirs::cache_db()`, out of backups; `<DYNO_DATA_DIR>/cache` in tests), never into `dyno.db` or the data folder; the
    downloaded lists and databases of this record follow it (§1).
  - ADR 0013 (alarms lifecycle), `dyno_core::alarms::feed` and `dyno_core::network_alerts` (the alarm sources).
  - Family: `crates/paddock-net` (the `Egress` door, `set_offline`), `crates/paddock-download` (`Integrity`, resumable,
    hashed while written), `crates/paddock-packs` (TOML packs, limits, hot reload), `crates/paddock-updater` (minisign
    check, for comparison), family ADR 0001 §"Local first" (offline by default, every network feature opt-in), ADR 0028
    (licence policy and notices; npm-only `CC-BY-4.0`), ADR 0041 / proposed rule 60 (tests control time), ADR 0037
    (composition contract), ADR 0034 / rule 55 (one implementation per mechanism).
  - Rules 7 (ER-14), 10 to 12 (ER-17 to ER-19), 26, 46, 48, 51 (ER-20); ER-27 (logs); principle "local first, opt-in
    online" (2026-10-29).
- Trigger: the user's requests for Dyno 1.1.0 (2026-11-03): a bad IP and hostname check, a world map of connections and a
  firewall view, decided as **core features with downloaded data packs, not plugin processes**.

## Context

### What exists (read in `origin/main` `b74c133c1`; paths under `apps/dyno/`)

| Piece | Where | What it gives this record |
| --- | --- | --- |
| Socket rows, the view, filters, exposure, the new-host tracker | `crates/dyno-core/src/network.rs` (`SocketRow`, `ConnectionRow`, `NetworkSnapshot`, `MAX_ROWS = 2000`, `Tracker`, `NetEvent`) | every connection with its remote address, port, state and owner; at most 2,000 rows per kind per answer |
| The agent's network centre | `crates/dyno-agent/src/network.rs` (`NetCentre::snapshot`, `tick` every `BACKGROUND_EVERY = 30 s`, `resolve`) | one place that sees every look, on the page and in the background |
| Reverse DNS on request | `NetCentre::resolve` (`RESOLVE_WORKERS`, `RESOLVE_DEADLINE`, cache with TTLs), `dyno-core/src/network_names.rs` (`MAX_RESOLVE = 64`, `clean_name`) | names only for addresses the page asks about; each lookup is a PTR query to the user's resolver |
| Address classes | `dyno-core/src/ip_lookup.rs` (`is_public`: private, loopback, link-local, CGNAT, multicast, documentation, reserved are not public) | the "local" side of the map and the addresses never matched against lists |
| Network alarms | `dyno-core/src/network_alerts.rs` (`NetworkAlerts { new_exposed_listener, new_remote_host }`, off by default), `alarms/feed.rs` (`Source::Network`) | the place a "flagged connection" alarm joins |
| Network change events (VPN up/down) | `dyno-core/src/netchange.rs` (roadmap WP-35) | the VPN note on the map |
| Network page | `app/src/app/features/network/network-page.ts` (one `pd-table`, tabs Connections / Listening, grouping), `state/network.store.ts`, `core/mock/network.ts` | where the new column, the Map tab and the "why" panel go |
| Wire contract style | hand-written `app/src/app/core/api.types.ts` checked by `*.contract.spec.ts` against a JSON golden written by a Rust test (`crates/dyno-core/tests/disk_wire.rs` → `core/disk-map.wire.json`) | the pattern for the new DTOs |
| Charts | `echarts` ^6.1.0 in `app/package.json`; `ui/projects/paddock-ui/testing/src/fake-chart.ts` | available, but see D6 |

Dyno has **no outbound HTTP of its own today**: `dyno-agent` depends on neither `paddock-net` nor `paddock-download`
(`crates/dyno-agent/Cargo.toml`); the app's `paddock-download` is the updater's. ADR 0027 §7 adds the first download (the
smartmontools package, opt-in, through `Egress`). These features add the second kind: data files that change daily or
monthly, so a SHA-256 pinned at build time cannot work for them (D9).

### What is knowable, honestly

- **Remote addresses: yes.** Every connection row carries the remote IP. Matching it against a list of ranges is exact.
- **Hostnames: mostly no.** Dyno reads sockets, not packets. The name a program asked DNS for, and the TLS SNI, are not in
  any socket table; Dyno sees neither without a packet capture, a DNS proxy or a network extension, all of which ADR 0021
  rules out (monitoring only, no driver, no elevation of the agent). The OS DNS caches are not a usable source: Windows
  `DnsGetCacheDataTable` is undocumented; macOS has no public API; `resolvectl show-cache` on Linux needs root (not
  measured; secondary knowledge, marked unverified). What Dyno *can* know is the **reverse name** (PTR) of an address,
  which for cloud and CDN addresses is the provider's name (`ec2-…compute.amazonaws.com`, `…1e100.net`), rarely the site.
- **A reverse lookup is not private.** The PTR query for `203.0.113.7` goes through the user's resolver to the DNS servers
  that the owner of that address range runs. For a command-and-control address, an automatic lookup tells its operator that
  someone is looking. So Dyno never resolves names on its own for this feature (D4).
- **Location: where the address is registered or announced, not where the server is.** Geo-IP databases map ranges to a
  country. Anycast CDNs and DNS (Cloudflare, Akamai, Google) answer from the nearest site but sit in the database under one
  country. With a **full-tunnel VPN** the socket table still shows each real destination (the tunnel is below the socket),
  so the map is right about destinations, while the user's own apparent location changes. With a **proxy or relay**
  (a SOCKS/HTTP proxy, Tor, iCloud Private Relay in Safari, a corporate proxy) the remote address *is* the proxy, so every
  proxied connection lands on the proxy's country.

## Decision

### 1. Shape: core plus downloaded data, three parts of one Network page

- **Core (tier 1):** a pure `dyno_core::intel` module (ranges, hostname suffixes, verdicts, "why flagged", country
  aggregation), a pure `dyno_core::firewall` model, the agent's `IntelCentre` (downloads, files, the current index) and
  firewall readers (ADR 0027 WP-9). No plugin process (ADR 0007 point 4: data → pack; behaviour every user gets → core).
- **Packs (tier 2), two kinds through `paddock-packs`:**
  - `intel-sources` (format 1): the **catalogue of downloadable sources**, built in (`packs/intel-sources.toml`). Each entry:
    `id`, `name`, `kind` (`ipList` | `geoCountry` | `geoAsn`), exact `url` (https), `host` (the only host the download may
    reach; redirects to another host are refused by `Egress`), `format` (`spamhausDropJson` | `plainCidr` | `netset` |
    `mmdbGz`), `licence`, `attribution`, `terms_url`, `min_interval` (never fetch more often), `default_interval`,
    `max_bytes`, `max_entries`, `info_url_template` (optional, for "why flagged", validated like ADR 0015's templates).
    A user pack of this kind may add sources **only with `file://` paths in the user's packs folder** (a list the user
    downloaded themselves); a user pack can never add a host to the egress allow-list (that would widen what Dyno may
    reach, the same reasoning as ADR 0007's "a pack cannot widen what Dyno may delete").
  - `intel-local` (format 1): the user's **own lists**: `allow` and `flag` entries of CIDRs, single addresses and hostnames
    (suffix match), each with an optional note. "Mark as safe" on a flagged row writes an `allow` entry into
    `packs/intel-local.toml` through an agent command (Dyno's own file; nothing on the system changes, ADR 0021 holds).
- **Downloaded data** is not a pack and not in `dyno.db`: it is rebuildable, so it lives in the **OS cache folder** of
  ADR 0029 §2, beside `cache.db`: `<cache folder>/intel/<source-id>/` (a `Dirs::intel_cache(source)` next to
  `Dirs::cache_db()`; `%LOCALAPPDATA%\Dyno\Cache\intel` on Windows, the macOS and Linux cache folders of ADR 0029, with
  the same explicit macOS backup exclusion, Unix modes `0700`/`0600`, and `<DYNO_DATA_DIR>/cache/intel` in tests, ER-14).
  Files: `current.bin` or `current-<yyyy-mm>.mmdb`, and `meta.json` (source, URL, ETag, fetched time, list date, entry
  count, SHA-256 of the bytes as received, format version). A cleared cache folder means "not downloaded yet": the next
  scheduled or manual update fetches it again, within the source's `min_interval` (the last fetch time is kept in the
  agent's settings, not in the cache, so clearing the cache cannot cause an early DROP fetch). Turning a source off
  deletes its folder. The user's own lists (`intel-local.toml`) are user data and stay in the packs folder.

### 2. Bad IP and hostname check

#### 2.1 Sources (read 2026-11-03)

| Source | What | Licence / terms | Cadence (publisher) | Size (measured 2026-11-03) | Access | Verdict |
| --- | --- | --- | --- | --- | --- | --- |
| **Spamhaus DROP** `drop_v4.json`, `drop_v6.json` ([page](https://www.spamhaus.org/blocklists/do-not-route-or-peer/), [terms](https://www.spamhaus.org/drop/terms/)) | hijacked and criminal netblocks, one JSON object per line (`cidr`, `sblid`, `rir`) plus a final metadata line (`type`, `timestamp`, `size`, `records`, `copyright`, `terms`) | free of charge "regardless of size or business type"; the DROP page: "When used in a product, credit for the use must be given to The Spamhaus Project, and the date and copy text should remain with the file and data"; terms §3.2: the Spamhaus name may not be used in marketing material (so not in release notes, the features list or store text; factual in-app labels naming the source are fine); §3.3: access can be revoked; copyright and database right kept, no redistribution granted | re-evaluated daily; the page: "Automated downloads must be at least one hour apart" **and** "The DROP list data should not be downloaded from our website more than once per day"; "Excessive downloads may result in your IP being firewalled" | v4: 107,206 bytes, 1,757 records (its metadata line) | public, no key | **the only default source (D1)**; `min_interval` **24 h** |
| **abuse.ch Feodo Tracker** `ipblocklist_recommended.txt` ([page](https://feodotracker.abuse.ch/blocklist/), [terms](https://abuse.ch/terms-of-use/)) | botnet C2 addresses | terms §3.4: "free to use without limitations according to Creative Commons principles (CC0)"; §7.3 forbids copying or derivative works without consent (so: no mirror) | regenerated every 5 min; recommended fetch at least every 15 min | **1 entry**, header "Last updated: 2026-03-04" | the abuse.ch terms (dated 2026-10-21, per the review) limit the platforms to authenticated, not-for-profit use and forbid adaptations (§7.3) | **not in 1.1.0** (P2 of the review): empty today, an index built from it is an adaptation, and at Dyno's 24 h cadence a 5-minute feed is stale anyway |
| **FireHOL level 1** `firehol_level1.netset` ([raw](https://raw.githubusercontent.com/firehol/blocklist-ipsets/master/firehol_level1.netset), [site](https://iplists.firehol.org/)) | aggregate of dshield, feodo, fullbogons, spamhaus_drop | FireHOL itself is GPL-2.0 (the generator); lists belong to their maintainers; no data licence in the file; DShield's own terms not checked (unverified) | "Update Frequency: 1 min"; file date 2026-11-02 | 3,940 subnets, 611,102,337 addresses | public (GitHub raw) | **offered, off by default**: mixed and unclear licences, and it contains `10.0.0.0/8`, `192.168.0.0/16`, `127.0.0.0/8`, `100.64.0.0/10` (fullbogons), which §2.2 never matches |
| abuse.ch URLhaus / ThreatFox ([API](https://urlhaus.abuse.ch/api/)) | malware URLs and hosts | free for not-for-profit; commercial use may need a Spamhaus subscription (terms §3.1, §4.1) | dump every 5 min | – | **Auth-Key required** for every download | **not in 1.1.0** (D4): needs the user's own key, and its hosts are mostly compromised ordinary sites, which a PTR name rarely equals |
| Spamhaus ASN-DROP `asndrop.json` | whole autonomous systems | as DROP | daily | – | public | **later**: needs an ASN per address (§3's ASN database makes it possible; one more format) |

DROP publishes no signature or a machine-readable checksum. Integrity therefore rests on TLS to the publisher's
own host, the host pin, size and entry caps and the format checks of §2.3 (D9 compares a signed mirror and rejects it).

#### 2.2 Matching (pure, `dyno_core::intel`)

- **Index.** Every list is parsed into `(start, end, source)` intervals per family (`u32` for IPv4, `u128` for IPv6;
  IPv4-mapped IPv6 addresses are matched as IPv4). One sweep over the sorted interval ends builds **disjoint segments**,
  each with the small set of sources that cover it (a `u16` bit set over at most 16 sources, a bounded catalogue limit).
  A lookup is one binary search: `O(log n)`, no allocation. This is the same answer a CIDR trie gives, with one sorted
  `Vec` instead of a node graph (KISS); the segment count is at most twice the interval count. Bounded:
  `MAX_ENTRIES_PER_SOURCE = 250_000`, `MAX_SOURCES = 16`. Estimated size for DROP v4+v6 and FireHOL level 1:
  under 15,000 segments, under 1 MB (an estimate; WP-1's benchmark records the real figure, rule 48).
- **Never matched:** every address that `ip_lookup::is_public` calls not public (private, loopback, link-local, CGNAT,
  multicast, documentation, reserved, unspecified), plus these special-purpose ranges, which N1 adds to `is_public` (so
  "Look up IP" refuses them too) with one test each:
  - `198.18.0.0/15` (benchmarking; also the fake-IP range of proxy tools such as Clash and Surge, whose "connections"
    there are local stand-ins for real names);
  - `192.0.0.0/24` (IETF protocol assignments);
  - `2002::/16` (6to4), `2001::/32` (Teredo), `100::/64` (discard-only).
  **NAT64** `64:ff9b::/96`: the embedded IPv4 address (the last 32 bits) is unwrapped and that address is matched and
  located, as an IPv4-mapped address is; the row keeps showing the IPv6 address with "via NAT64". FireHOL's bogons would
  otherwise flag the user's own network.
- **Order of a verdict:** a user **address** `allow` (an address or CIDR) wins over everything; then user `flag` entries
  (addresses or hostnames); then the downloaded lists. A user **hostname** `allow` suppresses only the user's own hostname
  `flag` entries and **never** a list match or an address flag: a reverse name is set by whoever controls the address's
  reverse zone, so an attacker on a DROP range can name it `update.microsoft.com` (test: a listed address whose cached
  reverse name matches a hostname allow stays `Flagged`). A verdict is `Clean`, `Allowed { entry }`, or `Flagged { matches: Vec<ListMatch> }` where a
  `ListMatch` is `{ source_id, source_name, entry: "1.19.0.0/16", reference: Some("SBL434604"), list_date, stale }`.
- **Hostnames:** suffix match on labels (`evil.example` matches `a.evil.example`, never `notevil.example`), names normalised
  once (lower case, IDNA to ASCII, trailing dot removed, `clean_name`'s bounds). Matched against the user's `flag` and
  `allow` hostnames and against **reverse names already in the agent's cache** (looked up because the user asked for names
  on the page). Wherever a reverse name is shown in this feature it is labelled "name set by the address's owner" (it is
  unverified: no forward confirmation is done, because that would be one more lookup Dyno does not make on its own,
  D4). The UI says what was checked: "Checked by address. Names are checked only when you look them up."
- **Where it runs:** in `NetCentre`, on every look (the page's snapshot and the 30 s background tick), over the remote
  addresses of the current rows. 2,000 rows × one binary search is microseconds; no extra thread (ER-20: the cheaper
  operation is already the operation). The index is built off the sampler thread when a list changes and swapped in whole.

#### 2.3 Downloads, updates, offline

- **Off until the user turns a source on** (Settings ▸ Network ▸ Online data). Turning one on shows: what is downloaded,
  from which host, how often, its licence and terms link, and that the publisher sees this computer's address and the time
  of each download, nothing else. With every source off, the agent constructs no `Egress` at all.
- **The one door:** the agent's `Egress` (`paddock-net`, feature `http`) with a policy built from the catalogue: public
  reach only, the source hosts as the allow-list, `MAX_REDIRECTS` within the same host, a body cap of the source's
  `max_bytes`. Dyno's **Offline** switch (Settings ▸ Network; the family's local-first principle) calls
  `Egress::set_offline(true)` and stops the scheduler; a job started while offline fails at once with "Dyno is offline".
- **One job per update** (`paddock-jobs`, rules 10 to 12): phases `download` (bytes, of the size when known), `check`
  (format, counts, dates), `index` (entries), `swap`. Cancel works in every phase and leaves the old data in use. Conditional
  requests (`Request::if_none_match`, `Response::not_modified`): an unchanged list costs one small reply.
- **Checks before a list or database is used** (parse, don't validate): size within `max_bytes`; gzip inflated with a cap
  (`max_bytes × 8`); every line parsed or the whole file refused with the line number; DROP's metadata line present, its
  `records` equal to the lines read and its `timestamp` not in the future and not older than 7 days; at least one entry
; an entry count that fell by more than 90 % against the last good copy is kept aside
  and the last good copy stays in use with a note ("The new DROP list has 3 entries instead of 1,757; kept the previous
  one"). The bytes go to `intel/<id>/.staging` (create-new), then a rename; the previous copy stays as `previous.*` until
  the next good update.
- **Schedule:** per source `manual` or `automatic` (default when turned on, D7) at `default_interval` (DROP: 24 h; geo
  databases: check on the 3rd of each month, then daily until the new month's file appears), never below `min_interval`
  (**DROP 24 h**, the publisher's "not more than once per day" with the risk of being firewalled; DB-IP 1 day), with up to
  10 % jitter added (never subtracted), on an injected `Clock` (ADR 0041). Missed runs while the computer slept run once
  at wake, not once per missed interval. **"Update now" respects `min_interval` too**: inside it the button is disabled
  with "Next update allowed at 14:05 tomorrow (Spamhaus asks for at most one download a day)". DROP v4 and v6 are one
  source with one fetch time.
- **Stale data:** a list older than three times its interval shows "DROP list is 4 days old" on the page and in each
  "why flagged"; matches from it carry `stale: true`. Offline, everything works from the files on disk, with their dates.
- **Removal:** turning a source off deletes its folder; the matching index drops it at once.

#### 2.4 False positives, "why flagged", alarms

- **Why flagged** (a side panel from the flag mark, also in the process-details Network tab): the list name, the matched
  entry and reference (`1.19.0.0/16`, `SBL434604`), the list date and age, the source's attribution and a "View on
  Spamhaus" link that opens the browser through ADR 0015's opener path (template validated in Rust, nothing fetched by
  Dyno). Text: "Listed means the address range has a bad record. It does not prove that this program is malicious;
  shared hosting and reassigned ranges cause false alarms."
- **Mark as safe** adds an address `allow` entry (the exact address by default; the matched range as an option; never
  a hostname, for the reason in §2.2), with an undo in
  the panel. The user's lists are editable in Settings and as the TOML file (hot reload through `paddock-packs`).
- **Alarm:** a new `NetworkAlerts.flagged_connection` (default **on when any list is on**, D8), `Source::Network`, name
  `flaggedConnection`, subject `<program>|<source_id>|<entry>` (so one range and program raise one alarm, not one per
  connection), severity Warning (Info when the only match is from a stale list), title "Firefox connected to an address
  on Spamhaus DROP". The event is always written to the activity log, whether the alarm is on or not, like ADR 0015's
  network events.

### 3. Connections world map

#### 3.1 Data sources (read 2026-11-03)

| Option | Content | Licence | Cadence | Size | Account | Verdict |
| --- | --- | --- | --- | --- | --- | --- |
| **DB-IP IP to Country Lite** ([page](https://db-ip.com/db/download/ip-to-country-lite)) | range → country | CC-BY-4.0; credit DB-IP.com; web apps link back: `IP Geolocation by DB-IP` | monthly | MMDB 8.1 MB (gz), 732,796 records; MD5 and SHA-1 on the HTML page only | none; file `https://download.db-ip.com/free/dbip-country-lite-YYYY-MM.mmdb.gz` (verified by the review, 2026-11-03); DB-IP's page asks the downloader to tick a licence-acceptance box, which Dyno's opt-in mirrors (the source card shows the CC-BY-4.0 terms and an "I accept the DB-IP licence" checkbox before the first download) | **default** |
| **DB-IP IP to ASN Lite** ([page](https://db-ip.com/db/download/ip-to-asn-lite)) | range → AS number and organisation | CC-BY-4.0 | monthly | MMDB 9.2 MB (gz), 485,292 records | none | **default with the country file**: names the network ("Cloudflare, Inc."), which is how a user tells a CDN from a server |
| DB-IP IP to City Lite ([page](https://db-ip.com/db/download/ip-to-city-lite)) | range → city, approximate latitude and longitude | CC-BY-4.0 | monthly | MMDB 121.0 MB (gz), 7,777,816 records | none | **not in 1.1.0** (D2): 15× the size for a precision that is false for cloud, CDN and mobile addresses |
| IPinfo Lite ([page](https://ipinfo.io/lite)) | country, continent, ASN, AS name | CC-BY-SA-4.0; "IP address data is powered by IPinfo" | daily | not stated | **token** (free sign-up) | **later option** for users with a token (D2) |
| MaxMind GeoLite2 ([EULA](https://www.maxmind.com/en/geolite2/eula)) | country, city, ASN | EULA plus CC-BY-SA-4.0: internal use, use updates promptly and **destroy old versions within 30 days**, notify MaxMind before disclosing to a third party, amendable at any time | weekly (secondary) | – | account and licence key | **rejected**: not an open licence; obligations Dyno cannot honour for the user |

The reader is the `maxminddb` crate (ISC, read on docs.rs 2026-11-03, version 0.27; ISC is on the allow-list). With its
`mmap` feature off, a database is read into memory (≈ 17 MB for country and ASN, D10 measures mmap against it).
DB-IP publishes only MD5 and SHA-1, on its HTML page; Dyno does not scrape them. Before a downloaded database is swapped
in, the agent opens it, checks `database_type` (`DBIP-Country-Lite` / `DBIP-ASN-Lite`), `ip_version` 6, a build epoch
inside the last 62 days, a node count within bounds, and runs `Reader::verify` (the crate's full structural check of the
search tree and data section); any failure keeps the previous month's file. The open-and-verify path has a fuzz target
(N4), since the bytes come from the network.

#### 3.2 What the map shows

- **Countries, not points.** Each public remote address → country (and AS name). The agent returns a **summary**, not
  rows: `GeoSummary { countries: [{ code, name, connections, programs, hosts, flagged }], local: [...], unknown, proxied_note,
  vpn_note, db_date, truncated }`, computed in Rust (the UI never aggregates domain data). It is aggregated from the
  socket rows **before** `build` applies `MAX_ROWS = 2000`, so the map counts every connection; `truncated` is the count
  of connections left out only if the socket read itself was capped (shown as "showing N of M"). A few thousand connections become at most
  about 250 countries; the page never draws per connection.
- **Beside the map, "On this computer and your network":** loopback, private, link-local, CGNAT and other non-public
  remote addresses, grouped by class with counts and programs, as a list (they have no place on a map). Addresses the
  database does not know are "Location unknown".
- **Honesty notes** (from data, not guessed): "Locations are where an address is registered. Content networks (for example
  Cloudflare, Akamai) serve from near you but are listed under one country." When an AS name is a known CDN or cloud
  network (a short list in `intel-sources`, data not code) the country tooltip says "includes content-network addresses".
  When `netchange` reports a VPN up: "A VPN is on: each connection still shows its real destination; your own location
  as seen by those sites is the VPN's." When most connections of a browser go to one address (a proxy or relay pattern,
  threshold in data): "Safari's connections go through a relay at 172.x; their real destinations are hidden."
- **Filters:** by program (the same picker as the table; the agent filters before aggregating), by protocol, flagged only.
  Clicking a country filters the Connections table to it (one shared query; viewport and selection survive refreshes,
  family ADR 0017).

#### 3.3 Rendering, accessibility, CSP

- **Plain SVG with geometry projected at build time** (D6). A WP-7 script turns Natural Earth's 1:110m Admin 0 countries
  (public domain, [terms](https://www.naturalearthdata.com/about/terms-of-use/), read 2026-11-03: "No permission is needed
  ... Crediting the authors is unnecessary") into Equal Earth projected, quantised path strings keyed by the
  **`ISO_A2_EH`** field (`ISO_A2` is `-99` for France and Norway, per the review). Countries and territories too small for
  1:110m (Singapore, Hong Kong, Luxembourg, Bahrain and others: every code DB-IP can return that has no polygon) get a
  **point** at a centroid taken from Natural Earth's 1:10m populated places or admin-0 label points, drawn as a focusable
  dot with the same encoding and aria label as a country; N7's test lists every ISO code without a shape and fails if one
  has neither a shape nor a dot. Committed as a generated TypeScript module with its source version and SHA-256 (estimated 100 to 250 KB; WP-7
  records it). Angular binds `[attr.d]` and CSS classes; no runtime map library, no tiles, no fonts, no inline style
  attributes, nothing remote. ECharts' `geo` component (already a dependency) is the fallback if SVG proves short of
  something; it renders to canvas, so its accessibility would rest entirely on the table.
- **Encoding:** a sequential fill in five steps from `--pd-*` tokens (validated by the token test for contrast and colour
  vision) for connection counts, a hatched pattern plus a flag icon for countries with flagged connections; **numbers are
  always shown** on hover and focus and in the legend ("1–2, 3–9, 10–49, 50–199, 200+"). Never colour alone.
- **Keyboard and screen readers:** each country with connections is focusable (`tabindex`, `role="button"`,
  `aria-label="Germany: 12 connections from 3 programs, 1 flagged"`); arrow keys move between them in reading order;
  Enter filters the table. A **Table view** toggle shows the same summary as a sortable `pd-table` (country, connections,
  programs, hosts, flagged), the default for screen-reader users and always one click away. Reduced motion: no transitions.
- **Performance:** at most about 180 paths and 250 count labels; redraws only when the summary changes (OnPush, signals).

### 4. Firewall view (read-only)

| OS | 1.1.0 | Read by | Shown | Limits |
| --- | --- | --- | --- | --- |
| Windows | **yes** | the agent, `INetFwPolicy2` (COM, works in every logon type; WP-0 measured it as a standard user: 3 profiles, 494 rules; the CIM cmdlets fail over SSH, so they are no test oracle) | per profile: on or off, default inbound and outbound action, notifications, the active profiles; rules (at most 4,096): name, group, enabled, direction, action, profiles, program, service, protocol, local and remote ports, remote addresses | only Windows Defender Firewall; a third-party firewall's own rules are not readable ("Another firewall may be in charge", if WSC reports one: WP-6 checks whether `IWSCProductList` answers as a standard user; unverified) |
| macOS | **Application Firewall: yes** | the agent, `socketfilterfw --getglobalstate`, `--getblockall`, `--getstealthmode`, `--getallowsigned`, `--listapps` (WP-0: exit 0 as the user; do not read `com.apple.alf`, it has no `globalstate` on 26.6.2) | on or off, block all incoming, stealth mode, allow signed software, per-app allow or block | the Application Firewall filters incoming connections only; content filters (Little Snitch, LuLu) are not readable |
| macOS | **`pf`: only through the helper** | the helper's `Firewall` request (ADR 0027 §5, `pfctl -s info`, `-s rules`; WP-0: `/dev/pf: Permission denied` as the user) | enabled, counters, rules as text lines (anchors listed, not expanded) | without the helper: "pf rules need the helper" with Turn on; in 1.1.0 (ADR 0027 D1a step 3, D5) |
| Linux | **only through the helper** | the helper's `Firewall` request (`nft -j list ruleset`, capped at 4 MB; ufw and firewalld state derived from it) | tables, chains with hook and policy, rule count, rules as text | without the helper or in the AppImage: "Firewall rules need the helper" / "not available in the AppImage"; in 1.1.0 (D5) |

- One typed model (`dyno_core::firewall::FirewallView { os, sources: Vec<FirewallSource> }`, each source with `state`,
  `rules: Vec<FirewallRule>`, `truncated`, `note`) for all readers (ADR 0027 WP-9's "one typed view"). ADR 0027 §2's
  "state and counts only for other accounts" is a rule of the **helper's** `Firewall` answer (`pf`, `nft`); the agent's own
  readers (Windows, the Application Firewall) show the signed-in user whatever the OS lets that user read. Rule addresses
  are shown, never logged.
- Read on opening the page and on Refresh, through a job when the read is slow (`--listapps`, 494 COM rule objects), with
  the page's cached last answer shown with its age (ADR 0029's pattern, in memory only: rules are cheap to read again).
- Not in 1.1.0: matching firewall rules to connections ("this program is blocked inbound"), Windows rule origins
  (Group Policy vs local), any change to a rule (ADR 0021).

### 5. Privacy and logs

- **No address ever leaves the computer for these features.** Lists and databases are downloaded whole; every lookup is
  local. The only network traffic is the opt-in downloads, to the catalogue's hosts, with no cookies and no query
  parameters beyond the file name. The User-Agent is `Dyno/<version>`, and the opt-in text says so: the publisher learns
  this computer's address, the time, and that it runs Dyno and which version (a generic User-Agent was considered; an
  honest one is what both publishers' fair-use pages ask of automated clients, and the address is revealed anyway). Reverse names are never looked up by this
  feature (§2.2, D4). "Look up IP" (ADR 0015) stays a separate, explicit browser hand-off.
- **Logs (ER-27) carry no addresses, hostnames, CIDRs, program names of matches or rule addresses.** They carry: job id,
  source id, phase, duration, HTTP status, bytes, `not_modified`, entry counts before and after, check failures by kind
  and line number, the swap outcome, scheduler decisions ("next DROP check at …"), `Egress` refusals by kind, match
  counts per source per look at most once a minute (as ADR 0027 §9 does for the sampler), and firewall reads by source,
  rule count, truncation and duration.
- **What Dyno keeps locally:** the activity entry and the alarm for a flagged connection are stored in `dyno.db` with
  the program, the matched entry (CIDR or address) and the source, for the history's retention (ADR 0008); the Settings
  text says so, and "Clear history" removes them. Nothing about matches is kept in the cache folder.
- **Attribution and notices:** About ▸ Data lists each enabled source with its licence, attribution and date (DB-IP's
  "IP Geolocation by DB-IP" link under the map as its terms ask; "(c) The Spamhaus Project" with the list date in "why
  flagged", a factual label, never in marketing text, release notes or the features list, per Spamhaus terms §3.2);
  Natural Earth credited though not required. If Spamhaus revokes access (§3.3) or the download fails for that reason, the
  source shows the HTTP status and "Spamhaus refused the download" and the last good list is kept with its age.

### 6. Wire contract

New Rust DTOs in `dyno_core::intel` and `dyno_core::firewall`, mirrored by hand in `app/src/app/core/api.types.ts` and
checked like the disk map: `crates/dyno-core/tests/intel_wire.rs` writes `app/src/app/core/network-intel.wire.json`, and
`network-intel.contract.spec.ts` builds the same values as TypeScript literals.

- `ConnectionRow` gains `verdict: Verdict` (`{ kind: 'clean' } | { kind: 'allowed', entry } | { kind: 'flagged', matches }`)
  and `geo: GeoTag | null` (`{ country, countryName, asn, asName }`).
- New requests: `intelStatus` → `IntelStatus { online: bool, sources: SourceState[] }` (per source: enabled, mode, last
  fetched, list date, entries, stale, last error, next check, licence, attribution); `intelSetSource { id, enabled, mode }`;
  `intelUpdate { id }` → job id; `intelWhy { address }` → `WhyFlagged`; `intelAllow { entry, note }`; `geoSummary
  { query }` → `GeoSummary`; `firewall` → `FirewallView`.
- Settings: `networkAlerts.flaggedConnection`, `network.offline`; per-source state lives in the agent's intel settings
  (versioned, defaults on missing or corrupt).

### 7. Tests (rule 7 / ER-14, rule 60 / ADR 0041)

- **Spoofing:** a listed address whose cached reverse name matches a user hostname `allow` stays `Flagged`; a hostname
  `allow` only cancels a hostname `flag`; Mark as safe never writes a hostname.
- **Special ranges:** one test per range of §2.2 (`198.18.0.0/15`, `192.0.0.0/24`, `2002::/16`, `2001::/32`, `100::/64`)
  never matched and refused by "Look up IP"; NAT64 `64:ff9b::203.0.113.7` matched and located as `203.0.113.7`.
- **Pure core:** the segment index against a naive linear matcher over generated ranges (proptest: random CIDR sets,
  overlaps, nesting, `/0`, `/32`, `/128`, IPv4-mapped IPv6); allow over flag over lists; non-public never matched (every
  `is_public` class, including a list that contains `10.0.0.0/8`); hostname suffix rules (label boundary, IDNA, trailing
  dot, case); stale marking; caps and one over each cap.
- **Parsers** on invented same-shape bytes (never real downloaded lists in fixtures): DROP JSON lines with a correct and a
  wrong metadata line, a future timestamp, a truncated last line; plain CIDR; FireHOL netset with comments; gzip bombs over
  the cap; fuzz targets for each parser.
- **Geo:** a tiny MMDB written by a test helper to the published format ([spec](https://maxmind.github.io/MaxMind-DB/))
  with documentation-range addresses (`192.0.2.0/24`, `2001:db8::/32`) mapped to invented countries; a corrupt file, a
  wrong `database_type`, a file that `Reader::verify` rejects (a broken pointer), a file over the cap, a fuzz target over
  open-and-verify; country aggregation, local grouping and the VPN and proxy notes from
  synthetic rows.
- **Downloads** over `paddock-net`'s `FakeWire` and `StaticLookup` only, never the real hosts: off means no request
  recorded; offline refuses before resolving; a redirect to another host is refused; 304 keeps the data; a 90 % drop keeps
  the last good copy; "Update now" inside DROP's 24 h floor
  makes no request and answers the next allowed time; a cleared cache folder does not reset the floor; Cancel in each phase leaves the old index; the scheduler on a `FakeClock` (min interval, jitter
  bounds, one catch-up run after sleep, monthly geo checks).
- **Alarms:** one alarm per program and range, not per connection; Info for stale matches; activity entry always.
- **Firewall parsers:** `socketfilterfw` outputs and invented `INetFwPolicy2` rule records through a fake COM port; the
  Windows reader on the Windows CI runner against the runner's own firewall (read-only) as a smoke test; `nft -j` and
  `pfctl` parsers are ADR 0027's.
- **Map geometry:** every ISO code DB-IP can return has a shape or a dot; codes come from `ISO_A2_EH`; the summary
  counts connections beyond `MAX_ROWS`.
- **UI:** the map with the fake summary (focus order, small-country dots, aria labels, the table toggle, numbers in the legend, filter on
  click), the "why flagged" panel, Settings states (off, updating, stale, error with Retry, offline), the contract spec,
  the mock backend; ADR 0037 declarations for the new tab and settings section.
- **VM checks (local only):** Windows 11 as a standard user (firewall page, rule count equals `netsh` count), macOS
  Application Firewall on and off, Ubuntu with and without the helper; downloads against a local fake HTTPS server in the
  VM, never the publishers.
- **Benchmark (rule 48):** index build and lookup for 250,000 entries and 2,000 rows; geo lookup per address with and
  without mmap; recorded in `docs/benchmarks.md`.

## Work packages

Agent-days are ranges for one agent. No package changes `crates/` (Dyno adopts `paddock-net`, `paddock-download` and
`paddock-packs` as they are), so none waits for a release window under family ADR 0022 §G.7.

| WP | Package | Depends on | Days |
| --- | --- | --- | --- |
| N1 | `dyno_core::intel` core: interval index, verdict order (address allows only override lists), hostname suffixes, non-public exclusion with the new ranges in `is_public`, NAT64 unwrapping, `WhyFlagged`, caps; proptests; benchmark | – | 2–3 |
| N2 | Parsers and packs: DROP JSON, plain CIDR, netset, the `intel-sources` and `intel-local` pack kinds with limits, the built-in catalogue (Spamhaus DROP v4/v6, FireHOL off, DB-IP Country and ASN), fuzz targets | N1 | 1.5–2 |
| N3 | Agent downloads: `Egress` from the catalogue, offline switch, jobs with phases and Cancel, checks, staging and swap in the cache folder (`Dirs::intel_cache`), last good copy, scheduler on `Clock` with the `min_interval` floor for "Update now", licence acceptance state, logs | N2 | 3–4 |
| N4 | Geo: `maxminddb` reader, MMDB checks with `Reader::verify` and its fuzz target, versioned files, per-address cache, `GeoSummary` aggregated before `MAX_ROWS` with `truncated`, local groups and the honesty notes, test MMDB writer | N3 | 2–3 |
| N5 | Agent wiring: verdicts on rows, alarm `flaggedConnection`, activity entries, `intel*` and `geoSummary` requests, Mark as safe | N1, N3 | 1.5–2 |
| N6 | Firewall view: `dyno_core::firewall` model, agent command glue over ADR 0027 WP-9's readers, cached answer with age, page states including "needs the helper" | ADR 0027 WP-9 | 1.5–2 |
| N7 | Map geometry: build script from Natural Earth 1:110m to Equal Earth paths keyed by `ISO_A2_EH`, centroid dots for small countries, provenance file, test that every DB-IP country code has a shape or a dot | – | 1–1.5 |
| N8 | UI: Map tab (SVG, small-country dots, legend, focus, Table view, filters, local list, notes), verdict column and "why flagged" panel, Settings ▸ Network ▸ Online data (licence checkbox, next allowed update) and Offline, Firewall tab, About ▸ Data, mock backend, specs | N4, N5, N6, N7 | 5–7 |
| N9 | Wire golden and contract spec; docs: user guide, privacy text, roadmap, `features.md`, notices | N5, N8 | 1–1.5 |
| N10 | VM checks and fixes (local only) | all | 1.5–2 |

**Total: 20 to 28 agent-days** (revision 2: N8 at 5 to 7, as the review estimated; about 21 to 28 in its count).
Critical path N1 → N2 → N3 → N4 → N8 → N10, about 14 to 19 working days; N6 and N7
run beside it. This adds to ADR 0027's 43 to 65 days for 1.1.0; WP-9 there is not counted again.

## Decisions for the user (with recommendations)

- **D1. Default lists.** Recommended (and the reviewer's answer): **Spamhaus DROP v4 and v6 only**, turned on by the user;
  FireHOL level 1 offered but off (unclear licences, bogons); Feodo, URLhaus and ThreatFox not in 1.1.0.
- **D2. Geo database.** Recommended: DB-IP Country Lite and ASN Lite (CC-BY-4.0, no account, about 17 MB, monthly).
  Not City Lite (121 MB, false precision). IPinfo Lite (daily, CC-BY-SA-4.0) later for users who paste a token; MaxMind
  GeoLite2 rejected for its EULA.
- **D3. Licence policy for downloaded data.** The code allow-list does not cover data a user downloads at run time.
  Recommended (revised to the reviewer's answer): a **runtime-data scope in ADR 0028's single
  `crates/paddock-deps/licences.toml`** (rule 55: one policy, not a Dyno-only list), allowing public domain, CC0 and
  CC-BY-4.0 for unmodified local use with attribution shown, and named publisher terms by source (Spamhaus DROP);
  refusing non-commercial-only and EULA data (MaxMind). **CC-BY-SA-4.0 stays out until IPinfo is adopted.** Each catalogue
  entry's `licence` is checked against that scope by a test. Dyno never bundles, mirrors or redistributes any of them.
  This needs the user's approval, as the npm-only `CC-BY-4.0` exception did (2026-10-29); the scope lands as a single
  declared line set in `licences.toml` by the integrating agent.
- **D4. Hostnames.** Recommended: match only the user's own hostname lists and reverse names the user already looked
  up; never resolve names automatically for flagged addresses (it alerts the address's operator); say plainly that
  hostnames are mostly not knowable without a packet or DNS filter, which monitoring-only Dyno does not install.
- **D5. Firewall scope per OS in 1.1.0.** Recommended: Windows Defender Firewall and the macOS Application Firewall in the
  agent; `pf` and `nft` through the helper, in 1.1.0 as ADR 0027 D1a step 3 decides (corrected in revision 2: they are
  last in the landing order, not deferred). Until that step lands, the helper answers `NotSupported` and the page shows
  "needs the helper", with no wire change.
- **D6. Map rendering.** Recommended: plain SVG with build-time projection (accessible per country, CSP-clean, no runtime
  library). Fallback: ECharts `geo`.
- **D7. Update mode when a source is turned on.** Recommended: automatic (DROP daily, geo monthly) with "Update now" and a
  manual-only choice per source; turning a source on is the opt-in; **a 24 h floor for DROP that "Update now" also
  respects** (the reviewer's answer). Alternative: manual by default.
- **D8. Alarm default.** Recommended: `flaggedConnection` on when any list is on (the user asked for the check), off
  otherwise; ADR 0015's two network alarms stay off by default.
- **D9. Integrity.** Recommended: upstream only (DB-IP's database also through `Reader::verify`), over TLS with the host pinned, caps, format and count checks and a last
  good copy. Rejected: a Paddock mirror signed with the family minisign key (as `paddock-updater` checks): Spamhaus' retained database right and abuse.ch's terms §7.3 do not grant redistribution, and it would add a daily CI job (rule 50).
- **D10. Geo memory.** Recommended: read the two MMDB files into memory only while the map's data source is on (≈ 17 MB);
  switch to `mmap` if N4's measurement shows a resident-memory win worth `memmap2`'s unsafe surface (files are replaced by
  versioned name, so a mapped file is never overwritten on Windows).

## Consequences

- Dyno makes its first outbound requests of its own beyond the updater and ADR 0027's tool download, all behind
  per-source opt-in and the Offline switch; the agent gains `paddock-net` (feature `http`) and `maxminddb`.
- The flag is as good as the list: DROP is conservative and small; a clean result means "not on these lists", and the
  page says so.
- Hostname threats largely go undetected; that is stated, not hidden.
- The map is country-level and says why; city dots and per-connection lines are not built.
- ADR 0015's "no country database is bundled" still holds: nothing is bundled, the database is downloaded on request.

## Sources (read 2026-11-03)

- Spamhaus DROP: <https://www.spamhaus.org/blocklists/do-not-route-or-peer/> (re-read for revision 2: "Automated
  downloads must be at least one hour apart", "should not be downloaded from our website more than once per day",
  "Excessive downloads may result in your IP being firewalled", and the credit sentence quoted in §2.1), <https://www.spamhaus.org/drop/drop_v4.json>
  (metadata line: 1,757 records, 107,206 bytes), terms <https://www.spamhaus.org/drop/terms/> (§3.2 name use, §3.3 revocation, as cited by
  the independent review; copyright and database right retained).
- abuse.ch: <https://feodotracker.abuse.ch/blocklist/>, <https://feodotracker.abuse.ch/downloads/ipblocklist_recommended.txt>
  (1 entry, header dated 2026-03-04), terms <https://abuse.ch/terms-of-use/> (§2.1, §3.1, §3.4, §4.1, §7.3), URLhaus
  <https://urlhaus.abuse.ch/api/> (Auth-Key on every download).
- FireHOL: <https://raw.githubusercontent.com/firehol/blocklist-ipsets/master/firehol_level1.netset> (file date
  2026-11-02, 3,940 subnets), <https://iplists.firehol.org/> (GPL-2.0 for FireHOL; lists belong to their maintainers).
- DB-IP: <https://db-ip.com/db/download/ip-to-country-lite>, <https://db-ip.com/db/download/ip-to-asn-lite>,
  <https://db-ip.com/db/download/ip-to-city-lite> (November 2026 releases, sizes and record counts above).
- IPinfo Lite: <https://ipinfo.io/lite>. MaxMind GeoLite2 EULA: <https://www.maxmind.com/en/geolite2/eula>.
- Natural Earth terms: <https://www.naturalearthdata.com/about/terms-of-use/>.
- `maxminddb` crate: <https://docs.rs/crate/maxminddb> (ISC, 0.27; the LICENSE file itself not read). MMDB format:
  <https://maxmind.github.io/MaxMind-DB/> (not re-read; cited for the test writer).
- Natural Earth `ISO_A2` / `ISO_A2_EH` and the countries missing at 1:110m, and the abuse.ch terms date (2026-10-21):
  from the independent review of revision 1, not re-read here.
- Not verified here and marked in the text: IPinfo's download size, DShield's licence,
  `resolvectl show-cache` rights, `IWSCProductList` as a standard user, ECharts 6 `geo` projection options.
