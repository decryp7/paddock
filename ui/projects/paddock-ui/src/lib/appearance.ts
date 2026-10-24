/** The attributes an app sets on <html> to select the look. */
export const SU_THEMES = ['system', 'dark', 'light'] as const;
/**
 * One accent per app of the family, and `graphite` for the demo apps (the token tests measure each in both schemes and
 * both contrast modes).
 */
export const SU_ACCENTS = ['debrief', 'pitcrew', 'pitwall', 'dyno', 'spotter', 'pacenotes', 'sighter', 'graphite'] as const;
export const SU_DENSITIES = ['comfortable', 'standard', 'compact'] as const;
/** `system` follows the OS (Increase Contrast, Windows contrast themes); `more` forces high contrast, `standard` turns it off. */
export const SU_CONTRASTS = ['system', 'standard', 'more'] as const;
/** `system` follows the OS (Reduce Motion); `reduce` forces fades only, `full` keeps motion although the OS asks to reduce. */
export const SU_MOTIONS = ['system', 'reduce', 'full'] as const;
export type SuTheme = (typeof SU_THEMES)[number];
export type SuAccent = (typeof SU_ACCENTS)[number];
export type SuDensity = (typeof SU_DENSITIES)[number];
export type SuContrast = (typeof SU_CONTRASTS)[number];
export type SuMotion = (typeof SU_MOTIONS)[number];

/** Everything `applyPaddockAppearance` selects; a missing field leaves that attribute as it is. */
export interface SuAppearance {
  theme?: SuTheme;
  accent?: SuAccent;
  density?: SuDensity;
  contrast?: SuContrast;
  motion?: SuMotion;
}

/**
 * Applies theme, accent, density, contrast and motion to the root element. `system` removes the attribute so the OS
 * preference decides. Unknown values are ignored (parse, don't validate: callers pass the typed unions).
 */
export function applyPaddockAppearance(root: HTMLElement, appearance: SuAppearance): void {
  const { theme, accent, density, contrast, motion } = appearance;
  if (theme !== undefined && SU_THEMES.includes(theme)) {
    if (theme === 'system') root.removeAttribute('data-theme');
    else root.setAttribute('data-theme', theme);
  }
  if (accent !== undefined && SU_ACCENTS.includes(accent)) root.setAttribute('data-accent', accent);
  if (density !== undefined && SU_DENSITIES.includes(density)) root.setAttribute('data-density', density);
  if (contrast !== undefined && SU_CONTRASTS.includes(contrast)) {
    if (contrast === 'system') root.removeAttribute('data-contrast');
    else root.setAttribute('data-contrast', contrast === 'more' ? 'more' : 'off');
  }
  if (motion !== undefined && SU_MOTIONS.includes(motion)) {
    if (motion === 'system') root.removeAttribute('data-motion');
    else root.setAttribute('data-motion', motion);
  }
}
