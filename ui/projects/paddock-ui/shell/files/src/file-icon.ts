import type { PdIconName } from '@decryp7/paddock-ui/icons';

const TEXT = new Set(['log', 'txt', 'text', 'md', 'markdown', 'rtf', 'out', 'err']);
const CODE = new Set(['json', 'jsonl', 'ndjson', 'toml', 'yaml', 'yml', 'xml', 'csv', 'tsv', 'ini', 'cfg', 'conf', 'sql', 'html', 'css', 'js', 'ts', 'rs', 'py', 'cs', 'java', 'c', 'cpp', 'h', 'sh', 'ps1']);
const ARCHIVE = new Set(['zip', 'gz', 'tgz', 'bz2', 'xz', '7z', 'tar', 'rar', 'cab', 'zst']);

/**
 * The `pd:` glyph for a recent entry: a folder, a text or log page, a page with code (data and source files), an archive, or a
 * plain page. Decided by the name's last extension only (case-insensitive), never by reading the file. Pure.
 */
export function fileIcon(path: string, folder = false): PdIconName {
  if (folder) return 'folder';
  const name = path.split(/[\\/]/).pop() ?? '';
  const dot = name.lastIndexOf('.');
  const ext = dot > 0 ? name.slice(dot + 1).toLowerCase() : '';
  if (TEXT.has(ext)) return 'file-text';
  if (CODE.has(ext)) return 'file-code';
  if (ARCHIVE.has(ext)) return 'archive';
  return 'file';
}
