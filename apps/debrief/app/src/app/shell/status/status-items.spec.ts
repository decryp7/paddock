import { ChangeDetectionStrategy, Component, Injector } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { ContributionRegistry } from '@decryp7/paddock-ui/shell/contributions';
import { contributionProblems } from '@decryp7/paddock-ui/shell/testing';
import { MemoryLogger, provideFakeDock, provideTinyBackend, settleUntil } from '../../../testing/helpers';
import { appConfig } from '../../app.config';
import { Logger } from '../../core/logger';
import { localMessage } from '../../extensions/contributions';
import { provideToolsBackend } from '../../core/tools-backend';
import { StatusBar } from '../status-bar';
import { debriefStatusItem, DEBRIEF_STATUS_ITEMS, provideStatusItems, type DebriefStatusItemDef } from './status-items.contributions';

@Component({ selector: 'debrief-test-start-a', template: `<span>start A</span>`, changeDetection: ChangeDetectionStrategy.OnPush })
class StartA {}
@Component({ selector: 'debrief-test-start-b', template: `<span>start B</span>`, changeDetection: ChangeDetectionStrategy.OnPush })
class StartB {}
@Component({ selector: 'debrief-test-end-a', template: `<span>end A</span>`, changeDetection: ChangeDetectionStrategy.OnPush })
class EndA {}
@Component({ selector: 'debrief-test-end-plugin', template: `<span>end plugin</span>`, changeDetection: ChangeDetectionStrategy.OnPush })
class EndPlugin {}

const item = (id: string, side: 'start' | 'end', order: number, load: DebriefStatusItemDef['load'], extra: Partial<DebriefStatusItemDef> = {}): DebriefStatusItemDef =>
  debriefStatusItem({ id, title: localMessage(id, () => id), side, priority: 500, order, load, ...extra });

describe('status items as contributions (M1)', () => {
  function bar(...batches: (readonly DebriefStatusItemDef[])[]) {
    TestBed.configureTestingModule({
      providers: [provideTinyBackend(), provideFakeDock(), provideToolsBackend(), ...batches.flatMap((b) => provideStatusItems(b)), { provide: Logger, useFactory: () => new MemoryLogger() }],
    });
    const fixture = TestBed.createComponent(StatusBar);
    const texts = () => [...(fixture.nativeElement as HTMLElement).querySelectorAll('debrief-test-start-a, debrief-test-start-b, debrief-test-end-a, debrief-test-end-plugin, .spacer')].map((e) => e.textContent?.trim() || 'spacer');
    return { fixture, texts };
  }

  it('renders the contributed items on their side, by order, with the spacer between, plugins’ items included', async () => {
    const { fixture, texts } = bar(
      [item('db.test.end-a', 'end', 200, async () => EndA), item('db.test.start-b', 'start', 200, async () => StartB), item('db.test.start-a', 'start', 100, async () => StartA)],
      [item('db.test.plugin', 'end', 150, async () => EndPlugin)],
    );
    await settleUntil(fixture, () => expect(texts()).toHaveLength(5));
    expect(texts()).toEqual(['start A', 'start B', 'spacer', 'end plugin', 'end A']);
  });

  it('leaves out an item whose code fails to load or whose when is false, and shows the others', async () => {
    const { fixture, texts } = bar([
      item('db.test.start-a', 'start', 100, async () => StartA),
      item('db.test.broken', 'start', 150, () => Promise.reject(new Error('chunk missing'))),
      item('db.test.end-a', 'end', 100, async () => EndA, { when: () => false }),
      item('db.test.start-b', 'start', 200, async () => StartB),
    ]);
    await settleUntil(fixture, () => expect(texts()).toHaveLength(3));
    expect(texts()).toEqual(['start A', 'start B', 'spacer']);
    expect((TestBed.inject(Logger) as MemoryLogger).entries.some((e) => e.message.includes('Status item'))).toBe(true);
  });

  it('registers Debrief’s built-in items in the shell’s statusItems kind, lazily, and the shell lint accepts them', () => {
    TestBed.configureTestingModule({ providers: [...appConfig.providers, { provide: Logger, useFactory: () => new MemoryLogger() }] });
    const items = TestBed.inject(ContributionRegistry).statusItems();
    expect(items.map((i) => i.id)).toEqual(DEBRIEF_STATUS_ITEMS.map((i) => i.id));
    expect(items.every((i) => typeof i.load === 'function')).toBe(true);
    expect(contributionProblems(TestBed.inject(Injector)).filter((p) => p.startsWith('status item'))).toEqual([]);
  });
});
