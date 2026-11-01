import { Component, ChangeDetectionStrategy, input, type ComponentRef } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { DIALOG_EVENTS } from '@decryp7/paddock-ui/shell';
import { FakeDialogIo } from '@decryp7/paddock-ui/shell/testing';
import { followDialogInputs, sendDialogInputs } from './retarget';

@Component({ selector: 'pd-test-target', changeDetection: ChangeDetectionStrategy.OnPush, template: '{{ page() }}' })
class Target {
  readonly page = input('');
}

const CHANNEL = 'paddock-dialog-inputs';

function target(): { ref: ComponentRef<unknown>; text: () => string; sets: () => number } {
  const ref = TestBed.createComponent(Target).componentRef as ComponentRef<unknown>;
  const setInput = vi.spyOn(ref, 'setInput');
  return { ref, text: () => (ref.location.nativeElement as HTMLElement).textContent ?? '', sets: () => setInput.mock.calls.length };
}

describe('dialog re-targeting', () => {
  it('sets the declared inputs of the dialog with that id, and nothing for another id or an undeclared name', () => {
    const t = target();
    const stop = followDialogInputs('t.one', t.ref);
    sendDialogInputs('t.two', { page: 'elsewhere' });
    expect(t.text()).toBe('');
    sendDialogInputs('t.one', { page: 'a', nope: 1 });
    expect(t.text()).toBe('a');
    expect(t.sets()).toBe(1);
    stop();
  });

  it('hears the window system\'s event: another webview\'s page re-targets this dialog with emitTo, which a BroadcastChannel may not carry', async () => {
    const t = target();
    const io = new FakeDialogIo();
    const stop = followDialogInputs('t.native', t.ref, io);
    await Promise.resolve();
    io.emit(DIALOG_EVENTS.inputs, { dialogId: 't.other', inputs: { page: 'no' } });
    expect(t.text()).toBe('');
    io.emit(DIALOG_EVENTS.inputs, { dialogId: 't.native', inputs: { page: 'yes', nope: 1 } });
    expect(t.text()).toBe('yes');
    io.emit(DIALOG_EVENTS.inputs, 'garbage');
    io.emit(DIALOG_EVENTS.inputs, { dialogId: 't.native', inputs: [1] });
    expect(t.sets()).toBe(1);
    stop();
    io.emit(DIALOG_EVENTS.inputs, { dialogId: 't.native', inputs: { page: 'late' } });
    expect(t.text()).toBe('yes');
  });

  it('applies a message once: a page does not act on its own echo from the channel', async () => {
    const t = target();
    const stop = followDialogInputs('t.echo', t.ref);
    sendDialogInputs('t.echo', { page: 'x' });
    // Give the channel time to echo (it would arrive as a task of its own), by waiting for a message sent after it.
    const probe = new BroadcastChannel(CHANNEL);
    const heard = new Promise<void>((r) => (probe.onmessage = () => r()));
    sendDialogInputs('t.other', { page: 'y' });
    await heard;
    probe.close();
    expect(t.sets()).toBe(1);
    stop();
  });

  it('reaches another page of the same origin over the channel, only for its own dialog id and only for a well-formed message', async () => {
    const t = target();
    const stop = followDialogInputs('t.far', t.ref);
    const other = new BroadcastChannel(CHANNEL);
    other.postMessage({ from: 'another-page', dialogId: 't.elsewhere', inputs: { page: 'wrong dialog' } });
    other.postMessage({ from: 'another-page', dialogId: 't.far', inputs: 'not a record' });
    other.postMessage({ dialogId: 't.far', inputs: { page: 'no sender' } });
    other.postMessage('garbage');
    other.postMessage({ from: 'another-page', dialogId: 't.far', inputs: { page: 'remote' } });
    await vi.waitFor(() => expect(t.text()).toBe('remote'));
    expect(t.sets()).toBe(1);
    other.close();
    stop();
  });

  it('stops listening when stopped: later messages change nothing, locally or over the channel', async () => {
    const t = target();
    const stop = followDialogInputs('t.stop', t.ref);
    stop();
    sendDialogInputs('t.stop', { page: 'local' });
    const other = new BroadcastChannel(CHANNEL);
    const probe = new BroadcastChannel(CHANNEL);
    const heard = new Promise<void>((r) => (probe.onmessage = () => r()));
    other.postMessage({ from: 'another-page', dialogId: 't.stop', inputs: { page: 'remote' } });
    await heard;
    other.close();
    probe.close();
    expect(t.sets()).toBe(0);
    expect(t.text()).toBe('');
  });
});
