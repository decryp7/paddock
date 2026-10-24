import type { SizeNode, StorageTree } from '../generated/storage';

/** What a tree line is: a folder (it has items below it), a file, or a fold of the smallest items. */
export type TreeRowKind = 'folder' | 'file' | 'fold';

/** One line of the tree table: an item inside a top-level folder, with its size and its share of the whole category. */
export interface TreeRow {
  readonly key: string;
  /** The top-level folder (or "Files here", "Smaller items") it is in: the table's group heading. */
  readonly group: string;
  /** The item's name, or its path below the folder ("data/index") one level deeper. */
  readonly name: string;
  readonly kind: TreeRowKind;
  /** 0 for a direct child of the group, 1 below it. */
  readonly depth: number;
  readonly bytes: number;
  /** Its share of the whole tree, 0 to 100, one decimal. */
  readonly share: number;
  /** How many items a fold stands for (0 for a real item). */
  readonly folded: number;
}

/** What a group heading carries: the folder's own size and share (the aggregate of its direct children). */
export interface TreeGroup {
  readonly bytes: number;
  readonly share: number;
}

/** The tree as table rows and the aggregate of each group. */
export interface TreeTable {
  readonly rows: readonly TreeRow[];
  readonly groups: Readonly<Record<string, TreeGroup>>;
}

/** The words the rows need; the page translates them. */
export interface TreeWords {
  /** Files directly in the category's folder. */
  readonly files: string;
  /** The group of top-level items too small to list one by one. */
  readonly smaller: string;
  /** The smallest items, folded: `n` items. */
  readonly others: (n: number) => string;
}

/**
 * The size tree as the flat rows of `pd-table`, grouped by top-level folder (the table's group rows fold a folder away). A
 * folder's own size and share are on its group heading and only its contents are rows, so the shares of a group's direct rows add
 * up to the group's (nothing is counted twice). Pure and bounded by the tree the engine sent. Rows keep the engine's order,
 * biggest first.
 */
export function treeTable(tree: StorageTree, words: TreeWords): TreeTable {
  const total = tree.root.bytes;
  const share = (bytes: number): number => (total > 0 ? Math.round((bytes / total) * 1000) / 10 : 0);
  const rows: TreeRow[] = [];
  const groups: Record<string, TreeGroup> = {};
  const addTo = (group: string, bytes: number): void => {
    const g = groups[group];
    const sum = (g?.bytes ?? 0) + bytes;
    groups[group] = { bytes: sum, share: share(sum) };
  };
  const row = (group: string, node: SizeNode, name: string, depth: number, key: string): TreeRow => ({
    key,
    group,
    name,
    kind: node.folded > 0 ? 'fold' : node.children.length > 0 ? 'folder' : 'file',
    depth,
    bytes: node.bytes,
    share: share(node.bytes),
    folded: node.folded,
  });
  tree.root.children.forEach((top, i) => {
    if (top.children.length > 0) {
      // A folder: the heading carries its size; its contents are the rows.
      groups[top.name] = { bytes: top.bytes, share: share(top.bytes) };
      top.children.forEach((child, j) => {
        const label = child.folded > 0 ? words.others(child.folded) : child.name;
        rows.push(row(top.name, child, label, 0, `${i}/${j}`));
        child.children.forEach((grand, k) => {
          const inner = grand.folded > 0 ? words.others(grand.folded) : `${child.name}/${grand.name}`;
          rows.push(row(top.name, grand, inner, 1, `${i}/${j}/${k}`));
        });
      });
    } else if (top.folded > 0) {
      rows.push(row(words.smaller, top, words.others(top.folded), 0, `${i}`));
      addTo(words.smaller, top.bytes);
    } else {
      rows.push(row(words.files, top, top.name, 0, `${i}`));
      addTo(words.files, top.bytes);
    }
  });
  return { rows, groups };
}
