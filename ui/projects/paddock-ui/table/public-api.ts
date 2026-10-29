/**
 * `@decryp7/paddock-ui/table`: `pd-table` (plan X21), the family's data table: sort, filter, column choice, optional groups,
 * virtual scroll on the CDK, row keyboard navigation, hover row actions with a "..." button and the same list as the
 * right-click menu. The pure parts (sorting, filtering, grouping, key paths) are exported for the apps' own lists.
 */
export { PD_TABLE_VIRTUAL_FROM, PdCell, PdTable, PdTableEmpty, type PdRowActionEvent } from './src/table';
export { PdActionTip } from './src/action-tip';
export { PD_MEASURE_TEXT, PdMiddleText, middleTruncate, tailOf, type PdMeasureText } from './src/middle-text';
export {
  actionColumnWidth,
  cellText,
  columnTrack,
  compareValues,
  fitColumns,
  filterRows,
  fromControl,
  groupItems,
  inlineActions,
  menuActions,
  nextIndex,
  nextSort,
  parentIndex,
  PD_HEADER_HEIGHT,
  PD_PRIMARY_MIN_PX,
  PD_ROW_HEIGHTS,
  rowHeightFor,
  PD_FOLD_EXTRA,
  shownColumns,
  sortRows,
  treePlaces,
  type PdColumn,
  type PdRowAction,
  type PdRowTree,
  type PdSort,
  type PdTableItem,
} from './src/model';
