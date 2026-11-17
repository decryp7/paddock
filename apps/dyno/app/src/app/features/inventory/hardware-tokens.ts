/**
 * The colours of the hardware drawings, as custom properties on the host of every component that draws hardware. Materials are
 * physical (green PCB, graphite packages, gold contacts, silicon, aluminium) and stay recognisable in both themes; the board and
 * socket surfaces around them follow the theme. `light-dark()` follows the root's `color-scheme`, which the app sets from the
 * system or from the theme switch. The accent is never used here: it is reserved for focus and selection.
 */
export const HW_TOKENS = `
  :host {
    /* Text, popover and tones: they follow the host app's tokens when it has them (the fallbacks are neutral). */
    --hw-text: var(--text-1, light-dark(#16171a, #ededf0));
    --hw-text-muted: var(--text-2, light-dark(#50535b, #a9abb3));
    --hw-text-subtle: var(--text-3, light-dark(#686b73, #8c8f98));
    --hw-hairline: var(--border, light-dark(#e2e2e7, #26272c));
    --hw-tag-bg: var(--surface-active, light-dark(#e8e8ec, #26272c));
    --hw-popover-bg: var(--overlay-bg, light-dark(#ffffff, #1b1c20));
    --hw-popover-shadow: var(--shadow-3, 0 2px 6px rgb(0 0 0 / 12%), 0 16px 40px rgb(0 0 0 / 24%));
    --hw-tone-warn: var(--warn, light-dark(#965400, #f2b84b));
    --hw-tone-warn-bg: var(--warn-soft, light-dark(#fbefd9, #2d2410));
    --hw-tone-ok: var(--ok, light-dark(#157347, #3dd68c));
    --hw-tone-ok-bg: var(--ok-soft, light-dark(#e3f4ea, #10261b));
    --hw-tone-info: var(--info, light-dark(#1d63b0, #7db2e8));
    --hw-tone-info-bg: var(--info-soft, light-dark(#e2eefb, #122233));
    /* Focus and selection come from the host (set --hw-focus); the art itself never uses an accent. */

    --hw-board: light-dark(#eceef2, #0f1114);
    --hw-board-edge: light-dark(#d4d8e0, #23272e);
    --hw-board-via: light-dark(#cfd4dd, #1d2128);
    --hw-hole-ring: light-dark(#b9bfca, #2b3038);
    --hw-hole: light-dark(#f8f9fb, #07080a);

    --hw-pcb-a: light-dark(#21705a, #1f614c);
    --hw-pcb-b: light-dark(#175040, #14463a);
    --hw-pcb-edge: light-dark(#37977a, #2f8068);
    --hw-pcb-inner: light-dark(#2d8168, #286e58);
    --hw-pcb-trace: light-dark(#52b093, #49a083);
    --hw-pcb-pad: light-dark(#8fd1b8, #6fb89c);

    --hw-pkg-a: #2c3037;
    --hw-pkg-b: #1a1c20;
    --hw-pkg-edge: #4a515c;
    --hw-pkg-mark: #7d8593;
    --hw-passive: #b9bfc9;

    --hw-gold-a: #ecd28a;
    --hw-gold-b: #b88d33;
    --hw-gold-gap: #6e5420;

    --hw-socket-a: light-dark(#d3d8e1, #343a44);
    --hw-socket-b: light-dark(#b4bbc7, #22262d);
    --hw-socket-edge: light-dark(#9ea6b4, #4a515d);
    --hw-groove: light-dark(#6f7787, #0a0b0d);
    --hw-latch: light-dark(#eef0f4, #4a515d);
    --hw-latch-edge: light-dark(#8f98a8, #6a7280);
    --hw-ghost: light-dark(#aab1bf, #3a414c);
    --hw-ghost-fill: light-dark(#e5e8ee, #14171b);

    --hw-substrate-a: light-dark(#3d4f4b, #2a3836);
    --hw-substrate-b: light-dark(#2b3a37, #1b2624);
    --hw-substrate-edge: light-dark(#5d726c, #435651);
    --hw-lid-ring: light-dark(#7d918b, #566b65);
    --hw-die-a: light-dark(#4b5666, #404a59);
    --hw-die-b: light-dark(#363f4c, #2d3541);
    --hw-die-edge: #7c8798;
    --hw-core: #aab4c4;
    --hw-region-cpu: #5b82b0;
    --hw-region-eff: #7d9fc6;
    --hw-region-gpu: #4f9a8a;
    --hw-region-npu: #9a82b8;
    --hw-region-fabric: #556072;
    --hw-region-io: #485160;

    --hw-metal-a: #dfe3ea;
    --hw-metal-b: #a9b0bc;
    --hw-metal-edge: #8a92a0;
    --hw-label: #f2f4f7;
    --hw-label-ink: #5b6372;
  }
`;
