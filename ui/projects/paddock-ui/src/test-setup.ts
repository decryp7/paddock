// Vitest setup: the jsdom gaps (matchMedia, ResizeObserver, pointer events) come from the shared test kit.
import { installTestKit } from '@decryp7/paddock-ui/testing';

installTestKit();

// `$localize` (the shell's messages, plan S15): installed as in every app's polyfills; no translations are loaded, so
// every message is its English source text.
import '@angular/localize/init';
