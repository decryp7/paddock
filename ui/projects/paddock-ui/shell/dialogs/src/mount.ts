import { ComponentRef, EnvironmentInjector, Injector, reflectComponentType, Type, ViewContainerRef } from '@angular/core';
import { DIALOG_INPUT, DIALOG_REF, type DialogRef } from './types';

/** The injector a dialog's content is created in: `DIALOG_REF` and `DIALOG_INPUT` over the dialog's own providers. */
export function contentInjector(parent: EnvironmentInjector | Injector, ref: DialogRef, input: Readonly<Record<string, unknown>>): Injector {
  return Injector.create({
    providers: [
      { provide: DIALOG_REF, useValue: ref },
      { provide: DIALOG_INPUT, useValue: input },
    ],
    parent,
  });
}

/**
 * Creates a dialog's content in `host` and sets the inputs the component declares (keys it does not declare stay
 * available through `DIALOG_INPUT`). Created here rather than by an outlet, so a throw while the content is constructed or
 * first rendered reaches the caller as an error state instead of a blank window.
 */
export function mountContent<C>(
  host: ViewContainerRef,
  component: Type<C>,
  injector: Injector,
  environment: EnvironmentInjector,
  input: Readonly<Record<string, unknown>>,
): ComponentRef<C> {
  const declared = new Set((reflectComponentType(component)?.inputs ?? []).map((i) => i.templateName));
  try {
    const ref = host.createComponent(component, { injector, environmentInjector: environment });
    for (const [name, value] of Object.entries(input)) if (declared.has(name)) ref.setInput(name, value);
    ref.changeDetectorRef.detectChanges();
    return ref;
  } catch (e) {
    host.clear();
    throw e;
  }
}

export function errorText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

export function isRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === 'object' && v !== null && !Array.isArray(v);
}
