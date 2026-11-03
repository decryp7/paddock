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
