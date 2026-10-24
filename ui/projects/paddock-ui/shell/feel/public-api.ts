/**
 * `@decryp7/paddock-ui/shell/feel`: the UI half of the native feel (plan "Gaps in planned items"): the webview behaves like
 * a desktop app, not a web page. Browser keys (reload, back/forward, page zoom, find, print, save, view source, devtools in
 * release builds), middle-click autoscroll, Ctrl+wheel zoom, the browser's context menu, dragging images and links out and
 * dropping files where nothing takes them are switched off; right-clicking selected text opens the shell's Copy menu.
 * `provideNativeFeel()` replaces every app's `core/native-feel.ts`; the Rust half is `paddock-window`'s `feel`.
 */
export {
  browserKeyAction,
  contextMenuAction,
  installNativeFeel,
  isEditable,
  isMac,
  provideNativeFeel,
  shouldBlockDrag,
  shouldBlockFileDrop,
  shouldBlockKey,
  shouldBlockMouseButton,
  shouldBlockWheel,
  type BrowserKeyAction,
  type ContextMenuAction,
  type ContextMenuTarget,
  type DragTarget,
  type FeelHooks,
  type FeelOptions,
  type KeyLike,
} from './src/native-feel';
