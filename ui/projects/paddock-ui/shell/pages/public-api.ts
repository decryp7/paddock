/**
 * `@decryp7/paddock-ui/shell/pages`: the pages frame of the manager apps (Pitwall, Pitcrew, Spotter, Dyno; ADR 0012,
 * plan S14). `provideShellPages(...)` and `<pd-shell-frame />` give a window its title bar and menu bar, a sidebar of the
 * contributed pages (`definePage`: lazy, with their own providers) beside the routed page, the status bar with the
 * contributed status items, notifications, jobs, dialogs and the start-up contract. The sidebar is expanded, an icon rail
 * or over the page as the layout plan (`planLayout`) leaves room for; F6 cycles the regions.
 */
export { PdShellFrame } from './src/frame';
export { PdPagesSidebar } from './src/sidebar';
export { PdPageHost } from './src/page-host';
export {
  ShellPages,
  PAGES_CONFIG,
  PAGE_DATA,
  pagePath,
  pageOrder,
  badgeText,
  pageOfSnapshot,
  type PagesConfig,
} from './src/pages';
export {
  provideShellPages,
  pagesRoutes,
  pageRoute,
  PagesTitleStrategy,
  PAGES_WINDOW_READY,
  type ShellPagesOptions,
} from './src/provide-pages';
export {
  PagesFrameStore,
  PAGES_FRAME_STORAGE,
  PAGES_FRAME_KEY,
  PAGES_FRAME_VERSION,
  PAGES_SIDEBAR_SIZES,
  loadCollapsed,
  saveCollapsed,
  type PagesFrameStorage,
  type PagesFrameStoreType,
} from './src/pages-frame.store';
export { sidebarFit, sidebarMode, sidebarSizes, SIDEBAR_SIZES, type SidebarMode, type SidebarSizes } from './src/sidebar-plan';
