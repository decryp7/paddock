import { inject, makeEnvironmentProviders, provideAppInitializer, type EnvironmentProviders } from '@angular/core';
import { MatIconRegistry } from '@angular/material/icon';
import { DomSanitizer } from '@angular/platform-browser';
import { outlineIconWithDots } from './outline';
import { PD_ICON_DOTS, PD_ICON_PATHS, type PdIconName } from './paths';

/** The namespace of the generic set: `<mat-icon svgIcon="pd:refresh">`. */
export const PD_ICON_NAMESPACE = 'pd';

/** The SVG literal of one glyph. */
export function pdIconSvg(name: PdIconName): string {
  return outlineIconWithDots([PD_ICON_PATHS[name]], PD_ICON_DOTS[name] ?? []);
}

/** Every glyph as `[name, svg literal]`, in a stable order. */
export function pdIconLiterals(): ReadonlyArray<readonly [PdIconName, string]> {
  return (Object.keys(PD_ICON_PATHS) as PdIconName[]).map((n) => [n, pdIconSvg(n)] as const);
}

/** The part of Angular Material's `MatIconRegistry` that registration uses (the shell does not depend on Material). */
export interface IconLiteralRegistry {
  addSvgIconLiteralInNamespace(namespace: string, iconName: string, literal: never): unknown;
}

/** The part of Angular's `DomSanitizer` that registration uses. */
export interface TrustedHtmlFactory {
  bypassSecurityTrustHtml(value: string): unknown;
}

/**
 * Registers the `pd:` set as literals with Angular Material's registry, so no icon is ever fetched (the CSP forbids it):
 * `registerPaddockIcons(inject(MatIconRegistry), inject(DomSanitizer))`. Registering again replaces the same icons.
 *
 * Why `bypassSecurityTrustHtml` is acceptable here, and only here: Material's literal API takes `TrustedHTML` and would
 * otherwise sanitise the SVG away. Every literal is a static string built in this module from the constants in `paths.ts`
 * (bundled with the package; `outlineIcon` also strips anything but path characters), never from an input, a setting, a
 * model or the network. Do not pass anything else through this function; an app's own glyphs are trusted the same way, from
 * its own bundled constants, in its own namespace. An app registers its own domain glyphs
 * the same way in its own namespace with `outlineIcon`.
 */
export function registerPaddockIcons(registry: IconLiteralRegistry, sanitizer: TrustedHtmlFactory): void {
  for (const [name, svg] of pdIconLiterals()) {
    registry.addSvgIconLiteralInNamespace(PD_ICON_NAMESPACE, name, sanitizer.bypassSecurityTrustHtml(svg) as never);
  }
}

const registered = new WeakSet<object>();

/** Registers the set once per registry (the glyph calls it on first use, the provider at start-up). */
export function ensurePaddockIcons(registry: IconLiteralRegistry, sanitizer: TrustedHtmlFactory): void {
  if (registered.has(registry)) return;
  registered.add(registry);
  registerPaddockIcons(registry, sanitizer);
}

/**
 * Registers the `pd:` set with the window's `MatIconRegistry` at start-up: put it in every bootstrap (main window, dialog
 * window, pop-out, tray) that draws `<mat-icon svgIcon="pd:...">`, `pd-glyph` or `pd-icon`. `providePaddockMaterial()` and
 * `provideShellCore()` include it, so a shell window has it already. `pd-glyph` also registers on first use, so a window
 * that forgot it still draws.
 */
export function providePaddockIcons(): EnvironmentProviders {
  return makeEnvironmentProviders([provideAppInitializer(() => ensurePaddockIcons(inject(MatIconRegistry), inject(DomSanitizer)))]);
}

/** The id `<mat-icon svgIcon>` takes for a glyph name ("refresh" and "pd:refresh" both give "pd:refresh"). */
export function pdIconId(name: string): string {
  return `${PD_ICON_NAMESPACE}:${name.replace(/^pd:/, '')}`;
}

/** Whether `name` ("refresh" or "pd:refresh") is a glyph of the set. */
export function isPdIcon(name: string): name is PdIconName {
  const n = name.replace(/^pd:/, '');
  return Object.hasOwn(PD_ICON_PATHS, n) && (PD_ICON_PATHS[n as PdIconName] !== '' || Object.hasOwn(PD_ICON_DOTS, n));
}
