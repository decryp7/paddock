import { Injector } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { msg } from '@decryp7/paddock-ui/shell';
import { assertContributionsValid } from '@decryp7/paddock-ui/shell/testing';
import { defineView, WORKBENCH_PORTS, WorkbenchRegistry } from '@decryp7/paddock-ui/workbench';
import { provideWorkbenchTesting, RecordingWorkbenchPorts } from './workbench-testing';

const view = defineView({ id: 'wd.a', title: msg('wd.a', 'A'), icon: 'wd:a', layout: { area: 'document', min: { width: 400, height: 200 }, pref: { width: 800, height: 400 }, priority: 50 }, load: () => Promise.resolve(class {}) });

describe('provideWorkbenchTesting', () => {
  it('records what the ports were asked and registers workbench contributions', async () => {
    TestBed.configureTestingModule({ providers: provideWorkbenchTesting({ contributions: [{ views: [view] }] }) });
    expect(() => assertContributionsValid(TestBed.inject(Injector))).not.toThrow();
    expect(TestBed.inject(WorkbenchRegistry).views).toEqual([view]);
    const ports = TestBed.inject(WORKBENCH_PORTS);
    const rec = TestBed.inject(RecordingWorkbenchPorts);
    expect(ports).toBe(rec);
    expect(await ports.showView('wd.a')).toBe(true);
    expect(await ports.showView('wd.a', { background: true })).toBe(true);
    rec.refuse.add('wd.b');
    expect(await ports.showView('wd.b')).toBe(false);
    expect([rec.openPanels(), rec.activePanel()]).toEqual([['wd.a'], 'wd.a']);
    ports.closeView('wd.a');
    expect(await ports.applyPerspective('wd.p')).toBe(true);
    ports.showExplorer('wd.files');
    expect(ports.explorerShown()).toBe('wd.files');
    ports.hideExplorer();
    ports.openPalette('>');
    expect([rec.shown.length, rec.closed, rec.perspectives, rec.activePerspective(), rec.explorerShown(), rec.palettes, rec.openPanels()]).toEqual([3, ['wd.a'], ['wd.p'], 'wd.p', null, ['>'], []]);
  });

  it('works with no options', () => {
    TestBed.configureTestingModule({ providers: provideWorkbenchTesting() });
    expect(TestBed.inject(WORKBENCH_PORTS)).toBeInstanceOf(RecordingWorkbenchPorts);
  });
});
