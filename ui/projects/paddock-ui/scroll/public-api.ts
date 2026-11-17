/**
 * `@decryp7/paddock-ui/scroll`: scrollers whose position our code sets (family ADR 0105). `PdOwnedScroll` turns off the browser's own
 * scroll moves on such a scroller. No dependencies besides `@angular/core`, so any component can use it without the root's weight.
 */
export { PdOwnedScroll } from './src/owned-scroll';
