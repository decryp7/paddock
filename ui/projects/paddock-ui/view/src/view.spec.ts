import { Component, provideZonelessChangeDetection, signal, viewChild } from '@angular/core';
import { TestBed, type ComponentFixture } from '@angular/core/testing';
import { PD_ECHARTS, fakeEchartsLoader } from '@decryp7/paddock-ui/chart';
import { FakeECharts, all, buttonByText, one, press, text, typeInto } from '@decryp7/paddock-ui/testing';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import type { ActionRef, GrantRef, Node, NodeKind, SecretRef, ViewAction, ViewDoc } from '../generated/view';
import { VIEW_EXTRA_LIMITS } from './limits';
import { KNOWN_KINDS, shapeOf } from './model';
import { NO_VIEW_HOST, VIEW_HOST, type ViewConfirmRequest, type ViewHost, type ViewOpenRequest, type ViewPickRequest } from './host';
import { sampleViewDoc } from './sample';
import { fieldProblem, initialValues } from './form';
import { PdView } from './view';

/** A host that records what it was asked and answers as the test says. */
class FakeHost implements ViewHost {
  confirms: ViewConfirmRequest[] = [];
  opens: ViewOpenRequest[] = [];
  paths: ViewPickRequest[] = [];
  secrets: ViewPickRequest[] = [];
  answer = true;
  failWith: Error | null = null;
  grant: GrantRef | null = 'grant-7';
  secret: SecretRef | null = 'secret-3';
  async confirm(r: ViewConfirmRequest): Promise<boolean> {
    this.confirms.push(r);
    if (this.failWith) throw this.failWith;
    return this.answer;
  }
  async openInBrowser(r: ViewOpenRequest): Promise<void> {
    this.opens.push(r);
    if (this.failWith) throw this.failWith;
  }
  async pickPath(r: ViewPickRequest): Promise<GrantRef | null> {
    this.paths.push(r);
    if (this.failWith) throw this.failWith;
    return this.grant;
  }
  async pickSecret(r: ViewPickRequest): Promise<SecretRef | null> {
    this.secrets.push(r);
    if (this.failWith) throw this.failWith;
    return this.secret;
  }
}

@Component({
  imports: [PdView],
  template: `<pd-view view="demo.view" [doc]="doc()" [busy]="busy()" [loading]="loading()" [showTitle]="showTitle()" (action)="actions.push($event)" (retry)="retries = retries + 1" />`,
})
class Host {
  readonly doc = signal<unknown>(sampleViewDoc());
  readonly busy = signal(false);
  readonly loading = signal(false);
  readonly showTitle = signal(true);
  readonly view = viewChild.required(PdView);
  readonly actions: ViewAction[] = [];
  retries = 0;
}

async function settle(f: { whenStable(): Promise<unknown> }, rounds = 3): Promise<void> {
  for (let i = 0; i < rounds; i++) {
    await f.whenStable();
    TestBed.tick();
    await Promise.resolve();
  }
}

/** The sample with one edit: `change` gets the root's children to edit in place. */
function edited(change: (children: Node[]) => void, etag = 1): ViewDoc {
  const doc = structuredClone(sampleViewDoc(etag));
  const kind = doc.root.kind as { stack: { children: Node[] } };
  change(kind.stack.children);
  return doc;
}
const child = (children: Node[], id: string): Node => children.find((c) => c.id === id) as Node;
const act = (id: string, over: Partial<ActionRef> = {}): ActionRef => ({ id, label: id, confirm: null, args: null, writes: false, ...over });

describe('pd-view', () => {
  let host: FakeHost;
  let fake: FakeECharts;

  async function setup(doc: unknown = sampleViewDoc(), provideHost = true): Promise<ComponentFixture<Host>> {
    host = new FakeHost();
    fake = new FakeECharts();
    TestBed.configureTestingModule({
      providers: [provideZonelessChangeDetection(), { provide: PD_ECHARTS, useValue: fakeEchartsLoader(fake.init) }, ...(provideHost ? [{ provide: VIEW_HOST, useValue: host }] : [])],
    });
    const f = TestBed.createComponent(Host);
    f.componentInstance.doc.set(doc);
    document.body.appendChild(f.nativeElement);
    await settle(f);
    return f;
  }
  const el = (f: ComponentFixture<Host>): HTMLElement => f.nativeElement as HTMLElement;
  const node = (f: ComponentFixture<Host>, id: string): HTMLElement => one(el(f), `[data-node-id="${id}"]`);
  const last = (f: ComponentFixture<Host>): ViewAction => f.componentInstance.actions.at(-1) as ViewAction;

  afterEach(() => {
    document.body.replaceChildren();
    TestBed.resetTestingModule();
  });

  describe('the document', () => {
    it('draws the title and a node of every kind, and one "update the app" for the kind it does not know', async () => {
      const f = await setup();
      expect(text(one(el(f), '[data-testid=view-title]'))).toBe('Release readiness');
      expect(one(el(f), 'article').getAttribute('aria-label')).toBe('Release readiness');
      const kinds = new Set(all(el(f), '[data-kind]').map((n) => n.getAttribute('data-kind')));
      for (const k of KNOWN_KINDS) expect(kinds.has(k), k).toBe(true);
      expect(all(el(f), '[data-testid=view-unknown]')).toHaveLength(1);
      expect(text(node(f, 'future'))).toBe('Update the app to view this');
      // The rest of the view still draws.
      expect(text(node(f, 'h'))).toContain('Release 1.4: what is left');
    });

    it('binds every text as text, never as markup', async () => {
      const evil = '<img src=x onerror=alert(1)><script>alert(2)</script>';
      const doc = edited((c) => {
        c.push({ id: 'evil', kind: { text: { text: evil, tone: 'neutral', style: 'body' } } });
        c.push({ id: 'evil2', kind: { badge: { text: evil, tone: 'bad' } } });
      });
      const f = await setup(doc);
      expect(el(f).querySelector('img')).toBeNull();
      expect(el(f).querySelector('script')).toBeNull();
      expect(text(node(f, 'evil'))).toContain(evil);
      expect(text(node(f, 'evil2'))).toContain(evil);
    });

    it('does not draw the title when asked not to', async () => {
      const f = await setup();
      f.componentInstance.showTitle.set(false);
      await settle(f);
      expect(el(f).querySelector('[data-testid=view-title]')).toBeNull();
    });

    it('maps text styles, tones and progress to roles, shapes and values', async () => {
      const f = await setup();
      expect(node(f, 'h').querySelector('p')?.getAttribute('role')).toBe('heading');
      expect(node(f, 'h').querySelector('p')?.getAttribute('aria-level')).toBe('3');
      expect(node(f, 'code').querySelector('p')?.className).toContain('pd-view-text--code');
      expect(node(f, 'cap').querySelector('p')?.getAttribute('role')).toBeNull();
      const bad = node(f, 'b5');
      expect(bad.querySelector('.pd-view-badge')?.className).toContain('pd-view-tone-bad');
      expect(text(bad)).toBe('Problem: Failing');
      expect(text(node(f, 'b1'))).toBe('Neutral');
      expect(node(f, 'prog').querySelector('[role=progressbar]')?.getAttribute('aria-valuenow')).toBe('62');
      expect(text(node(f, 'prog'))).toContain('Checks passed');
      expect(text(node(f, 'prog'))).toContain('62%');
      const row = node(f, 'badges').querySelector('.pd-view-stack');
      expect(row?.className).toContain('pd-view-stack--row');
    });

    it('draws a progress without a label and clamps a value out of range', async () => {
      const doc = edited((c) => {
        child(c, 'prog').kind = { progress: { value: 0.004, label: '' } };
      });
      const f = await setup(doc);
      expect(text(node(f, 'prog'))).toContain('Progress');
      expect(node(f, 'prog').querySelector('[role=progressbar]')?.getAttribute('aria-valuenow')).toBe('0');
    });

    it('shows loading while the host fetches and no document yet', async () => {
      const f = await setup(null);
      expect(el(f).querySelector('article')).toBeNull();
      f.componentInstance.loading.set(true);
      await settle(f);
      expect(text(one(el(f), '[data-testid=view-loading]'))).toBe('Loading…');
      expect(el(f).querySelector('pd-view')?.getAttribute('aria-busy')).toBe('true');
      f.componentInstance.doc.set(sampleViewDoc());
      await settle(f);
      expect(el(f).querySelector('[data-testid=view-loading]')).toBeNull();
      expect(el(f).querySelector('article')).not.toBeNull();
    });

    it('turns every control off while the host is busy', async () => {
      const f = await setup();
      f.componentInstance.busy.set(true);
      await settle(f);
      expect(el(f).querySelector('pd-view')?.getAttribute('aria-busy')).toBe('true');
      buttonByText(el(f), 'Run the checks').click();
      await settle(f);
      expect(f.componentInstance.actions).toHaveLength(0);
      expect(buttonByText(el(f), 'Run the checks').disabled).toBe(true);
    });
  });

  describe('errors', () => {
    it('shows an inline error with Retry for a document that breaks a limit, and nothing of it', async () => {
      const f = await setup({ ...sampleViewDoc(), schema: 99 });
      const err = one(el(f), '[data-testid=view-error]');
      expect(err.getAttribute('role')).toBe('alert');
      expect(text(err)).toContain('This view cannot be shown');
      expect(text(err)).toContain('Update the app');
      expect(el(f).querySelector('article')).toBeNull();
      one(el(f), '[data-testid=view-retry]').click();
      expect(f.componentInstance.retries).toBe(1);
      f.componentInstance.doc.set(sampleViewDoc(2));
      await settle(f);
      expect(el(f).querySelector('[data-testid=view-error]')).toBeNull();
      expect(el(f).querySelector('article')).not.toBeNull();
    });

    it('refuses a document that is not JSON of the right shape without throwing', async () => {
      for (const bad of ['text', 5, [], { schema: 1 }]) {
        const f = await setup(bad);
        expect(el(f).querySelector('[data-testid=view-error]'), String(bad)).not.toBeNull();
        TestBed.resetTestingModule();
        document.body.replaceChildren();
      }
    });
  });

  describe('patches', () => {
    const caption = { id: 'cap', kind: { text: { text: 'Updated just now', tone: 'neutral', style: 'caption' } } } satisfies Node;

    it('replaces a node by id and keeps the rest', async () => {
      const f = await setup();
      const r = f.componentInstance.view().applyPatch({ replace: { baseEtag: 1, etag: 2, id: 'cap', node: caption } });
      await settle(f);
      expect(r.ok).toBe(true);
      expect(text(node(f, 'cap'))).toBe('Updated just now');
      expect(text(node(f, 'h'))).toContain('Release 1.4');
    });

    it('replaces the whole document', async () => {
      const f = await setup();
      const next = sampleViewDoc(2);
      next.title = 'Another title';
      f.componentInstance.view().applyPatch({ doc: next });
      await settle(f);
      expect(text(one(el(f), '[data-testid=view-title]'))).toBe('Another title');
    });

    it('shows a stale patch as an error with Retry and keeps the document on screen', async () => {
      const f = await setup();
      const r = f.componentInstance.view().applyPatch({ replace: { baseEtag: 7, etag: 8, id: 'cap', node: caption } });
      await settle(f);
      expect(r.ok).toBe(false);
      const err = one(el(f), '[data-testid=view-error]');
      expect(err.getAttribute('data-kind')).toBe('stale');
      expect(text(err)).toContain('out of date');
      expect(text(node(f, 'cap'))).toBe('Updated a minute ago');
      one(el(f), '[data-testid=view-retry]').click();
      expect(f.componentInstance.retries).toBe(1);
      f.componentInstance.doc.set(sampleViewDoc(9));
      await settle(f);
      expect(el(f).querySelector('[data-testid=view-error]')).toBeNull();
    });

    it('shows a patch for a missing node or one that breaks a limit as an error', async () => {
      const f = await setup();
      f.componentInstance.view().applyPatch({ replace: { baseEtag: 1, etag: 2, id: 'nowhere', node: caption } });
      await settle(f);
      expect(text(one(el(f), '[data-testid=view-error]'))).toContain('not there');
      f.componentInstance.view().applyPatch({ replace: { baseEtag: 1, etag: 2, id: 'cap', node: { id: 'h', kind: caption.kind } } });
      await settle(f);
      expect(text(one(el(f), '[data-testid=view-error]'))).toContain('repeats the id');
      // The good document is still there and a later good patch works.
      f.componentInstance.view().applyPatch({ replace: { baseEtag: 1, etag: 2, id: 'cap', node: caption } });
      await settle(f);
      expect(el(f).querySelector('[data-testid=view-error]')).toBeNull();
      expect(text(node(f, 'cap'))).toBe('Updated just now');
    });
  });

  describe('actions', () => {
    it('sends a button action with the view id, the action id and its arguments', async () => {
      const doc = edited((c) => {
        child(c, 'empty').kind = { empty: { icon: 'search', title: 'Nothing', hint: '', action: act('rerun', { label: 'Run the checks', args: { scope: 'all' } }) } } as NodeKind;
      });
      const f = await setup(doc);
      buttonByText(el(f), 'Run the checks').click();
      await settle(f);
      expect(last(f)).toEqual({ view: 'demo.view', action: 'rerun', args: { scope: 'all' } });
    });

    it('asks the host before it sends an action with a question, and sends nothing when declined', async () => {
      const doc = edited((c) => {
        child(c, 'empty').kind = { empty: { icon: 'info', title: 'T', hint: '', action: act('wipe', { label: 'Wipe', confirm: 'Wipe everything?', writes: true }) } } as NodeKind;
      });
      const f = await setup(doc);
      expect(one(node(f, 'empty'), '[data-testid=view-writes]').textContent).toContain('Changes data');
      host.answer = false;
      buttonByText(el(f), /^Wipe/).click();
      await settle(f);
      expect(host.confirms).toHaveLength(1);
      expect(host.confirms[0]).toMatchObject({ question: 'Wipe everything?', writes: true });
      expect(f.componentInstance.actions).toHaveLength(0);
      host.answer = true;
      buttonByText(el(f), /^Wipe/).click();
      await settle(f);
      expect(last(f).action).toBe('wipe');
    });

    it('shows the host refusal inline and sends nothing when the host cannot ask', async () => {
      const doc = edited((c) => {
        child(c, 'empty').kind = { empty: { icon: 'info', title: 'T', hint: '', action: act('wipe', { label: 'Wipe', confirm: 'Sure?' }) } } as NodeKind;
      });
      const f = await setup(doc, false);
      buttonByText(el(f), 'Wipe').click();
      await settle(f);
      expect(f.componentInstance.actions).toHaveLength(0);
      expect(text(one(el(f), '[data-testid=view-notice]'))).toContain('cannot ask for a confirmation');
      expect(NO_VIEW_HOST.confirm).toBeTypeOf('function');
    });

    it('shows the message of a host that fails and clears it on the next try', async () => {
      const doc = edited((c) => {
        child(c, 'empty').kind = { empty: { icon: 'info', title: 'T', hint: '', action: act('wipe', { label: 'Wipe', confirm: 'Sure?' }) } } as NodeKind;
      });
      const f = await setup(doc);
      host.failWith = new Error('The window could not open.');
      buttonByText(el(f), 'Wipe').click();
      await settle(f);
      expect(text(one(el(f), '[data-testid=view-notice]'))).toBe('The window could not open.');
      host.failWith = null;
      buttonByText(el(f), 'Wipe').click();
      await settle(f);
      expect(el(f).querySelector('[data-testid=view-notice]')).toBeNull();
      expect(last(f).action).toBe('wipe');
    });

    it('refuses an action whose arguments, with the host\'s own keys, are too large', async () => {
      const doc = edited((c) => {
        const t = child(c, 't1').kind as { table: { rowAction: ActionRef } };
        // Exactly the limit as the plugin sent it; the row key the host adds takes it over.
        t.table.rowAction.args = { blob: 'x'.repeat(VIEW_EXTRA_LIMITS.maxArgsBytes - 11) };
      });
      const f = await setup(doc);
      (node(f, 't1').querySelector('button[aria-label="Open: Unit tests"]') as HTMLButtonElement).click();
      await settle(f);
      expect(f.componentInstance.actions).toHaveLength(0);
      expect(text(one(el(f), '[data-testid=view-notice]'))).toContain('arguments larger than');
    });

    it('names a writes action for assistive technology and badges it', async () => {
      const f = await setup();
      const accept = buttonByText(el(f), /^Accept/);
      expect(accept.getAttribute('aria-label')).toBe('Accept: Unchecked unwrap on user data, Changes data');
      expect(accept.querySelector('[data-testid=view-writes]')).not.toBeNull();
    });
  });

  describe('links', () => {
    it('sends the action of a link and hands a web address to the host, which confirms and opens it', async () => {
      const f = await setup();
      buttonByText(el(f), 'Open the run').click();
      await settle(f);
      expect(last(f)).toEqual({ view: 'demo.view', action: 'open-run', args: null });
      const guide = node(f, 'l2').querySelector('button') as HTMLButtonElement;
      expect(guide.getAttribute('aria-label')).toBe('Release guide (opens in your browser)');
      guide.click();
      await settle(f);
      expect(host.opens).toEqual([{ url: 'https://example.com/guide', label: 'Release guide' }]);
      expect(f.componentInstance.actions).toHaveLength(1);
    });

    it('shows a host that cannot open pages as an inline message', async () => {
      const f = await setup(sampleViewDoc(), false);
      (node(f, 'l2').querySelector('button') as HTMLButtonElement).click();
      await settle(f);
      expect(text(one(el(f), '[data-testid=view-notice]'))).toContain('cannot open a web page');
    });

    it('badges a link that changes something', async () => {
      const doc = edited((c) => {
        child(c, 'l1').kind = { link: { label: 'Wipe', target: { action: act('wipe', { label: 'Wipe all', writes: true }) } } } as NodeKind;
      });
      const f = await setup(doc);
      expect(node(f, 'l1').querySelector('[data-testid=view-writes]')).not.toBeNull();
    });

    it('refuses every request of the host an app did not provide', async () => {
      await expect(NO_VIEW_HOST.confirm({ question: 'q', action: act('a'), writes: false })).rejects.toThrow('confirmation');
      await expect(NO_VIEW_HOST.openInBrowser({ url: 'https://example.com', label: 'x' })).rejects.toThrow('web page');
      await expect(NO_VIEW_HOST.pickPath({ fieldId: 'f', label: 'F' })).rejects.toThrow('file or folder');
      await expect(NO_VIEW_HOST.pickSecret({ fieldId: 'f', label: 'F' })).rejects.toThrow('secret');
    });

    it('shows a failure of the host opening a page', async () => {
      const f = await setup();
      host.failWith = new Error('No browser.');
      (node(f, 'l2').querySelector('button') as HTMLButtonElement).click();
      await settle(f);
      expect(text(one(el(f), '[data-testid=view-notice]'))).toBe('No browser.');
    });

    it('sends the needs-AI setup action and the empty action', async () => {
      const f = await setup();
      const ai = one(el(f), '[data-testid=view-needs-ai]');
      expect(text(ai)).toContain('Explain the failing check');
      expect(text(ai)).toContain('Pitwall is not running.');
      buttonByText(ai, 'Set up Pitwall').click();
      await settle(f);
      expect(last(f).action).toBe('setup-ai');
    });

    it('says why in the needs-AI state when the plugin gave no reason', async () => {
      const doc = edited((c) => {
        child(c, 'ai').kind = { needsAi: { feature: 'Summarise', reason: '', setup: act('setup-ai', { label: 'Set up' }) } } as NodeKind;
      });
      const f = await setup(doc);
      expect(text(one(el(f), '[data-testid=view-needs-ai]'))).toContain('needs local AI');
    });
  });

  describe('table', () => {
    const t1 = (f: ComponentFixture<Host>): HTMLElement => node(f, 't1');
    const t2 = (f: ComponentFixture<Host>): HTMLElement => node(f, 't2');

    it('is a real table with headers, a caption, tones and a sort that says its direction', async () => {
      const f = await setup();
      const t = t1(f);
      expect(all(t, 'th[scope=col]').map((h) => text(h)).slice(0, 4)).toEqual(['Check', 'State', 'Time (ms)', 'Log']);
      expect(t.querySelector('th[aria-sort]')?.getAttribute('aria-sort')).toBe('ascending');
      expect(text(t.querySelector('caption'))).toBe('Release readiness');
      expect(all(t, 'tbody tr')).toHaveLength(3);
      expect(text(all(t, 'tbody tr')[2])).toContain('Problem: Failing');
      expect(t.querySelector('[role=region]')?.getAttribute('tabindex')).toBe('0');
    });

    it('sends sort with the column and the direction, flipping the sorted column', async () => {
      const f = await setup();
      (t1(f).querySelector('button[aria-label="Sort by Check, descending"]') as HTMLButtonElement).click();
      await settle(f);
      expect(last(f)).toEqual({ view: 'demo.view', action: 'sort', args: { column: 'name', descending: true } });
      (t1(f).querySelector('button[aria-label="Sort by State, ascending"]') as HTMLButtonElement).click();
      await settle(f);
      expect(last(f).args).toEqual({ column: 'state', descending: false });
      expect(all(t1(f), 'th button')).toHaveLength(3);
    });

    it('shows the descending arrow and flips back', async () => {
      const doc = edited((c) => {
        const t = child(c, 't1').kind as { table: { sort: { column: string; descending: boolean } } };
        t.table.sort = { column: 'state', descending: true };
      });
      const f = await setup(doc);
      expect(t1(f).querySelector('th[aria-sort]')?.getAttribute('aria-sort')).toBe('descending');
      (t1(f).querySelector('button[aria-label="Sort by State, ascending"]') as HTMLButtonElement).click();
      await settle(f);
      expect(last(f).args).toEqual({ column: 'state', descending: false });
    });

    it('sends the row action with the row index from the button and from a click on the row', async () => {
      const f = await setup();
      (t1(f).querySelector('button[aria-label="Open: Integration tests"]') as HTMLButtonElement).click();
      await settle(f);
      expect(last(f)).toEqual({ view: 'demo.view', action: 'open-check', args: { row: 1 } });
      expect(f.componentInstance.actions).toHaveLength(1);
      (all(t1(f), 'tbody tr')[2] as HTMLElement).click();
      await settle(f);
      expect(last(f).args).toEqual({ row: 2 });
    });

    it('sends a cell link without also activating its row, and opens a web link through the host', async () => {
      const f = await setup();
      (t1(f).querySelector('button[aria-label="Open the log: log"]') as HTMLButtonElement).click();
      await settle(f);
      expect(f.componentInstance.actions).toHaveLength(1);
      expect(last(f)).toEqual({ view: 'demo.view', action: 'open-log', args: { row: 0 } });
      (t1(f).querySelector('button[aria-label^="Open the lint guide"]') as HTMLButtonElement).click();
      await settle(f);
      expect(host.opens).toHaveLength(1);
      expect(f.componentInstance.actions).toHaveLength(1);
    });

    it('pages: says which rows it shows and sends page with the number', async () => {
      const f = await setup();
      expect(text(one(t2(f), '[data-testid=view-table-range]'))).toBe('Rows 3 to 4 of 120');
      one(t2(f), '[data-testid=view-table-next]').click();
      await settle(f);
      expect(last(f)).toEqual({ view: 'demo.view', action: 'page', args: { page: 2 } });
      one(t2(f), '[data-testid=view-table-prev]').click();
      await settle(f);
      expect(last(f).args).toEqual({ page: 0 });
      expect(t2(f).querySelector('th button')).not.toBeNull();
    });

    it('disables Previous on the first page and Next on the last, and handles an empty window', async () => {
      const doc = edited((c) => {
        const t = child(c, 't2').kind as { table: { rows: { paged: { page: number; total: number; rows: unknown[] } } } };
        t.table.rows.paged.page = 0;
      });
      const f = await setup(doc);
      expect((one(t2(f), '[data-testid=view-table-prev]') as HTMLButtonElement).disabled).toBe(true);
      const doc2 = edited((c) => {
        const t = child(c, 't2').kind as { table: { rows: { paged: { page: number; total: number; rows: unknown[] } } } };
        t.table.rows.paged = { ...t.table.rows.paged, total: 4, page: 1 };
      });
      f.componentInstance.doc.set(doc2);
      await settle(f);
      expect((one(t2(f), '[data-testid=view-table-next]') as HTMLButtonElement).disabled).toBe(true);
      const doc3 = edited((c) => {
        const t = child(c, 't2').kind as { table: { rows: { paged: { page: number; total: number; rows: unknown[] } } } };
        t.table.rows.paged = { ...t.table.rows.paged, total: 0, page: 0, rows: [] };
      });
      f.componentInstance.doc.set(doc3);
      await settle(f);
      expect(text(one(t2(f), '[data-testid=view-table-range]'))).toBe('No rows');
      expect(text(t2(f).querySelector('tbody'))).toBe('No rows.');
    });

    it('shows an inline table with no rows', async () => {
      const doc = edited((c) => {
        const t = child(c, 't1').kind as { table: { rows: { inline: unknown[] }; rowAction: unknown } };
        t.table.rows.inline = [];
      });
      const f = await setup(doc);
      expect(text(t1(f).querySelector('tbody'))).toBe('No rows.');
      expect(t1(f).querySelector('nav')).toBeNull();
    });
  });

  describe('tree', () => {
    const tree = (f: ComponentFixture<Host>): HTMLElement => node(f, 'tree');
    const labels = (f: ComponentFixture<Host>): string[] => all(tree(f), '[role=treeitem]').map((i) => text(i.querySelector('.pd-view-tree__label')));

    it('shows a lazy tree collapsed, with levels and set sizes', async () => {
      const f = await setup();
      expect(labels(f)).toEqual(['crates', 'ui']);
      const items = all(tree(f), '[role=treeitem]');
      expect(items[0]?.getAttribute('aria-level')).toBe('1');
      expect(items[0]?.getAttribute('aria-expanded')).toBe('false');
      expect(items[1]?.getAttribute('aria-posinset')).toBe('2');
      expect(items[1]?.getAttribute('aria-setsize')).toBe('2');
      expect(tree(f).querySelector('[role=tree]')?.getAttribute('aria-label')).toBe('Release readiness');
    });

    it('opens nodes whose children are in the list without asking, and asks for the children of one that has none', async () => {
      const f = await setup();
      (tree(f).querySelector('button[aria-label="Expand: crates"]') as HTMLButtonElement).click();
      await settle(f);
      expect(labels(f)).toEqual(['crates', 'paddock-view', 'ui']);
      (tree(f).querySelector('button[aria-label="Expand: paddock-view"]') as HTMLButtonElement).click();
      await settle(f);
      expect(labels(f)).toEqual(['crates', 'paddock-view', 'model.rs', 'ui']);
      expect(f.componentInstance.actions).toHaveLength(0);
      (tree(f).querySelector('button[aria-label="Expand: ui"]') as HTMLButtonElement).click();
      await settle(f);
      expect(last(f)).toEqual({ view: 'demo.view', action: 'expand', args: { node: 'ui' } });
      // Closing is local.
      (tree(f).querySelector('button[aria-label="Collapse: crates"]') as HTMLButtonElement).click();
      await settle(f);
      expect(labels(f)).toEqual(['crates', 'ui']);
      expect(f.componentInstance.actions).toHaveLength(1);
    });

    it('sends the action of a node with its id', async () => {
      const f = await setup();
      (tree(f).querySelector('button[aria-label="Expand: crates"]') as HTMLButtonElement).click();
      await settle(f);
      (tree(f).querySelector('button[aria-label="Expand: paddock-view"]') as HTMLButtonElement).click();
      await settle(f);
      buttonByText(tree(f), 'model.rs').click();
      await settle(f);
      expect(last(f)).toEqual({ view: 'demo.view', action: 'open-file', args: { node: 'model' } });
    });

    it('moves with the arrow keys, opens with Right, closes with Left and acts with Enter', async () => {
      const f = await setup();
      const items = () => all(tree(f), '[role=treeitem]');
      expect(items()[0]?.getAttribute('tabindex')).toBe('0');
      expect(items()[1]?.getAttribute('tabindex')).toBe('-1');
      items()[0]?.focus();
      press(items()[0] as HTMLElement, 'ArrowRight');
      await settle(f);
      expect(labels(f)).toEqual(['crates', 'paddock-view', 'ui']);
      // Right on an open node goes to its first child.
      press(items()[0] as HTMLElement, 'ArrowRight');
      expect(document.activeElement).toBe(items()[1]);
      press(items()[1] as HTMLElement, 'ArrowRight');
      await settle(f);
      expect(labels(f)).toEqual(['crates', 'paddock-view', 'model.rs', 'ui']);
      press(items()[1] as HTMLElement, 'ArrowDown');
      expect(document.activeElement).toBe(items()[2]);
      // Left from a leaf goes to its parent; Left on an open node closes it.
      press(items()[2] as HTMLElement, 'ArrowLeft');
      expect(document.activeElement).toBe(items()[1]);
      press(items()[1] as HTMLElement, 'ArrowLeft');
      await settle(f);
      expect(labels(f)).toEqual(['crates', 'paddock-view', 'ui']);
      press(items()[1] as HTMLElement, 'End');
      expect(document.activeElement).toBe(items()[2]);
      press(items()[2] as HTMLElement, 'Home');
      expect(document.activeElement).toBe(items()[0]);
      press(items()[0] as HTMLElement, 'ArrowUp');
      expect(document.activeElement).toBe(items()[0]);
      // Enter on an expandable node without an action toggles it; on a node with an action it sends it.
      press(items()[0] as HTMLElement, 'Enter');
      await settle(f);
      expect(labels(f)).toEqual(['crates', 'ui']);
      press(items()[0] as HTMLElement, 'Enter');
      await settle(f);
      press(items()[1] as HTMLElement, 'ArrowRight');
      await settle(f);
      press(items()[2] as HTMLElement, 'Enter');
      await settle(f);
      expect(last(f).action).toBe('open-file');
      expect(press(items()[0] as HTMLElement, 'x').defaultPrevented).toBe(false);
    });

    it('is open by default when it is not lazy, and closes a node on request', async () => {
      const doc = edited((c) => {
        const t = child(c, 'tree').kind as { tree: { lazy: boolean; nodes: { expandable: boolean }[] } };
        t.tree.lazy = false;
        t.tree.nodes.forEach((n) => (n.expandable = false));
      });
      const f = await setup(doc);
      expect(labels(f)).toEqual(['crates', 'paddock-view', 'model.rs', 'ui']);
      (tree(f).querySelector('button[aria-label="Collapse: crates"]') as HTMLButtonElement).click();
      await settle(f);
      expect(labels(f)).toEqual(['crates', 'ui']);
      expect(f.componentInstance.actions).toHaveLength(0);
    });
  });

  describe('graph and diagram', () => {
    it('draws the graph as a named image with a text twin and its boxes', async () => {
      const f = await setup();
      const g = node(f, 'g1');
      const svg = one(g, 'svg');
      expect(svg.getAttribute('role')).toBe('group');
      expect(svg.getAttribute('aria-label')).toContain('5 items and 6 connections');
      expect(all(g, 'g.pd-view-scene__node')).toHaveLength(5);
      expect(all(g, 'path.pd-view-scene__edge')).toHaveLength(6);
      expect(text(g.querySelector('details'))).toContain('Build to Unit tests');
      expect(text(g.querySelector('details'))).toContain('Build to Integration tests (needs db)');
      expect(g.querySelector('[role=region]')?.getAttribute('tabindex')).toBe('0');
    });

    it('sends the action of a box with its id, by click and by keyboard, and leaves other boxes alone', async () => {
      const f = await setup();
      const g = node(f, 'g1');
      const buttons = all(g, 'g[role=button]');
      expect(buttons).toHaveLength(2);
      expect(buttons[0]?.getAttribute('aria-label')).toBe('Open step: Unit tests');
      buttons[0]?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
      await settle(f);
      expect(last(f)).toEqual({ view: 'demo.view', action: 'open-node', args: { node: 'b' } });
      buttons[1]?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
      await settle(f);
      expect(last(f).args).toEqual({ node: 'c' });
      all(g, 'g.pd-view-scene__node')[0]?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
      buttons[0]?.dispatchEvent(new KeyboardEvent('keydown', { key: ' ', bubbles: true }));
      await settle(f);
      expect(f.componentInstance.actions).toHaveLength(3);
    });

    it('draws a radial graph and the diagrams', async () => {
      const f = await setup();
      expect(all(node(f, 'g2'), 'g.pd-view-scene__node')).toHaveLength(4);
      expect(all(node(f, 'd2'), 'line.pd-view-scene__life')).toHaveLength(3);
      expect(text(node(f, 'd2'))).toContain('ViewAction');
      expect(text(node(f, 'd1'))).toContain('schema: u16');
      expect(text(node(f, 'd1'))).toContain('model');
      expect(one(node(f, 'd2'), 'svg').getAttribute('aria-label')).toContain('Release readiness');
    });

    it('turns box actions off while busy', async () => {
      const f = await setup();
      f.componentInstance.busy.set(true);
      await settle(f);
      const b = all(node(f, 'g1'), 'g[role=button]')[0] as Element;
      expect(b.getAttribute('aria-disabled')).toBe('true');
      b.dispatchEvent(new MouseEvent('click', { bubbles: true }));
      await settle(f);
      expect(f.componentInstance.actions).toHaveLength(0);
    });
  });

  describe('form', () => {
    const form = (f: ComponentFixture<Host>): HTMLElement => node(f, 'form');
    const name = (f: ComponentFixture<Host>) => one<HTMLInputElement>(form(f), 'input[type=text]');
    const publish = (f: ComponentFixture<Host>): HTMLButtonElement => buttonByText(form(f), /^Publish( Changes data)?$/);

    it('builds every field with its label, help and value, and badges the submit', async () => {
      const f = await setup();
      const fm = form(f);
      expect(all(fm, 'label').map((l) => text(l).replace(' *', '')).slice(0, 4)).toEqual(['Release name', 'Notes', 'Retries', 'Channel']);
      expect(name(f).value).toBe('v1.4');
      expect(one<HTMLTextAreaElement>(fm, 'textarea').placeholder).toBe('What changed');
      expect(one<HTMLInputElement>(fm, 'input[type=number]').value).toBe('2');
      expect(one<HTMLInputElement>(fm, 'input[type=number]').getAttribute('max')).toBe('5');
      expect(one<HTMLSelectElement>(fm, 'select').value).toBe('beta');
      expect(one<HTMLInputElement>(fm, 'input[type=checkbox]').checked).toBe(true);
      expect(one<HTMLInputElement>(fm, 'input[type=checkbox]').getAttribute('role')).toBe('switch');
      expect(text(fm)).toContain('Shown in the changelog.');
      expect(text(fm)).toContain('No folder chosen');
      expect(text(fm)).toContain('No secret chosen');
      expect(text(one(publish(f), '[data-testid=view-writes]'))).toBe('Changes data');
    });

    it('never submits by itself and refuses an invalid form with the errors shown', async () => {
      const f = await setup();
      const submit = new Event('submit', { bubbles: true, cancelable: true });
      one(form(f), 'form').dispatchEvent(submit);
      expect(submit.defaultPrevented).toBe(true);
      expect(f.componentInstance.actions).toHaveLength(0);
      typeInto(name(f), '');
      await settle(f);
      publish(f).click();
      await settle(f);
      expect(host.confirms).toHaveLength(0);
      expect(f.componentInstance.actions).toHaveLength(0);
      expect(text(one(form(f), '[data-testid=field-error]'))).toBe('This is required.');
    });

    it('checks number limits', async () => {
      const f = await setup();
      const n = one<HTMLInputElement>(form(f), 'input[type=number]');
      typeInto(n, '9');
      n.dispatchEvent(new Event('blur'));
      await settle(f);
      expect(text(one(form(f), '[data-testid=field-error]'))).toBe('Enter 5 or less.');
      typeInto(n, '-3');
      await settle(f);
      expect(text(one(form(f), '[data-testid=field-error]'))).toBe('Enter 0 or more.');
      typeInto(n, '');
      await settle(f);
      expect(form(f).querySelector('[data-testid=field-error]')).toBeNull();
    });

    it('asks the host to confirm, then sends the values of all fields', async () => {
      const f = await setup();
      typeInto(name(f), 'v2');
      typeInto(one<HTMLTextAreaElement>(form(f), 'textarea'), 'Fixes');
      typeInto(one<HTMLInputElement>(form(f), 'input[type=number]'), '4');
      typeInto(one<HTMLSelectElement>(form(f), 'select'), 'stable', 'change');
      const sign = one<HTMLInputElement>(form(f), 'input[type=checkbox]');
      sign.checked = false;
      sign.dispatchEvent(new Event('change', { bubbles: true }));
      await settle(f);
      publish(f).click();
      await settle(f);
      expect(host.confirms[0]).toMatchObject({ question: 'Publish release v1.4?', writes: true });
      expect(last(f)).toEqual({
        view: 'demo.view',
        action: 'publish',
        args: { values: { name: 'v2', notes: 'Fixes', retries: 4, channel: 'stable', sign: false, out: null, token: null } },
      });
    });

    it('sends nothing when the confirmation is declined', async () => {
      const f = await setup();
      host.answer = false;
      publish(f).click();
      await settle(f);
      expect(host.confirms).toHaveLength(1);
      expect(f.componentInstance.actions).toHaveLength(0);
    });

    it('chooses a path with the host picker and keeps only its grant', async () => {
      const f = await setup();
      buttonByText(form(f), 'Choose a folder…').click();
      await settle(f);
      expect(host.paths).toEqual([{ fieldId: 'out', label: 'Output folder', folder: true }]);
      expect(text(form(f))).toContain('Chosen');
      publish(f).click();
      await settle(f);
      expect((last(f).args as { values: Record<string, unknown> }).values['out']).toBe('grant-7');
    });

    it('leaves the field alone when the picker is cancelled or fails', async () => {
      const f = await setup();
      host.grant = null;
      buttonByText(form(f), 'Choose a folder…').click();
      await settle(f);
      expect(text(form(f))).toContain('No folder chosen');
      host.failWith = new Error('No access.');
      buttonByText(form(f), 'Choose a folder…').click();
      await settle(f);
      expect(text(one(el(f), '[data-testid=view-notice]'))).toBe('No access.');
    });

    it('chooses a secret reference, never the secret, and a file when the field wants one', async () => {
      const doc = edited((c) => {
        const fm = child(c, 'form').kind as { form: { fields: { id: string; kind: unknown }[] } };
        fm.form.fields[5]!.kind = { path: { value: null, folder: false } };
      });
      const f = await setup(doc);
      expect(text(form(f))).toContain('No file chosen');
      buttonByText(form(f), 'Choose a secret…').click();
      await settle(f);
      expect(host.secrets).toEqual([{ fieldId: 'token', label: 'Publish token' }]);
      expect(text(form(f))).toContain('A stored secret is chosen');
      expect(form(f).innerHTML).not.toContain('secret-3');
      buttonByText(form(f), 'Choose a file…').click();
      await settle(f);
      expect(host.paths[0]?.folder).toBe(false);
      publish(f).click();
      await settle(f);
      expect((last(f).args as { values: Record<string, unknown> }).values).toMatchObject({ token: 'secret-3', out: 'grant-7' });
    });

    it('starts again from the plugin\'s values when a different form arrives', async () => {
      const f = await setup();
      typeInto(name(f), 'typed');
      await settle(f);
      f.componentInstance.doc.set(sampleViewDoc(2));
      await settle(f);
      expect(name(f).value).toBe('v1.4');
    });

    it('turns the fields off while busy and when a required choice has no empty option', async () => {
      const f = await setup();
      expect(all(form(f), 'option').map((o) => text(o))).toEqual(['Beta', 'Stable']);
      f.componentInstance.busy.set(true);
      await settle(f);
      expect(name(f).disabled).toBe(true);
    });

    it('offers "Not chosen" for an optional choice with no value', async () => {
      const doc = edited((c) => {
        const fm = child(c, 'form').kind as { form: { fields: { required: boolean; kind: { choice?: { value: string } } }[] } };
        fm.form.fields[3]!.required = false;
        fm.form.fields[3]!.kind.choice!.value = '';
      });
      const f = await setup(doc);
      expect(all(form(f), 'option').map((o) => text(o))).toEqual(['Not chosen', 'Beta', 'Stable']);
    });
  });

  describe('review list', () => {
    const review = (f: ComponentFixture<Host>): HTMLElement => node(f, 'review');
    const titles = (f: ComponentFixture<Host>): string[] => all(review(f), '.pd-view-review__title').map((t) => text(t));

    it('lists findings with a labelled state and the actions of that state', async () => {
      const f = await setup();
      expect(titles(f)).toHaveLength(3);
      const first = one(review(f), '[data-finding=f1]');
      expect(text(one(first, '[data-testid=view-finding-state]'))).toBe('Warning: Open');
      expect(text(first)).toContain('src/codec.rs:42');
      expect(text(first)).toContain('The parser calls unwrap');
      expect(buttonByText(first, /^Accept/)).toBeTruthy();
      expect(buttonByText(first, /^Dismiss/)).toBeTruthy();
      expect(first.querySelector('[data-finding=f2]')).toBeNull();
      expect(one(review(f), '[data-finding=f2]').querySelector('.pd-view-review__where')).not.toBeNull();
      expect(one(review(f), '[data-finding=f2]').querySelector('.pd-view-review__detail')).toBeNull();
    });

    it('sends an action with the finding and opens its link through the host', async () => {
      const f = await setup();
      buttonByText(one(review(f), '[data-finding=f1]'), /^Accept/).click();
      await settle(f);
      expect(last(f)).toEqual({ view: 'demo.view', action: 'accept', args: { finding: 'f1' } });
      buttonByText(one(review(f), '[data-finding=f1]'), 'Read the rule').click();
      await settle(f);
      expect(host.opens[0]?.url).toBe('https://example.com/rule/unwrap');
    });

    it('filters by state with counts', async () => {
      const f = await setup();
      const group = one(review(f), '[role=group]');
      expect(all(group, 'button').map((b) => text(b))).toEqual(['All (3)', 'Open (1)', 'Suppressed (1)', 'Blocked (1)']);
      buttonByText(group, 'Blocked (1)').click();
      await settle(f);
      expect(titles(f)).toEqual(['Hard-coded version in a test']);
      expect(buttonByText(group, 'Blocked (1)').getAttribute('aria-pressed')).toBe('true');
      buttonByText(group, 'All (3)').click();
      await settle(f);
      expect(titles(f)).toHaveLength(3);
    });

    it('shows a friendly line when the filter leaves nothing', async () => {
      const doc = edited((c) => {
        const r = child(c, 'review').kind as { reviewList: { items: { state: string }[] } };
        r.reviewList.items = [];
      });
      const f = await setup(doc);
      expect(text(review(f))).toContain('No findings.');
    });

    it('asks for a reason before an action that needs one, refuses an empty one and sends it trimmed', async () => {
      const f = await setup();
      buttonByText(review(f), /^Suppress(?!ed)/).click();
      await settle(f);
      expect(one(review(f), '[data-testid=view-reason]')).toBeTruthy();
      expect(f.componentInstance.actions).toHaveLength(0);
      one(review(f), '[data-testid=view-reason-send]').click();
      await settle(f);
      expect(f.componentInstance.actions).toHaveLength(0);
      expect(text(one(review(f), '[data-testid=field-error]'))).toBe('This is required.');
      typeInto(one(review(f), '[data-testid=view-reason]') as HTMLTextAreaElement, '  Known issue  ');
      await settle(f);
      one(review(f), '[data-testid=view-reason-send]').click();
      await settle(f);
      expect(last(f)).toEqual({ view: 'demo.view', action: 'suppress', args: { finding: 'f3', reason: 'Known issue' } });
      expect(review(f).querySelector('[data-testid=view-reason]')).toBeNull();
    });

    it('cancels the reason and keeps it when the confirmation is declined', async () => {
      const doc = edited((c) => {
        const r = child(c, 'review').kind as { reviewList: { states: { id: string; actions: ActionRef[] }[] } };
        r.reviewList.states[2]!.actions[0]!.confirm = 'Suppress it?';
      });
      const f = await setup(doc);
      buttonByText(review(f), /^Suppress(?!ed)/).click();
      await settle(f);
      one(review(f), '[data-testid=view-reason-cancel]').click();
      await settle(f);
      expect(review(f).querySelector('[data-testid=view-reason]')).toBeNull();
      buttonByText(review(f), /^Suppress(?!ed)/).click();
      await settle(f);
      typeInto(one(review(f), '[data-testid=view-reason]') as HTMLTextAreaElement, 'why');
      host.answer = false;
      one(review(f), '[data-testid=view-reason-send]').click();
      await settle(f);
      expect(f.componentInstance.actions).toHaveLength(0);
      expect((one(review(f), '[data-testid=view-reason]') as HTMLTextAreaElement).value).toBe('why');
      host.answer = true;
      one(review(f), '[data-testid=view-reason-send]').click();
      await settle(f);
      expect(last(f).args).toEqual({ finding: 'f3', reason: 'why' });
    });

    it('has no filter for a single state', async () => {
      const doc = edited((c) => {
        const r = child(c, 'review').kind as { reviewList: { states: unknown[]; items: { state: string }[] } };
        r.reviewList.states = r.reviewList.states.slice(0, 1);
        r.reviewList.items = r.reviewList.items.slice(0, 1);
      });
      const f = await setup(doc);
      expect(review(f).querySelector('[role=group]')).toBeNull();
    });
  });

  describe('matrix', () => {
    it('draws headings and sparse cells and sends a cell action with its place', async () => {
      const f = await setup();
      const m = node(f, 'matrix');
      expect(all(m, 'thead th').map((h) => text(h))).toEqual(['Unit', 'End to end']);
      expect(all(m, 'tbody th').map((h) => text(h))).toEqual(['REQ-1 Open a file', 'REQ-2 Save a file']);
      expect(all(m, 'tbody td')).toHaveLength(4);
      expect(text(all(m, 'tbody td')[2])).toBe('');
      expect(text(all(m, 'tbody td')[3])).toBe('Problem: Missing');
      const b = m.querySelector('button[aria-label="Open case: REQ-1 Open a file, End to end (Covered)"]') as HTMLButtonElement;
      b.click();
      await settle(f);
      expect(last(f)).toEqual({ view: 'demo.view', action: 'open-case', args: { row: 0, col: 1, rowId: 'r1', colId: 'c2' } });
    });

    it('uses the action label for a cell without text', async () => {
      const doc = edited((c) => {
        const m = child(c, 'matrix').kind as { matrix: { cells: { text: string }[] } };
        m.matrix.cells[2]!.text = '';
      });
      const f = await setup(doc);
      expect(text(all(node(f, 'matrix'), 'tbody td')[3])).toContain('Open case');
    });
  });

  describe('chart', () => {
    it('draws line, bar, area and pie through pd-chart, and a scatter through its own option', async () => {
      const f = await setup();
      await settle(f, 6);
      expect(all(el(f), 'pd-chart')).toHaveLength(5);
      expect(fake.instances).toHaveLength(5);
      const options = fake.instances.map((i) => JSON.stringify(i.lastOption));
      expect(options.some((o) => o.includes('"type":"scatter"'))).toBe(true);
      expect(options.some((o) => o.includes('"type":"pie"'))).toBe(true);
      expect(text(node(f, 'c1'))).toContain('Test time per run');
      expect(node(f, 'c1').querySelector('[role=img]')?.getAttribute('aria-label')).toBe('Test time per run');
      expect(text(node(f, 'c1').querySelector('p.pd-sr'))).toContain('Seconds from 39 to 52');
    });
  });

  describe('empty', () => {
    it('draws icon, title and hint, and the action as the one primary button', async () => {
      const f = await setup();
      const e = node(f, 'empty');
      expect(text(e)).toContain('Nothing matches');
      expect(text(e)).toContain('Change the filter');
      expect(e.querySelector('.pd-btn--primary')).not.toBeNull();
      expect(e.querySelector('pd-glyph')?.getAttribute('aria-hidden')).toBe('true');
    });
    it('omits the hint and the action when there are none', async () => {
      const doc = edited((c) => {
        child(c, 'empty').kind = { empty: { icon: 'rocket', title: 'Quiet', hint: '', action: null } } as unknown as NodeKind;
      });
      const f = await setup(doc);
      expect(node(f, 'empty').querySelector('.pd-view-empty__hint')).toBeNull();
      expect(node(f, 'empty').querySelector('button')).toBeNull();
    });
  });

  it('checks a field value the way the form does', () => {
    const num = { id: 'n', label: 'N', help: '', required: true, kind: { number: { value: 1, min: 0, max: 3 } } } as const;
    expect(fieldProblem(num, Number.POSITIVE_INFINITY)).toEqual({ kind: 'invalid' });
    expect(fieldProblem(num, Number.NaN)).toEqual({ kind: 'required' });
    expect(fieldProblem(num, 2)).toBeNull();
    expect(fieldProblem({ ...num, kind: { toggle: { value: false } } }, null)).toBeNull();
    const form = (sampleViewDoc().root.kind as { stack: { children: Node[] } }).stack.children.find((c) => c.id === 'form')!.kind as { form: Parameters<typeof initialValues>[0] };
    expect(initialValues(form.form)).toMatchObject({ name: 'v1.4', retries: 2, channel: 'beta', sign: true, out: null, token: null });
  });

  it('knows every node kind the sample uses', () => {
    const doc = sampleViewDoc();
    const tags = new Set<string>();
    const walk = (n: Node): void => {
      const s = shapeOf(n);
      tags.add(s.tag);
      if (s.tag === 'stack') s.v.children.forEach(walk);
    };
    walk(doc.root);
    for (const k of KNOWN_KINDS.filter((k) => k !== 'stack')) expect(tags.has(k), k).toBe(true);
    expect(tags.has('unknown')).toBe(true);
  });

  beforeEach(() => {
    document.documentElement.removeAttribute('data-motion');
  });
});
