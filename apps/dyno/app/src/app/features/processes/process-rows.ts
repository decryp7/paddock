import type { AppGroup, ProcessRef, ProcessSample } from '../../core/api.types';

export type ViewMode = 'groups' | 'tree' | 'flat';
export type SortKey = 'name' | 'cpu' | 'memory' | 'disk' | 'user' | 'pid';
export interface Sort {
  key: SortKey;
  dir: 'asc' | 'desc';
}

/** One line of the task manager: an app group, or a process (nested under its group or parent in those views). */
export interface Row {
  key: string;
  kind: 'group' | 'process';
  depth: number;
  name: string;
  /** Processes in a group (1 for a process). */
  count: number;
  cpu: number;
  memory: number;
  disk: number;
  user: string | null;
  pid: number | null;
  ref: ProcessRef | null;
  groupId: string | null;
  expandable: boolean;
  expanded: boolean;
  suspended: boolean;
}

const disk = (p: ProcessSample) => p.diskReadBps + p.diskWriteBps;

function value(r: Pick<Row, SortKey>, key: SortKey): string | number {
  switch (key) {
    case 'name':
      return r.name.toLowerCase();
    case 'user':
      return (r.user ?? '').toLowerCase();
    default:
      return r[key] ?? 0;
  }
}

/** Orders rows by `sort`; ties by name so the order is stable while values wobble. */
export function compareRows(a: Row, b: Row, sort: Sort): number {
  const x = value(a, sort.key);
  const y = value(b, sort.key);
  const c = x < y ? -1 : x > y ? 1 : 0;
  return (sort.dir === 'asc' ? c : -c) || a.name.localeCompare(b.name) || (a.pid ?? 0) - (b.pid ?? 0);
}

function processRow(p: ProcessSample, depth: number, expandable = false, expanded = false): Row {
  return {
    key: `p${p.pid}`,
    kind: 'process',
    depth,
    name: p.name,
    count: 1,
    cpu: p.cpu,
    memory: p.memory,
    disk: disk(p),
    user: p.user,
    pid: p.pid,
    ref: { pid: p.pid, started: p.started },
    groupId: null,
    expandable,
    expanded,
    suspended: p.state === 'suspended',
  };
}

/** Whether a process matches the search text (name, user, pid, executable). */
export function matches(p: ProcessSample, query: string): boolean {
  const q = query.trim().toLowerCase();
  if (!q) return true;
  return p.name.toLowerCase().includes(q) || (p.user ?? '').toLowerCase().includes(q) || String(p.pid) === q || (p.exe ?? '').toLowerCase().includes(q);
}

export interface RowInput {
  mode: ViewMode;
  processes: readonly ProcessSample[];
  groups: readonly AppGroup[];
  sort: Sort;
  query: string;
  /** Keys of the group/process rows the user opened. */
  expanded: ReadonlySet<string>;
}

/** The visible rows for a view: grouped by app, as a parent/child tree, or flat. Pure; the view only renders it. */
export function buildRows(input: RowInput): Row[] {
  const { mode, processes, groups, sort, query, expanded } = input;
  const searching = query.trim() !== '';
  const kept = processes.filter((p) => matches(p, query));
  if (mode === 'flat') return kept.map((p) => processRow(p, 0)).sort((a, b) => compareRows(a, b, sort));

  if (mode === 'groups') {
    const byPid = new Map(kept.map((p) => [p.pid, p]));
    const rows: Row[] = [];
    const out: { group: Row; children: Row[] }[] = [];
    for (const g of groups) {
      const members = g.pids.map((pid) => byPid.get(pid)).filter((p): p is ProcessSample => !!p);
      if (members.length === 0) continue;
      const open = searching || expanded.has(`g${g.id}`);
      const group: Row = {
        key: `g${g.id}`,
        kind: 'group',
        depth: 0,
        name: g.name,
        count: g.pids.length,
        cpu: g.cpu,
        memory: g.memory,
        disk: g.diskBps,
        user: members.length === 1 ? members[0]!.user : null,
        pid: null,
        ref: null,
        groupId: g.id,
        expandable: g.pids.length > 1,
        expanded: open && g.pids.length > 1,
        suspended: members.every((m) => m.state === 'suspended'),
      };
      const children = group.expanded ? members.map((m) => processRow(m, 1)).sort((a, b) => compareRows(a, b, sort)) : [];
      out.push({ group, children });
    }
    out.sort((a, b) => compareRows(a.group, b.group, sort));
    for (const { group, children } of out) rows.push(group, ...children);
    return rows;
  }

  // Tree: a process under its parent; a process whose parent is gone is a root.
  const present = new Set(kept.map((p) => p.pid));
  const children = new Map<number, ProcessSample[]>();
  const roots: ProcessSample[] = [];
  for (const p of kept) {
    const parent = p.parent;
    // A match whose parent did not match (or is gone) shows as a root, so nothing hides.
    if (parent !== null && present.has(parent)) {
      const list = children.get(parent) ?? [];
      list.push(p);
      children.set(parent, list);
    } else roots.push(p);
  }
  const rows: Row[] = [];
  const walk = (p: ProcessSample, depth: number, seen: Set<number>): void => {
    if (seen.has(p.pid)) return;
    seen.add(p.pid);
    const kids = children.get(p.pid) ?? [];
    const open = searching || expanded.has(`p${p.pid}`) || (depth === 0 && p.parent === null && !expanded.has(`-p${p.pid}`));
    rows.push(processRow(p, depth, kids.length > 0, open && kids.length > 0));
    if (open) {
      const sorted = kids.map((k) => processRow(k, depth + 1)).sort((a, b) => compareRows(a, b, sort));
      for (const k of sorted) walk(kept.find((x) => x.pid === k.pid)!, depth + 1, seen);
    }
  };
  const rootRows = roots.map((r) => processRow(r, 0)).sort((a, b) => compareRows(a, b, sort));
  for (const r of rootRows) walk(kept.find((x) => x.pid === r.pid)!, 0, new Set());
  return rows;
}
