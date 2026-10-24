import type { CategoryClearResult, ClearRequest, ItemView, SizeNode, Skipped, StorageCategory, StorageClearResult, StorageEvent, StorageKind, StorageTree, StorageUsage } from '../generated/storage';

// Payloads from the engine are parsed once, at this edge: what is not the expected shape is dropped, and what passes is rebuilt
// from known fields only (an unknown extra field never travels on).

function isObject(v: unknown): v is Record<string, unknown> {
  return typeof v === 'object' && v !== null && !Array.isArray(v);
}

const isCount = (v: unknown): v is number => typeof v === 'number' && Number.isFinite(v) && v >= 0;
const optText = (v: unknown): v is string | undefined | null => v === undefined || v === null || typeof v === 'string';

function parseItem(v: unknown): ItemView | null {
  if (!isObject(v) || typeof v['name'] !== 'string' || !isCount(v['bytes']) || !optText(v['protected'])) return null;
  const item: ItemView = { name: v['name'], bytes: v['bytes'] };
  if (typeof v['protected'] === 'string') item.protected = v['protected'];
  return item;
}

function parseCategory(v: unknown): StorageCategory | null {
  if (!isObject(v)) return null;
  const { id, label, kind, exists, bytes, files, clearableBytes, clearableItems, capped, clearable, items } = v;
  if (typeof id !== 'string' || typeof label !== 'string' || (kind !== 'cache' && kind !== 'data')) return null;
  if (typeof exists !== 'boolean' || typeof capped !== 'boolean' || typeof clearable !== 'boolean') return null;
  if (!isCount(bytes) || !isCount(files) || !isCount(clearableBytes) || !isCount(clearableItems) || !Array.isArray(items)) return null;
  if (!optText(v['path']) || !optText(v['note'])) return null;
  const category: StorageCategory = {
    id,
    label,
    kind: kind as StorageKind,
    exists,
    bytes,
    files,
    clearableBytes,
    clearableItems,
    capped,
    clearable,
    items: items.map(parseItem).filter((i): i is ItemView => i !== null),
  };
  if (typeof v['path'] === 'string') category.path = v['path'];
  if (typeof v['note'] === 'string') category.note = v['note'];
  return category;
}

/** A `StorageUsage` from `storage_last` or the event, or null when the payload is not one. */
export function parseStorageUsage(v: unknown): StorageUsage | null {
  if (!isObject(v) || !Array.isArray(v['categories']) || !isCount(v['totalBytes']) || typeof v['capped'] !== 'boolean') return null;
  const categories = v['categories'].map(parseCategory);
  if (categories.some((c) => c === null)) return null;
  return { categories: categories as StorageCategory[], totalBytes: v['totalBytes'], capped: v['capped'] };
}

/** A size node (and everything below it, to the depth the engine keeps), or null when it is not one. Bounded in depth here too. */
function parseNode(v: unknown, depth = 0): SizeNode | null {
  if (depth > 8 || !isObject(v) || typeof v['name'] !== 'string' || !isCount(v['bytes']) || !isCount(v['folded']) || !Array.isArray(v['children'])) return null;
  const children = v['children'].map((c) => parseNode(c, depth + 1));
  if (children.some((c) => c === null)) return null;
  return { name: v['name'], bytes: v['bytes'], folded: v['folded'], children: children as SizeNode[] };
}

/** A `StorageTree` from the event, or null when the payload is not one. */
export function parseStorageTree(v: unknown): StorageTree | null {
  if (!isObject(v) || typeof v['id'] !== 'string' || typeof v['capped'] !== 'boolean') return null;
  const root = parseNode(v['root']);
  return root ? { id: v['id'], root, capped: v['capped'] } : null;
}

function parseSkipped(v: unknown): Skipped | null {
  return isObject(v) && typeof v['path'] === 'string' && typeof v['reason'] === 'string' ? { path: v['path'], reason: v['reason'] } : null;
}

function parseCategoryResult(v: unknown): CategoryClearResult | null {
  if (!isObject(v) || typeof v['id'] !== 'string' || !isCount(v['freedBytes']) || !isCount(v['removedFiles']) || !isCount(v['skippedCount']) || !Array.isArray(v['skipped']) || !optText(v['error'])) return null;
  const result: CategoryClearResult = {
    id: v['id'],
    freedBytes: v['freedBytes'],
    removedFiles: v['removedFiles'],
    skipped: v['skipped'].map(parseSkipped).filter((s): s is Skipped => s !== null),
    skippedCount: v['skippedCount'],
  };
  if (typeof v['error'] === 'string') result.error = v['error'];
  return result;
}

function parseClearResult(v: unknown): StorageClearResult | null {
  if (!isObject(v) || !isCount(v['freedBytes']) || typeof v['cancelled'] !== 'boolean' || !Array.isArray(v['categories'])) return null;
  const categories = v['categories'].map(parseCategoryResult);
  if (categories.some((c) => c === null)) return null;
  return { freedBytes: v['freedBytes'], categories: categories as CategoryClearResult[], cancelled: v['cancelled'] };
}

/** The outcome of a job from `paddock://storage`, or null when the payload is not one. */
export function parseStorageEvent(v: unknown): StorageEvent | null {
  if (!isObject(v)) return null;
  switch (v['state']) {
    case 'usage': {
      const usage = parseStorageUsage(v['usage']);
      return usage ? { state: 'usage', usage } : null;
    }
    case 'cleared': {
      const result = parseClearResult(v['result']);
      return result ? { state: 'cleared', result } : null;
    }
    case 'tree': {
      const tree = parseStorageTree(v['tree']);
      return tree ? { state: 'tree', tree } : null;
    }
    case 'cancelled':
      return { state: 'cancelled' };
    case 'failed':
      return typeof v['message'] === 'string' ? { state: 'failed', message: v['message'] } : null;
    default:
      return null;
  }
}

/** The request as the engine takes it: known fields only. */
export function clearRequest(choices: readonly { id: string; confirmed: boolean }[]): ClearRequest {
  return { categories: choices.map((c) => ({ id: c.id, confirmed: c.confirmed })) };
}
