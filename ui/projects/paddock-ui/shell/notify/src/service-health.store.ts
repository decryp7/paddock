import { computed, inject } from '@angular/core';
import { patchState, signalStore, withComputed, withHooks, withMethods, withState } from '@ngrx/signals';
import { ContributionRegistry } from '@decryp7/paddock-ui/shell';
import { errorMessage, setError, setLoaded, setLoading, withRequestStatus } from '@decryp7/paddock-ui/shell/store';
import { ServiceHealthBackend, type Unlisten } from './backends';
import { EMPTY_HEALTH, isServing, type ServiceHealth, type ServiceHealthSnapshot } from './wire.types';

interface HealthState {
  snapshot: ServiceHealthSnapshot;
  /** Bumped by "See what happened" links: the status chip opens its details. */
  detailsRequested: number;
  /** A repair request that was refused, per service (shown in the chip next to the button). */
  repairErrors: Readonly<Record<string, string>>;
}

const FALLBACK = (): string => $localize`:@@pd.health.no-answer:The service-health service did not answer.`;

/**
 * The latest service-health snapshot of this window and nothing else (ADR 0010, Debrief ADR 0055): the engine decides, an
 * event announces, this store holds the newest. Nothing is derived from error text or from which commands succeeded.
 * Until the first snapshot arrived the window claims nothing is healthy (`isReady` and `isServing` are false).
 * Provided per window by `provideShellNotify`.
 */
export const ServiceHealthStore = signalStore(
  { protectedState: true },
  withState<HealthState>({ snapshot: EMPTY_HEALTH, detailsRequested: 0, repairErrors: {} }),
  withRequestStatus(),
  withComputed((store, registry = inject(ContributionRegistry, { optional: true })) => ({
    /**
     * Every service as the engine reports it, named by the app's `defineHealthService` label when it registered one: that
     * label is a message in the window's language, the engine's is English (plan S15).
     */
    services: computed(() =>
      store.snapshot().services.map((s) => {
        const label = registry?.healthServices().find((d) => d.id === s.service)?.label.text;
        return label && label !== s.label ? { ...s, label } : s;
      }),
    ),
  })),
  withComputed((store) => ({
    /** The services that are not working (any state but ok and starting), in the registry's order. */
    problems: computed(() => store.services().filter((s) => !isServing(s.state))),
    /** The first snapshot arrived (by event or by the list). */
    isReady: computed(() => store.snapshot().revision > 0 || store.requestStatus() === 'loaded'),
  })),
  withMethods((store, backend = inject(ServiceHealthBackend)) => {
    // Events and answers can cross: a window keeps the newest revision.
    const apply = (next: ServiceHealthSnapshot): void => {
      if (next.revision >= store.snapshot().revision) patchState(store, { snapshot: next });
    };
    return {
      /** The service now, or null while the window has not heard of it. */
      health(service: string): ServiceHealth | null {
        return store.services().find((s) => s.service === service) ?? null;
      },
      /** Whether things that need `service` can be asked of it. Before the first snapshot: not. */
      isServing(service: string): boolean {
        if (!store.isReady()) return false;
        const h = store.services().find((s) => s.service === service);
        return !h || isServing(h.state);
      },
      /** A snapshot from the engine's event. */
      apply,
      /** Asks the engine for every service's state (when the window starts, and to try again). */
      async load(): Promise<void> {
        patchState(store, setLoading());
        try {
          const next = await backend.snapshot();
          apply(next);
          patchState(store, setLoaded());
        } catch (e) {
          patchState(store, setError(errorMessage(e, FALLBACK())));
        }
      },
      /** The Repair button: the shell repairs the service (never through the service); its state follows by event. */
      async repair(service: string): Promise<void> {
        patchState(store, (s) => ({ repairErrors: without(s.repairErrors, service) }));
        try {
          apply(await backend.repair(service));
        } catch (e) {
          patchState(store, (s) => ({ repairErrors: { ...s.repairErrors, [service]: errorMessage(e, FALLBACK()) } }));
        }
      },
      /** Opens the status chip's details (the "See what happened" link of `pd-waiting-for`). */
      showDetails(): void {
        patchState(store, (s) => ({ detailsRequested: s.detailsRequested + 1 }));
      },
    };
  }),
  withHooks((store, backend = inject(ServiceHealthBackend)) => {
    let off: Unlisten | undefined;
    let destroyed = false;
    return {
      onInit() {
        // Listen first, then ask: an event during the read is kept (newest revision wins), none is missed.
        void backend
          .listen((s) => store.apply(s))
          .then((u) => (destroyed ? u() : (off = u)))
          .catch((e: unknown) => console.warn(`[paddock-notify] cannot listen for service health: ${errorMessage(e, FALLBACK())}`));
        void store.load();
      },
      onDestroy() {
        destroyed = true;
        off?.();
      },
    };
  }),
);

export type ServiceHealthStoreType = InstanceType<typeof ServiceHealthStore>;

function without(r: Readonly<Record<string, string>>, key: string): Record<string, string> {
  const { [key]: _gone, ...rest } = r;
  return rest;
}
