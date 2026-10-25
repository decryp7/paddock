import { TestBed } from '@angular/core/testing';
import { text } from '../../../testing/test-helpers';
import { RunnerStatusText } from './runner-status';

describe('RunnerStatusText', () => {
  function mount(status: string) {
    const f = TestBed.createComponent(RunnerStatusText);
    f.componentRef.setInput('status', status);
    f.detectChanges();
    return f.nativeElement as HTMLElement;
  }

  it('shows a dot and a sentence-case word, never a pill', () => {
    const el = mount('busy');
    expect(text(el)).toBe('Running a job');
    expect(el.querySelector('.badge')).toBeNull();
    expect(el.querySelector('.dot')?.className).toContain('tone-info');
  });

  it('keeps idle neutral and muted, colours exceptions', () => {
    const idle = mount('idle');
    expect(idle.querySelector('.dot')?.className).toContain('tone-neutral');
    expect(idle.querySelector('.muted-word')).not.toBeNull();
    expect(mount('crashLoop').querySelector('.dot')?.className).toContain('tone-danger');
    expect(text(mount('paused'))).toBe('Paused');
  });
});
