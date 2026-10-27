import { ChangeDetectionStrategy, Component, computed, input } from '@angular/core';
import type { SocPackageModel } from './hardware-models';
import { HW_TOKENS } from './hardware-tokens';
import { applePackage, solderedBoard } from './package-geometry';

let nextId = 0;

/**
 * Memory that is part of the package or soldered to the board: an Apple-silicon chip drawn as a die on its substrate with the
 * unified-memory packages beside it (CPU performance and efficiency clusters, GPU, Neural Engine), or a laptop's SoC with LPDDR
 * packages soldered to the mainboard. Sized from the data; the legend beside it names every block, so colour is never the only signal.
 */
@Component({
  selector: 'dy-chip-package',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <figure class="pkg">
      <svg role="img" [attr.aria-label]="model().alt" [attr.viewBox]="'0 0 ' + layout().width + ' ' + layout().height" [style.max-width.px]="layout().width * 1.5">
        <defs>
          <linearGradient [attr.id]="id + '-sub'" x1="0" y1="0" x2="0" y2="1"><stop offset="0" class="sub-a" /><stop offset="1" class="sub-b" /></linearGradient>
          <linearGradient [attr.id]="id + '-pcb'" x1="0" y1="0" x2="0" y2="1"><stop offset="0" class="pcb-a" /><stop offset="1" class="pcb-b" /></linearGradient>
          <linearGradient [attr.id]="id + '-die'" x1="0" y1="0" x2="1" y2="1"><stop offset="0" class="die-a" /><stop offset="1" class="die-b" /></linearGradient>
          <linearGradient [attr.id]="id + '-pkg'" x1="0" y1="0" x2="0" y2="1"><stop offset="0" class="pkg-a" /><stop offset="1" class="pkg-b" /></linearGradient>
          <linearGradient [attr.id]="id + '-metal'" x1="0" y1="0" x2="1" y2="1"><stop offset="0" class="metal-a" /><stop offset="1" class="metal-b" /></linearGradient>
        </defs>
        @let l = layout();
        <rect class="substrate" [class.board]="l.kind === 'soldered'" [attr.x]="l.substrate.x" [attr.y]="l.substrate.y" [attr.width]="l.substrate.w" [attr.height]="l.substrate.h" rx="12" [attr.fill]="'url(#' + id + (l.kind === 'soldered' ? '-pcb' : '-sub') + ')'" />
        <rect class="inset" x="4.5" y="4.5" [attr.width]="l.width - 9" [attr.height]="l.height - 9" rx="9" />
        @for (p of l.passives; track $index) {
          <rect class="passive" [attr.x]="p.x" [attr.y]="p.y" [attr.width]="p.w" [attr.height]="p.h" rx="0.75" />
        }
        @if (l.kind === 'apple') {
          <rect class="ring" [attr.x]="l.lid.x" [attr.y]="l.lid.y" [attr.width]="l.lid.w" [attr.height]="l.lid.h" rx="8" />
          <path class="pin1" d="M12 12 L24 12 L12 24 Z" />
          @for (m of l.memory; track $index) {
            <rect class="memsub" [attr.x]="m.x - 3" [attr.y]="m.y - 3" [attr.width]="m.w + 6" [attr.height]="m.h + 6" rx="6" />
            <rect class="dram" [attr.x]="m.x" [attr.y]="m.y" [attr.width]="m.w" [attr.height]="m.h" rx="4" [attr.fill]="'url(#' + id + '-pkg)'" />
            <rect class="memlid" [attr.x]="m.x + 3" [attr.y]="m.y + 3" [attr.width]="m.w - 6" [attr.height]="m.h - 6" rx="2.5" />
            <rect class="mark" [attr.x]="m.x + 7" [attr.y]="m.y + 7" [attr.width]="m.w - 14" [attr.height]="m.h - 14" rx="1.5" />
            <circle class="dot" [attr.cx]="m.x + 10" [attr.cy]="m.y + 10" r="1.25" />
          }
          @for (d of l.dies; track $index) {
            <rect class="die" [attr.x]="d.x" [attr.y]="d.y" [attr.width]="d.w" [attr.height]="d.h" rx="4" [attr.fill]="'url(#' + id + '-die)'" />
            @for (r of d.regions; track $index) {
              <rect class="region" [attr.data-kind]="r.kind" [attr.x]="r.x" [attr.y]="r.y" [attr.width]="r.w" [attr.height]="r.h" rx="2" />
              @for (c of r.cells; track $index) {
                <rect class="cell" [attr.data-kind]="r.kind" [attr.x]="c.x" [attr.y]="c.y" [attr.width]="c.w" [attr.height]="c.h" rx="1" />
              }
            }
          }
          @if (l.dies.length > 1) {
            <rect class="bridge" [attr.x]="l.dies[0]!.x + l.dies[0]!.w - 2" [attr.y]="l.dies[0]!.y + l.dies[0]!.h / 2 - 14" width="14" height="28" rx="2" />
          }
        } @else {
          @for (m of l.memory; track $index) {
            <rect class="silk" [attr.x]="m.x - 5" [attr.y]="m.y - 5" [attr.width]="m.w + 10" [attr.height]="m.h + 10" rx="3" />
            <rect class="balls" [attr.x]="m.x - 2" [attr.y]="m.y - 2" [attr.width]="m.w + 4" [attr.height]="m.h + 4" rx="4" />
            <rect class="dram" [attr.x]="m.x" [attr.y]="m.y" [attr.width]="m.w" [attr.height]="m.h" rx="3" [attr.fill]="'url(#' + id + '-pkg)'" />
            <rect class="mark" [attr.x]="m.x + 5" [attr.y]="m.y + 5" [attr.width]="m.w - 10" [attr.height]="m.h - 10" rx="1.5" />
            <circle class="dot" [attr.cx]="m.x + 7" [attr.cy]="m.y + 7" r="1.25" />
          }
          @for (d of l.dies; track $index) {
            <rect class="silk" [attr.x]="d.x - 6" [attr.y]="d.y - 6" [attr.width]="d.w + 12" [attr.height]="d.h + 12" rx="4" />
            <rect class="balls" [attr.x]="d.x - 3" [attr.y]="d.y - 3" [attr.width]="d.w + 6" [attr.height]="d.h + 6" rx="6" />
            <rect class="soc" [attr.x]="d.x" [attr.y]="d.y" [attr.width]="d.w" [attr.height]="d.h" rx="5" [attr.fill]="'url(#' + id + '-pkg)'" />
            <rect class="lid" [attr.x]="l.lid.x" [attr.y]="l.lid.y" [attr.width]="l.lid.w" [attr.height]="l.lid.h" rx="4" [attr.fill]="'url(#' + id + '-metal)'" />
            <path class="pin1" [attr.d]="'M' + (d.x + 5) + ' ' + (d.y + 5) + 'h8l-8 8z'" />
          }
        }
      </svg>
      <figcaption>
        <h3>{{ model().title }}</h3>
        <ul class="legend">
          @for (e of layout().legend; track e.label) {
            <li><span class="sw" [attr.data-kind]="e.kind"></span><span class="lab"><strong>{{ e.label }}</strong><span>{{ e.detail }}</span></span></li>
          }
        </ul>
      </figcaption>
    </figure>
  `,
  styles: [
    HW_TOKENS,
    `
    :host { display: block; }
    .pkg { margin: 0; display: flex; align-items: center; gap: 24px; flex-wrap: wrap; }
    svg { flex: 1 1 280px; min-width: 240px; width: 100%; height: auto; display: block; }
    figcaption { flex: 1 1 220px; min-width: 0; display: grid; gap: 12px; }
    h3 { margin: 0; font: 600 16px / 22px var(--hw-font, system-ui, sans-serif); }
    .legend { list-style: none; margin: 0; padding: 0; display: grid; gap: 8px; }
    .legend li { display: flex; gap: 12px; align-items: flex-start; }
    .lab { display: grid; font-size: 12px; line-height: 16px; color: var(--hw-text-subtle); }
    .lab strong { font-size: 13px; line-height: 20px; font-weight: 500; color: var(--hw-text); }
    .sw { flex: none; width: 12px; height: 12px; margin-top: 4px; border-radius: 3px; background: var(--hw-region-fabric); }
    .sw[data-kind='cpu-perf'] { background: var(--hw-region-cpu); }
    .sw[data-kind='cpu-eff'] { background: var(--hw-region-eff); }
    .sw[data-kind='gpu'] { background: var(--hw-region-gpu); }
    .sw[data-kind='npu'] { background: var(--hw-region-npu); }
    .sw[data-kind='memory'] { background: linear-gradient(var(--hw-pkg-a), var(--hw-pkg-b)); box-shadow: inset 0 0 0 1px var(--hw-pkg-edge); }

    .sub-a { stop-color: var(--hw-substrate-a); } .sub-b { stop-color: var(--hw-substrate-b); }
    .pcb-a { stop-color: var(--hw-pcb-a); } .pcb-b { stop-color: var(--hw-pcb-b); }
    .die-a { stop-color: var(--hw-die-a); } .die-b { stop-color: var(--hw-die-b); }
    .pkg-a { stop-color: var(--hw-pkg-a); } .pkg-b { stop-color: var(--hw-pkg-b); }
    .metal-a { stop-color: var(--hw-metal-a); } .metal-b { stop-color: var(--hw-metal-b); }
    .substrate { stroke: var(--hw-substrate-edge); stroke-width: 1; }
    .substrate.board { stroke: var(--hw-pcb-edge); }
    .inset { fill: none; stroke: var(--hw-lid-ring); stroke-opacity: 0.25; stroke-width: 1; }
    .passive { fill: var(--hw-passive); fill-opacity: 0.6; }
    .ring { fill: none; stroke: var(--hw-lid-ring); stroke-opacity: 0.7; stroke-width: 1; }
    .pin1 { fill: var(--hw-gold-a); fill-opacity: 0.9; }
    .memsub { fill: var(--hw-substrate-a); fill-opacity: 0.75; stroke: var(--hw-substrate-edge); stroke-opacity: 0.8; stroke-width: 1; }
    .memlid { fill: var(--hw-metal-b); fill-opacity: 0.14; stroke: var(--hw-metal-edge); stroke-opacity: 0.45; stroke-width: 1; }
    .dram, .soc { stroke: var(--hw-pkg-edge); stroke-width: 1; }
    .mark { fill: none; stroke: var(--hw-pkg-mark); stroke-opacity: 0.35; stroke-width: 1; shape-rendering: crispEdges; }
    .dot { fill: var(--hw-pkg-mark); fill-opacity: 0.7; }
    .die { stroke: var(--hw-die-edge); stroke-opacity: 0.6; stroke-width: 1; }
    .region { fill: var(--hw-region-fabric); fill-opacity: 0.55; }
    .region[data-kind='cpu-perf'] { fill: var(--hw-region-cpu); fill-opacity: 0.3; }
    .region[data-kind='cpu-eff'] { fill: var(--hw-region-eff); fill-opacity: 0.25; }
    .region[data-kind='gpu'] { fill: var(--hw-region-gpu); fill-opacity: 0.3; }
    .region[data-kind='npu'] { fill: var(--hw-region-npu); fill-opacity: 0.3; }
    .region[data-kind='io'] { fill: var(--hw-region-io); fill-opacity: 0.7; }
    .cell { fill: var(--hw-region-fabric); }
    .cell[data-kind='cpu-perf'] { fill: var(--hw-region-cpu); }
    .cell[data-kind='cpu-eff'] { fill: var(--hw-region-eff); }
    .cell[data-kind='gpu'] { fill: var(--hw-region-gpu); }
    .cell[data-kind='npu'] { fill: var(--hw-region-npu); }
    .bridge { fill: var(--hw-region-io); stroke: var(--hw-die-edge); stroke-opacity: 0.5; }
    .silk { fill: none; stroke: var(--hw-pcb-pad); stroke-opacity: 0.5; stroke-width: 1; }
    .balls { fill: none; stroke: var(--hw-pcb-pad); stroke-opacity: 0.7; stroke-width: 2; stroke-dasharray: 0.1 4; stroke-linecap: round; }
    .lid { stroke: var(--hw-metal-edge); stroke-width: 1; }
  `,
  ],
})
export class ChipPackage {
  readonly model = input.required<SocPackageModel>();
  protected readonly id = `dy-pkg-${(nextId++).toString()}`;
  protected readonly layout = computed(() => {
    const m = this.model();
    return m.style === 'apple' ? applePackage(m.name, m.physicalCores, m.memoryBytes, m.memoryTech) : solderedBoard(m.memoryBytes, m.memoryTech);
  });
}
