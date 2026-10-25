// Vitest setup: the jsdom gaps and a clean window between tests (spec files share one per worker) from the shared test kit.
import { installTestKit } from '@decryp7/paddock-ui/testing';

installTestKit();

// `$localize`, as in the app's polyfills; no translations are loaded, so every message is its English source text.
import '@angular/localize/init';
