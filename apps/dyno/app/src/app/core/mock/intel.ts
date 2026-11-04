import { ISO_CODES } from '../../features/map/iso-countries';
import type {
  CommandError,
  CountryCount,
  DedupePreview,
  EditOutcome,
  FirewallView,
  GeoSummary,
  IntelEdit,
  IntelStatus,
  ListSource,
  ListStatus,
  NetSwitches,
  SubscriptionStatus,
} from '../api.types';

const MAX_LISTS = 32;
const MAX_NAME = 60;
const NOW_FALLBACK = 1_700_000_000_000;

const err = (code: CommandError['code'], message: string): CommandError => ({ code, message });

const FIRST_RUN_NOTE =
  'The map downloads two location databases from DB-IP once a month (download.db-ip.com) and asks Cloudflare, or ipify if Cloudflare does not answer, for your public IP so it can place you on the map; that service sees your public IP when Dyno asks for it, as any website would. The addresses your programs connect to never leave this computer: they are located here, in the downloaded databases. A list you subscribe to is fetched from the address you give, and its owner sees your public IP too.';
const TERMS =
  'The terms of a list are your responsibility: check that you may use it and how often it may be fetched. Dyno sends only the request for the file, with its name and version as the User-Agent, to the address you gave.';
const ATTRIBUTION = 'IP Geolocation by DB-IP (https://db-ip.com), licensed CC-BY-4.0.';

interface Stored {
  id: string;
  name: string;
  enabled: boolean;
  source: ListSource;
  text: string;
  counts: SubscriptionStatus;
}

const EMPTY: SubscriptionStatus = { lastFetchedMs: null, lastAttemptMs: null, entries: 0, duplicates: 0, covered: 0, skipped: 0, error: null, note: null };

/** The entry lines of a typed list: comments and blanks skipped, `entry # note` split. */
function lines(text: string): { entry: string; note: string | null }[] {
  return text
    .split('\n')
    .map((l) => l.trim())
    .filter((l) => l !== '' && !l.startsWith('#'))
    .map((l) => {
      const [entry, ...rest] = l.split('#');
      return { entry: (entry ?? '').trim().toLowerCase(), note: rest.join('#').trim() || null };
    })
    .filter((l) => l.entry !== '' && !/\s/.test(l.entry));
}

function ipv4(entry: string): { start: number; end: number } | null {
  const m = /^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})(?:\/(\d{1,2}))?$/.exec(entry);
  if (!m) return null;
  const o = [m[1], m[2], m[3], m[4]].map(Number);
  if (o.some((n) => n > 255)) return null;
  const prefix = m[5] === undefined ? 32 : Number(m[5]);
  if (prefix > 32) return null;
  const base = ((o[0]! << 24) | (o[1]! << 16) | (o[2]! << 8) | o[3]!) >>> 0;
  const host = prefix === 0 ? 0xffffffff : prefix === 32 ? 0 : (0xffffffff >>> prefix) >>> 0;
  const start = (base & ~host) >>> 0;
  return { start, end: (start | host) >>> 0 };
}

/** Whether an https address points to this computer or a private network (the agent refuses these when the list is added). */
export function isLocalAddress(url: string): boolean {
  const m = /^https:\/\/(\[[^\]]*\]|[^/:?#]+)/i.exec(url.trim());
  const host = (m?.[1] ?? '').replace(/^\[|\]$/g, '').toLowerCase().replace(/\.$/, '');
  if (host === 'localhost' || /\.(localhost|local|lan|internal)$/.test(host) || host.endsWith('.home.arpa')) return true;
  if (host === '::1' || host.startsWith('fe80:') || /^f[cd]/.test(host) && host.includes(':')) return true;
  const o = /^(\d+)\.(\d+)\.(\d+)\.(\d+)$/.exec(host)?.slice(1).map(Number);
  if (!o) return false;
  const [a, b] = [o[0]!, o[1]!];
  return a === 10 || a === 127 || a === 0 || (a === 172 && b >= 16 && b <= 31) || (a === 192 && b === 168) || (a === 169 && b === 254) || (a === 100 && b >= 64 && b <= 127);
}

/** What a typed list would lose to "Remove duplicates" (the same rules as the agent: exact repeats, wider ranges, parent names). */
export function dedupe(text: string): { kept: string[]; preview: DedupePreview } {
  const all = lines(text);
  const removed: [string, string][] = [];
  let duplicates = 0;
  let covered = 0;
  const rangeOf = (e: string) => ipv4(e);
  // Exact repeats first, so each removal is counted once and a repeat is a duplicate even when a wider entry also covers it.
  const firstSeen = new Set<string>();
  const unique: { entry: string; note: string | null }[] = [];
  for (const l of all) {
    if (firstSeen.has(l.entry)) {
      duplicates++;
      removed.push([l.entry, l.entry]);
    } else {
      firstSeen.add(l.entry);
      unique.push(l);
    }
  }
  const kept: { entry: string; note: string | null }[] = [];
  for (const l of unique) {
    const r = rangeOf(l.entry);
    const cover = unique.find((o) => {
      if (o.entry === l.entry) return false;
      const or = rangeOf(o.entry);
      if (r && or) return or.start <= r.start && or.end >= r.end;
      return !r && !or && l.entry.endsWith('.' + o.entry);
    });
    if (cover) {
      covered++;
      removed.push([l.entry, cover.entry]);
    } else kept.push(l);
  }
  return {
    kept: kept.map((l) => (l.note ? `${l.entry} # ${l.note}` : l.entry)),
    preview: { before: all.length, after: kept.length, duplicates, covered, examples: removed.slice(0, 20) },
  };
}

/** The lists, switches and map data of the browser mock: plausible, editable, and the same refusals the agent makes. */
export class MockIntel {
  private lists: Stored[] = [
    {
      id: 'list-1',
      name: 'Work blocklist',
      enabled: true,
      source: 'manual',
      text: '203.0.113.0/24 # scanner seen last week\n198.51.100.7\nevil.example # c2 host',
      counts: { ...EMPTY, entries: 3 },
    },
  ];
  private allow = '';
  private switches: NetSwitches = { locationDatabases: true, ownLocation: true, firstRunNoteSeen: false };
  private nextId = 2;

  /** `?geo=many`: a destination in every country (to see 250 arcs). */
  many = false;

  constructor(private readonly now: () => number = () => NOW_FALLBACK) {}

  status(): IntelStatus {
    const now = this.now();
    const lists: ListStatus[] = this.lists.map((l) => ({
      id: l.id,
      name: l.name,
      enabled: l.enabled,
      source: l.source,
      counts: l.source === 'manual' || 'file' in (l.source as object) ? this.countsOf(l) : l.counts,
      text: typeof l.source === 'object' && 'url' in l.source ? null : l.text,
      nextAllowedMs: typeof l.source === 'object' && 'url' in l.source && l.counts.lastAttemptMs && l.counts.lastAttemptMs + 3_600_000 > now ? l.counts.lastAttemptMs + 3_600_000 : null,
    }));
    return {
      lists,
      allow: this.allow,
      indexSegments: lists.reduce((n, l) => n + (l.enabled ? l.counts.entries : 0), 0),
      indexRanges: lists.reduce((n, l) => n + (l.enabled ? l.counts.entries : 0), 0),
      indexBytes: 1_024,
      indexError: null,
      geo: {
        switches: this.switches,
        databaseOn: true,
        dbDate: '2026-11-01',
        country: { ...EMPTY, lastFetchedMs: now - 86_400_000 },
        asn: { ...EMPTY, lastFetchedMs: now - 86_400_000 },
        homeSource: 'Your location: from your public IP (via Cloudflare)',
        attribution: ATTRIBUTION,
      },
      firstRunNote: FIRST_RUN_NOTE,
      termsNotice: TERMS,
    };
  }

  private countsOf(l: Stored): SubscriptionStatus {
    // As typed: duplicates are counted only once "Remove duplicates" was chosen (the preview says what would go).
    const total = lines(l.text).length;
    const skipped = Math.max(0, l.text.split('\n').filter((x) => x.trim() !== '' && !x.trim().startsWith('#')).length - total);
    return { ...EMPTY, entries: total, skipped };
  }

  private find(id: string): Stored {
    const l = this.lists.find((x) => x.id === id);
    if (!l) throw err('notFound', 'That list no longer exists.');
    return l;
  }

  private checkName(name: string, except?: string): string {
    const n = name.trim();
    if (n === '' || n.length > MAX_NAME) throw err('badRequest', `A list name must be 1 to ${MAX_NAME} characters, without control characters.`);
    if (this.lists.some((l) => l.id !== except && l.name.toLowerCase() === n.toLowerCase())) throw err('badRequest', 'Another list already has that name.');
    return n;
  }

  edit(edit: IntelEdit): EditOutcome {
    if ('addList' in edit) {
      if (this.lists.length >= MAX_LISTS) throw err('badRequest', `You can have at most ${MAX_LISTS} lists. Remove or merge a list first.`);
      const name = this.checkName(edit.addList.name);
      const s = edit.addList.source;
      if ('url' in s && !/^https:\/\/[^\s@/]+/.test(s.url.url.trim())) throw err('badRequest', 'The address must start with https://. Plain http is not used.');
      if ('url' in s && isLocalAddress(s.url.url)) throw err('badRequest', 'Dyno fetches lists only from public addresses: this one points to this computer or a private network.');
      const id = `list-${this.nextId++}`;
      if ('url' in s) {
        const hours = Math.min(720, Math.max(1, s.url.everyHours ?? 24));
        const now = this.now();
        this.lists.push({
          id,
          name,
          enabled: true,
          source: { url: { url: s.url.url.trim(), schedule: { automatic: { every: hours } } } },
          text: '',
          counts: { ...EMPTY, entries: 2, lastFetchedMs: now, lastAttemptMs: now },
        });
      } else if ('file' in s) {
        this.lists.push({ id, name, enabled: true, source: { file: { name: s.file.fileName } }, text: s.file.text, counts: { ...EMPTY } });
      } else {
        this.lists.push({ id, name, enabled: true, source: 'manual', text: s.manual.text, counts: { ...EMPTY } });
      }
      return { added: { id } };
    }
    if ('removeList' in edit) {
      this.find(edit.removeList.id);
      this.lists = this.lists.filter((l) => l.id !== edit.removeList.id);
      return { removed: { id: edit.removeList.id } };
    }
    if ('renameList' in edit) {
      const name = this.checkName(edit.renameList.name, edit.renameList.id);
      this.find(edit.renameList.id).name = name;
      return 'done';
    }
    if ('setEnabled' in edit) {
      this.find(edit.setEnabled.id).enabled = edit.setEnabled.enabled;
      return 'done';
    }
    if ('setText' in edit) {
      const l = this.find(edit.setText.id);
      if (typeof l.source === 'object' && 'url' in l.source) throw err('badRequest', 'That cannot be done to this kind of list.');
      l.text = edit.setText.text;
      return 'done';
    }
    if ('setSchedule' in edit) {
      const l = this.find(edit.setSchedule.id);
      if (typeof l.source !== 'object' || !('url' in l.source)) throw err('badRequest', 'That cannot be done to this kind of list.');
      l.source = { url: { url: l.source.url.url, schedule: edit.setSchedule.everyHours === null ? 'manual' : { automatic: { every: edit.setSchedule.everyHours } } } };
      return 'done';
    }
    if ('dedupePreview' in edit || 'dedupeApply' in edit) {
      const id = 'dedupePreview' in edit ? edit.dedupePreview.id : edit.dedupeApply.id;
      const l = this.find(id);
      if (typeof l.source === 'object' && 'url' in l.source) throw err('badRequest', 'That cannot be done to this kind of list.');
      const d = dedupe(l.text);
      if ('dedupePreview' in edit) return { preview: d.preview };
      l.text = d.kept.join('\n');
      return 'done';
    }
    if ('setAllow' in edit) {
      this.allow = edit.setAllow.text;
      return 'done';
    }
    this.switches = edit.setSwitches.switches;
    return 'done';
  }

  geo(): GeoSummary {
    const c = (code: string, name: string, connections: number, programs: number, hosts: number, flagged = 0, cdn = false, top: string[] = []): CountryCount => ({ code, name, connections, programs, hosts, flagged, includesContentNetworks: cdn, topPrograms: top });
    if (this.many) {
      const names = new Intl.DisplayNames(['en'], { type: 'region' });
      const countries = ISO_CODES.filter((code) => code !== 'SG').map((code, i) => c(code, names.of(code) ?? code, (i * 7) % 230 + 1, 1 + (i % 5), 1 + (i % 9), i % 40 === 0 ? 1 : 0, false, ['Google Chrome']));
      return { countries, local: [], unknown: 0, dbDate: '2026-11-01', truncated: 0, databaseOn: true, homeCountry: 'SG', homeSource: 'Your location: from your public IP (via Cloudflare)' };
    }
    return {
      countries: [
        c('US', 'United States', 54, 6, 21, 1, true, ['Google Chrome', 'Slack', 'Spotify']),
        c('IE', 'Ireland', 18, 3, 6, 0, false, ['Code Helper', 'Google Chrome']),
        c('DE', 'Germany', 12, 4, 5, 0, false, ['Mail', 'cargo', 'node']),
        c('JP', 'Japan', 4, 1, 2),
        c('SG', 'Singapore', 3, 2, 2),
        c('BR', 'Brazil', 1, 1, 1),
      ],
      local: [
        { class: 'loopback', connections: 14, programs: 5 },
        { class: 'private', connections: 6, programs: 3 },
      ],
      unknown: 2,
      dbDate: '2026-11-01',
      truncated: 0,
      databaseOn: true,
      homeCountry: 'SG',
      homeSource: 'Your location: from your public IP (via Cloudflare)',
    };
  }

  firewall(): FirewallView {
    return {
      os: 'macos',
      sources: [
        {
          source: 'applicationFirewall',
          states: [{ profile: 'all', enabled: true }],
          switches: [
            ['blockAll', false],
            ['stealth', true],
          ],
          defaultIn: null,
          defaultOut: null,
          ruleCount: 3,
          rules: [
            { name: 'Example Sync helper (all networks)', direction: 'out', action: 'allow', enabled: true, program: 'C:\\Program Files\\WindowsApps\\ExampleVendor.ExampleSync_12.4.2026.0_x64__8wekyb3d8bbwe\\Helper\\ExampleSyncBackgroundService.exe', ports: '443', addresses: '192.168.0.0/16, 10.0.0.0/8, 172.16.0.0/12, fe80::/10, 2001:db8:1234:5678::/64, 2001:db8:abcd::/48', profile: 'all', source: 'applicationFirewall' },
            { name: 'Safari', direction: 'in', action: 'allow', enabled: true, program: '/Applications/Safari.app', ports: null, addresses: null, profile: 'all', source: 'applicationFirewall' },
            { name: 'Example Sync', direction: 'in', action: 'block', enabled: true, program: '/Applications/Example Sync.app', ports: null, addresses: null, profile: 'all', source: 'applicationFirewall' },
          ],
          truncated: 0,
          rulesHidden: false,
          note: null,
          via: 'local',
        },
        {
          source: 'pf',
          states: [],
          switches: [],
          defaultIn: null,
          defaultOut: null,
          ruleCount: 0,
          rules: [],
          truncated: 0,
          rulesHidden: true,
          note: 'The rules of the pf firewall can be read only by an administrator.',
          via: { needsHelper: { reason: 'The rules of the pf firewall can be read only by an administrator. Turn on Administrator readings in Settings so the helper can read this.', next: { openSettings: 'helper' } } },
        },
      ],
    };
  }
}
