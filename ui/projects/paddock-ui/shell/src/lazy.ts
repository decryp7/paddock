import { createEnvironmentInjector, EnvironmentInjector, EnvironmentProviders, Provider, Type } from '@angular/core';

/** Loads a standalone component on first use: `() => import('./models-page').then((m) => m.ModelsPage)`. */
export type ComponentLoader<C = unknown> = () => Promise<Type<C>>;

/**
 * Loads the providers a view, page, dialog or status item needs (its stores and backends), on first use:
 * `() => import('./models.providers').then((m) => m.MODELS_PROVIDERS)`. They live in an environment injector of their
 * own, so a pop-out or a dialog window composes only what it shows (rule 34; plan §9).
 */
export type ProvidersLoader = () => Promise<readonly (Provider | EnvironmentProviders)[]>;

/** Something the shell renders on demand: the component and, optionally, the providers it needs. */
export interface LazyPart<C = unknown> {
  readonly load: ComponentLoader<C>;
  readonly providers?: ProvidersLoader;
}

/** A loaded part: the component and the injector to create it in (a child of `parent` when it declared providers). */
export interface LoadedPart<C = unknown> {
  readonly component: Type<C>;
  readonly injector: EnvironmentInjector;
}

/**
 * Loads a part's component and providers in parallel. The caller owns the returned injector when it differs from
 * `parent` and destroys it with the view it creates.
 */
export async function loadPart<C>(part: LazyPart<C>, parent: EnvironmentInjector, name = 'contribution'): Promise<LoadedPart<C>> {
  const [component, providers] = await Promise.all([part.load(), part.providers ? part.providers() : Promise.resolve(null)]);
  const injector = providers?.length ? createEnvironmentInjector([...providers], parent, name) : parent;
  return { component, injector };
}
