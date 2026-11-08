# 31. Amendments (network threat lists, world map and firewall view)

Amends ADR 0031 (`apps/dyno/docs/adr/0031-network-threat-lists-world-map-and-firewall-view.md` on `docs/dyno-network-features`).

## Amendment 1 (2026-11-03, the user): no built-in threat lists; user-supplied lists and URLs only

The user first removed the bad-IP feature from 1.1.0 and the same day restored it with these changes. Where this amendment
disagrees with ADR 0031 §2 and decisions D1 and D3, this amendment wins.

- **Dyno ships with no list.** No Spamhaus DROP, no Feodo, no FireHOL entry in a catalogue. Nothing is downloaded for the check
  until the user adds a list. The DB-IP geo databases (§3) are unchanged.
- **Several named lists** (at most 32, at most 1,000,000 entries in all). Each has a name, an on/off switch and its own source:
  typed entries, a text file the user imported, or a URL subscription. A match names every list that matched and shows the entry's
  note as the reason.
- **URL subscriptions** are fetched only through `paddock-net` `Egress` (opt-in per subscription, refused in Offline mode), on a
  schedule the user chooses (at most hourly, daily by default), as plain text (one address, CIDR range or host name per line, `#`
  comments), with a size and a line limit. Each shows last fetched, entry count and last error. The list's terms are the
  user's responsibility, and the add dialog says so.
- **The address allow list is separate** and overrides every list. A host name allow cancels only the user's own host name
  entries (ADR 0031 §2.2 spoofing rule is unchanged). No automatic reverse lookups (D4 unchanged).
- **Private and special-purpose addresses are never matched or located** (the §2.2 list, with the NAT64 unwrap) - unchanged.
- **Deduplication.** Within a list, exact duplicates and entries covered by a wider range or a parent host name are dropped on
  import and fetch, with the counts kept per list. Across lists each list keeps its entries (provenance) but the match index
  stores a range once with the set of lists that contain it. A manual list has a "Remove duplicates" action with a preview.
- **Storage.** User-made lists live in the agent's data folder (`intel-lists.json`, versioned); the fetched text of subscriptions
  and the geo databases live in the cache folder of ADR 0029 (`Dirs::intel_cache`). The index is rebuilt in memory at start;
  a packed index is persisted only if the rebuild measures over about 200 ms (`docs/performance.md`).
- **D3 (licence scope for runtime data)** now concerns the DB-IP CC-BY-4.0 data only; user lists carry no licence check.

## Amendment 2 (2026-11-03, the user): the map's own location, lines to each destination, network features on by default

These replace the "opt-in dialog", "licence checkbox" and "Offline mode" parts of ADR 0031 §2.3, §3.1 and D7, and the "no address
leaves the computer" sentence of §5 as far as the map's own address is concerned.

- **Offline mode does not exist** (removed family-wide by the user on 2026-10-31). Nothing here is gated by it. Each network
  feature has its **own Settings switch, on by default**; a first-run note names what is fetched and from where.
- **The location databases** (DB-IP IP to Country Lite and IP to ASN Lite, CC-BY-4.0) download without a dialog when the map is
  first opened, monthly, through `paddock-net` `Egress` pinned to `download.db-ip.com`, with a 24 hour floor between attempts
  (also for "Update now"). The DB-IP attribution stays visible under the map and in About.
- **The user's own location** is found by asking a public-IP echo service for the address, and only that: Cloudflare's trace page
  (`https://1.1.1.1/cdn-cgi/trace`, the `ip=` line), with `https://api.ipify.org` as the fallback. Both hosts are pinned,
  answers are capped at 4 KiB, no redirect is followed, the text is parsed strictly into an address that must be public. The
  address is then located **locally** in the DB-IP database; no third-party geolocation service is used. It runs when the map
  opens, at most once an hour (and again on a network change), and the result lives in memory only. The page says "Your
  location: from your public IP (via Cloudflare)" or, when the lookup fails or is switched off, "from your region setting" (the OS
  region and time zone, a country centroid). A manual location or "hide my location" in Settings always wins. Logs name the
  service and the duration, never the address. The echo service sees the user's address, as any site does; the first-run note
  says so.
- **Lines on the map** (map work package, UI): one great-circle arc from the user's location to each destination country (the
  country centroid, as the database is country-level), aggregated per destination, at most about 250 arcs (the rest are
  aggregated), drawn as SVG paths precomputed with the build-time projection. Width or opacity by connection count, never
  colour alone; hover and focus show the country, the connection count and the top programs; no animated arcs under reduced
  motion; the table view lists the same data.
- **User URL subscriptions** still start when the user adds them (Amendment 1).
- **Licence scope (D3).** The runtime-data scope in `crates/paddock-deps/licences.toml` (ADR 0028) could not be added: that file
  and crate do not exist yet (ADR 0028's DP1 is not merged). It is added by DP1's integrating agent with `CC0-1.0`, public
  domain and `CC-BY-4.0` for unmodified local use with attribution shown; until then the DB-IP source is the only data covered.

## Amendment 3 (2026-11-03): what was built for the map, the lists page and the firewall page

- **Where "you" are.** The public-address lookup (Amendment 2) is tried first; when it is off or fails, the **OS region setting** is read
  (`LC_ALL`/`LC_MESSAGES`/`LANG`, macOS `AppleLocale`, Windows' Geo registry value) and its country is used, and the page says "from
  your region setting". When the OS names no country the page says "unknown (no region setting found)" and draws no arcs. A bare
  language (`en`) never yields a country.
- **The map** is bundled Natural Earth 1:110m geometry (public domain) in the Equal Earth projection, generated by
  `scripts/build-world-map.mjs` (source SHA-256 in the output, 54 KB), with a dot for each of the 75 ISO codes too small for it. A spec
  checks that every ISO code has a shape or a dot. Arcs are great circles to the destination country's centre, at most 250 (the rest
  are in the table), cut where they cross the map edge; width by connection count, flagged countries carry a hatched marker and the word
  "flagged". Enter or click on a country opens it in the table (instead of filtering the Connections table: that filter needs a country
  field on the connection rows, which is later work).
- **The firewall page** consumes `FirewallReader`s from `feat/dyno-helper-requests` (merged here): the Windows COM reader and the macOS
  Application Firewall reader run in the agent; `pf` and `nft` sources say "These rules need the helper". The page is read-only and
  reads on open and on Refresh; a read is reused for 5 seconds. It is not a job (the reads are short); that is open.
- **Settings** has "Network ▸ Online data": the two switches, the first-run note and the DB-IP credit.

## Amendment 4 (2026-11-03): the name

The user removed the built-in "bad IP" feature and chose "my lists and my own URLs", so the page and the copy say **IP lists**, not
"bad-IP lists". A match is still called **flagged**.

## Real-engine run (2026-11-03)

The IP lists, the map and the firewall were driven against a **real running `dyno-agent`** (not the mock) in a clone of the Tart VM
`vmgui-base-tahoe-4aaa751f` (macOS 26, headless, deleted afterwards), by `crates/dyno-agent/examples/real_run.rs` over the agent's real
IPC. Both network switches were turned off first, so nothing reached a list or database publisher; a tiny location database for the
test host's /24 was staged in the cache folder. What differed from the mock, and what was done:

| Found | Fix |
| --- | --- |
| Before any lookup the map's "you" was "not found yet" with no country, so no arcs were drawn even though the region setting names one. | `IntelCentre::home()` reads the OS region setting when no lookup has run (VM: `AppleLocale` `en_US` gives US). |
| A subscription to `https://127.0.0.1:8099/list.txt` was accepted as an address and then failed with "kept failing (the request is invalid (127.0.0.1 was not contacted ...)); check the network and run it again to resume". | The address is refused when added ("Dyno fetches lists only from public addresses: this one points to this computer or a private network"); a name that resolves to a private address gets the same plain message at fetch time. A local test URL cannot be served to the real engine by design (https only, public addresses only, ADR 0031 section 2.3). |
| `http://` is refused at the door with a clear message (as in the mock). | none needed |
| The Application Firewall read took 120 ms on the first run and 253 ms on the second in the VM (about 0 ms per command on the Mac itself). | The read runs on its own thread; a request waits at most 2 s and otherwise answers "Still reading", with the result served to the next request. |
| The connection to the test host was placed in "Testland" by the real database lookup, was flagged by the list holding its /24, and the Connections request filtered to that country returned it. | matches the mock |
