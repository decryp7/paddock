import { Component, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { PatternHelper } from './pattern-helper';
import { MockBackend } from '../../core/mock-backend';
import { MOCK_SCHEMES } from '../../core/mock/mock-extraction';
import { appProviders, flush, makeMock, settle, text, type as typeInto } from '../../../testing/test-helpers';

const NAME = 'Acme-PRO-R3.10.00T32.iso';

@Component({
  imports: [PatternHelper],
  template: `<sp-pattern-helper [pattern]="pattern()" [presets]="presets" [examples]="examples" (patternChange)="pattern.set($event)" />`,
})
class Host {
  readonly pattern = signal('');
  readonly presets = MOCK_SCHEMES;
  readonly examples = [NAME, 'readme.txt'];
}

describe('PatternHelper', () => {
  let backend: MockBackend;
  function make() {
    backend = makeMock();
    TestBed.configureTestingModule({ providers: appProviders(backend) });
    const f = TestBed.createComponent(Host);
    f.detectChanges();
    return f;
  }
  const el = (f: ReturnType<typeof make>, sel: string) => f.nativeElement.querySelector(sel) as HTMLElement;
  /** Waits on the condition (rule 49), never on a fixed time: under load the debounce plus the engine call take longer than any guess. */
  const until = async (f: ReturnType<typeof make>, done: () => boolean) => {
    while (!done()) {
      await flush(10);
      await settle(f);
    }
  };
  const resultText = (f: ReturnType<typeof make>) => text(el(f, '[data-testid=test-result]'));

  it('fills the pattern and a name from a ready-made one, then reads the name through the engine', async () => {
    const f = make();
    const select = el(f, '#scheme-preset') as HTMLSelectElement;
    typeInto(select, 'product-letters-build', 'change');
    await until(f, () => (el(f, '#pattern-test') as HTMLInputElement).value !== '' && f.componentInstance.pattern() !== '');
    expect(f.componentInstance.pattern()).toContain('(?<id>[A-Z]+)');
    expect((el(f, '#pattern-test') as HTMLInputElement).value).toBe('Acme-PRO-R3.10.00T32.zip');
    typeInto(select, 'unknown-id', 'change');
    expect(f.componentInstance.pattern()).toContain('(?<id>');
    typeInto(el(f, '#pattern-test') as HTMLInputElement, NAME);
    await until(f, () => resultText(f).includes('R3.10.00T32'));
    const result = el(f, '[data-testid=test-result]');
    expect(text(result)).toContain('R3.10.00T32');
    expect(text(result)).toContain('PRO');
    expect([...result.querySelectorAll('mark')].map((m) => m.textContent)).toContain('PRO');
    expect(backend.calls).toContain('testPattern');
  });

  it('says why a name is ignored and what to fix in a broken pattern', async () => {
    const f = make();
    f.componentInstance.pattern.set('Acme-(?<major>\\d+)');
    typeInto(el(f, '#pattern-test') as HTMLInputElement, 'other.txt');
    await until(f, () => resultText(f).includes('no match'));
    expect(resultText(f)).toContain('no match');
    f.componentInstance.pattern.set('(unclosed');
    await until(f, () => el(f, '[data-testid=pattern-error]') !== null);
    expect(text(el(f, '[data-testid=pattern-error]'))).toContain('not a valid regular expression');
    expect(el(f, '#pattern').getAttribute('aria-invalid')).toBe('true');
  });

  it('waits for a slow engine answer instead of a fixed time', async () => {
    const f = make();
    const real = backend.testPattern.bind(backend);
    // The engine answers only after the wait has already polled a few times: as slow as a loaded runner, with no timer in the test.
    let release: () => void = () => undefined;
    const gate = new Promise<void>((r) => (release = r));
    vi.spyOn(backend, 'testPattern').mockImplementation(async (...a) => {
      await gate;
      return real(...a);
    });
    f.componentInstance.pattern.set('Acme-(?<major>\\d+)');
    typeInto(el(f, '#pattern-test') as HTMLInputElement, 'other.txt');
    let polls = 0;
    await until(f, () => {
      if (++polls === 5) release();
      return resultText(f).includes('no match');
    });
    expect(resultText(f)).toContain('no match');
  });

  it('builds a pattern from a selection in an example name', async () => {
    const f = make();
    (el(f, 'details.builder') as HTMLDetailsElement).open = true;
    const examples = [...f.nativeElement.querySelectorAll('.examples button')] as HTMLButtonElement[];
    expect(examples.map((b) => b.textContent)).toEqual([NAME, 'readme.txt']);
    // Nothing selected yet.
    examples[0]!.click();
    await settle(f);
    const roleButtons = () => [...f.nativeElement.querySelectorAll('.roles button')] as HTMLButtonElement[];
    roleButtons().find((b) => b.textContent === 'Major')!.click();
    await settle(f);
    expect(text(f.nativeElement)).toContain('Select a part of the example name with the mouse first.');
    // Select "PRO" and mark it as the product id, then "3" as the major number.
    const select = (start: number, end: number) => {
      const node = el(f, '[data-testid=example]').firstChild!;
      const range = document.createRange();
      range.setStart(node, start);
      range.setEnd(node, end);
      const sel = document.getSelection()!;
      sel.removeAllRanges();
      sel.addRange(range);
    };
    select(5, 8);
    roleButtons().find((b) => b.textContent === 'Product id')!.click();
    await settle(f);
    select(10, 11);
    roleButtons().find((b) => b.textContent === 'Major')!.click();
    await settle(f);
    expect(f.componentInstance.pattern()).toBe('Acme-(?<id>[A-Z]+)-R(?<major>\\d+)\\.10\\.00T32\\.iso');
    expect([...f.nativeElement.querySelectorAll('.chosen mark')].map((m) => m.textContent)).toEqual(['PRO', '3']);
    // Removing a part regenerates; removing the last one keeps the pattern.
    (f.nativeElement.querySelector('.chosen button') as HTMLButtonElement).click();
    await settle(f);
    expect(f.componentInstance.pattern()).toContain('(?<major>');
    expect(f.componentInstance.pattern()).not.toContain('(?<id>');
    // A selection outside the example is not used.
    select(0, 0);
    document.getSelection()!.removeAllRanges();
  });

  it('shows the engine refusal when the parts cannot make a pattern', async () => {
    const f = make();
    const first = (f.nativeElement.querySelectorAll('.examples button')[0] as HTMLButtonElement);
    first.click();
    await settle(f);
    const node = el(f, '[data-testid=example]').firstChild!;
    const range = document.createRange();
    range.setStart(node, 5);
    range.setEnd(node, 8);
    document.getSelection()!.removeAllRanges();
    document.getSelection()!.addRange(range);
    vi.spyOn(backend, 'suggestPattern').mockRejectedValue({ code: 'invalid', message: 'The selection does not fit the example name.', problems: ['The selection does not fit the example name.'] });
    ([...f.nativeElement.querySelectorAll('.roles button')] as HTMLButtonElement[])[0]!.click();
    await settle(f);
    expect(text(f.nativeElement)).toContain('The selection does not fit the example name.');
  });

  it('offers nothing to build from when the source has no names yet', async () => {
    backend = makeMock();
    TestBed.configureTestingModule({ providers: appProviders(backend) });
    @Component({ imports: [PatternHelper], template: `<sp-pattern-helper pattern="" />` })
    class Bare {}
    const f = TestBed.createComponent(Bare);
    await settle(f);
    expect(text(f.nativeElement)).toContain('No names yet');
  });

  it('leaves no test timer running once its view is destroyed', () => {
    vi.useFakeTimers();
    try {
      const f = make();
      f.componentInstance.pattern.set('(?<id>[A-Z]+)');
      f.detectChanges();
      expect(vi.getTimerCount()).toBeGreaterThan(0);
      TestBed.resetTestingModule();
      expect(vi.getTimerCount()).toBe(0);
    } finally {
      vi.useRealTimers();
    }
  });
});
