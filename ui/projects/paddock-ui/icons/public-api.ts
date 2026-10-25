/**
 * `@decryp7/paddock-ui/icons`: the generic `pd:` icon set (plan X36): about forty glyphs on one 24 px grid, stroke 1.5 screen px,
 * round caps and joins, themed by `currentColor`, registered as literals (never fetched), and the `outlineIcon` helper an
 * app draws its own domain glyphs with.
 */
export { ICON_GRID, ICON_STROKE, outlineIcon, outlineIconWithDots, type OutlineDot } from './src/outline';
export { PD_ICON_DOTS, PD_ICON_PATHS, type PdIconDot, type PdIconName } from './src/paths';
export {
  PD_ICON_NAMESPACE,
  pdIconLiterals,
  pdIconSvg,
  registerPaddockIcons,
  type IconLiteralRegistry,
  type TrustedHtmlFactory,
} from './src/register';
export { PdGlyph } from './src/glyph';
export { PD_STATUS_GLYPHS, PdStatusGlyph, statusGlyph, type PdStatus } from './src/status';
