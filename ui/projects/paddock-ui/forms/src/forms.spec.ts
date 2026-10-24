import { Component, provideZonelessChangeDetection, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { form, FormField, maxLength, min, minLength, required, validate } from '@angular/forms/signals';
import { describe, expect, it } from 'vitest';
import { PdField, PdFieldControl } from './field';
import { defaultErrorText, errorText, fieldMessage, showErrors, type PdFieldStateLike } from './messages';

const state = (over: Partial<Record<'touched' | 'disabled' | 'pending', boolean>> & { errors?: { kind: string; message?: string }[] } = {}): PdFieldStateLike => ({
  errors: () => over.errors ?? [{ kind: 'required' }],
  touched: () => over.touched ?? false,
  disabled: () => over.disabled ?? false,
  pending: () => over.pending ?? false,
});

describe('validation display', () => {
  it('has plain words for the built-in kinds and for any other', () => {
    expect(defaultErrorText({ kind: 'required' })).toBe('This is required.');
    expect(defaultErrorText({ kind: 'min', min: 3 } as never)).toBe('Enter 3 or more.');
    expect(defaultErrorText({ kind: 'max', max: 9 } as never)).toBe('Enter 9 or less.');
    expect(defaultErrorText({ kind: 'minLength', minLength: 4 } as never)).toBe('Use at least 4 characters.');
    expect(defaultErrorText({ kind: 'maxLength', maxLength: 8 } as never)).toBe('Use at most 8 characters.');
    expect(defaultErrorText({ kind: 'pattern' })).toContain('format');
    expect(defaultErrorText({ kind: 'email' })).toContain('email');
    expect(defaultErrorText({ kind: 'custom' })).toBe('This value is not valid.');
    expect(defaultErrorText({ kind: 'min', min: 'x' } as never)).toBe('Enter 0 or more.');
  });

  it('prefers the error’s own message (an engine refusal), then the app’s words, then the default', () => {
    expect(errorText({ kind: 'required', message: 'Pitwall refused: no key.' })).toBe('Pitwall refused: no key.');
    expect(errorText({ kind: 'required' }, { required: 'Give it a name.' })).toBe('Give it a name.');
    expect(errorText({ kind: 'min', min: 2 } as never, { min: (e) => `At least ${(e as unknown as { min: number }).min}.` })).toBe('At least 2.');
    expect(errorText({ kind: 'required' })).toBe('This is required.');
  });

  it('shows errors after the field was left or on submit, not while pending, disabled or without errors', () => {
    expect(showErrors(state())).toBe(false);
    expect(showErrors(state({ touched: true }))).toBe(true);
    expect(showErrors(state(), true)).toBe(true);
    expect(showErrors(state({ touched: true, pending: true }))).toBe(false);
    expect(showErrors(state({ touched: true, disabled: true }))).toBe(false);
    expect(showErrors(state({ touched: true, errors: [] }))).toBe(false);
    expect(showErrors({ errors: () => [{ kind: 'x' }], touched: () => true })).toBe(true);
  });

  it('gives the first error’s message, or null', () => {
    expect(fieldMessage(null)).toBeNull();
    expect(fieldMessage(state())).toBeNull();
    expect(fieldMessage(state({ touched: true, errors: [{ kind: 'required' }, { kind: 'pattern' }] }))).toBe('This is required.');
    expect(fieldMessage(state({ touched: true, errors: [] }), {}, true)).toBeNull();
  });
});

interface Model {
  name: string;
  port: number;
}

@Component({
  imports: [PdField, PdFieldControl, FormField],
  template: `
    <pd-field label="Name" hint="Shown in the list" [state]="f.name()" [submitted]="submitted()" [messages]="messages">
      <input class="pd-input" type="text" pdFieldControl [formField]="f.name" />
    </pd-field>
    <pd-field label="Port" [state]="f.port()">
      <input class="pd-input" type="number" pdFieldControl [formField]="f.port" />
    </pd-field>
    <pd-field label="Plain"><input pdFieldControl /></pd-field>
  `,
})
class Host {
  readonly model = signal<Model>({ name: '', port: 80 });
  readonly submitted = signal(false);
  readonly messages = { required: 'Give it a name.' };
  readonly f = form(this.model, (p) => {
    required(p.name);
    minLength(p.name, 3);
    maxLength(p.name, 8);
    min(p.port, 1);
    validate(p.port, ({ value }) => (value() === 13 ? { kind: 'unlucky', message: 'Pick another port.' } : undefined));
  });
}

async function settle(f: { whenStable(): Promise<unknown> }): Promise<void> {
  await f.whenStable();
  TestBed.tick();
  await f.whenStable();
}

describe('pd-field', () => {
  async function setup() {
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection()] });
    const f = TestBed.createComponent(Host);
    document.body.appendChild(f.nativeElement);
    await settle(f);
    return f;
  }
  const fields = (f: { nativeElement: HTMLElement }) => [...f.nativeElement.querySelectorAll<HTMLElement>('pd-field')];

  it('labels the control, marks required fields and says nothing about an untouched field', async () => {
    const f = await setup();
    const [name] = fields(f);
    const input = name!.querySelector('input')!;
    expect(name!.querySelector('label')!.getAttribute('for')).toBe(input.id);
    expect(name!.querySelector('label')!.textContent).toContain('Name');
    expect(name!.querySelector('.pd-required')).not.toBeNull();
    expect(name!.querySelector('[data-testid=field-error]')).toBeNull();
    expect(input.getAttribute('aria-invalid')).toBeNull();
    expect(input.getAttribute('aria-describedby')).toBe(name!.querySelector('.pd-field__hint')!.id);
  });

  it('shows the error with a 14 px alert icon after the user left the field, and wires aria', async () => {
    const f = await setup();
    const input = fields(f)[0]!.querySelector('input')!;
    input.dispatchEvent(new Event('blur'));
    await settle(f);
    const error = fields(f)[0]!.querySelector('[data-testid=field-error]')!;
    expect(error.textContent).toContain('Give it a name.');
    expect(error.querySelector('pd-glyph svg')?.getAttribute('width')).toBe('14');
    expect(input.getAttribute('aria-invalid')).toBe('true');
    expect(input.getAttribute('aria-describedby')).toContain(error.id);
    expect(fields(f)[0]!.classList.contains('pd-field--invalid')).toBe(true);
    input.value = 'ab';
    input.dispatchEvent(new Event('input'));
    await settle(f);
    expect(fields(f)[0]!.querySelector('[data-testid=field-error]')?.textContent).toContain('at least 3');
    input.value = 'Valid';
    input.dispatchEvent(new Event('input'));
    await settle(f);
    expect(fields(f)[0]!.querySelector('[data-testid=field-error]')).toBeNull();
    expect(input.getAttribute('aria-invalid')).toBeNull();
  });

  it('shows the errors of untouched fields once the form is submitted, and the validator’s own message', async () => {
    const f = await setup();
    expect(fields(f)[1]!.querySelector('[data-testid=field-error]')).toBeNull();
    f.componentInstance.model.set({ name: 'Valid', port: 13 });
    f.componentInstance.submitted.set(true);
    await settle(f);
    expect(fields(f)[0]!.querySelector('[data-testid=field-error]')).toBeNull();
    const port = fields(f)[1]!.querySelector('input')!;
    port.dispatchEvent(new Event('blur'));
    await settle(f);
    expect(fields(f)[1]!.querySelector('[data-testid=field-error]')?.textContent).toContain('Pick another port.');
  });

  it('works for a control without a form state', async () => {
    const f = await setup();
    const plain = fields(f)[2]!;
    expect(plain.querySelector('input')!.getAttribute('aria-describedby')).toBeNull();
    expect(plain.querySelector('[data-testid=field-error]')).toBeNull();
  });
});
