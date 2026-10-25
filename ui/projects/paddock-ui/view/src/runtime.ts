import { computed, inject, Injectable, signal, type Signal } from '@angular/core';
import { errorMessage } from '@decryp7/paddock-ui/shell/store';
import type { ActionRef, GrantRef, Link, Node, SecretRef, ViewAction } from '../generated/view';
import { VIEW_HOST, type ViewPicked } from './host';
import { validateViewAction } from './limits';
import { mergeArgs, primaryNode } from './model';

/** What `<pd-view>` gives its runtime: the view's id, whether the host is busy, and where actions go. */
export interface ViewRuntimeContext {
  readonly view: Signal<string>;
  /** The document's title: it names the tables, drawings and lists inside it. */
  readonly title: Signal<string>;
  /** The root of the document on screen (null before the first), for the one primary button of the view. */
  readonly root: Signal<Node | null>;
  readonly busy: Signal<boolean>;
  readonly emit: (action: ViewAction) => void;
}

/**
 * What the nodes of one `<pd-view>` call to act (provided by `PdView`, so each view has its own): builds the `ViewAction` from an
 * action reference plus the host's own keys (`row`, `page`, `node`, `finding`, `reason`, `values`, `column`, `descending`), asks the
 * `VIEW_HOST` to confirm when the action has a question, checks the action's limits, and emits it. It never talks to the engine
 * itself. A refusal of the host (or a limit) is `error`, shown inline by the view.
 */
@Injectable()
export class ViewRuntime {
  private readonly host = inject(VIEW_HOST);
  private readonly context = signal<ViewRuntimeContext | null>(null);
  /** A confirmation or a picker is open. */
  readonly pending = signal(false);
  /** The last refusal, as text; cleared by the next try. */
  readonly error = signal<string | null>(null);
  /** Controls are off while the host is busy with the last action or the user is being asked something. */
  readonly disabled = computed(() => this.pending() || this.context()?.busy() === true);
  /** The node that draws its action as the view's one filled button (`primaryNode`); every other action is secondary. */
  readonly primary = computed(() => primaryNode(this.context()?.root()));
  /** The document's title, for the accessible names of its parts. */
  readonly title = computed(() => this.context()?.title() ?? '');

  /** Called once by `PdView`. */
  attach(ctx: ViewRuntimeContext): void {
    this.context.set(ctx);
  }

  /** Chooses an action of the plugin: confirms when it asks to, then sends it with the host's keys. */
  async run(ref: ActionRef, extra: Readonly<Record<string, unknown>> = {}): Promise<boolean> {
    const ctx = this.context();
    if (!ctx || this.disabled()) return false;
    const view = ctx.view();
    this.error.set(null);
    if (ref.confirm) {
      this.pending.set(true);
      try {
        if (!(await this.host.confirm({ question: ref.confirm, action: ref, writes: ref.writes === true }))) return false;
      } catch (e) {
        this.error.set(errorMessage(e, $localize`:@@pd.view.confirm-failed:The question could not be asked, so nothing was done.`));
        return false;
      } finally {
        this.pending.set(false);
      }
    }
    const action: ViewAction = { view, action: ref.id, args: mergeArgs(ref.args, extra) };
    const issue = validateViewAction(action);
    if (issue) {
      this.error.set(issue.message);
      return false;
    }
    ctx.emit(action);
    return true;
  }

  /** Follows a link: an action runs like any other; a web address goes to the host, which confirms and opens it. */
  async follow(link: Link, extra: Readonly<Record<string, unknown>> = {}): Promise<boolean> {
    if ('action' in link) return this.run(link.action, extra);
    if (this.disabled()) return false;
    this.error.set(null);
    this.pending.set(true);
    try {
      await this.host.openInBrowser({ url: link.openInBrowser.url, label: link.openInBrowser.label });
      return true;
    } catch (e) {
      this.error.set(errorMessage(e, $localize`:@@pd.view.open-failed:The page could not be opened.`));
      return false;
    } finally {
      this.pending.set(false);
    }
  }

  /** The host's picker for a path field: the grant and the name to show, or null when cancelled or refused (then `error` says why). */
  async pickPath(fieldId: string, label: string, folder: boolean): Promise<ViewPicked<GrantRef> | null> {
    return this.pick(() => this.host.pickPath({ fieldId, label, folder }));
  }

  /** The host's picker for a secret field: a `SecretRef` (never the secret) and its name, or null. */
  async pickSecret(fieldId: string, label: string): Promise<ViewPicked<SecretRef> | null> {
    return this.pick(() => this.host.pickSecret({ fieldId, label }));
  }

  private async pick<T>(ask: () => Promise<T | null>): Promise<T | null> {
    if (this.disabled()) return null;
    this.error.set(null);
    this.pending.set(true);
    try {
      return await ask();
    } catch (e) {
      this.error.set(errorMessage(e, $localize`:@@pd.view.pick-failed:The choice could not be made.`));
      return null;
    } finally {
      this.pending.set(false);
    }
  }
}
