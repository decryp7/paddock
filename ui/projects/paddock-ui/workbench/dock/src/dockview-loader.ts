/**
 * dockview is loaded on demand: only a window that shows the dock needs it (a pop-out panel window does not, which
 * keeps about 390 KB of script out of every pop-out).
 */
type Dockview = typeof import('dockview');

const importDockview = (): Promise<Dockview> => import('dockview');

let loaded: Dockview | null = null;
let loading: Promise<Dockview> | null = null;
let importer: () => Promise<Dockview> = importDockview;

export function loadDockview(): Promise<Dockview> {
  loading ??= importer().then((m) => (loaded = m));
  return loading;
}

/** The module once `loadDockview` resolved, else null. */
export function dockview(): Dockview | null {
  return loaded;
}

/**
 * Forgets the loaded module, so a spec can start from "dockview is not loaded" whatever ran before it in the same worker.
 * `load` stands in for the import until the next call (a spec holds it to see the dock before dockview is there; spec
 * files share a worker, where the real import is often cached and resolves before the spec can look).
 */
export function forgetDockviewForTest(load: () => Promise<Dockview> = importDockview): void {
  loaded = null;
  loading = null;
  importer = load;
}
