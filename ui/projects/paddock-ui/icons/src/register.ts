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
 * `registerPaddockIcons(inject(MatIconRegistry), inject(DomSanitizer))`. The literals are built here from fixed path data,
 * which is why trusting them is safe. Registering again replaces the same icons. An app registers its own domain glyphs
 * the same way in its own namespace with `outlineIcon`.
 */
export function registerPaddockIcons(registry: IconLiteralRegistry, sanitizer: TrustedHtmlFactory): void {
  for (const [name, svg] of pdIconLiterals()) {
    registry.addSvgIconLiteralInNamespace(PD_ICON_NAMESPACE, name, sanitizer.bypassSecurityTrustHtml(svg) as never);
  }
}
