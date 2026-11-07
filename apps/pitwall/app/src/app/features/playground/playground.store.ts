import { computed, inject } from '@angular/core';
import { patchState, signalStore, withComputed, withHooks, withMethods, withState } from '@ngrx/signals';
import type { CommandError, LocalModel, PlaygroundEvent, PlaygroundMessage } from '../../core/api.types';
import { BACKEND, type Unsubscribe } from '../../core/backend';
import { toCommandError } from '../../core/command-error';

export interface TurnStats {
  promptTokens: number;
  completionTokens: number;
  ttftMs: number;
  genTps: number;
  coldStartMs: number | null;
}

export interface Turn {
  role: 'user' | 'assistant';
  content: string;
  streaming: boolean;
  stopped: boolean;
  stats: TurnStats | null;
  error: CommandError | null;
}

export interface Pane {
  modelId: string | null;
  turns: Turn[];
}

interface PlaygroundState {
  models: LocalModel[];
  loading: boolean;
  error: CommandError | null;
  system: string;
  temperature: number;
  maxTokens: number;
  compare: boolean;
  /** This build has the chat (Rust decides); `null` until known. */
  chatEnabled: boolean | null;
  panes: [Pane, Pane];
}

const initial: PlaygroundState = {
  models: [],
  loading: false,
  error: null,
  system: '',
  temperature: 0.7,
  maxTokens: 512,
  compare: false,
  chatEnabled: null,
  panes: [
    { modelId: null, turns: [] },
    { modelId: null, turns: [] },
  ],
};

/** The messages to send for a pane: the system prompt, the pane's finished turns, and the new prompt. */
export function buildMessages(system: string, turns: Turn[], prompt: string): PlaygroundMessage[] {
  const out: PlaygroundMessage[] = [];
  if (system.trim()) out.push({ role: 'system', content: system.trim() });
  for (const t of turns) if (!t.error && t.content) out.push({ role: t.role, content: t.content });
  out.push({ role: 'user', content: prompt });
  return out;
}

/** Folds one stream event into the last (assistant) turn of a pane. */
export function applyEvent(pane: Pane, e: PlaygroundEvent): Pane {
  const turns = pane.turns.slice();
  const last = turns[turns.length - 1];
  if (!last || last.role !== 'assistant' || !last.streaming) return pane;
  if (e.kind === 'delta') turns[turns.length - 1] = { ...last, content: last.content + e.text };
  else if (e.kind === 'done') {
    const { kind: _kind, ...stats } = e;
    turns[turns.length - 1] = { ...last, streaming: false, stats };
  } else turns[turns.length - 1] = { ...last, streaming: false, error: e.error };
  return { ...pane, turns };
}

function mapPanes(panes: [Pane, Pane], f: (p: Pane) => Pane): [Pane, Pane] {
  return [f(panes[0]), f(panes[1])];
}

/** The chat-capable models, loaded ones first. */
export function chatModels(models: LocalModel[]): LocalModel[] {
  return models.filter((m) => m.capabilities.includes('chat')).sort((a, b) => Number(b.loaded) - Number(a.loaded) || a.name.localeCompare(b.name));
}

/** The playground: one or two panes chatting with local models, streamed over `backend.chat`. */
export const PlaygroundStore = signalStore(
  withState(initial),
  withComputed(({ models, panes, compare }) => ({
    chatModels: computed(() => chatModels(models())),
    streaming: computed(() => panes().some((p) => p.turns.some((t) => t.streaming))),
    activePanes: computed(() => (compare() ? [0, 1] : [0]) as (0 | 1)[]),
    empty: computed(() => panes().every((p) => p.turns.length === 0)),
  })),
  withMethods((store, backend = inject(BACKEND)) => {
    const streams: (Unsubscribe | null)[] = [null, null];

    const setPane = (i: 0 | 1, f: (p: Pane) => Pane) => {
      const panes = store.panes().slice() as [Pane, Pane];
      panes[i] = f(panes[i]);
      patchState(store, { panes });
    };

    const stop = () => {
      streams.forEach((s, i) => {
        s?.();
        streams[i] = null;
      });
      const panes = mapPanes(store.panes(), (p) => ({ ...p, turns: p.turns.map((t) => (t.streaming ? { ...t, streaming: false, stopped: true } : t)) }));
      patchState(store, { panes });
    };

    return {
      async load(): Promise<void> {
        patchState(store, { loading: true, error: null });
        if (store.chatEnabled() === null) {
          // A build that cannot say has no chat; with the chat, Compare always shows its two models (the one-model chat is the Chat tab).
          const chatEnabled = await (async () => backend.chatEnabled())().catch(() => false);
          patchState(store, { chatEnabled, ...(chatEnabled ? { compare: true } : {}) });
        }
        try {
          const models = await backend.localModels();
          patchState(store, { models, loading: false });
          const list = chatModels(models);
          const [a, b] = store.panes();
          if (!a.modelId && list[0]) setPane(0, (p) => ({ ...p, modelId: list[0]!.id }));
          if (!b.modelId && list.length) setPane(1, (p) => ({ ...p, modelId: (list[1] ?? list[0])!.id }));
        } catch (e) {
          patchState(store, { loading: false, error: toCommandError(e) });
        }
      },

      setModel(i: 0 | 1, modelId: string): void {
        setPane(i, (p) => ({ ...p, modelId }));
      },
      setSystem(system: string): void {
        patchState(store, { system });
      },
      setTemperature(temperature: number): void {
        if (Number.isFinite(temperature)) patchState(store, { temperature: Math.min(2, Math.max(0, temperature)) });
      },
      setMaxTokens(maxTokens: number): void {
        if (Number.isFinite(maxTokens)) patchState(store, { maxTokens: Math.round(Math.min(32768, Math.max(1, maxTokens))) });
      },
      setCompare(compare: boolean): void {
        if (store.streaming()) stop();
        patchState(store, { compare });
      },

      /** Sends the prompt to every active pane; each streams on its own. */
      send(prompt: string): boolean {
        const text = prompt.trim();
        if (!text || store.streaming()) return false;
        for (const i of store.activePanes()) {
          const pane = store.panes()[i];
          if (!pane.modelId) continue;
          const messages = buildMessages(store.system(), pane.turns, text);
          const user: Turn = { role: 'user', content: text, streaming: false, stopped: false, stats: null, error: null };
          const answer: Turn = { role: 'assistant', content: '', streaming: true, stopped: false, stats: null, error: null };
          setPane(i, (p) => ({ ...p, turns: [...p.turns, user, answer] }));
          streams[i] = backend.chat({ modelId: pane.modelId, messages, temperature: store.temperature(), maxTokens: store.maxTokens() }, (e) => {
            setPane(i, (p) => applyEvent(p, e));
            if (e.kind !== 'delta') streams[i] = null;
          });
        }
        return true;
      },

      stop,

      clear(): void {
        stop();
        patchState(store, { panes: mapPanes(store.panes(), (p) => ({ ...p, turns: [] })) });
      },
    };
  }),
  withHooks({
    onDestroy(store) {
      store.stop();
    },
  }),
);

export type PlaygroundStore = InstanceType<typeof PlaygroundStore>;
