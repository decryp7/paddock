import { patchState, signalStore, withMethods, withState } from '@ngrx/signals';
import type { Tone } from '../shared/labels';

export interface Toast {
  id: number;
  tone: Tone;
  icon: string;
  title: string;
  body: string | null;
}

interface ToastState {
  toasts: Toast[];
}

/** How long a toast stays (errors stay until dismissed). */
export const TOAST_MS = 6_000;
const MAX = 4;

/** Transient messages: notifications from the service and the results of the user's actions. */
export const ToastStore = signalStore(
  { providedIn: 'root' },
  withState<ToastState>({ toasts: [] }),
  withMethods((store) => {
    let next = 1;
    const dismiss = (id: number) => patchState(store, { toasts: store.toasts().filter((t) => t.id !== id) });
    return {
      show(toast: Omit<Toast, 'id'>): number {
        const id = next++;
        patchState(store, { toasts: [...store.toasts(), { ...toast, id }].slice(-MAX) });
        if (toast.tone !== 'danger') setTimeout(() => dismiss(id), TOAST_MS);
        return id;
      },
      dismiss,
    };
  }),
);
