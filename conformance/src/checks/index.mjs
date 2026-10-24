import C03 from './C03-icons.mjs';
import C04 from './C04-macos-signing.mjs';
import C06 from './C06-key-strip.mjs';
import C07 from './C07-release-preflight.mjs';
import C08 from './C08-release-artifacts.mjs';
import C13 from './C13-rust-lints.mjs';
import C14 from './C14-hermetic.mjs';
import C15 from './C15-installed-chrome.mjs';
import C16 from './C16-hardcoded-version.mjs';
import C23 from './C23-process.mjs';
import C24 from './C24-reviewed-images.mjs';
import C28 from './C28-baseline.mjs';
import C29 from './C29-window-weight.mjs';
import C30 from './C30-wall-clock.mjs';
import C31 from './C31-adr-numbers.mjs';
import C32 from './C32-sidecar-self-test.mjs';
import C33 from './C33-untimed-calls.mjs';
import C34 from './C34-parser-hardening.mjs';

/** Every check, in id order. A check is { id, title, rule, run(ctx) -> findings[] | Promise<findings[]> }. */
export const checks = [C03, C04, C06, C07, C08, C13, C14, C15, C16, C23, C24, C28, C29, C30, C31, C32, C33, C34];
