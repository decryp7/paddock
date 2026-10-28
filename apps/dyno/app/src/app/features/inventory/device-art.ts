import { ChangeDetectionStrategy, Component, computed, input } from '@angular/core';
import { type DriveKind, nandCount, nandSlots } from './device-geometry';
import { HW_TOKENS } from './hardware-tokens';

let nextId = 0;

/** The kind of device to draw: the processor package or a drive. */
export type DeviceArtKind = 'cpu' | DriveKind;

/**
 * Small illustrations of the processor package and of drives, in the same flat material style as the memory drawings:
 * an LGA processor under its metal lid, an M.2 2280 stick with controller and NAND, a 2.5" SATA drive, Apple's soldered
 * flash and a USB stick. Decorative: the facts are in the text beside them.
 */
@Component({
  selector: 'dy-device-art',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <!-- conformance: svg-ok — device illustration --><svg [attr.viewBox]="box()" aria-hidden="true" focusable="false">
      <defs>
        <linearGradient [attr.id]="id + '-pcb'" x1="0" y1="0" x2="0" y2="1"><stop offset="0" class="pcb-a" /><stop offset="1" class="pcb-b" /></linearGradient>
        <linearGradient [attr.id]="id + '-pkg'" x1="0" y1="0" x2="0" y2="1"><stop offset="0" class="pkg-a" /><stop offset="1" class="pkg-b" /></linearGradient>
        <linearGradient [attr.id]="id + '-gold'" x1="0" y1="0" x2="0" y2="1"><stop offset="0" class="gold-a" /><stop offset="1" class="gold-b" /></linearGradient>
        <linearGradient [attr.id]="id + '-metal'" x1="0" y1="0" x2="1" y2="1"><stop offset="0" class="metal-a" /><stop offset="1" class="metal-b" /></linearGradient>
        <linearGradient [attr.id]="id + '-sub'" x1="0" y1="0" x2="0" y2="1"><stop offset="0" class="sub-a" /><stop offset="1" class="sub-b" /></linearGradient>
        <pattern [attr.id]="id + '-rows'" width="8" height="3" patternUnits="userSpaceOnUse"><rect x="0" y="2" width="8" height="1" class="gap" /></pattern>
      </defs>
      @switch (kind()) {
        @case ('cpu') {
          <rect class="pcb" x="0.5" y="0.5" width="119" height="119" rx="7" [attr.fill]="'url(#' + id + '-pcb)'" />
          <path class="pin1" d="M8 8 L22 8 L8 22 Z" />
          @for (i of caps; track i) {
            <rect class="passive" [attr.x]="14 + i * 12" y="8" width="6" height="3" rx="0.5" />
            <rect class="passive" [attr.x]="14 + i * 12" y="109" width="6" height="3" rx="0.5" />
            <rect class="passive" x="8" [attr.y]="30 + i * 12" width="3" height="6" rx="0.5" />
            <rect class="passive" x="109" [attr.y]="14 + i * 12" width="3" height="6" rx="0.5" />
          }
          <rect class="lid" x="20.5" y="20.5" width="79" height="79" rx="6" [attr.fill]="'url(#' + id + '-metal)'" />
          <rect class="lid-inner" x="28.5" y="28.5" width="63" height="63" rx="3" />
          <path class="etch" d="M38 52H82M38 60H82M38 68H66" />
          <path class="lid-notch" d="M20.5 34 L34 20.5" />
        }
        @case ('nvme') {
          <path class="pcb" [attr.fill]="'url(#' + id + '-pcb)'" d="M6 8H282A4 4 0 0 1 286 12V34A4 4 0 0 0 286 42V64A4 4 0 0 1 282 68H6A4 4 0 0 1 2 64V47H10V41H2V12A4 4 0 0 1 6 8Z" />
          <rect class="gold" x="2.5" y="12" width="12" height="29" [attr.fill]="'url(#' + id + '-gold)'" />
          <rect class="gold" x="2.5" y="47" width="12" height="17" [attr.fill]="'url(#' + id + '-gold)'" />
          <rect x="2.5" y="12" width="12" height="29" [attr.fill]="'url(#' + id + '-rows)'" />
          <rect x="2.5" y="47" width="12" height="17" [attr.fill]="'url(#' + id + '-rows)'" />
          <rect class="chip" x="40" y="20" width="40" height="36" rx="3" [attr.fill]="'url(#' + id + '-pkg)'" />
          <rect class="mark" x="45" y="25" width="30" height="26" rx="1" />
          <rect class="chip small" x="94" y="26" width="26" height="24" rx="2.5" [attr.fill]="'url(#' + id + '-pkg)'" />
          @for (x of nand(); track x) {
            <rect class="chip" [attr.x]="x" y="18" width="44" height="40" rx="3" [attr.fill]="'url(#' + id + '-pkg)'" />
            <rect class="mark" [attr.x]="x + 5" y="23" width="34" height="30" rx="1" />
            <circle class="dot" [attr.cx]="x + 5" cy="23" r="1.2" />
          }
          @for (i of caps; track i) {
            <rect class="passive" [attr.x]="22 + i * 5" y="14" width="3" height="2" />
          }
          <circle class="hole" cx="286" cy="38" r="3" />
        }
        @case ('sata') {
          <rect class="case" x="0.5" y="0.5" width="239" height="159" rx="9" [attr.fill]="'url(#' + id + '-metal)'" />
          <rect class="plate" x="14.5" y="14.5" width="150" height="131" rx="5" />
          <path class="etch dark" d="M30 40H148M30 54H148M30 68H112" />
          @for (i of screws; track i) {
            <circle class="screw" [attr.cx]="i % 2 ? 224 : 14" [attr.cy]="i < 2 ? 24 : 136" r="2.5" />
          }
          <rect class="housing" x="176" y="118" width="58" height="28" rx="3" />
          <rect class="housing" x="176" y="84" width="24" height="26" rx="3" />
          @for (i of pins; track i) {
            <rect class="goldpin" [attr.x]="180 + i * 7" y="124" width="3" height="16" />
          }
          @for (i of pins7; track i) {
            <rect class="goldpin" [attr.x]="180 + i * 3" y="89" width="2" height="16" />
          }
        }
        @case ('soldered') {
          <rect class="substrate" x="0.5" y="0.5" width="199" height="99" rx="8" [attr.fill]="'url(#' + id + '-sub)'" />
          <rect class="lid-ring" x="10.5" y="10.5" width="179" height="79" rx="5" />
          <rect class="chip" x="24" y="24" width="48" height="52" rx="3" [attr.fill]="'url(#' + id + '-pkg)'" />
          <rect class="mark" x="29" y="29" width="38" height="42" rx="1" />
          <rect class="chip" x="88" y="24" width="38" height="52" rx="3" [attr.fill]="'url(#' + id + '-pkg)'" />
          <rect class="mark" x="93" y="29" width="28" height="42" rx="1" />
          <rect class="chip" x="138" y="24" width="38" height="52" rx="3" [attr.fill]="'url(#' + id + '-pkg)'" />
          <rect class="mark" x="143" y="29" width="28" height="42" rx="1" />
          <rect class="balls" x="21" y="21" width="54" height="58" rx="5" />
          <rect class="balls" x="85" y="21" width="44" height="58" rx="5" />
          <rect class="balls" x="135" y="21" width="44" height="58" rx="5" />
        }
        @case ('removable') {
          <rect class="housing" x="0.5" y="12.5" width="64" height="39" rx="3" />
          <rect class="metal" x="8.5" y="18.5" width="52" height="27" rx="1.5" [attr.fill]="'url(#' + id + '-metal)'" />
          <rect class="hole" x="22" y="28" width="6" height="8" rx="1" />
          <rect class="hole" x="40" y="28" width="6" height="8" rx="1" />
          <rect class="body" x="64.5" y="2.5" width="120" height="59" rx="12" [attr.fill]="'url(#' + id + '-pkg)'" />
          <rect class="body-inner" x="72.5" y="8.5" width="96" height="47" rx="8" />
          <circle class="hole" cx="170" cy="32" r="4" />
        }
      }
    </svg>
  `,
  styles: [
    HW_TOKENS,
    `
    :host { display: block; }
    svg { width: 100%; height: auto; display: block; }
    .pcb-a { stop-color: var(--hw-pcb-a); } .pcb-b { stop-color: var(--hw-pcb-b); }
    .pkg-a { stop-color: var(--hw-pkg-a); } .pkg-b { stop-color: var(--hw-pkg-b); }
    .gold-a { stop-color: var(--hw-gold-a); } .gold-b { stop-color: var(--hw-gold-b); }
    .metal-a { stop-color: var(--hw-metal-a); } .metal-b { stop-color: var(--hw-metal-b); }
    .sub-a { stop-color: var(--hw-substrate-a); } .sub-b { stop-color: var(--hw-substrate-b); }
    .gap { fill: var(--hw-gold-gap); fill-opacity: 0.55; }
    .pcb { stroke: var(--hw-pcb-edge); stroke-width: 1; }
    .pin1 { fill: var(--hw-gold-a); }
    .passive { fill: var(--hw-passive); fill-opacity: 0.7; }
    .lid, .case { stroke: var(--hw-metal-edge); stroke-width: 1; }
    .lid-inner { fill: none; stroke: var(--hw-metal-edge); stroke-opacity: 0.6; stroke-width: 1; }
    .etch { fill: none; stroke: var(--hw-metal-edge); stroke-width: 1; stroke-opacity: 0.7; shape-rendering: crispEdges; }
    .etch.dark { stroke: var(--hw-label-ink); stroke-opacity: 0.35; }
    .lid-notch { stroke: var(--hw-metal-edge); stroke-width: 1; }
    .gold { stroke: var(--hw-gold-b); stroke-width: 0.5; }
    .chip { stroke: var(--hw-pkg-edge); stroke-width: 1; }
    .mark { fill: none; stroke: var(--hw-pkg-mark); stroke-opacity: 0.35; stroke-width: 1; shape-rendering: crispEdges; }
    .dot { fill: var(--hw-pkg-mark); fill-opacity: 0.7; }
    .hole { fill: var(--hw-hole); stroke: var(--hw-hole-ring); }
    .plate { fill: var(--hw-label); stroke: var(--hw-metal-edge); stroke-opacity: 0.6; }
    .screw { fill: var(--hw-metal-b); stroke: var(--hw-metal-edge); }
    .housing { fill: var(--hw-pkg-b); stroke: var(--hw-pkg-edge); stroke-width: 1; }
    .goldpin { fill: var(--hw-gold-a); }
    .substrate { stroke: var(--hw-substrate-edge); stroke-width: 1; }
    .lid-ring { fill: none; stroke: var(--hw-lid-ring); stroke-opacity: 0.5; stroke-width: 1; }
    .balls { fill: none; stroke: var(--hw-pkg-mark); stroke-width: 2; stroke-dasharray: 0.1 4; stroke-linecap: round; stroke-opacity: 0.8; }
    .metal { stroke: var(--hw-metal-edge); stroke-width: 1; }
    .body { stroke: var(--hw-pkg-edge); stroke-width: 1; }
    .body-inner { fill: none; stroke: var(--hw-pkg-edge); stroke-opacity: 0.6; }
  `,
  ],
})
export class DeviceArt {
  readonly kind = input.required<DeviceArtKind>();
  /** Drive size in bytes: it sets the number of NAND packages on an M.2 stick. */
  readonly totalBytes = input(0);
  protected readonly id = `dy-dev-${(nextId++).toString()}`;
  protected readonly caps = [0, 1, 2, 3, 4, 5, 6];
  protected readonly screws = [0, 1, 2, 3];
  protected readonly pins = [0, 1, 2, 3, 4, 5, 6];
  protected readonly pins7 = [0, 1, 2, 3, 4, 5, 6, 7];
  protected readonly nand = computed(() => nandSlots(nandCount(this.totalBytes()), 130, 44, 6));
  protected readonly box = computed(() => {
    switch (this.kind()) {
      case 'cpu':
        return '0 0 120 120';
      case 'nvme':
        return '0 0 288 76';
      case 'sata':
        return '0 0 240 160';
      case 'soldered':
        return '0 0 200 100';
      case 'removable':
        return '0 0 186 64';
    }
  });
}
