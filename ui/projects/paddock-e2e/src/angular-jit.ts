// A spec may import app source that imports `@decryp7/paddock-ui/*` (a formatter, a model). Those files are partially
// compiled Angular; the browser build links them, Playwright's Node process does not, so Angular needs its JIT compiler
// there. Imported first by the kit's index, so it is in place before any app source a spec imports after the kit.
import '@angular/compiler';
