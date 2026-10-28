/**
 * `@decryp7/paddock-ui/format`: one API for the text of numbers (plan X35): bytes, speeds, durations, remaining time,
 * counts, percentages, clock times and relative times in the active locale, and the middle ellipsis of long paths.
 * Pure functions: engine values in, text out.
 */
export {
  NONE,
  byteStep,
  TransferMeter,
  formatBytes,
  formatCompact,
  formatCount,
  formatDateTime,
  formatDecimal,
  formatDuration,
  formatElapsed,
  formatEta,
  formatExactBytes,
  formatNumber,
  formatPercent,
  formatRate,
  formatRelativeTime,
  formatWholeBytes,
  parseSize,
  type ByteKind,
  formatTime,
  percent,
  share,
  unitLabels,
  type Units,
} from './src/units';
export { DEFAULT_KEEP_TAIL, ELLIPSIS, fitEnd, fitPath, middleEllipsis, splitForEllipsis, splitPath, type SplitPath, type SplitText } from './src/middle-ellipsis';
