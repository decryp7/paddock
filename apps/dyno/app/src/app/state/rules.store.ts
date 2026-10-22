import { computed, inject } from '@angular/core';
import { patchState, signalStore, withComputed, withMethods, withState } from '@ngrx/signals';
import type { CommandError, ProcessRef, ProfileId, Rule, RuleSet } from '../core/api.types';
import { BACKEND } from '../core/backend';
import { toCommandError } from '../core/command-error';
import { Notifier } from '../shared/notifier';
import { ActivityStore } from './activity.store';

interface RulesState {
  ruleSet: RuleSet | null;
  loading: boolean;
  error: CommandError | null;
  /** A change is being saved (disables the controls that would race it). */
  saving: boolean;
  /** The engine's message for the last rejected change, for forms that show it inline. */
  saveError: CommandError | null;
}

/** The rule set: profiles, rules, the protected list and the pause switch. Shared by the shell and the Rules page. */
export const RulesStore = signalStore(
  { providedIn: 'root' },
  withState<RulesState>({ ruleSet: null, loading: false, error: null, saving: false, saveError: null }),
  withComputed(({ ruleSet }) => ({
    loaded: computed(() => ruleSet() !== null),
    profile: computed(() => ruleSet()?.profile ?? 'normal'),
    paused: computed(() => ruleSet()?.paused ?? false),
    profiles: computed(() => ruleSet()?.profiles ?? []),
    activeProfile: computed(() => {
      const rs = ruleSet();
      return rs?.profiles.find((p) => p.id === rs.profile) ?? null;
    }),
  })),
  withMethods((store, backend = inject(BACKEND), notifier = inject(Notifier), activity = inject(ActivityStore)) => {
    async function mutate(run: () => Promise<RuleSet>, done: (rs: RuleSet) => string | null, quiet = false): Promise<boolean> {
      patchState(store, { saving: true, saveError: null });
      try {
        const ruleSet = await run();
        patchState(store, { ruleSet, saving: false, error: null });
        const message = done(ruleSet);
        if (message) notifier.show(message);
        return true;
      } catch (e) {
        const saveError = toCommandError(e);
        patchState(store, { saving: false, saveError });
        if (!quiet) notifier.error(saveError);
        return false;
      }
    }
    return {
      async load(): Promise<void> {
        patchState(store, { loading: true, error: null });
        try {
          const ruleSet = await backend.rules();
          patchState(store, { ruleSet, loading: false });
        } catch (e) {
          patchState(store, { loading: false, error: toCommandError(e) });
        }
      },
      /** Loads once (the shell and pages call it freely). */
      async ensure(): Promise<void> {
        if (!store.ruleSet() && !store.loading()) await this.load();
      },
      setProfile(profile: ProfileId): Promise<boolean> {
        return mutate(
          () => backend.setProfile(profile),
          (rs) => `${rs.profiles.find((p) => p.id === profile)?.name ?? 'Profile'} profile on`,
        );
      },
      pause(paused: boolean): Promise<boolean> {
        return mutate(
          () => backend.pauseRules(paused),
          () => (paused ? 'All rules paused. Everything Dyno did was released.' : 'Rules resumed'),
        );
      },
      /** Saves a changed or new rule (matched by id). `quiet` leaves a rejection to the caller (`saveError`) instead of a toast. */
      saveRule(rule: Rule, quiet = false): Promise<boolean> {
        const rs = store.ruleSet();
        if (!rs) return Promise.resolve(false);
        const exists = rs.rules.some((r) => r.id === rule.id);
        const rules = exists ? rs.rules.map((r) => (r.id === rule.id ? rule : r)) : [...rs.rules, rule];
        return mutate(() => backend.saveRules({ ...rs, rules }), () => (exists ? `Saved “${rule.name}”` : `Added “${rule.name}”`), quiet);
      },
      deleteRule(id: string): Promise<boolean> {
        const rs = store.ruleSet();
        if (!rs) return Promise.resolve(false);
        const name = rs.rules.find((r) => r.id === id)?.name ?? 'rule';
        return mutate(() => backend.saveRules({ ...rs, rules: rs.rules.filter((r) => r.id !== id) }), () => `Deleted “${name}”`);
      },
      toggleRule(id: string, enabled: boolean): Promise<boolean> {
        const rs = store.ruleSet();
        if (!rs) return Promise.resolve(false);
        const rules = rs.rules.map((r) => (r.id === id ? { ...r, enabled } : r));
        return mutate(() => backend.saveRules({ ...rs, rules }), () => null);
      },
      setProtected(list: string[]): Promise<boolean> {
        const rs = store.ruleSet();
        if (!rs) return Promise.resolve(false);
        return mutate(() => backend.saveRules({ ...rs, protected: list }), () => 'Protected list saved');
      },
      /** Releases what a rule is doing to a process right now. */
      async release(target: ProcessRef): Promise<boolean> {
        try {
          const entry = await backend.releaseAction(target);
          activity.receive(entry);
          notifier.show(entry.title);
          return true;
        } catch (e) {
          notifier.error(toCommandError(e));
          return false;
        }
      },
    };
  }),
);
