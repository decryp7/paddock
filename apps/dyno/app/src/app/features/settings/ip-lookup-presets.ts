/**
 * The lookup presets offered in Settings. They mirror `dyno_core::ip_lookup::PRESETS` (the engine validates every template
 * again; this list only fills the dropdown). Each site accepts the address in the path.
 */
export interface LookupPreset {
  id: string;
  name: string;
  about: string;
  ip: string;
  asn: string;
}

export const LOOKUP_PRESETS: readonly LookupPreset[] = [
  { id: 'ipinfo', name: 'ipinfo.io', about: 'Location and network owner', ip: 'https://ipinfo.io/{ip}', asn: 'https://ipinfo.io/{asn}' },
  { id: 'bgp', name: 'bgp.he.net', about: 'Network owner and BGP routing', ip: 'https://bgp.he.net/ip/{ip}', asn: 'https://bgp.he.net/{asn}' },
  { id: 'abuseipdb', name: 'AbuseIPDB', about: 'Abuse reports about the address', ip: 'https://www.abuseipdb.com/check/{ip}', asn: '' },
];

/** The preset whose IP template is `template`, or `custom`. */
export function presetIdFor(template: string): string {
  return LOOKUP_PRESETS.find((p) => p.ip === template)?.id ?? 'custom';
}

/** A quick check the editor shows while typing; the engine's check is the one that counts. */
export function templateProblem(template: string, placeholder: 'ip' | 'asn'): string | null {
  const t = template.trim();
  if (!t) return placeholder === 'asn' ? null : 'Enter an address that contains {ip}.';
  if (!t.startsWith('https://')) return 'The address must start with https://.';
  if (!t.includes(`{${placeholder}}`)) return `The address must contain {${placeholder}} where the value goes.`;
  return null;
}
