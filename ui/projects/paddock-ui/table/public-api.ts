/**
 * `@decryp7/paddock-ui/table`: `pd-table` (plan X21), the family's data table: sort, filter, column choice, optional groups,
 * virtual scroll on the CDK, row keyboard navigation, hover row actions with a "..." button and the same list as the
 * right-click menu. The pure parts (sorting, filtering, grouping, key paths) are exported for the apps' own lists.
 */
export { PD_TABLE_VIRTUAL_FROM, PdCell, PdTable, type PdRowActionEvent } from './src/table';
export {
  cellText,
  compareValues,
  filterRows,
  fromControl,
  groupItems,
  inlineActions,
  isMenuKey,
  menuActions,
  nextIndex,
  nextSort,
  shownColumns,
  sortRows,
  type PdColumn,
  type PdRowAction,
  type PdSort,
  type PdTableItem,
} from './src/model';
