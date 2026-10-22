/** The attributes an app sets on <html> to select the look. */
export const SU_THEMES = ['system', 'dark', 'light'] as const;
export const SU_ACCENTS = ['debrief', 'pitcrew', 'pitwall', 'dyno', 'spotter'] as const;
export const SU_DENSITIES = ['comfortable', 'standard', 'compact'] as const;
export type SuTheme = (typeof SU_THEMES)[number];
export type SuAccent = (typeof SU_ACCENTS)[number];
export type SuDensity = (typeof SU_DENSITIES)[number];

/**
 * Applies theme, accent and density to the root element. `system` removes `data-theme` so the OS preference
 * decides. Unknown values are ignored (parse, don't validate: callers pass the typed unions).
 */
export function applyPaddockAppearance(
  root: HTMLElement,
  appearance: { theme?: SuTheme; accent?: SuAccent; density?: SuDensity },
): void {
  const { theme, accent, density } = appearance;
  if (theme !== undefined) {
    if (theme === 'system') root.removeAttribute('data-theme');
    else if (SU_THEMES.includes(theme)) root.setAttribute('data-theme', theme);
  }
  if (accent !== undefined && SU_ACCENTS.includes(accent)) root.setAttribute('data-accent', accent);
  if (density !== undefined && SU_DENSITIES.includes(density)) root.setAttribute('data-density', density);
}
