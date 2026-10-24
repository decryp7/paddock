import { Injectable } from '@angular/core';

/*
 * The demo's stand-in for an engine (the browser build has no Rust): a small repository with folders, files, symbols and
 * problems. A real app answers these through its engine commands (an Explorer provider calls `index_children`, the palette
 * calls `names_search`); here the matching is a mock, never a reference for behaviour (AGENTS: the mock backend).
 */

export interface RepoFile {
  readonly path: string;
  readonly text: string;
  readonly symbols: readonly { readonly name: string; readonly kind: 'fn' | 'struct' | 'mod'; readonly line: number }[];
}

export interface RepoEntry {
  readonly path: string;
  readonly name: string;
  readonly folder: boolean;
}

export interface Problem {
  readonly path: string;
  readonly line: number;
  readonly level: 'error' | 'warning';
  readonly message: string;
}

const FILES: readonly RepoFile[] = [
  {
    path: 'src/main.rs',
    text: 'use crate::server::Server;\n\nfn main() {\n    let server = Server::bind("127.0.0.1:8080");\n    server.run();\n}\n',
    symbols: [{ name: 'main', kind: 'fn', line: 3 }],
  },
  {
    path: 'src/server/mod.rs',
    text: 'pub struct Server {\n    addr: String,\n}\n\nimpl Server {\n    pub fn bind(addr: &str) -> Self {\n        Server { addr: addr.into() }\n    }\n\n    pub fn run(&self) {\n        println!("listening on {}", self.addr);\n    }\n}\n',
    symbols: [
      { name: 'Server', kind: 'struct', line: 1 },
      { name: 'bind', kind: 'fn', line: 6 },
      { name: 'run', kind: 'fn', line: 10 },
    ],
  },
  {
    path: 'src/server/request_handler.rs',
    text: 'pub fn handle_request(line: &str) -> Option<String> {\n    if line.is_empty() { return None; }\n    Some(line.to_uppercase())\n}\n',
    symbols: [{ name: 'handle_request', kind: 'fn', line: 1 }],
  },
  { path: 'docs/architecture-overview-of-the-request-pipeline.md', text: '# Architecture\n\nRequests enter through `Server::run` and reach `handle_request`.\n', symbols: [] },
  { path: 'README.md', text: '# Demo repository\n\nA small project to show the Paddock workbench.\n', symbols: [] },
  ...Array.from({ length: 240 }, (_, i) => ({ path: `generated/fixtures/case-${String(i + 1).padStart(3, '0')}.json`, text: `{ "case": ${i + 1} }\n`, symbols: [] })),
];

const PROBLEMS: readonly Problem[] = [
  { path: 'src/server/mod.rs', line: 11, level: 'warning', message: 'println! in a server loop; use the logger' },
  { path: 'src/main.rs', line: 4, level: 'error', message: 'the address should come from the settings' },
  { path: 'src/server/request_handler.rs', line: 2, level: 'warning', message: 'empty lines are dropped silently' },
];

/** A page of a list. */
export interface Page<T> {
  readonly items: readonly T[];
  readonly next?: string;
  readonly total: number;
}

@Injectable({ providedIn: 'root' })
export class DemoRepo {
  readonly files = FILES;
  readonly problems = PROBLEMS;

  /** The entries directly under `folder` ('' is the root): folders first, then files, by name; paged. */
  children(folder: string, cursor: string | undefined, limit: number): Page<RepoEntry> {
    const prefix = folder ? `${folder}/` : '';
    const seen = new Map<string, RepoEntry>();
    for (const f of FILES) {
      if (!f.path.startsWith(prefix)) continue;
      const rest = f.path.slice(prefix.length);
      const slash = rest.indexOf('/');
      const name = slash < 0 ? rest : rest.slice(0, slash);
      const path = prefix + name;
      if (!seen.has(path)) seen.set(path, { path, name, folder: slash >= 0 });
    }
    const all = [...seen.values()].sort((a, b) => Number(b.folder) - Number(a.folder) || a.name.localeCompare(b.name));
    const start = cursor ? Number(cursor) : 0;
    const items = all.slice(start, start + limit);
    const end = start + items.length;
    return { items, total: all.length, ...(end < all.length ? { next: String(end) } : {}) };
  }

  file(path: string): RepoFile | undefined {
    return FILES.find((f) => f.path === path);
  }

  /** Files whose name contains every typed word (a mock of the engine's names search). */
  findFiles(query: string, limit: number): RepoFile[] {
    const words = query.toLowerCase().split(/[\s/._-]+/).filter(Boolean);
    return FILES.filter((f) => words.every((w) => f.path.toLowerCase().includes(w))).slice(0, limit);
  }

  /** Symbols matching the query, with the file they are in. */
  findSymbols(query: string, limit: number): { file: RepoFile; name: string; kind: string; line: number }[] {
    const q = query.toLowerCase().replace(/[\s_-]+/g, '');
    return FILES.flatMap((file) => file.symbols.map((s) => ({ file, ...s })))
      .filter((s) => s.name.toLowerCase().replace(/_/g, '').includes(q))
      .slice(0, limit);
  }

  /** Lines containing the text. */
  searchText(query: string, limit: number): { path: string; line: number; text: string }[] {
    const q = query.toLowerCase();
    if (!q) return [];
    const out: { path: string; line: number; text: string }[] = [];
    for (const f of FILES)
      f.text.split('\n').forEach((text, i) => {
        if (out.length < limit && text.toLowerCase().includes(q)) out.push({ path: f.path, line: i + 1, text: text.trim() });
      });
    return out;
  }
}
