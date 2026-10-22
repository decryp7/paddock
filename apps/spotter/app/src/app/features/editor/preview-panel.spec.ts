import { Component, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { PreviewPanel } from './preview-panel';
import type { SourcePreview } from '../../core/api.types';
import { idle, type Loadable } from '../../state/loadable';
import { byText, settle, text } from '../../../testing/test-helpers';

@Component({
  imports: [PreviewPanel],
  template: `<sp-preview-panel [state]="state()" [ready]="ready()" (rerun)="reruns = reruns + 1" />`,
})
class Host {
  readonly state = signal<Loadable<SourcePreview>>(idle());
  readonly ready = signal(true);
  reruns = 0;
}

const rel = (v: string, over = {}) => ({ version: v, prerelease: false, name: `tool-${v}`, location: 'x', publishedAtMs: Date.UTC(2026, 5, 15, 12), firstSeenMs: Date.UTC(2026, 5, 16, 12), size: 2048, sha256: null, ...over });
const preview = (over: Partial<SourcePreview> = {}): SourcePreview => ({ problems: [], error: null, releases: [], withoutVersion: [], prereleasesSkipped: 0, namesSkipped: 0, tookMs: 380, ...over });
const value = (v: SourcePreview, loading = false): Loadable<SourcePreview> => ({ value: v, loading, error: null });

function make() {
  const f = TestBed.createComponent(Host);
  f.detectChanges();
  return f;
}
const render = async (f: ReturnType<typeof make>, s: Loadable<SourcePreview>) => {
  f.componentInstance.state.set(s);
  await settle(f);
  return text(f.nativeElement);
};

describe('PreviewPanel', () => {
  it('waits for the required fields and disables Test again until then', () => {
    const f = make();
    f.componentInstance.ready.set(false);
    f.detectChanges();
    expect(text(f.nativeElement)).toContain('Fill in the required fields');
    expect((byText(f.nativeElement, 'button', 'Test again') as HTMLButtonElement).disabled).toBe(true);
  });

  it('asks for a re-run', () => {
    const f = make();
    (byText(f.nativeElement, 'button', 'Test again') as HTMLButtonElement).click();
    expect(f.componentInstance.reruns).toBe(1);
  });

  it('summarises the releases found, what was left out and what held no version', async () => {
    const f = make();
    const t = await render(f, value(preview({ releases: [rel('1.2.0'), rel('1.3.0-rc.1', { prerelease: true, publishedAtMs: null })], prereleasesSkipped: 3, namesSkipped: 1, withoutVersion: ['README.txt'] })));
    expect(t).toContain('2 releases found in');
    expect(t).toContain('3 pre-releases left out.');
    expect(t).toContain('1 did not match the filters.');
    expect(t).toContain('1.2.0');
    expect(t).toContain('Pre-release');
    expect(t).toContain('2.0 KiB');
    expect(t).toContain('1 name held no version');
    expect(t).toContain('README.txt');
    expect(f.nativeElement.querySelectorAll('tbody tr')).toHaveLength(2);
  });

  it('uses the singular forms', async () => {
    const f = make();
    const t = await render(f, value(preview({ releases: [rel('1.0.0')], prereleasesSkipped: 1, withoutVersion: ['a', 'b'] })));
    expect(t).toContain('1 release found');
    expect(t).toContain('1 pre-release left out.');
    expect(t).toContain('2 names held no version');
  });

  it('shows no table when nothing was found', async () => {
    const f = make();
    const t = await render(f, value(preview()));
    expect(t).toContain('0 releases found');
    expect(f.nativeElement.querySelector('table')).toBeNull();
    expect(f.nativeElement.querySelector('details')).toBeNull();
  });

  it('lists the problems the engine found instead of a result', async () => {
    const f = make();
    await render(f, value(preview({ problems: ['Repository is required.', 'A custom version scheme needs a pattern.'] })));
    expect(f.nativeElement.querySelectorAll('.problems li')).toHaveLength(2);
    expect(f.nativeElement.querySelector('table')).toBeNull();
  });

  it('shows a source error with its status word', async () => {
    const f = make();
    const t = await render(f, value(preview({ error: { kind: 'unreachable', message: 'The server did not answer.' } })));
    expect(t).toContain('Unreachable');
    expect(t).toContain('The server did not answer.');
    expect(f.nativeElement.querySelector('[role=alert]')).not.toBeNull();
  });

  it('keeps the last result while testing again, and shows a failed test with Retry', async () => {
    const f = make();
    const t = await render(f, value(preview({ releases: [rel('1.0.0')] }), true));
    expect(t).toContain('Testing…');
    expect(t).toContain('1.0.0');
    const failed = await render(f, { value: null, loading: false, error: { code: 'unavailable', message: 'Engine is down.', problems: [] } });
    expect(failed).toContain('Could not load the test');
    (byText(f.nativeElement, 'button', /retry/i) as HTMLButtonElement).click();
    expect(f.componentInstance.reruns).toBe(1);
  });
});
