// Vitest setup: the jsdom gaps and a clean window between tests (spec files share one per worker) from the shared test kit.
import { installLeakDetector, installTestKit } from '@decryp7/paddock-ui/testing';

installTestKit();
// After the kit, so the detector's afterEach runs first and sees what the kit's own clean-up would hide. Failing mode since
// the package had no findings (docs/lessons.md, 2026-11-16): a test that leaks fails itself, not a later one.
installLeakDetector({ mode: 'fail' });

// `$localize`, as in the app's polyfills; no translations are loaded, so every message is its English source text.
import '@angular/localize/init';
