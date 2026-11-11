import { computed, inject } from '@angular/core';
import { patchState, signalStore, withComputed, withHooks, withMethods, withProps, withState } from '@ngrx/signals';
import { ShellContextHost } from '@decryp7/paddock-ui/shell';
import { errorMessage, setError, setLoaded, setLoading, withRequestStatus } from '@decryp7/paddock-ui/shell/store';
import { WorkbenchRegistry } from '@decryp7/paddock-ui/workbench';
import { ASSISTANT_LIMITS, type ApprovalChoice, type AssistantInfo, type Attachment, type ContextItem, type ContextUse, type ConversationSummary, type Envelope } from '../generated/assistant';
import { AssistantBackend, type Unlisten } from './backend';
import { RASTER_IMAGE_TYPES, AssistantAttachments, AssistantSpeech, attachmentChip, type AttachmentChip, type AttachRules, type AttachSource, type SpeechState } from './composer-ports';
import { answerText, chipText, conversationTitle, reduceTurn, setDeclined, startTurn, turnsOf, type TurnView } from './chat-model';
import { assistantMessage } from './wire';

/**
 * How a host gates the tools that write: `tick`, the user ticks "Allow actions" per message (each call still asks);
 * `per-call`, they are always offered and the engine's policy asks on every call (Pitwall: Dyno's own allow is the other gate).
 */
export type ActionsGate = 'tick' | 'per-call';

/** Where the conversation is: nothing going on, a question on its way to the engine, answering, or being stopped. */
export type AssistantRun = 'idle' | 'sending' | 'running' | 'stopping';

/** A context chip as the composer shows it. */
export interface ContextChip {
  readonly id: string;
  readonly label: string;
  readonly detail: string | null;
  readonly icon: string | null;
  /** It goes with the next question (the user can switch it off, or on). */
  readonly on: boolean;
  /** What the model will be told (bounded). */
  readonly text: string;
}

/** A write waiting for the user's decision. */
export interface PendingApproval {
  readonly callId: string;
  readonly name: string;
  readonly summary: string;
  /** The arguments of the call. */
  readonly input: unknown;
  /** What it would change cannot be undone. */
  readonly destructive: boolean;
  /** It changes things (an action): asked every call, its arguments shown at once. */
  readonly write: boolean;
  /** The engine offers "Always allow" for it. */
  readonly remember: boolean;
}

interface AssistantState {
  info: AssistantInfo | null;
  conversations: readonly ConversationSummary[];
  /** The conversation on screen; `null`: a new one (the first question creates it). */
  activeId: string | null;
  turns: readonly TurnView[];
  draft: string;
  /** Chips the user switched off for the next question (those on by default). */
  off: readonly string[];
  /** Chips the user switched on (those off by default). */
  on: readonly string[];
  allowWrite: boolean;
  actionsGate: ActionsGate;
  run: AssistantRun;
  /** The last command that failed, in the engine's words. */
  failure: string | null;
  listOpen: boolean;
  /** Conversations deleted but still restorable (until the undo time passes). */
  deleting: readonly { id: string; title: string; wasActive: boolean }[];
  /** The heading of a new conversation from its first question, until the engine's list names it. */
  pendingTitle: string | null;
  /** Files the engine took for the next question (ADR 0023 WP-4b). */
  attached: readonly AttachmentChip[];
  /** Files on their way to the engine. */
  attaching: number;
  /** What the host lets the composer attach (`null`: no attachments in this host, or not known yet). */
  attachRules: AttachRules | null;
  /** How full the model's context is for the conversation on screen, as the engine measured it (`null`: not measured). */
  context: ContextUse | null;
  /** The conversation on screen is full without the files waiting in the composer (then a new chat is the way on; otherwise
   *  the files are what does not fit). */
  chatFull: boolean;
  /** Push-to-talk, as the host's engine reports it. */
  speech: SpeechState;
}

const initialState: AssistantState = {
  info: null,
  conversations: [],
  activeId: null,
  turns: [],
  draft: '',
  off: [],
  on: [],
  allowWrite: false,
  actionsGate: 'tick',
  run: 'idle',
  failure: null,
  listOpen: false,
  deleting: [],
  pendingTitle: null,
  attached: [],
  attaching: 0,
  attachRules: null,
  context: null,
  chatFull: false,
  speech: { kind: 'idle' },
};

/** How long a deleted conversation can still be brought back. */
export const UNDO_MS = 8000;

const cut = (text: string, max: number): string => (text.length <= max ? text : `${text.slice(0, max - 1)}…`);

/**
 * The assistant pane's store (Debrief's `chat.store`, rules 30 to 32): the conversation on screen, the turn that streams in,
 * the context chips and the user's choices. The engine is the source of truth (turns, tools, budgets, approvals, saving);
 * this keeps what the pane shows and asks the backend for what the user chose. Provide it with the view (`ASSISTANT_VIEW`).
 */
export const AssistantStore = signalStore(
  { protectedState: true },
  withState(initialState),
  withRequestStatus({
    retry: false,
    kind: 'temporary',
    why: 'not adopted yet (ADR 0058 rollout step 3): the Retry of the first load reads again',
    until: '2026-12-31',
  }),
  withProps(() => ({
    _backend: inject(AssistantBackend),
    _registry: inject(WorkbenchRegistry),
    _host: inject(ShellContextHost),
    _attachments: inject(AssistantAttachments, { optional: true }),
    _speech: inject(AssistantSpeech, { optional: true }),
  })),
  withComputed((store) => {
    const chips = computed<readonly ContextChip[]>(() => {
      const ctx = store._host.context();
      if (!ctx) return [];
      const out: ContextChip[] = [];
      for (const def of store._registry.assistantContext) {
        let value;
        try {
          value = def.value(ctx);
        } catch {
          // One chip that throws is left out, never the composer.
          continue;
        }
        if (!value || typeof value.text !== 'string' || !value.text.trim()) continue;
        const defaultOn = def.defaultOn !== false;
        out.push({
          id: def.id,
          label: def.label.text,
          detail: value.detail ?? null,
          icon: def.icon ?? null,
          on: defaultOn ? !store.off().includes(def.id) : store.on().includes(def.id),
          text: cut(value.text, ASSISTANT_LIMITS.contextChars),
        });
      }
      return out.slice(0, ASSISTANT_LIMITS.contextItems);
    });
    const turn = computed(() => store.turns().at(-1) ?? null);
    const visible = computed(() => {
      const gone = store.deleting().map((d) => d.id);
      return store.conversations().filter((c) => !gone.includes(c.id));
    });
    return {
      chips,
      /** The conversations the list shows (those waiting out their undo time are hidden). */
      visibleConversations: visible,
      /** The conversation deleted last, still restorable. */
      undoable: computed(() => store.deleting().at(-1) ?? null),
      /** The pane's heading: the conversation's title, "New conversation" when there is none yet. */
      title: computed(() => {
        const id = store.activeId();
        const raw = id ? store.conversations().find((c) => c.id === id)?.title : null;
        return (raw && conversationTitle(raw)) || store.pendingTitle() || $localize`:@@pd.wb.assistant.new:New conversation`;
      }),
      /** The turn that streams in, or the last one. */
      lastTurn: turn,
      /** Something is going on: a question is sent and not finished. */
      busy: computed(() => store.run() !== 'idle'),
      /** A write waits for the user's decision. */
      approval: computed<PendingApproval | null>(() => {
        const t = turn();
        if (!t?.running) return null;
        for (const p of t.parts)
          if (p.kind === 'tool' && p.card.state === 'awaiting')
            return { callId: p.card.callId, name: p.card.name, summary: p.card.summary, input: p.card.input, destructive: p.card.destructive, write: p.card.write, remember: p.card.remember === true };
        return null;
      }),
      /** The chat fills what the model can read: nothing more can be sent in it (the engine refuses it too). */
      full: computed(() => store.context()?.level === 'full'),
      /**
       * What fills the model's window: `'chat'` (the conversation itself: start a new one), `'files'` (the files waiting in a
       * new chat, or the ones that tip a chat that fits: remove a file), `null` when it is not full.
       */
      fullBy: computed((): 'chat' | 'files' | null => {
        if (store.context()?.level !== 'full') return null;
        return store.attached().length && (!store.activeId() || !store.chatFull()) ? 'files' : 'chat';
      }),
      /**
       * Why the pictures waiting in the composer cannot be sent (the host's words: the chosen model cannot read pictures), or
       * `null`: none waits, or nothing blocks them.
       */
      imageBlock: computed((): string | null => {
        if (!store.attached().some((c) => c.attachment.kind === 'image')) return null;
        return store._attachments?.imageBlock() ?? null;
      }),
      canSend: computed(
        () =>
          store.run() === 'idle' &&
          store.draft().trim().length > 0 &&
          store.attaching() === 0 &&
          !(store._attachments?.imageBlock() && store.attached().some((c) => c.attachment.kind === 'image')) &&
          store.context()?.level !== 'full' &&
          store.speech().kind !== 'recording' &&
          store.speech().kind !== 'transcribing',
      ),
      /** The host takes attachments. */
      canAttach: computed(() => store.attachRules() !== null),
      /** Another file fits the next question. */
      roomToAttach: computed(() => {
        const rules = store.attachRules();
        return rules !== null && store.attached().length + store.attaching() < Math.min(rules.maxCount, ASSISTANT_LIMITS.attachments);
      }),
      /** The host has speech input. */
      canSpeak: computed(() => store._speech !== null),
      hasWriteTools: computed(() => store.info()?.hasWrite === true),
      /** The text limit the composer shows. */
      limit: computed(() => ASSISTANT_LIMITS.userChars),
    };
  }),
  withMethods((store) => {
    let unlisten: Unlisten | null = null;
    let unlistenSpeech: Unlisten | null = null;
    let disposed = false;
    const timers = new Map<string, ReturnType<typeof setTimeout>>();
    const refreshList = async (): Promise<void> => {
      try {
        patchState(store, { conversations: await store._backend.conversations() });
      } catch (e) {
        patchState(store, { failure: assistantMessage(e, $localize`:@@pd.wb.assistant.err.list:The conversations could not be listed.`) });
      }
    };
    /** Asks the engine how full the context is for the conversation on screen (the meter); a failure only hides it. */
    const refreshContext = async (): Promise<void> => {
      const id = store.activeId();
      const files = store.attached().map((c) => c.attachment);
      try {
        const context = await store._backend.context(id, files);
        // Full with files waiting in an open chat: is the chat full on its own (start a new one), or do the files tip it (remove
        // one)? The engine is asked once more, without them.
        const alone = context?.level === 'full' && id && files.length ? await store._backend.context(id, []) : context;
        if (store.activeId() === id && store.attached().length === files.length) patchState(store, { context, chatFull: alone?.level === 'full' });
      } catch {
        patchState(store, { context: null, chatFull: false });
      }
    };
    const update = (change: (t: TurnView) => TurnView): void => {
      const turns = store.turns();
      if (turns.length) patchState(store, { turns: [...turns.slice(0, -1), change(turns[turns.length - 1] as TurnView)] });
    };
    const onEvent = (e: Envelope): void => {
      // A new conversation's events can arrive before the ack that names it.
      if (store.activeId() === null && store.run() === 'sending') patchState(store, { activeId: e.conversationId });
      if (e.conversationId !== store.activeId()) {
        if (e.type === 'done' || e.type === 'error') void refreshList();
        return;
      }
      update((t) => reduceTurn(t, e));
      if (e.type === 'started') patchState(store, { run: 'running' });
      if (e.type === 'done' || e.type === 'error') {
        patchState(store, { run: 'idle' });
        void refreshList();
        void refreshContext();
      }
    };
    const remove = async (id: string): Promise<void> => {
      try {
        await store._backend.delete(id);
      } catch (e) {
        patchState(store, { failure: assistantMessage(e, $localize`:@@pd.wb.assistant.err.delete:The conversation could not be deleted.`) });
      }
      if (store.activeId() === id) {
        patchState(store, { activeId: null, turns: [], run: 'idle', pendingTitle: null });
        void refreshContext();
      }
      await refreshList();
    };
    const openNow = async (id: string): Promise<void> => {
      try {
        const c = await store._backend.load(id);
        patchState(store, { activeId: c.id, turns: turnsOf(c), failure: null, listOpen: false, pendingTitle: null });
        void refreshContext();
      } catch (e) {
        patchState(store, { failure: assistantMessage(e, $localize`:@@pd.wb.assistant.err.open:The conversation could not be opened.`) });
      }
    };
    /** Starts a turn with this question and context. */
    const submit = async (text: string, context: readonly ContextItem[], chips: readonly AttachmentChip[] = []): Promise<void> => {
      const allowWrite = (store.actionsGate() === 'per-call' || store.allowWrite()) && store.hasWriteTools();
      const conversationId = store.activeId();
      const attachments: Attachment[] = chips.map((c) => c.attachment);
      patchState(store, {
        run: 'sending',
        failure: null,
        draft: '',
        off: [],
        on: [],
        attached: [],
        listOpen: false,
        turns: [...store.turns(), startTurn(text, context, attachments)],
        // The heading names a new conversation from the moment its question is sent.
        ...(conversationId ? {} : { pendingTitle: conversationTitle(text) }),
      });
      try {
        const ack = await store._backend.send({ ...(conversationId ? { conversationId } : {}), text, context: [...context], allowWrite, ...(attachments.length ? { attachments } : {}) });
        patchState(store, { activeId: ack.conversationId });
      } catch (e) {
        // Refused before it started (empty, busy, gone): the question comes back to the draft.
        patchState(store, {
          run: 'idle',
          draft: text,
          attached: chips,
          failure: assistantMessage(e, $localize`:@@pd.wb.assistant.err.send:The question could not be sent.`),
          turns: store.turns().slice(0, -1),
          // A question that was not taken never names the conversation.
          ...(conversationId ? {} : { pendingTitle: null }),
        });
      }
    };
    return {
      /** Asks for the info and the conversations (also the Retry of a failed first load). */
      async load(): Promise<void> {
        patchState(store, setLoading());
        try {
          const [info, conversations, attachRules] = await Promise.all([
            store._backend.info(),
            store._backend.conversations(),
            // Attachments are optional: a host that cannot say its rules simply offers none.
            store._attachments ? store._attachments.rules().catch(() => null) : Promise.resolve(null),
          ]);
          patchState(store, { info, conversations, attachRules }, setLoaded());
          void refreshContext();
        } catch (e) {
          patchState(store, setError(errorMessage(e, $localize`:@@pd.wb.assistant.err.load:The assistant could not be reached.`)));
        }
      },
      /** Subscribes to the engine's events once. */
      async listen(): Promise<void> {
        if (unlisten || disposed) return;
        const stop = await store._backend.listen(onEvent);
        if (disposed) stop();
        else unlisten = stop;
        if (store._speech && !unlistenSpeech) {
          const stopSpeech = await store._speech.listen((speech) => patchState(store, { speech }));
          if (disposed) stopSpeech();
          else unlistenSpeech = stopSpeech;
        }
      },
      dispose(): void {
        disposed = true;
        // A delete still waiting out its undo time is what the user chose: it goes through.
        for (const [id, timer] of timers) {
          clearTimeout(timer);
          void store._backend.delete(id).catch(() => undefined);
        }
        timers.clear();
        unlisten?.();
        unlisten = null;
        unlistenSpeech?.();
        unlistenSpeech = null;
        // A recording nobody will finish is thrown away, never left running.
        if (store._speech && store.speech().kind === 'recording') void store._speech.cancel().catch(() => undefined);
      },
      setDraft(draft: string): void {
        patchState(store, { draft });
      },
      setAllowWrite(allowWrite: boolean): void {
        patchState(store, { allowWrite });
      },
      /** The host's gate for tools that write (the pane's `actionsGate` input). */
      setActionsGate(actionsGate: ActionsGate): void {
        patchState(store, { actionsGate });
      },
      toggleListOpen(): void {
        patchState(store, { listOpen: !store.listOpen() });
      },
      /** Switches a chip off (or on) for the next question. */
      toggleChip(id: string): void {
        if (!store.chips().some((c) => c.id === id)) return;
        const defaultOn = store._registry.assistantContext.find((d) => d.id === id)?.defaultOn !== false;
        const key = defaultOn ? 'off' : 'on';
        const list = store[key]();
        patchState(store, { [key]: list.includes(id) ? list.filter((x) => x !== id) : [...list, id] });
      },
      /** A new conversation: the next question starts it. */
      newConversation(): void {
        if (store.run() !== 'idle') return;
        patchState(store, { activeId: null, turns: [], failure: null, listOpen: false, pendingTitle: null });
        void refreshContext();
      },
      async open(id: string): Promise<void> {
        if (store.run() !== 'idle') return;
        await openNow(id);
      },
      remove,
      /** Deletes a conversation after the undo time: it leaves the list now and stays in the store until then. */
      removeLater(id: string): void {
        const c = store.conversations().find((x) => x.id === id);
        if (!c || timers.has(id)) return;
        const wasActive = store.activeId() === id;
        const rest = store.visibleConversations();
        const at = rest.findIndex((x) => x.id === id);
        const next = rest[at + 1] ?? rest[at - 1] ?? null;
        patchState(store, { deleting: [...store.deleting(), { id, title: c.title, wasActive }] });
        // The open conversation goes with its row: the view moves to the next one, or to a new one, at once.
        if (wasActive) {
          patchState(store, { activeId: null, turns: [], run: 'idle', pendingTitle: null });
          if (next) void openNow(next.id);
        }
        timers.set(
          id,
          setTimeout(() => {
            timers.delete(id);
            patchState(store, { deleting: store.deleting().filter((d) => d.id !== id) });
            void remove(id);
          }, UNDO_MS),
        );
      },
      /** Brings a conversation back that is still waiting out its undo time. */
      undoRemove(id: string): void {
        const timer = timers.get(id);
        if (timer === undefined) return;
        clearTimeout(timer);
        timers.delete(id);
        const was = store.deleting().find((d) => d.id === id);
        patchState(store, { deleting: store.deleting().filter((d) => d.id !== id) });
        if (was?.wasActive) void openNow(id);
      },
      /** Sends the draft with the chips that are on. */
      async send(): Promise<void> {
        if (!store.canSend()) return;
        const context: ContextItem[] = store
          .chips()
          .filter((c) => c.on)
          .map((c) => ({ id: c.id, label: chipText(c.label, c.detail), text: c.text }));
        await submit(store.draft().trim(), context, store.attached());
      },
      /** Asks the last question again, with the context it had (the Retry of a failed turn). */
      async retry(): Promise<void> {
        const last = store.turns().at(-1);
        if (!last || store.run() !== 'idle' || !last.error) return;
        await submit(last.user, last.context, last.attachments.map((a) => store._attachments?.chipOf(a) ?? attachmentChip(a)));
      },
      /** Stops the running turn (Escape, the Stop button). */
      async stop(): Promise<void> {
        const id = store.activeId();
        if (!id || (store.run() !== 'running' && store.run() !== 'sending')) return;
        patchState(store, { run: 'stopping' });
        try {
          await store._backend.stop(id);
        } catch (e) {
          patchState(store, { failure: assistantMessage(e, $localize`:@@pd.wb.assistant.err.stop:The assistant could not be stopped.`) });
        }
      },
      /** The user's choice on a call that waits (Allow once, Always allow when offered, Deny). */
      async approve(callId: string, choice: ApprovalChoice): Promise<void> {
        const id = store.activeId();
        if (!id) return;
        const denied = choice === 'deny';
        // A declined call is the user's choice: mark it before the engine's "did not run" arrives, so it is never drawn as a failure.
        if (denied) update((t) => setDeclined(t, callId, true));
        try {
          await store._backend.approve(id, callId, choice);
        } catch (e) {
          if (denied) update((t) => setDeclined(t, callId, false));
          patchState(store, { failure: assistantMessage(e, $localize`:@@pd.wb.assistant.err.approve:The decision could not be sent.`) });
        }
      },
      /**
       * Hands files to the host's engine (picker, paste, drop), one after another in the order given. Files past the count or
       * the size the host allows are refused here first (no point reading them); the engine checks every other one again.
       * Refusals are said together, in the engine's words.
       */
      async attach(files: readonly AttachSource[]): Promise<void> {
        const port = store._attachments;
        const rules = store.attachRules();
        if (!port || !rules || !files.length) return;
        const refused: string[] = [];
        const max = Math.min(rules.maxCount, ASSISTANT_LIMITS.attachments);
        for (const file of files) {
          if (store.attached().length + store.attaching() >= max) {
            refused.push($localize`:@@pd.wb.assistant.attach.too-many:A question can carry at most ${max}:max: attachments.`);
            break;
          }
          const isImage = RASTER_IMAGE_TYPES.includes(file.type);
          const block = isImage ? port.imageBlock() : null;
          if (block) {
            // An image for a model that cannot read images: the host's words, at once (the engine would refuse it too).
            refused.push(block);
            continue;
          }
          const largest = isImage ? (rules.maxImageBytes ?? rules.maxFileBytes) : rules.maxFileBytes;
          if (isImage && file.size > largest) {
            const mib = Math.floor(largest / (1024 * 1024));
            const kib = Math.floor(largest / 1024);
            refused.push(
              mib > 0
                ? $localize`:@@pd.wb.assistant.attach.image-too-big:"${file.name}:name:" is larger than ${mib}:mib: MiB. Attach a smaller image, or export a smaller copy.`
                : kib > 0
                  ? $localize`:@@pd.wb.assistant.attach.image-too-big-kib:"${file.name}:name:" is larger than ${kib}:kib: KiB. Attach a smaller image, or export a smaller copy.`
                  : $localize`:@@pd.wb.assistant.attach.image-too-big-bytes:"${file.name}:name:" is larger than ${largest}:bytes: bytes. Attach a smaller image, or export a smaller copy.`,
            );
            continue;
          }
          if (!isImage && file.size > rules.maxFileBytes) {
            const kib = Math.floor(rules.maxFileBytes / 1024);
            refused.push(
              kib > 0
                ? $localize`:@@pd.wb.assistant.attach.too-big:"${file.name}:name:" is larger than ${kib}:kib: KiB. Attach a smaller file, or paste the part that matters.`
                : $localize`:@@pd.wb.assistant.attach.too-big-bytes:"${file.name}:name:" is larger than ${rules.maxFileBytes}:bytes: bytes. Attach a smaller file, or paste the part that matters.`,
            );
            continue;
          }
          patchState(store, { attaching: store.attaching() + 1 });
          try {
            const chip = await port.add(file);
            const known = store.attached().some((c) => c.attachment.sha256 === chip.attachment.sha256);
            if (!known) patchState(store, { attached: [...store.attached(), chip] });
          } catch (e) {
            refused.push(assistantMessage(e, $localize`:@@pd.wb.assistant.attach.failed:"${file.name}:name:" could not be attached.`));
          } finally {
            patchState(store, { attaching: store.attaching() - 1 });
          }
        }
        if (refused.length) patchState(store, { failure: refused.join(' ') });
        void refreshContext();
      },
      /** Takes an attachment off the next question. */
      detach(sha256: string): void {
        const gone = store.attached().find((c) => c.attachment.sha256 === sha256);
        if (!gone) return;
        patchState(store, { attached: store.attached().filter((c) => c !== gone) });
        // The host may forget the bytes; a failure there changes nothing for the question.
        void store._attachments?.remove(gone.attachment).catch(() => undefined);
        void refreshContext();
      },
      /**
       * Push-to-talk: starts recording, or stops it and puts the transcript into the question (after what is typed), to read
       * and edit before sending. Nothing is sent by voice alone.
       */
      async toggleSpeech(): Promise<void> {
        const port = store._speech;
        if (!port) return;
        const state = store.speech().kind;
        try {
          if (state === 'recording') {
            const heard = (await port.finish()).trim();
            if (heard) {
              const draft = store.draft();
              patchState(store, { draft: draft.trim() ? `${draft.replace(/\s+$/, '')} ${heard}` : heard });
            }
          } else if (state === 'idle' || state === 'failed') {
            await port.start();
          }
        } catch (e) {
          patchState(store, { failure: assistantMessage(e, $localize`:@@pd.wb.assistant.speech.failed:Speech input did not work.`) });
        }
      },
      /** Throws a recording away (Escape while recording). */
      async cancelSpeech(): Promise<void> {
        if (!store._speech || store.speech().kind !== 'recording') return;
        try {
          await store._speech.cancel();
        } catch (e) {
          patchState(store, { failure: assistantMessage(e, $localize`:@@pd.wb.assistant.speech.failed:Speech input did not work.`) });
        }
      },
      /** Measures the context again (the meter). */
      refreshContext,
      dismissFailure(): void {
        patchState(store, { failure: null });
      },
      /** The answer so far (the last turn's text). */
      answer(): string {
        const t = store.turns().at(-1);
        return t ? answerText(t) : '';
      },
    };
  }),
  withHooks({
    onInit(store) {
      void store._host.load();
      void store.listen();
      void store.load();
    },
    onDestroy(store) {
      store.dispose();
    },
  }),
);

export type AssistantStoreType = InstanceType<typeof AssistantStore>;
