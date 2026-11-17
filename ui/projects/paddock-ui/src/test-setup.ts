// Vitest setup: the jsdom gaps (matchMedia, ResizeObserver, pointer events) come from the shared test kit.
import { installLeakDetector, installTestKit } from '@decryp7/paddock-ui/testing';

installTestKit();
// After the kit, so the detector's afterEach runs first and sees what the kit's own clean-up would hide. Failing mode since
// the package had no findings (docs/lessons.md, 2026-11-16): a test that leaks fails itself, not a later one.
installLeakDetector({ mode: 'fail' });

// `$localize` (the shell's messages, plan S15): installed as in every app's polyfills; no translations are loaded, so
// every message is its English source text.
import '@angular/localize/init';
