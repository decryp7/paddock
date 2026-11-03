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
