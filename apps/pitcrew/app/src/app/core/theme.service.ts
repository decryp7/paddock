import { DOCUMENT } from '@angular/common';
import { Injectable, computed, inject, signal } from '@angular/core';
import { readLocal, writeLocal } from './storage';

export type ThemePreference = 'system' | 'light' | 'dark';
export type Theme = 'light' | 'dark';

const KEY = 'pitcrew.theme';

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

function isPreference(v: string | null): v is ThemePreference {
  return v === 'system' || v === 'light' || v === 'dark';
}

/** The theme: follows the system unless the user picked one (remembered in localStorage). */
@Injectable({ providedIn: 'root' })
export class ThemeService {
  private readonly doc = inject(DOCUMENT);
  private readonly media = this.doc.defaultView?.matchMedia?.('(prefers-color-scheme: dark)') ?? null;
  private readonly systemDark = signal(this.media?.matches ?? false);
  private readonly pref = signal<ThemePreference>(((v) => (isPreference(v) ? v : 'system'))(readLocal(KEY)));

  /** What the user picked. */
  readonly preference = this.pref.asReadonly();
  /** The theme in effect. */
  readonly theme = computed<Theme>(() => {
    const p = this.pref();
    return p === 'system' ? (this.systemDark() ? 'dark' : 'light') : p;
  });
  /** Chart colours for the theme in effect (re-read whenever it changes). */
  readonly chartTokens = computed<ChartTokens>(() => this.readTokens(this.theme()));

  constructor() {
    this.media?.addEventListener?.('change', (e) => this.systemDark.set(e.matches));
    this.apply(this.pref());
  }

  setPreference(p: ThemePreference): void {
    this.apply(p);
    this.pref.set(p);
    writeLocal(KEY, p);
  }

  private apply(p: ThemePreference): void {
    const root = this.doc.documentElement;
    if (p === 'system') delete root.dataset['theme'];
    else root.dataset['theme'] = p;
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
