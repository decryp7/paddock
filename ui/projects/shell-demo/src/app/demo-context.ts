import { inject } from '@angular/core';
import { AppearanceStore, type AppContextFactory, type Appearance } from '@decryp7/paddock-ui/shell';
import { LanguageStore, PSEUDO_LOCALE } from '@decryp7/paddock-ui/shell/i18n';
import { DEMO_ENGINE } from './demo-engine';
import { DemoEngineData } from './demo-health';
import { GreetingLog } from './greeting-log';
import { RunnerData } from './runner-data';

/** The demo's part of the shell context: small and purpose-built (interface segregation). */
export interface DemoContext {
  readonly greetings: () => number;
  greet(): number;
  reset(): void;
  /** Starts the sample job (phases, a step that cannot be cancelled); resolves to its id once the engine took it. */
  startJob(): Promise<number>;
  /** Starts a job that reports nothing until cancelled ("Still working"). */
  startQuietJob(): Promise<number>;
  /** Whether the page is laid out compact (a menu toggle). */
  readonly compact: () => boolean;
  toggleCompact(): void;
  /** The demo engine fails (its Repair works); the engine panel's load meets it. */
  breakEngine(): Promise<void>;
  /** The search index fails (its Repair fails too). */
  breakIndex(): Promise<void>;
  /** The appearance choice, the same in every window (plan S15). */
  readonly theme: () => Appearance['theme'];
  setTheme(theme: Appearance['theme']): void;
  readonly highContrast: () => boolean;
  toggleHighContrast(): void;
  readonly reducedMotion: () => boolean;
  toggleReducedMotion(): void;
  /** Whether the window runs in the pseudo-locale; switching saves the choice and reloads the window. */
  readonly pseudoLocale: () => boolean;
  togglePseudoLocale(): void;
  /** Whether a theme pack (the demo's "Warm paper") is on. */
  readonly pack: () => string | null;
  setPack(id: string | null): void;
  /** Runners that need the user (the Runners page's badge). */
  readonly attention: () => number;
  /** Developer mode: the harness pages are offered (on unless local storage `sd.developer` is "off"). */
  readonly developer: () => boolean;
}

/** Whether the demo runs in developer mode (a per-machine flag; unreadable storage means on). */
export function developerMode(storage: Pick<Storage, 'getItem'> | null = safeStorage()): boolean {
  try {
    return storage?.getItem('sd.developer') !== 'off';
  } catch {
    return true;
  }
}

function safeStorage(): Storage | null {
  try {
    return globalThis.localStorage ?? null;
  } catch {
    return null;
  }
}

/** Built once, in the root injection context, when the shell first needs the context. */
export const demoContext: AppContextFactory<DemoContext> = () => {
  const log = inject(GreetingLog);
  const engine = inject(DEMO_ENGINE);
  const data = inject(DemoEngineData);
  const appearance = inject(AppearanceStore);
  const language = inject(LanguageStore);
  const runners = inject(RunnerData);
  return {
    greetings: log.greetings,
    greet: () => log.greet(),
    reset: () => log.reset(),
    startJob: () => engine.startImport(),
    startQuietJob: () => engine.startQuiet(),
    compact: log.compact,
    toggleCompact: () => log.toggleCompact(),
    breakEngine: async () => {
      await engine.breakEngine();
      data.load();
    },
    breakIndex: () => engine.breakIndex(),
    theme: appearance.theme,
    setTheme: (theme) => appearance.setTheme(theme),
    highContrast: () => appearance.resolved().highContrast,
    toggleHighContrast: () => appearance.setContrast(appearance.resolved().highContrast ? 'standard' : 'more'),
    reducedMotion: () => appearance.resolved().reducedMotion,
    toggleReducedMotion: () => appearance.setMotion(appearance.resolved().reducedMotion ? 'full' : 'reduce'),
    pack: () => appearance.pack()?.id ?? null,
    setPack: (id) => appearance.setPack(id),
    pseudoLocale: () => language.locale() === PSEUDO_LOCALE,
    togglePseudoLocale: () => {
      language.choose(language.locale() === PSEUDO_LOCALE ? 'en' : 'pseudo');
      language.reload();
    },
    attention: () => runners.attention,
    developer: (() => {
      const on = developerMode();
      return () => on;
    })(),
  };
};
