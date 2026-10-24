import { SU_CONTRASTS, SU_DENSITIES, SU_MOTIONS, SU_THEMES } from '@decryp7/paddock-ui';
import type { Appearance } from '@decryp7/paddock-ui/shell';
import type { SettingValue } from '../generated/settings';

const PACK_ID = /^[a-z][a-z0-9]*(?:\.[a-z0-9]+(?:-[a-z0-9]+)*)+$/;

/**
 * The look in the shell's Appearance section (`paddock_settings::shell::appearance`): each field that holds a known value
 * (an empty `pack` is no pack). Fields the engine did not send, or sent with an unknown value, are left out.
 */
export function appearanceFromSettings(values: Readonly<Record<string, SettingValue>>): Partial<Appearance> {
  const out: { -readonly [K in keyof Appearance]?: Appearance[K] } = {};
  const theme = SU_THEMES.find((t) => t === values['theme']);
  const contrast = SU_CONTRASTS.find((t) => t === values['contrast']);
  const motion = SU_MOTIONS.find((t) => t === values['motion']);
  const density = SU_DENSITIES.find((d) => d === values['density']);
  const pack = values['pack'];
  if (theme) out.theme = theme;
  if (contrast) out.contrast = contrast;
  if (motion) out.motion = motion;
  if (density) out.density = density;
  if (pack === '') out.pack = null;
  else if (typeof pack === 'string' && PACK_ID.test(pack)) out.pack = pack;
  return out;
}

/** The Appearance section's values for a look (what `settings_set` is sent). */
export function appearanceToSettings(a: Appearance): Record<string, SettingValue> {
  return { theme: a.theme, contrast: a.contrast, motion: a.motion, density: a.density, pack: a.pack ?? '' };
}
