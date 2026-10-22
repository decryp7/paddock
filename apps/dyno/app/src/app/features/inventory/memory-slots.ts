import { ChangeDetectionStrategy, Component, computed, input } from '@angular/core';
import { type ModuleGeometry, facePath, moduleGeometry, speedText, techLabel } from './dimm-geometry';
import { HW_TOKENS } from './hardware-tokens';
import type { DimmSlotModel } from './hardware-models';

let nextId = 0;

/** Room either side of the module for the socket's latches, and the drawing's height. */
const LATCH = 24;
const TOP = 8;
const ROW_H = 92;

interface RowArt {
  slot: DimmSlotModel;
  index: number;
  geo: ModuleGeometry;
  face: string;
  /** Total width of the row's drawing. */
  width: number;
  speed: string | null;
  aria: string;
}

/**
 * The memory slots drawn as a board section: real DIMM sockets with latches and a key rib, seen from the front, grouped by
 * channel. A filled socket holds a module drawn from its data (SO-DIMM or full size, the key notch of its generation, DRAM
 * packages, SPD, and the PMIC on DDR5); an empty one is calm, with a faint outline where a module would go. Size, type and
 * speed are printed beside the drawing, never on it, and maker and part number are in the popover. Colour is never the only signal.
 */
@Component({
  selector: 'dy-memory-slots',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <figure class="bank" [style.--art-w.px]="artWidth()">
      <svg class="board" role="img" [attr.aria-label]="alt()" focusable="false">
        <defs>
          <pattern [attr.id]="id + '-vias'" width="16" height="16" patternUnits="userSpaceOnUse"><circle cx="8" cy="8" r="0.75" class="via" /></pattern>
        </defs>
        <rect class="plate" x="0.5" y="0.5" rx="10" />
        <rect class="plate-vias" x="0.5" y="0.5" rx="10" [attr.fill]="'url(#' + id + '-vias)'" />
        <g class="hole"><circle cx="12" cy="12" r="4" /><circle cx="12" cy="12" r="1.5" class="core" /></g>
        <svg x="100%" y="0" overflow="visible"><g class="hole"><circle cx="-12" cy="12" r="4" /><circle cx="-12" cy="12" r="1.5" class="core" /></g></svg>
        <svg x="0" y="100%" overflow="visible"><g class="hole"><circle cx="12" cy="-12" r="4" /><circle cx="12" cy="-12" r="1.5" class="core" /></g></svg>
        <svg x="100%" y="100%" overflow="visible"><g class="hole"><circle cx="-12" cy="-12" r="4" /><circle cx="-12" cy="-12" r="1.5" class="core" /></g></svg>
      </svg>

      <svg class="defs" width="0" height="0" aria-hidden="true" focusable="false">
        <defs>
          <linearGradient [attr.id]="id + '-pcb'" x1="0" y1="0" x2="0" y2="1"><stop offset="0" class="pcb-a" /><stop offset="1" class="pcb-b" /></linearGradient>
          <linearGradient [attr.id]="id + '-pkg'" x1="0" y1="0" x2="0" y2="1"><stop offset="0" class="pkg-a" /><stop offset="1" class="pkg-b" /></linearGradient>
          <linearGradient [attr.id]="id + '-gold'" x1="0" y1="0" x2="0" y2="1"><stop offset="0" class="gold-a" /><stop offset="1" class="gold-b" /></linearGradient>
          <linearGradient [attr.id]="id + '-socket'" x1="0" y1="0" x2="0" y2="1"><stop offset="0" class="socket-a" /><stop offset="1" class="socket-b" /></linearGradient>
          <pattern [attr.id]="id + '-fingers'" width="3" height="8" patternUnits="userSpaceOnUse"><rect x="2" y="0" width="1" height="8" class="gap" /></pattern>
        </defs>
      </svg>

      @for (g of groups(); track $index) {
        <section class="group" [attr.aria-label]="g.channel ? 'Channel ' + g.channel : 'Memory slots'">
          @if (g.channel) {
            <h3 class="channel"><span>Channel {{ g.channel }}</span></h3>
          }
          @for (r of g.rows; track r.index) {
            <div class="slot" tabindex="0" role="group" [class.free]="!r.slot.module" [class.flagged]="r.slot.tags.length > 0" [attr.aria-label]="r.aria" [attr.aria-describedby]="id + '-pop-' + r.index">
              <svg class="art" [attr.viewBox]="'0 0 ' + r.width + ' ' + rowH" aria-hidden="true" focusable="false">
                @for (side of sides; track side) {
                  <g class="latch" [class.open]="!r.slot.module" [attr.transform]="latchTransform(r, side, !r.slot.module)">
                    <rect x="-5" y="-60" width="10" height="64" rx="4" class="arm" />
                    <rect x="-3" y="-52" width="6" height="1" class="shine" />
                    <circle [attr.cx]="side === 'left' ? 7 : -7" cy="-26" r="3.5" class="hook" />
                  </g>
                }
                @if (r.slot.module; as m) {
                  <g [attr.transform]="'translate(' + latch + ' ' + top + ')'" class="module">
                    <path [attr.d]="r.face" [attr.fill]="'url(#' + id + '-pcb)'" class="pcb" />
                    <rect class="inner" x="6.5" y="3.5" [attr.width]="r.geo.width - 13" [attr.height]="r.geo.height - r.geo.contactsH - 10" rx="2.5" />
                    @for (x of r.geo.passives; track x) {
                      <rect class="passive" [attr.x]="x" [attr.y]="r.geo.height - r.geo.contactsH - 8" width="4" height="2" rx="0.5" />
                    }
                    @for (c of r.geo.chips; track c.x) {
                      <path class="trace" [attr.d]="'M' + (c.x + c.w / 2 - 3.5) + ' ' + (r.geo.chipY + r.geo.chipH) + 'V' + (r.geo.height - r.geo.contactsH - 9) + 'M' + (c.x + c.w / 2 + 3.5) + ' ' + (r.geo.chipY + r.geo.chipH) + 'V' + (r.geo.height - r.geo.contactsH - 9)" />
                      <rect class="chip" [attr.x]="c.x" [attr.y]="r.geo.chipY" [attr.width]="c.w" [attr.height]="r.geo.chipH" rx="2.5" [attr.fill]="'url(#' + id + '-pkg)'" />
                      <rect class="mark" [attr.x]="c.x + 4.5" [attr.y]="r.geo.chipY + 4.5" [attr.width]="c.w - 9" [attr.height]="r.geo.chipH - 9" rx="1" />
                      <circle class="dot" [attr.cx]="c.x + 4" [attr.cy]="r.geo.chipY + 4" r="1" />
                    }
                    <rect class="chip small" [attr.x]="r.geo.spd.x" [attr.y]="r.geo.chipY + 6" [attr.width]="r.geo.spd.w" height="10" rx="1.5" [attr.fill]="'url(#' + id + '-pkg)'" />
                    @if (r.geo.pmic; as p) {
                      <rect class="chip small" [attr.x]="p.x" [attr.y]="r.geo.chipY + 2" [attr.width]="p.w" height="18" rx="2" [attr.fill]="'url(#' + id + '-pkg)'" />
                      <rect class="mark" [attr.x]="p.x + 4" [attr.y]="r.geo.chipY + 6" [attr.width]="p.w - 8" height="10" rx="1" />
                      <rect class="inductor" [attr.x]="p.x + 3" [attr.y]="r.geo.chipY + 24" width="8" height="5" rx="1" />
                      <rect class="inductor" [attr.x]="p.x + 14" [attr.y]="r.geo.chipY + 24" width="8" height="5" rx="1" />
                    }
                    <rect class="gold" [attr.x]="2" [attr.y]="r.geo.height - r.geo.contactsH" [attr.width]="r.geo.notch.x - r.geo.notch.w / 2 - 2" [attr.height]="r.geo.contactsH" [attr.fill]="'url(#' + id + '-gold)'" />
                    <rect class="gold" [attr.x]="r.geo.notch.x + r.geo.notch.w / 2" [attr.y]="r.geo.height - r.geo.contactsH" [attr.width]="r.geo.width - 2 - (r.geo.notch.x + r.geo.notch.w / 2)" [attr.height]="r.geo.contactsH" [attr.fill]="'url(#' + id + '-gold)'" />
                    <rect [attr.x]="2" [attr.y]="r.geo.height - r.geo.contactsH" [attr.width]="r.geo.notch.x - r.geo.notch.w / 2 - 2" [attr.height]="r.geo.contactsH" [attr.fill]="'url(#' + id + '-fingers)'" />
                    <rect [attr.x]="r.geo.notch.x + r.geo.notch.w / 2" [attr.y]="r.geo.height - r.geo.contactsH" [attr.width]="r.geo.width - 2 - (r.geo.notch.x + r.geo.notch.w / 2)" [attr.height]="r.geo.contactsH" [attr.fill]="'url(#' + id + '-fingers)'" />
                  </g>
                } @else {
                  <g [attr.transform]="'translate(' + latch + ' ' + top + ')'" class="ghost">
                    <rect x="0.5" y="0.5" [attr.width]="r.geo.width - 1" [attr.height]="r.geo.height - 4" rx="4" />
                    <g [attr.transform]="'translate(' + r.geo.width / 2 + ' ' + (r.geo.height / 2 - 4) + ')'" class="plus"><circle r="11" /><path d="M-4.5 0H4.5M0 -4.5V4.5" /></g>
                  </g>
                }
                <rect class="socket" [attr.x]="latch - 10" [attr.y]="top + 69" [attr.width]="r.geo.width + 20" height="14" rx="3" [attr.fill]="'url(#' + id + '-socket)'" />
                <rect class="groove" [attr.x]="latch - 4" [attr.y]="top + 72" [attr.width]="r.geo.width + 8" height="4" rx="1.5" />
                <rect class="rib" [attr.x]="latch + r.geo.notch.x - r.geo.notch.w / 2 + 1" [attr.y]="top + 72 - r.geo.notch.depth + 3" [attr.width]="r.geo.notch.w - 2" [attr.height]="r.geo.notch.depth + 1" rx="1" />
              </svg>

              <div class="info">
                <span class="slotname">{{ r.slot.name }}</span>
                @if (r.slot.module; as m) {
                  <span class="cap"><strong class="size">{{ m.sizeLabel }}</strong><span class="type">{{ tech(m.tech) }}</span></span>
                  @if (r.speed) {
                    <span class="speed" [class.below]="!!m.ratedMts && !!m.runningMts && m.ratedMts !== m.runningMts">{{ r.speed }}</span>
                  }
                  <span class="maker">{{ maker(m) }}</span>
                } @else {
                  <span class="cap"><strong class="size free">Free slot</strong></span>
                }
                <span class="tags">
                  @for (t of r.slot.tags; track t.label) {
                    <span class="tag" [attr.data-tone]="t.tone">{{ t.label }}</span>
                  }
                </span>
              </div>

              <div class="pop" role="tooltip" [attr.id]="id + '-pop-' + r.index">
                <strong>{{ r.slot.name }}</strong>
                @if (r.slot.module; as m) {
                  <dl>
                    <dt>Size</dt><dd>{{ m.sizeLabel }}</dd>
                    <dt>Type</dt><dd>{{ tech(m.tech) }} {{ m.form === 'soDimm' ? 'SO-DIMM' : 'DIMM' }}</dd>
                    <dt>Rated speed</dt><dd>{{ m.ratedMts ? m.ratedMts + ' MT/s' : 'Unknown' }}</dd>
                    <dt>Running at</dt><dd>{{ m.runningMts ? m.runningMts + ' MT/s' : 'Unknown' }}</dd>
                    <dt>Maker</dt><dd>{{ m.maker ?? 'Unknown' }}</dd>
                    <dt>Part number</dt><dd class="mono">{{ m.part ?? 'Unknown' }}</dd>
                  </dl>
                } @else {
                  <p>Nothing is installed in this slot.</p>
                }
                @if (r.slot.note) {
                  <p class="hint">{{ r.slot.note }}</p>
                }
              </div>
            </div>
          }
        </section>
      }
      <figcaption class="visually-hidden">{{ alt() }}</figcaption>
    </figure>
  `,
  styles: [
    HW_TOKENS,
    `
    :host { display: block; container-type: inline-size; container-name: bank; }
    .bank { position: relative; margin: 0; padding: 20px 32px 20px; display: grid; gap: 4px; }
    .board { position: absolute; inset: 0; width: 100%; height: 100%; font: inherit; }
    .board .plate, .board .plate-vias { width: calc(100% - 1px); height: calc(100% - 1px); }
    .board .plate { fill: var(--hw-board); stroke: var(--hw-board-edge); }
    .board .via { fill: var(--hw-board-via); }
    .board .hole circle { fill: var(--hw-hole); stroke: var(--hw-hole-ring); stroke-width: 1; }
    .board .hole .core { stroke: none; fill: var(--hw-hole-ring); }
    .defs { position: absolute; }
    .pcb-a { stop-color: var(--hw-pcb-a); } .pcb-b { stop-color: var(--hw-pcb-b); }
    .pkg-a { stop-color: var(--hw-pkg-a); } .pkg-b { stop-color: var(--hw-pkg-b); }
    .gold-a { stop-color: var(--hw-gold-a); } .gold-b { stop-color: var(--hw-gold-b); }
    .socket-a { stop-color: var(--hw-socket-a); } .socket-b { stop-color: var(--hw-socket-b); }
    .gap { fill: var(--hw-gold-gap); fill-opacity: 0.55; }

    .group { position: relative; display: grid; gap: 2px; }
    .group + .group { margin-top: 8px; }
    .channel { margin: 0 0 2px; display: flex; align-items: center; gap: 8px; font: 500 11px / 16px var(--hw-font, system-ui, sans-serif); letter-spacing: 0.04em; text-transform: uppercase; color: var(--hw-text-subtle); }
    .channel::after { content: ''; flex: 1; height: 1px; background: var(--hw-board-edge); }

    .slot { position: relative; display: grid; grid-template-columns: minmax(0, var(--art-w)) minmax(0, 1fr); gap: 16px; align-items: center; padding: 6px 8px 6px 0; border-radius: 7px; outline: none; }
    .slot:focus-visible { outline: 2px solid var(--hw-focus, currentColor); outline-offset: 2px; }
    .art { width: 100%; height: auto; display: block; }
    .module { shape-rendering: geometricPrecision; }
    .pcb { stroke: var(--hw-pcb-edge); stroke-width: 1; }
    .inner { fill: none; stroke: var(--hw-pcb-inner); stroke-opacity: 0.55; stroke-width: 1; }
    .trace { fill: none; stroke: var(--hw-pcb-trace); stroke-opacity: 0.4; stroke-width: 1; shape-rendering: crispEdges; }
    .passive { fill: var(--hw-passive); fill-opacity: 0.7; }
    .chip { stroke: var(--hw-pkg-edge); stroke-width: 1; }
    .mark { fill: none; stroke: var(--hw-pkg-mark); stroke-opacity: 0.35; stroke-width: 1; shape-rendering: crispEdges; }
    .dot { fill: var(--hw-pkg-mark); fill-opacity: 0.7; }
    .inductor { fill: var(--hw-pkg-b); stroke: var(--hw-pkg-edge); stroke-width: 1; }
    .gold { stroke: var(--hw-gold-b); stroke-width: 0.5; }
    .socket { stroke: var(--hw-socket-edge); stroke-width: 1; }
    .groove { fill: var(--hw-groove); }
    .rib { fill: var(--hw-socket-b); stroke: var(--hw-socket-edge); stroke-width: 1; }
    .arm { fill: var(--hw-latch); stroke: var(--hw-latch-edge); stroke-width: 1; }
    .shine { fill: #fff; fill-opacity: 0.5; }
    .hook { fill: var(--hw-latch); stroke: var(--hw-latch-edge); stroke-width: 1; }
    .ghost rect { fill: var(--hw-ghost-fill); fill-opacity: 0.5; stroke: var(--hw-ghost); stroke-width: 1; stroke-opacity: 0.8; }
    .ghost .plus circle { fill: none; stroke: var(--hw-ghost); }
    .ghost .plus path { fill: none; stroke: var(--hw-ghost); stroke-width: 1.5; stroke-linecap: round; }
    @media (prefers-reduced-motion: no-preference) {
      .latch { transition: transform 200ms cubic-bezier(0.2, 0.8, 0.2, 1); }
      .pop { transition: opacity 140ms cubic-bezier(0.2, 0.8, 0.2, 1), transform 140ms cubic-bezier(0.2, 0.8, 0.2, 1); }
    }

    .info { display: grid; gap: 2px; justify-items: start; min-width: 0; }
    .slotname { font: 500 11px / 16px var(--hw-font-mono, ui-monospace, monospace); color: var(--hw-text-subtle); letter-spacing: 0.02em; }
    .cap { display: flex; align-items: baseline; gap: 8px; }
    .size { font: 600 20px / 26px var(--hw-font, system-ui, sans-serif); font-variant-numeric: tabular-nums; color: var(--hw-text); }
    .size.free { font-size: 14px; font-weight: 500; color: var(--hw-text-muted); line-height: 22px; }
    .type { font: 600 12px / 16px var(--hw-font, system-ui, sans-serif); color: var(--hw-text-muted); }
    .speed { font: 400 13px / 20px var(--hw-font, system-ui, sans-serif); font-variant-numeric: tabular-nums; color: var(--hw-text); }
    .speed.below { color: var(--hw-tone-warn); }
    .maker { font-size: 12px; line-height: 16px; color: var(--hw-text-subtle); max-width: 100%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .tags { display: flex; flex-wrap: wrap; gap: 4px; margin-top: 2px; }
    .tags:empty { display: none; }
    .tag { font: 500 11px / 16px var(--hw-font, system-ui, sans-serif); padding: 0 8px; border-radius: 999px; background: var(--hw-tag-bg); color: var(--hw-text-muted); }
    .tag[data-tone='warn'] { background: var(--hw-tone-warn-bg); color: var(--hw-tone-warn); }
    .tag[data-tone='ok'] { background: var(--hw-tone-ok-bg); color: var(--hw-tone-ok); }
    .tag[data-tone='info'] { background: var(--hw-tone-info-bg); color: var(--hw-tone-info); }

    .pop { position: absolute; z-index: 10; left: 8px; top: calc(100% - 10px); width: max-content; max-width: min(320px, calc(100% - 16px)); padding: 12px; border-radius: 10px; background: var(--hw-popover-bg); box-shadow: var(--hw-popover-shadow); color: var(--hw-text); font-size: 12px; line-height: 18px; opacity: 0; visibility: hidden; transform: translateY(-2px); pointer-events: none; }
    .pop dl { margin: 8px 0 0; display: grid; grid-template-columns: auto 1fr; gap: 2px 12px; }
    .pop dt { color: var(--hw-text-subtle); }
    .pop dd { margin: 0; font-variant-numeric: tabular-nums; overflow-wrap: anywhere; }
    .pop .mono { font-family: var(--hw-font-mono, ui-monospace, monospace); font-size: 11px; }
    .pop p { margin: 8px 0 0; color: var(--hw-text-muted); }
    .pop .hint { padding-top: 8px; border-top: 1px solid var(--hw-hairline); }
    .slot:hover .pop, .slot:focus-visible .pop { opacity: 1; visibility: visible; transform: none; }

    @container bank (max-width: 640px) {
      .bank { padding-inline: 20px; }
      .slot { grid-template-columns: minmax(0, 1fr); gap: 4px; }
      .info { padding-left: 4px; }
    }
  `,
  ],
})
export class MemorySlots {
  /** The sockets in drawing order. */
  readonly slots = input.required<readonly DimmSlotModel[]>();
  /** Text alternative of the whole drawing. */
  readonly alt = input.required<string>();
  protected readonly id = `dy-mem-${(nextId++).toString()}`;
  protected readonly latch = LATCH;
  protected readonly top = TOP;
  protected readonly rowH = ROW_H;
  protected readonly sides = ['left', 'right'] as const;
  protected readonly tech = techLabel;
  protected readonly groups = computed(() => {
    const list = this.slots();
    // Empty sockets are keyed like the installed modules: the first module's generation and form.
    const ref = list.find((s) => s.module)?.module ?? null;
    const out: { channel: string | null; rows: RowArt[] }[] = [];
    list.forEach((slot, index) => {
      const m = slot.module ?? ref;
      const geo = moduleGeometry(m?.tech ?? 'ddr4', m?.form ?? 'dimm', slot.module?.capacityBytes ?? 0);
      const row: RowArt = { slot, index, geo, face: facePath(geo), width: geo.width + LATCH * 2, speed: slot.module ? speedText(slot.module.ratedMts, slot.module.runningMts) : null, aria: this.aria(slot) };
      const last = out[out.length - 1];
      if (last && last.channel === slot.channel) last.rows.push(row);
      else out.push({ channel: slot.channel, rows: [row] });
    });
    return out;
  });
  protected readonly artWidth = computed(() => Math.max(...this.groups().flatMap((g) => g.rows.map((r) => r.width)), 0));

  protected maker(m: { maker: string | null; part: string | null }): string {
    return [m.maker, m.part].filter((x): x is string => !!x).join(' · ') || 'Maker unknown';
  }

  /** The latches stand at each end of the socket: closed (upright, hooked over the module) when it is filled, tilted out when empty. */
  protected latchTransform(r: RowArt, side: 'left' | 'right', open: boolean): string {
    const base = TOP + 78;
    const x = side === 'left' ? LATCH - 9 : r.width - LATCH + 9;
    const angle = open ? (side === 'left' ? -20 : 20) : 0;
    return `translate(${x.toString()} ${base.toString()}) rotate(${angle.toString()})`;
  }

  private aria(s: DimmSlotModel): string {
    const note = s.note ? `. ${s.note}` : '';
    const m = s.module;
    if (!m) return `${s.name}: free slot${note}`;
    const speed = speedText(m.ratedMts, m.runningMts);
    return `${s.name}: ${m.sizeLabel} ${techLabel(m.tech)}${speed ? `, ${speed}` : ''}${m.maker ? `, ${m.maker}` : ''}${note}`;
  }
}
