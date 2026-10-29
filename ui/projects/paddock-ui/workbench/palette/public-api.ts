/**
 * `@decryp7/paddock-ui/workbench/palette`: the command palette (ADR 0013 §1; plan W5). `providePalette()` and
 * `<pd-command-palette />` (placed by the workbench frame; a pages-frame app places it beside `<pd-shell-frame />` and provides `providePagesPalette()`): the palette's own list of commands and views (`>`, ranked with
 * recent and common commands first, "did you mean" when nothing matches) and the contributed palette providers chosen by
 * prefix, each debounced, cancelled on the next keystroke and capped. `namesPaletteProvider` puts an engine's
 * `names_search` (paddock-names, rule 20) behind a prefix.
 */
export { PdCommandPalette } from './src/command-palette';
export { PaletteStore, PALETTE_LIMIT, PALETTE_TIMERS, PALETTE_COMMON_COMMANDS, type PaletteRow, type PaletteSource, type PaletteTimers, type PaletteStoreType } from './src/palette.store';
export { PaletteRecent, PALETTE_RECENT_STORAGE, PALETTE_RECENT_KEY, MAX_RECENT } from './src/recent';
export { providePagesPalette, GO_TO_PAGE_PROVIDER } from './src/pages-palette';
export { providePalette, PALETTE_COMMANDS, type PaletteOptions } from './src/provide-palette';
export {
  namesPaletteProvider,
  namesToItems,
  NAMES_SEARCH,
  NAMES_MAX_MATCHES,
  NAMES_MAX_QUERY,
  type NameHow,
  type NameMatch,
  type NamesSearchRequest,
  type NamesSearchReply,
  type NamePaletteItem,
  type NamesPaletteOptions,
} from './src/names';
export { rank, matchWord, showsCategory, plain, titleParts, marksOf, editDistance, tolerance, suggestionScore, didYouMean, type Rankable, type Ranked, type TitlePart } from './src/match';
