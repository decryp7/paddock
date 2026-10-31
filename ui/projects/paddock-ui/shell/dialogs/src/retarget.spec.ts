import { Component, ChangeDetectionStrategy, input, type ComponentRef } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { followDialogInputs, sendDialogInputs } from './retarget';

@Component({ selector: 'pd-test-target', changeDetection: ChangeDetectionStrategy.OnPush, template: '{{ page() }}' })
class Target {
  readonly page = input('');
}

describe('dialog re-targeting', () => {
  it('sets the declared inputs of the dialog with that id, and nothing for another id or an undeclared name', () => {
    const ref = TestBed.createComponent(Target).componentRef as ComponentRef<unknown>;
    const stop = followDialogInputs('t.one', ref);
    sendDialogInputs('t.two', { page: 'elsewhere' });
    expect((ref.location.nativeElement as HTMLElement).textContent).toBe('');
    sendDialogInputs('t.one', { page: 'a', nope: 1 });
    expect((ref.location.nativeElement as HTMLElement).textContent).toBe('a');
    stop();
    sendDialogInputs('t.one', { page: 'b' });
    expect((ref.location.nativeElement as HTMLElement).textContent).toBe('a');
  });

  it('reaches another page of the same origin over the channel', async () => {
    const ref = TestBed.createComponent(Target).componentRef as ComponentRef<unknown>;
    const stop = followDialogInputs('t.far', ref);
    const other = new BroadcastChannel('paddock-dialog-inputs');
    other.postMessage({ dialogId: 't.far', inputs: { page: 'remote' } });
    other.postMessage({ dialogId: 't.far', inputs: 'not a record' });
    await vi.waitFor(() => expect((ref.location.nativeElement as HTMLElement).textContent).toBe('remote'));
    other.close();
    stop();
  });
});
