// `--scope apps/<app>` (C99, the release path, ER-31): what one app's release is judged on. The app's folder, the shared
// crates/, and the UI projects it ships (ui/projects/<app> and the shared ui/projects/paddock-ui, whose TypeScript layer 2 scans).

/** The scope as given, without a trailing slash, or null. */
export const normalizeScope = (scope) => (typeof scope === 'string' && scope ? scope.replace(/\/+$/, '') : null);

/** Whether a repository path (a file, or a waiver path with a `#key` suffix) is inside the scope. No scope: everything. */
export function inScope(scope, path) {
  const s = normalizeScope(scope);
  if (!s) return true;
  const f = String(path).split('#')[0];
  const app = s.split('/').pop();
  return f === s || f.startsWith(`${s}/`) || f.startsWith('crates/') || f.startsWith('ui/projects/paddock-ui/') || f.startsWith(`ui/projects/${app}/`);
}
