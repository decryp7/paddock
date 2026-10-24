import { InjectionToken, makeEnvironmentProviders, provideAppInitializer, type EnvironmentProviders } from '@angular/core';

/**
 * Which backend an app runs on (plan X28): the real one inside Tauri, the mock in a browser (`ng serve`, Playwright, unit
 * tests). The mock is imported lazily, so it stays out of the release bundle; and it is chosen only where `allowMock`
 * says so (a dev build), never because a release build was opened with `?backend=mock`.
 */

/** The two backends an app can run on. */
export type BackendKind = 'tauri' | 'mock';

/** What the app provides: how to make each backend. The mock factory should `import()` its module. */
export interface BackendChoice<B, O = unknown> {
  /** The real backend (Tauri IPC). */
  readonly tauri: () => B | Promise<B>;
  /** The mock backend, made with the options read from the URL (`parseMockOptions` of `@decryp7/paddock-ui/ipc/testing`). */
  readonly mock: (options: O) => B | Promise<B>;
}

/** The environment the choice is made in (injectable for tests). */
export interface BackendEnv<O = unknown> {
  /** Whether the page runs inside Tauri (`__TAURI_INTERNALS__` exists). */
  readonly inTauri: boolean;
  /** `location.search`. */
  readonly search: string;
  /** Whether the mock may be chosen in a Tauri-less page (true for dev builds and tests, false for release). */
  readonly allowMock: boolean;
  /** Reads the mock's options from the query string. */
  readonly mockOptions: (search: string) => O;
}

/** Whether the page runs inside Tauri. */
export function inTauri(globals: object = globalThis): boolean {
  return '__TAURI_INTERNALS__' in globals;
}

/**
 * The backend kind for an environment: Tauri when the page runs in it; otherwise the mock where `allowMock`, and an error
 * where it does not (a release build in a plain browser must say so, not silently show demo data).
 */
export function backendKind(env: Pick<BackendEnv, 'inTauri' | 'allowMock'>): BackendKind {
  if (env.inTauri) return 'tauri';
  if (env.allowMock) return 'mock';
  throw new Error('This build needs the desktop app: there is no Tauri bridge in this window and the mock backend is not part of a release build.');
}

/** Makes the backend the environment calls for. */
export async function chooseBackend<B, O>(choice: BackendChoice<B, O>, env: BackendEnv<O>): Promise<{ kind: BackendKind; backend: B }> {
  const kind = backendKind(env);
  return { kind, backend: kind === 'tauri' ? await choice.tauri() : await choice.mock(env.mockOptions(env.search)) };
}

/**
 * Provides `token` with the chosen backend. The choice is asynchronous (the mock is imported lazily), so it is made in an
 * app initializer, before the first component injects the token.
 */
export function provideBackend<B, O>(token: InjectionToken<B>, choice: BackendChoice<B, O>, env: () => BackendEnv<O>): EnvironmentProviders {
  const holder: { backend?: B } = {};
  return makeEnvironmentProviders([
    provideAppInitializer(async () => {
      holder.backend = (await chooseBackend(choice, env())).backend;
    }),
    {
      provide: token,
      useFactory: (): B => {
        if (holder.backend === undefined) throw new Error('The backend was injected before the app finished starting.');
        return holder.backend;
      },
    },
  ]);
}
