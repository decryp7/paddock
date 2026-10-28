import type {
  SearchSeed,
  StartupEntry,
  StartupFilter,
  StartupOverview,
} from '../api.types';

// The mock side of right-click search, hash search, the startup filter . It mimics the engine's contract
// with plausible data; the engine (`dyno_core::weblinks`, `startup`) is the reference for behaviour.


/** The user name, computer name and home folder the mock pretends to run as (what the redaction removes). */
export const MOCK_IDENTITY = { user: 'alex', machine: 'alexs-macbook', home: '/Users/alex' };

/** A simple stand-in for the engine's redaction: home folder, names, IP addresses and GUIDs are removed. */
export function mockSearchSeed(text: string): SearchSeed {
  const removed: string[] = [];
  let out = text;
  const strip = (re: RegExp, one: string, many: string, to = ' '): void => {
    const n = (out.match(re) ?? []).length;
    if (n > 0) removed.push(`${n} ${n === 1 ? one : many}`);
    out = out.replace(re, to);
  };
  strip(new RegExp(`${MOCK_IDENTITY.home}/`, 'g'), 'home folder', 'home folders');
  strip(new RegExp(`\\b${MOCK_IDENTITY.user}\\b`, 'gi'), 'user name', 'user names');
  strip(new RegExp(`\\b${MOCK_IDENTITY.machine}\\b`, 'gi'), 'computer name', 'computer names');
  strip(/\b\d{1,3}(\.\d{1,3}){3}\b/g, 'IP address', 'IP addresses');
  strip(/\{?[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\}?/gi, 'GUID', 'GUIDs');
  return { text: out.split(/\s+/).filter(Boolean).join(' ').slice(0, 200), removed };
}

/** The address the engine would build (DuckDuckGo, query encoded). */
export function mockSearchUrl(query: string): string {
  const q = query.split(/\s+/).filter(Boolean).join(' ');
  if (!q) throw new Error('Type something to search for.');
  if (q.length > 200) throw new Error('The search text may have at most 200 characters.');
  return `https://duckduckgo.com/?q=${encodeURIComponent(q).replace(/%20/g, '+')}`;
}

/** A stable, plausible SHA-256 for a path (not a real hash). */
export function mockHash(path: string): string {
  let h = 0x811c9dc5;
  let out = '';
  for (let round = 0; round < 8; round++) {
    for (let i = 0; i < path.length; i++) h = Math.imul(h ^ (path.charCodeAt(i) + round), 0x01000193) >>> 0;
    out += h.toString(16).padStart(8, '0');
  }
  return out;
}

const norm = (p: string) => p.replace(/\\/g, '/').toLowerCase();

/** Whether the OS owns the entry (the Windows rules of the engine; the mock's startup list is a Windows one). */
export function mockIsOs(e: StartupEntry): boolean {
  const exe = norm(e.exe ?? '');
  return /^[a-z]:\/windows\//.test(exe) || /^\/(system\/|usr\/libexec\/)/.test(exe) || norm(e.name).startsWith('/microsoft/windows/') || e.publisher === 'Microsoft Windows';
}

/** The startup list after the filter, with counts that do not depend on it. */
export function mockStartupOverview(all: StartupEntry[], filter: StartupFilter): StartupOverview {
  const kind = (e: StartupEntry) => (mockIsOs(e) ? 'os' : e.publisher?.trim() ? 'signed' : 'other');
  const entries = all.filter((e) => !((kind(e) === 'os' && filter.hideOs) || (kind(e) === 'signed' && filter.hideSigned)));
  return {
    entries,
    total: all.length,
    osCount: all.filter((e) => kind(e) === 'os').length,
    signedCount: all.filter((e) => kind(e) === 'signed').length,
  };
}

