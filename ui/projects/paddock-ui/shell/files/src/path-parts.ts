/** The last segment of a path (either separator); the whole text when it has none. */
export function baseName(path: string): string {
  const trimmed = path.replace(/[\\/]+$/, '');
  return trimmed.split(/[\\/]/).pop() || path;
}

/** The folder a path is in, without the trailing separator ("/logs/a.log" is "/logs"); empty for a bare name, "/" for a root child. */
export function parentPath(path: string): string {
  const trimmed = path.replace(/[\\/]+$/, '');
  const at = Math.max(trimmed.lastIndexOf('/'), trimmed.lastIndexOf('\\'));
  if (at < 0) return '';
  return at === 0 ? trimmed.slice(0, 1) : trimmed.slice(0, at);
}
