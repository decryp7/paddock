import { inject, type Signal } from '@angular/core';
import { toSignal } from '@angular/core/rxjs-interop';
import { ActivatedRoute } from '@angular/router';
import { map } from 'rxjs';

/** A route parameter as a signal (the pages frame routes pages itself, so there is no component input binding). */
export function routeParam(name: string): Signal<string | undefined> {
  const route = inject(ActivatedRoute);
  return toSignal(route.paramMap.pipe(map((p) => p.get(name) ?? undefined)), { initialValue: route.snapshot.paramMap.get(name) ?? undefined });
}
