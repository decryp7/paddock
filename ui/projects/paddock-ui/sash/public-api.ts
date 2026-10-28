/**
 * `@decryp7/paddock-ui/sash`: the one splitter behaviour for a pane the user resizes by hand (the workbench's Explorer, an
 * app's side bar). It owns the pointer drag (primary button, pointer capture), the keys (arrows, Shift for big steps, Home and
 * End, flipped in right-to-left text) and the `separator` aria; the consumer owns the width, the limits and the look.
 */
export { PdSash, SASH_STEP, SASH_BIG_STEP, clampSash, type SashLimits } from './src/sash';
