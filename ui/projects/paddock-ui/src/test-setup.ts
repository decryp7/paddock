// Vitest setup: jsdom lacks matchMedia and ResizeObserver.
if (typeof window !== 'undefined' && !window.matchMedia) {
  window.matchMedia = (query: string) =>
    ({ matches: false, media: query, addEventListener: () => undefined, removeEventListener: () => undefined }) as unknown as MediaQueryList;
}

// `$localize` (the shell's messages, plan S15): installed as in every app's polyfills; no translations are loaded, so
// every message is its English source text.
import '@angular/localize/init';
