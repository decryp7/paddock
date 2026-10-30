import { DOCUMENT } from '@angular/common';
import { DestroyRef, Injectable, computed, inject, signal } from '@angular/core';
import { AppearanceStore } from '@decryp7/paddock-ui/shell';

export type Theme = 'light' | 'dark';

/** The colours charts need, read from the CSS tokens of the current theme. */
export interface ChartTokens {
  theme: Theme;
  font: string;
  text: string;
  text2: string;
  text3: string;
  surface: string;
  grid: string;
  axis: string;
  series: [string, string, string, string];
  muted: string;
  faint: string;
  success: string;
  info: string;
  warning: string;
  danger: string;
  accent: string;
}

/**
 * The colours charts need, read from the CSS tokens of the current look. The look itself (theme, contrast, accent) is the
 * shell's appearance (Settings > Appearance, the same in every window); this only reads what it produced.
 */
@Injectable({ providedIn: 'root' })
export class ThemeService {
  private readonly doc = inject(DOCUMENT);
  private readonly appearance = inject(AppearanceStore);
  /** Bumped when the page's look attributes change, so tokens are read after the shell applied them (not before). */
  private readonly applied = signal(0);

  /** The theme in effect. */
  readonly theme = computed<Theme>(() => this.appearance.resolved().scheme);
  /** Chart colours for the theme in effect (re-read whenever the look changes). */
  readonly chartTokens = computed<ChartTokens>(() => {
    this.applied();
    return this.readTokens(this.theme());
  });

  constructor() {
    const MO = this.doc.defaultView?.MutationObserver;
    if (!MO) return;
    const observer = new MO(() => this.applied.update((n) => n + 1));
    observer.observe(this.doc.documentElement, { attributes: true, attributeFilter: ['data-theme', 'data-contrast', 'data-accent'] });
    inject(DestroyRef).onDestroy(() => observer.disconnect());
  }

  private readTokens(theme: Theme): ChartTokens {
    const style = this.doc.defaultView?.getComputedStyle(this.doc.documentElement);
    const v = (name: string, fallback: string) => style?.getPropertyValue(name).trim() || fallback;
    const dark = theme === 'dark';
    return {
      theme,
      font: v('--font-sans', 'system-ui, sans-serif'),
      text: v('--text', dark ? '#ececf0' : '#17181c'),
      text2: v('--text-2', dark ? '#a3a7b1' : '#555a65'),
      text3: v('--text-3', dark ? '#868a96' : '#6b707c'),
      surface: v('--surface', dark ? '#1b1c21' : '#ffffff'),
      grid: v('--chart-grid', dark ? '#24262c' : '#ececf0'),
      axis: v('--chart-axis', dark ? '#353841' : '#d2d3da'),
      series: [
        v('--chart-1', dark ? '#3987e5' : '#2a78d6'),
        v('--chart-2', dark ? '#d95926' : '#eb6834'),
        v('--chart-3', dark ? '#199e70' : '#1baf7a'),
        v('--chart-4', dark ? '#c98500' : '#eda100'),
      ],
      muted: v('--chart-muted', dark ? '#5a5e69' : '#b9bcc5'),
      faint: v('--chart-faint', dark ? '#3a3d46' : '#dcdee3'),
      success: v('--success', dark ? '#3fbfa3' : '#0b7563'),
      info: v('--info', dark ? '#6aa6ff' : '#1f5fc4'),
      warning: v('--warning', dark ? '#f0a243' : '#9a4b00'),
      danger: v('--danger', dark ? '#ff7a6b' : '#c22f1c'),
      accent: v('--accent', dark ? '#8b97ff' : '#4152d9'),
    };
  }
}
