/**
 * `@decryp7/paddock-ui/prefs`: a safe wrapper over `localStorage` for per-viewer conveniences (plan X35): every read and
 * write is guarded, damaged JSON gives the fallback, and a storage that is missing or throws never breaks the page.
 */
export { createPrefs, localPrefs, memoryStorage, type Prefs, type PrefsStorage } from './src/prefs';
