import type { SizeNode, StorageTree } from '../generated/storage';

/** One line of the tree table: an item, the folder (group) it belongs to, its size and its share of the category. */
export interface TreeRow {
  readonly key: string;
  /** The top-level folder it is in (the table's group heading). */
  readonly group: string;
  /** Its path below that folder ("data/index"), or the file or fold name. */
  readonly name: string;
  readonly bytes: number;
  /** Its share of the whole tree, 0 to 100. */
  readonly share: number;
  /** How many items a fold stands for (0 for a real item). */
  readonly folded: number;
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
 * The size tree as the flat rows of `pd-table`, grouped by top-level folder (the table's group rows fold a folder away). Pure
 * and bounded by the tree the engine sent (a few levels, a few items per folder). Rows keep the engine's order, biggest first.
 */
export function treeRows(tree: StorageTree, words: TreeWords): readonly TreeRow[] {
  const total = tree.root.bytes;
  const share = (bytes: number): number => (total > 0 ? Math.round((bytes / total) * 1000) / 10 : 0);
  const rows: TreeRow[] = [];
  const add = (group: string, node: SizeNode, prefix: string, key: string): void => {
    const name = node.folded > 0 ? words.others(node.folded) : prefix ? `${prefix}/${node.name}` : node.name;
    rows.push({ key, group, name, bytes: node.bytes, share: share(node.bytes), folded: node.folded });
    node.children.forEach((c, i) => add(group, c, node.folded > 0 ? '' : name, `${key}/${i}`));
  };
  tree.root.children.forEach((top, i) => {
    if (top.children.length > 0) {
      // A folder: its own line first, then everything below it.
      rows.push({ key: `${i}`, group: top.name, name: top.name, bytes: top.bytes, share: share(top.bytes), folded: 0 });
      top.children.forEach((c, j) => add(top.name, c, '', `${i}/${j}`));
    } else if (top.folded > 0) {
      add(words.smaller, top, '', `${i}`);
    } else {
      add(words.files, top, '', `${i}`);
    }
  });
  return rows;
}
