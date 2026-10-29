import { Component, ElementRef, ErrorHandler, provideZonelessChangeDetection, signal, viewChild, ChangeDetectionStrategy, input } from '@angular/core';
import { TestBed, type ComponentFixture } from '@angular/core/testing';
import { PD_ECHARTS, fakeEchartsLoader } from '@decryp7/paddock-ui/chart';
import { FakeECharts, all, buttonByText, one, text } from '@decryp7/paddock-ui/testing';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { Node, NodeKind, ViewAction, ViewDoc } from '../generated/view';
import { VIEW_HOST, VIEW_NEEDS_AI, VIEW_OPEN_AI_SETTINGS, type ViewHost, type ViewPicked } from './host';
import { NEEDS_AI_WITHOUT_REPAIR } from './node';
import { primaryNode, unknownKindName } from './model';
import { sampleViewDoc } from './sample';
import { PdViewScroll } from './scroll';
import { PdView } from './view';

// The independent review of <pd-view> (review 1, docs/review/x51-view/REVIEW.md) as tests: each finding that can be seen in the DOM.

@Component({
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdView],
  template: `<pd-view view="demo.view" [plugin]="plugin()" [logsLink]="logs()" [doc]="doc()" [busy]="busy()" [loading]="loading()" (action)="actions.push($event)" (retry)="retries = retries + 1" (openLogs)="opened = opened + 1" />`,
})
class Host {
  readonly doc = signal<unknown>(sampleViewDoc());
  readonly busy = signal(false);
  readonly loading = signal(false);
  readonly plugin = signal('');
  readonly logs = signal(false);
  readonly view = viewChild.required(PdView);
  readonly actions: ViewAction[] = [];
  retries = 0;
  opened = 0;
}

@Component({ selector: 'app-live-needs-ai', changeDetection: ChangeDetectionStrategy.OnPush, template: `<p data-testid="live-needs-ai">{{ feature() }} from the app</p>` })
class LiveNeedsAi {
  readonly feature = input.required<string>();
}

const noHost: ViewHost = {
  confirm: async () => true,
  openInBrowser: async () => undefined,
  pickPath: async () => ({ value: 'g', name: '/a/b' }) as ViewPicked<string>,
  pickSecret: async () => ({ value: 's', name: 'token' }) as ViewPicked<string>,
};

async function settle(f: { whenStable(): Promise<unknown> }): Promise<void> {
  for (let i = 0; i < 3; i++) {
    await f.whenStable();
    TestBed.tick();
    await Promise.resolve();
  }
}
const without = (doc: ViewDoc, ...ids: string[]): ViewDoc => {
  const copy = structuredClone(doc);
  const kids = (copy.root.kind as { stack: { children: Node[] } }).stack.children;
  (copy.root.kind as { stack: { children: Node[] } }).stack.children = kids.filter((c) => !ids.includes(c.id));
  return copy;
};

describe('pd-view review 1 fixes', () => {
  async function setup(doc: unknown = sampleViewDoc(), extra: { provide: unknown; useValue: unknown }[] = []): Promise<ComponentFixture<Host>> {
    TestBed.configureTestingModule({
      providers: [provideZonelessChangeDetection(), { provide: PD_ECHARTS, useValue: fakeEchartsLoader(new FakeECharts().init) }, { provide: VIEW_HOST, useValue: noHost }, { provide: VIEW_OPEN_AI_SETTINGS, useValue: () => undefined }, ...extra],
    });
    const f = TestBed.createComponent(Host);
    f.componentInstance.doc.set(doc);
    document.body.appendChild(f.nativeElement);
    await settle(f);
    return f;
  }
  const el = (f: ComponentFixture<Host>): HTMLElement => f.nativeElement as HTMLElement;
  const node = (f: ComponentFixture<Host>, id: string): HTMLElement => one(el(f), `[data-node-id="${id}"]`);
  afterEach(() => {
    document.body.replaceChildren();
    TestBed.resetTestingModule();
    vi.unstubAllGlobals();
  });

  describe('P1-2: a status is never colour alone, and the list is an equal twin', () => {
    it('draws the status glyph in every non-neutral box, and none in a neutral one', async () => {
      const f = await setup();
      const g = node(f, 'g1');
      const marked = all(g, 'g.pd-view-scene__node').map((n) => [n.querySelector('title')?.textContent, n.querySelector('.pd-view-scene__mark path')?.getAttribute('d')?.length ?? 0] as const);
      expect(marked.map(([t]) => t)).toEqual(['Build — Good', 'Unit tests — Good', 'Integration tests — Warning', 'Package', 'Publish — Problem']);
      expect(marked.map(([, d]) => d > 0)).toEqual([true, true, true, false, true]);
      // Each tone has its own shape: good and bad are not the same glyph.
      const shape = (title: string) => all(g, 'g.pd-view-scene__node').find((n) => n.querySelector('title')?.textContent === title)!.querySelector('.pd-view-scene__mark path')!.getAttribute('d');
      expect(new Set([shape('Build — Good'), shape('Integration tests — Warning'), shape('Publish — Problem')]).size).toBe(3);
    });
    it('puts the status word in the list and the members of a class with their group', async () => {
      const f = await setup();
      expect(text(one(node(f, 'g1'), '[data-testid=view-scene-nodes]'))).toContain('Publish — Problem');
      const classes = text(one(node(f, 'd1'), '[data-testid=view-scene-nodes]'));
      expect(classes).toContain('ViewDoc');
      expect(classes).toContain('«model»');
      expect(classes).toContain('schema: u16');
      expect(classes).toContain('kind: NodeKind');
    });
  });

  describe('P1-3: one "needs local AI" look and a repair the app owns', () => {
    it('draws the family card with the plugin reason and no button when the host has neither Pitwall state nor a settings opener', async () => {
      const errors: unknown[] = [];
      const f = await setup(sampleViewDoc(), [
        { provide: VIEW_OPEN_AI_SETTINGS, useValue: null },
        { provide: ErrorHandler, useValue: { handleError: (e: unknown) => errors.push(e) } },
      ]);
      const card = one(node(f, 'ai'), '[data-testid=view-needs-ai]');
      expect(text(card)).toContain('Explain the failing check needs local AI');
      expect(text(card)).toContain('Pitwall is not running.');
      expect(card.closest('pd-needs-ai-card')).not.toBeNull();
      expect(card.getAttribute('aria-labelledby')).toBeTruthy();
      expect(card.querySelector('button')).toBeNull();
      expect(text(one(card, '[data-testid=needs-ai-on-builtin]'))).toContain("Set up local AI in this app's settings.");
      expect(text(card)).not.toContain('Set up Pitwall');
      // Rule 19, dev mode: the host that shows views must provide a way to set up AI, so this wiring gap is reported once.
      expect(errors).toHaveLength(1);
      expect(String(errors[0])).toBe(`Error: ${NEEDS_AI_WITHOUT_REPAIR}`);
    });
    it('reports nothing when the host provides the opener or the live component, or when no needsAi node is drawn', async () => {
      const seen = async (extra: { provide: unknown; useValue: unknown }[], doc: unknown = sampleViewDoc()): Promise<unknown[]> => {
        const errors: unknown[] = [];
        await setup(doc, [...extra, { provide: ErrorHandler, useValue: { handleError: (e: unknown) => errors.push(e) } }]);
        TestBed.resetTestingModule();
        document.body.replaceChildren();
        return errors;
      };
      expect(await seen([{ provide: VIEW_OPEN_AI_SETTINGS, useValue: () => undefined }])).toEqual([]);
      expect(await seen([{ provide: VIEW_OPEN_AI_SETTINGS, useValue: null }, { provide: VIEW_NEEDS_AI, useValue: LiveNeedsAi }])).toEqual([]);
      expect(await seen([{ provide: VIEW_OPEN_AI_SETTINGS, useValue: null }], without(sampleViewDoc(), 'ai'))).toEqual([]);
    });
    it('offers only the app\'s Open AI settings when the host provides it, and never sends the plugin\'s setup action', async () => {
      const open = vi.fn();
      const f = await setup(sampleViewDoc(), [{ provide: VIEW_OPEN_AI_SETTINGS, useValue: open }]);
      const card = one(node(f, 'ai'), '[data-testid=view-needs-ai]');
      expect(all(card, 'button')).toHaveLength(1);
      expect(card.querySelector('[data-testid=needs-ai-on-builtin]')).toBeNull();
      one<HTMLButtonElement>(card, '[data-testid=view-needs-ai-settings]').click();
      await settle(f);
      expect(open).toHaveBeenCalledTimes(1);
      expect(f.componentInstance.actions).toEqual([]);
    });
    it('disables the settings button while the host is busy', async () => {
      const f = await setup(sampleViewDoc(), [{ provide: VIEW_OPEN_AI_SETTINGS, useValue: vi.fn() }]);
      f.componentInstance.busy.set(true);
      await settle(f);
      expect(one<HTMLButtonElement>(node(f, 'ai'), '[data-testid=view-needs-ai-settings]').disabled).toBe(true);
    });
    it('draws the app\'s live component instead when the host provides one, ignoring the reason and the plugin action', async () => {
      const f = await setup(sampleViewDoc(), [{ provide: VIEW_NEEDS_AI, useValue: LiveNeedsAi }]);
      const ai = node(f, 'ai');
      expect(text(one(ai, '[data-testid=live-needs-ai]'))).toBe('Explain the failing check from the app');
      expect(ai.querySelector('button')).toBeNull();
      expect(text(ai)).not.toContain('Pitwall is not running');
    });
  });

  describe('P2-4: stale is a warning, a failure is an error', () => {
    it('shows stale as a warning with Reload, and an invalid view as an error with Retry and the plugin named', async () => {
      const f = await setup();
      f.componentInstance.plugin.set('Release checks');
      f.componentInstance.logs.set(true);
      f.componentInstance.view().applyPatch({ replace: { baseEtag: 99, etag: 100, id: 'cap', node: { id: 'cap', kind: { text: { text: 'x', tone: 'neutral', style: 'caption' } } } } });
      await settle(f);
      const stale = one(el(f), '[data-testid=view-error]');
      expect(stale.getAttribute('data-kind')).toBe('stale');
      expect(stale.classList.contains('pd-view-error--stale')).toBe(true);
      expect(text(one(stale, '[data-testid=view-retry]'))).toBe('Reload');
      expect(stale.querySelector('[data-testid=view-logs]')).toBeNull();
      f.componentInstance.doc.set({ ...sampleViewDoc(5), schema: 2 });
      await settle(f);
      const failed = one(el(f), '[data-testid=view-error]');
      expect(failed.getAttribute('data-kind')).toBe('failed');
      expect(failed.classList.contains('pd-view-error--stale')).toBe(false);
      expect(text(failed)).toContain('Release checks sent a view this app cannot show');
      expect(text(one(failed, '[data-testid=view-retry]'))).toBe('Retry');
      one(failed, '[data-testid=view-logs]').click();
      expect(f.componentInstance.opened).toBe(1);
    });
    it('names no plugin and offers no logs unless the host says so', async () => {
      const f = await setup({ ...sampleViewDoc(), schema: 2 });
      expect(text(one(el(f), '[data-testid=view-error]'))).toContain('This view cannot be shown');
      expect(el(f).querySelector('[data-testid=view-logs]')).toBeNull();
    });
  });

  describe('P2-5: loading and busy are designed', () => {
    it('shows a skeleton while the host fetches the view, announced once as loading', async () => {
      const f = await setup(null);
      f.componentInstance.loading.set(true);
      await settle(f);
      const sk = one(el(f), '[data-testid=view-loading]');
      expect(sk.getAttribute('role')).toBe('status');
      expect(all(sk, 'span:not(.pd-view-sr)').length).toBeGreaterThanOrEqual(3);
      expect(text(sk)).toBe('Loading…');
      expect(one(el(f), 'pd-view').getAttribute('aria-busy')).toBe('true');
    });
    it('shows a "Working…" status with a bar while the engine answers, and nothing otherwise', async () => {
      const f = await setup();
      expect(el(f).querySelector('[data-testid=view-busy]')).toBeNull();
      f.componentInstance.busy.set(true);
      await settle(f);
      const busy = one(el(f), '[data-testid=view-busy]');
      expect(busy.getAttribute('role')).toBe('status');
      expect(text(busy)).toBe('Working…');
      expect(busy.querySelector('.pd-view-busy__bar')).not.toBeNull();
    });
  });

  describe('P2-6: one filled accent per view', () => {
    it('fills only the first node with a main action; the review filter is a segmented control', async () => {
      const f = await setup();
      const filled = all(el(f), '.pd-btn--primary');
      expect(filled).toHaveLength(1);
      expect(filled[0]?.closest('[data-node-id]')?.getAttribute('data-node-id')).toBe('form');
      const filter = one(node(f, 'review'), '.pd-view-review__filter');
      expect(filter.classList.contains('pd-segmented')).toBe(true);
      expect(all(filter, 'button[aria-pressed=true]')).toHaveLength(1);
    });
    it('gives the primary to the empty state, then to the needs-AI settings button, as the earlier ones leave', async () => {
      const f = await setup(without(sampleViewDoc(), 'form'), [{ provide: VIEW_OPEN_AI_SETTINGS, useValue: vi.fn() }]);
      expect(all(el(f), '.pd-btn--primary').map((b) => b.closest('[data-node-id]')?.getAttribute('data-node-id'))).toEqual(['empty']);
      f.componentInstance.doc.set(without(sampleViewDoc(), 'form', 'empty'));
      await settle(f);
      expect(all(el(f), '.pd-btn--primary').map((b) => b.closest('[data-node-id]')?.getAttribute('data-node-id'))).toEqual(['ai']);
    });
    it('finds the primary node in document order, bounded in depth', () => {
      const leaf = (id: string, kind: NodeKind): Node => ({ id, kind });
      const empty: NodeKind = { empty: { icon: 'info', title: 'T', hint: '', action: null } };
      expect(primaryNode(null)).toBeNull();
      expect(primaryNode(leaf('e', empty))).toBeNull();
      let deep: Node = leaf('f', { form: { fields: [], submit: { id: 's', label: 'S', confirm: null, args: null, writes: false } } });
      for (let i = 0; i < 40; i++) deep = leaf(`s${i}`, { stack: { dir: 'column', children: [deep] } });
      expect(primaryNode(deep)).toBeNull();
    });
  });

  describe('P2-7: one "Changes data" badge', () => {
    it('uses the same element, with the pencil, on a button, a link, the form and the reason button', async () => {
      const f = await setup();
      const review = node(f, 'review');
      one<HTMLButtonElement>(review, 'button[aria-label^="Suppress: "]').click();
      await settle(f);
      const badges = all(el(f), '[data-testid=view-writes]');
      expect(badges.length).toBeGreaterThanOrEqual(5);
      for (const b of badges) {
        expect(b.tagName.toLowerCase()).toBe('pd-view-writes');
        expect(b.querySelector('pd-glyph')).not.toBeNull();
        expect(text(b)).toBe('Changes data');
      }
      expect(one(review, '[data-testid=view-reason-send] pd-view-writes')).toBeTruthy();
    });
  });

  describe('P2-8 and P2-9: tree tone on the mark only, numbers on the right with their unit', () => {
    it('gives a toned tree row a tooltip with the word and keeps the label a plain span', async () => {
      const f = await setup();
      one<HTMLButtonElement>(node(f, 'tree'), 'button[aria-label^="Expand: crates"]').click();
      await settle(f);
      const row = one(node(f, 'tree'), '[data-node="view"]');
      expect(row.getAttribute('title')).toBe('Good');
      expect(row.querySelector('pd-view-tone')).not.toBeNull();
      expect(one(node(f, 'tree'), '[data-node="crates"]').getAttribute('title')).toBeNull();
    });
    it('right-aligns a numeric column, header and cells, and shows its unit in the header and the sort name', async () => {
      const f = await setup();
      const t = node(f, 't1');
      const heads = all(t, 'th');
      const ms = heads[2]!;
      expect(ms.classList.contains('pd-view-table__num')).toBe(true);
      expect(text(ms)).toContain('Time (ms)');
      expect(one(ms, 'button').getAttribute('aria-label')).toBe('Sort by Time (ms), ascending');
      expect(heads[0]!.classList.contains('pd-view-table__num')).toBe(false);
      for (const row of all(t, 'tbody tr')) expect(row.querySelectorAll('td')[2]!.classList.contains('pd-view-table__num')).toBe(true);
      expect(all(t, 'tbody tr')[0]!.querySelectorAll('td')[0]!.classList.contains('pd-view-table__num')).toBe(false);
      // A status cell stays on one line.
      expect(all(t, 'tbody tr')[0]!.querySelectorAll('td')[1]!.classList.contains('pd-view-table__status')).toBe(true);
    });
  });

  describe('P2-10: a chosen path is shown by its name', () => {
    it('shows the host\'s name for the path (middle-truncated by pd-path-text, the full value as tooltip) and for the secret', async () => {
      const f = await setup();
      const form = node(f, 'form');
      buttonByText(form, 'Choose a folder…').click();
      await settle(f);
      buttonByText(form, 'Choose a secret…').click();
      await settle(f);
      const chosen = all(form, '[data-testid=view-chosen]');
      expect(one(chosen[0]!, '[data-testid=path-text]').getAttribute('data-full')).toBe('/a/b');
      expect(text(chosen[1]!)).toContain('token');
    });
    it('says "Chosen" for a value the plugin sent without a name', async () => {
      const doc = structuredClone(sampleViewDoc());
      const fm = ((doc.root.kind as { stack: { children: Node[] } }).stack.children.find((c) => c.id === 'form')!.kind as { form: { fields: { id: string; kind: unknown }[] } }).form;
      fm.fields[5]!.kind = { path: { value: 'grant-1', folder: true } };
      const f = await setup(doc);
      expect(text(all(node(f, 'form'), '[data-testid=view-chosen]')[0]!)).toBe('Chosen');
      const doc2 = structuredClone(doc);
      const fm2 = ((doc2.root.kind as { stack: { children: Node[] } }).stack.children.find((c) => c.id === 'form')!.kind as { form: { fields: { id: string; kind: unknown }[] } }).form;
      fm2.fields[6]!.kind = { secretRef: { value: 'secret-1' } };
      f.componentInstance.doc.set(doc2);
      await settle(f);
      expect(text(all(node(f, 'form'), '[data-testid=view-chosen]')[1]!)).toBe('A stored secret is chosen');
    });
    it('marks path, secret and toggle fields touched once the user has been at them', async () => {
      const doc = structuredClone(sampleViewDoc());
      const fm = ((doc.root.kind as { stack: { children: Node[] } }).stack.children.find((c) => c.id === 'form')!.kind as { form: { fields: { id: string; required: boolean }[] } }).form;
      fm.fields[5]!.required = true;
      const f = await setup(doc);
      const form = node(f, 'form');
      expect(form.querySelector('[data-testid=field-error]')).toBeNull();
      // Cancelled picker: the field is touched, so its "required" shows.
      TestBed.inject(VIEW_HOST).pickPath = async () => null;
      buttonByText(form, 'Choose a folder…').click();
      await settle(f);
      expect(text(one(form, '[data-testid=field-error]'))).toBe('This is required.');
    });
  });

  describe('P2-11: a class diagram looks like one', () => {
    it('writes the group as a stereotype, a rule under the name and the members from the left', async () => {
      const f = await setup();
      const d = node(f, 'd1');
      expect(all(d, '.pd-view-scene__caption').map((c) => c.textContent)).toEqual(['«model»', '«model»']);
      expect(all(d, 'line.pd-view-scene__rule')).toHaveLength(2);
      expect(all(d, 'text.pd-view-scene__detail').every((t) => t.getAttribute('text-anchor') === 'start')).toBe(true);
      // A sequence diagram has no compartments.
      expect(node(f, 'd2').querySelector('line.pd-view-scene__rule')).toBeNull();
    });
  });

  describe('P3', () => {
    it('names the unknown kind in a muted suffix, bounded', () => {
      expect(unknownKindName('hologram')).toBe('hologram');
      expect(unknownKindName('x'.repeat(100))).toHaveLength(40);
    });
    it('draws the empty icon at 32 px', async () => {
      const f = await setup();
      expect(one(node(f, 'empty'), 'pd-glyph svg').getAttribute('width')).toBe('32');
    });
    it('keeps the disclosure of the list inside the frame, under the drawing', async () => {
      const f = await setup();
      const frame = one(node(f, 'g1'), '.pd-view-scene__frame');
      expect(frame.querySelector(':scope > details.pd-view-scene__list')).not.toBeNull();
    });
    it('draws the scatter series in the colour of the accent tokens, like the others', async () => {
      const f = await setup();
      expect(node(f, 'c5')).toBeTruthy();
    });
  });
});

describe('PdViewScroll: a scroll region is a tab stop only while it overflows', () => {
  afterEach(() => {
    document.body.replaceChildren();
    TestBed.resetTestingModule();
    vi.unstubAllGlobals();
  });
  @Component({ changeDetection: ChangeDetectionStrategy.OnPush, imports: [PdViewScroll], template: `<div #r role="region" aria-label="r" pdViewScroll><p>content</p></div>` })
  class Region {
    readonly region = viewChild.required<ElementRef<HTMLElement>>('r');
  }
  it('adds tabindex when the content is wider than the box and removes it when it fits', async () => {
    let notify: () => void = () => undefined;
    vi.stubGlobal(
      'ResizeObserver',
      class {
        constructor(cb: () => void) {
          notify = cb;
        }
        observe(): void {}
        disconnect(): void {}
      },
    );
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection()] });
    const f = TestBed.createComponent(Region);
    document.body.appendChild(f.nativeElement);
    const region = f.componentInstance.region().nativeElement;
    const size = { sw: 100, cw: 100, sh: 10, ch: 10 };
    Object.defineProperties(region, { scrollWidth: { get: () => size.sw }, clientWidth: { get: () => size.cw }, scrollHeight: { get: () => size.sh }, clientHeight: { get: () => size.ch } });
    await settle(f);
    expect(region.getAttribute('tabindex')).toBeNull();
    size.sw = 400;
    notify();
    await settle(f);
    expect(region.getAttribute('tabindex')).toBe('0');
    size.sw = 100;
    size.sh = 90;
    notify();
    await settle(f);
    expect(region.getAttribute('tabindex')).toBe('0');
    size.sh = 10;
    notify();
    await settle(f);
    expect(region.getAttribute('tabindex')).toBeNull();
  });
});
