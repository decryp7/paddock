/**
 * `@decryp7/paddock-ui/workbench/explorer`: the Explorer side bar (ADR 0013 §1; plan W4). `<pd-explorer />` draws one
 * section per contributed Explorer provider (`defineExplorerProvider` in the core) as a lazy, paged, cancellable tree with
 * middle-truncated names, context menus and drag data; `ExplorerStore` holds the trees (provided by the workbench frame).
 */
export { PdExplorer } from './src/explorer';
export { ExplorerStore, EXPLORER_PAGE, EXPLORER_MAX_CHILDREN, EXPLORER_MAX_TEXT, treeKey, type ChildList, type TreeRow, type ExplorerStoreType } from './src/explorer.store';
