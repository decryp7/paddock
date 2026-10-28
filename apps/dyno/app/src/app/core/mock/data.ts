import { defaultTrayItems, layoutFromPreset } from './widget-presets';
import type {
  ActivityEntry,
  Alert,
  Anomaly,
  HealthReason,
  PluginInfo,
  Settings,
  SizeNode,
  StartupEntry,
} from '../api.types';
import { prng } from './noise';

const GB = 1024 ** 3;
const MB = 1024 ** 2;
const MIN = 60_000;
const HOUR = 60 * MIN;
const DAY = 24 * HOUR;

// -------------------------------------------------------------------------------------------- health

export function healthReasons(): HealthReason[] {
  return [
    {
      id: 'low-disk-c',
      title: 'Drive C: is 78% full',
      detail: '210 GB free of 953 GB, and build output fills about 5 GB a day: full in about 41 days. 63 GB can be freed safely.',
      impact: 8,
      severity: 'warning',
    },
    {
      id: 'uptime',
      title: 'Not restarted for 12 days',
      detail: 'Windows installed an update that finishes on restart, and long uptimes let memory fragment.',
      impact: 6,
      severity: 'warning',
    },
    {
      id: 'startup-apps',
      title: '9 apps start at sign-in',
      detail: 'Teams, Docker Desktop and Spotify have high impact: the desktop takes 48 s to settle after sign-in.',
      impact: 4,
      severity: 'info',
    },
    {
      id: 'battery-wear',
      title: 'Battery health 87%',
      detail: 'Full charge holds 87% of the design capacity after 412 cycles. Normal for a two-year-old laptop.',
      impact: 2,
      severity: 'info',
    },
  ];
}

export function demoStartup(): StartupEntry[] {
  const e = (
    id: string,
    name: string,
    command: string,
    location: StartupEntry['location'],
    publisher: string | null,
    cpuSecs: number,
    diskMb: number,
    rating: StartupEntry['impact']['rating'],
    state: StartupEntry['state'] = 'enabled',
  ): StartupEntry => ({
    id,
    name,
    command,
    exe: command.split('"')[1] ?? command.split(' ')[0] ?? null,
    location,
    state,
    publisher,
    impact: { cpuSecs, diskBytes: diskMb * MB, rating },
  });
  return [
    e('teams', 'Microsoft Teams', '"C:\\Program Files\\WindowsApps\\MSTeams\\ms-teams.exe" --process-start-reason=StartupTask', 'userRunKey', 'Microsoft Corporation', 9.8, 310, 'high'),
    e('docker', 'Docker Desktop', '"C:\\Program Files\\Docker\\Docker\\Docker Desktop.exe" -Autostart', 'userRunKey', 'Docker Inc.', 14.2, 520, 'high'),
    e('spotify', 'Spotify', '"C:\\Users\\alex\\AppData\\Roaming\\Spotify\\Spotify.exe" --autostart --minimized', 'userRunKey', 'Spotify AB', 4.1, 180, 'medium'),
    e('onedrive', 'Microsoft OneDrive', '"C:\\Program Files\\Microsoft OneDrive\\OneDrive.exe" /background', 'userRunKey', 'Microsoft Corporation', 3.2, 140, 'medium'),
    e('slack', 'Slack', '"C:\\Users\\alex\\AppData\\Local\\slack\\slack.exe" --startup', 'userRunKey', 'Slack Technologies, LLC', 2.6, 96, 'low', 'enabled'),
    e('nvidia', 'NVIDIA Broadcast', '"C:\\Program Files\\NVIDIA Corporation\\NVIDIA Broadcast\\NVIDIA Broadcast UI.exe"', 'machineRunKey', 'NVIDIA Corporation', 2.1, 88, 'low'),
    e('security', 'Windows Security notification icon', '"C:\\Windows\\System32\\SecurityHealthSystray.exe"', 'machineRunKey', 'Microsoft Corporation', 0.3, 4, 'low'),
    e('logi', 'Logi Options+', '"C:\\Program Files\\LogiOptionsPlus\\logioptionsplus_agent.exe"', 'commonStartupFolder', 'Logitech Inc', 1.9, 60, 'low', 'enabled'),
    e('adobe', 'Adobe Acrobat Update Task', '"C:\\Program Files (x86)\\Common Files\\Adobe\\ARM\\1.0\\AdobeARM.exe"', 'scheduledTask', 'Adobe Inc.', 0, 0, 'notMeasured'),
    e('steam', 'Steam', '"C:\\Program Files (x86)\\Steam\\steam.exe" -silent', 'userRunKey', 'Valve Corp.', 0, 0, 'notMeasured', 'disabled'),
    e('dyno', 'Dyno', '"C:\\Program Files\\Dyno\\Dyno.exe" --tray', 'userRunKey', 'Dyno contributors', 0.4, 12, 'low'),
  ];
}

// -------------------------------------------------------------------------------------------- reports and misc

export function demoAnomalies(now: number): Anomaly[] {
  return [
    {
      id: 'teams-growth',
      kind: 'memoryGrowth',
      app: 'Microsoft Teams',
      since: now - 26 * HOUR,
      title: 'Teams memory keeps growing',
      explanation: 'Teams grew from 610 MB to 1.25 GB over 26 hours without dropping back: probably a leak. Restarting Teams frees it.',
      evidence: [
        { label: 'Growth', value: '+24 MB / hour' },
        { label: 'Now', value: '1.25 GB' },
        { label: 'Fit', value: 'R² 0.94' },
      ],
    },
    {
      id: 'indexer-spikes',
      kind: 'periodicSpike',
      app: 'Windows Search',
      since: now - 5 * HOUR,
      title: 'SearchIndexer spikes every 15 minutes',
      explanation: 'Indexing restarts after each build because target/ folders are indexed. Excluding build folders stops it.',
      evidence: [
        { label: 'Period', value: '15 min' },
        { label: 'Peak', value: '18% CPU' },
        { label: 'Seen', value: '20 times' },
      ],
    },
    {
      id: 'docker-restarts',
      kind: 'restartLoop',
      app: 'Docker Desktop',
      since: now - 2 * HOUR,
      title: 'Docker backend restarted 6 times in 2 hours',
      explanation: 'com.docker.backend.exe crashes and restarts. Its last crash dump points at the WSL integration.',
      evidence: [
        { label: 'Restarts', value: '6 in 2 h' },
        { label: 'Last', value: '52 min ago' },
        { label: 'Dumps', value: '1 (640 MB)' },
      ],
    },
  ];
}

export function demoAlerts(now: number): Alert[] {
  return [
    { id: 'al-6', at: now - 52 * MIN, kind: 'crashLoop', severity: 'warning', title: 'Docker backend keeps restarting', detail: '6 restarts in 2 hours.' },
    { id: 'al-5', at: now - 3 * HOUR, kind: 'longHighCpu', severity: 'info', title: 'rustc.exe used over 50% CPU for 12 minutes', detail: 'It stayed busy for a long time.' },
    { id: 'al-4', at: now - 27 * HOUR, kind: 'memoryPressure', severity: 'warning', title: 'Memory pressure high', detail: 'Commit reached 92% of the limit while Docker and two browsers were open.' },
    { id: 'al-3', at: now - 2 * DAY - 4 * HOUR, kind: 'lowDisk', severity: 'warning', title: 'Drive C: below 25% free', detail: '210 GB free of 953 GB.' },
    { id: 'al-2', at: now - 4 * DAY, kind: 'overheating', severity: 'critical', title: 'CPU reached 98 °C', detail: 'Thermal throttling for 3 minutes during a build with the lid closed.' },
    { id: 'al-1', at: now - 6 * DAY, kind: 'longHighCpu', severity: 'info', title: 'MsMpEng.exe used 25% CPU for 9 minutes', detail: 'A scheduled Defender scan.' },
  ];
}

export function demoActivity(now: number): ActivityEntry[] {
  const a = (id: string, ago: number, kind: ActivityEntry['kind'], title: string, detail: string): ActivityEntry => ({ id, at: now - ago, kind, title, detail });
  return [
    a('act-4', 26 * HOUR, 'alert', 'Memory pressure high', 'Commit reached 92% of the limit'),
    a('act-2', 3 * DAY, 'alert', 'Docker backend keeps restarting', '6 restarts in 2 hours.'),
    a('act-1', 12 * DAY, 'system', 'Dyno started', 'Sampling every 1 s, processes every 2 s'),
  ];
}

export function demoPlugins(): PluginInfo[] {
  return [
    { id: 'dyno-plugin-example', name: 'Example plugin', version: '0.1.0', state: 'running', error: null, contributes: ['Sensor: uptime'] },
    { id: 'gpu-fans', name: 'GPU fan sensor', version: '0.3.2', state: 'quarantined', error: 'Crashed 3 times in 10 minutes (access violation reading the vendor API).', contributes: ['Sensor: GPU fans'] },
  ];
}

export function demoSettings(): Settings {
  return {
    version: 1,
    launchAtLogin: true,
    sampling: { intervalMs: 1000, processIntervalMs: 2000 },
    retention: { rawHours: 24, minuteDays: 14, quarterHourDays: 90, hourDays: 730 },
    tray: { metrics: ['cpu', 'memory', 'disk'], style: 'graph' },
    alerts: { enabled: true, lowDiskPercent: 10, overheatCelsius: 95, longHighCpuMinutes: 10 },
    webApi: { enabled: false, port: 7878, lan: false },
    dashboard: layoutFromPreset('overview'),
    ipLookup: { ipTemplate: 'https://ipinfo.io/{ip}', asnTemplate: '' },
    trayItems: defaultTrayItems(),
    alarms: { quiet: { enabled: false, startMinute: 22 * 60, endMinute: 7 * 60, criticalBypass: true }, chatterTransitions: 6, chatterWindowMinutes: 30, maxAlarms: 500, keepClearedDays: 14, utcOffsetMinutes: 0 },
    flightRecorder: true,
    customAlerts: [{ id: 'gpu-limit', name: 'GPU limit', metric: 'gpuLoad', op: 'above', limit: 90, forSecs: 120, severity: 'warning', enabled: true }],
    networkAlerts: { newExposedListener: false, newRemoteHost: false },
    notifications: true,
    baselines: { enabled: true, learningDays: 7, alarms: false },
    hardwareAlarms: { throttling: true, lifespan: true },
    unusual: { enabled: true, learningDays: 7 },
    maintenance: {
      enabled: true,
      windows: [
        { days: ['mon', 'tue', 'wed', 'thu', 'fri'], startMinute: 22 * 60, endMinute: 2 * 60 },
        { days: ['sat', 'sun'], startMinute: 23 * 60, endMinute: 5 * 60 },
      ],
    },
    backup: { alertWhenStale: true, staleAfterHours: 48, overrides: [] },
  };
}

/** A plausible size tree for the disk-usage treemap. */
export function diskTree(path: string | null): SizeNode {
  const node = (name: string, p: string, gb: number, children: SizeNode[] = []): SizeNode => {
    const childSum = children.reduce((a, c) => a + c.bytes, 0);
    return { name, path: p, bytes: Math.max(Math.round(gb * GB), childSum), children };
  };
  const rand = prng(42);
  const leaves = (base: string, names: string[], total: number): SizeNode[] => {
    const w = names.map(() => 0.3 + rand());
    const s = w.reduce((a, b) => a + b, 0);
    return names.map((n, i) => node(n, `${base}\\${n}`, (total * w[i]!) / s));
  };
  const dev = node('dev', 'C:\\dev', 182, [
    node('old-raytracer', 'C:\\dev\\old-raytracer', 9.4, leaves('C:\\dev\\old-raytracer', ['target', 'src', '.git'], 9.4)),
    node('dyno', 'C:\\dev\\dyno', 21, leaves('C:\\dev\\dyno', ['target', 'app', '.git', 'crates'], 21)),
    node('imt', 'C:\\dev\\imt', 34, leaves('C:\\dev\\imt', ['target', 'app', 'samples', '.git'], 34)),
    node('aoc-2025', 'C:\\dev\\aoc-2025', 5.1, leaves('C:\\dev\\aoc-2025', ['target', 'src'], 5.1)),
    node('electron-test', 'C:\\dev\\electron-test', 3.2, leaves('C:\\dev\\electron-test', ['node_modules', 'dist'], 3.2)),
    node('tauri-playground', 'C:\\dev\\tauri-playground', 4.6, leaves('C:\\dev\\tauri-playground', ['target', 'node_modules', 'src'], 4.6)),
    node('other projects', 'C:\\dev\\…', 104.7),
  ]);
  const users = node('Users', 'C:\\Users', 356, [
    node('alex', 'C:\\Users\\alex', 352, [
      node('AppData', 'C:\\Users\\alex\\AppData', 118, leaves('C:\\Users\\alex\\AppData', ['Local\\Docker', 'Local\\Google', 'Local\\Temp', 'Roaming\\Spotify', 'Local\\npm-cache', 'Local\\Packages'], 118)),
      node('.rustup', 'C:\\Users\\alex\\.rustup', 9.2),
      node('.cargo', 'C:\\Users\\alex\\.cargo', 6.8),
      node('.nuget', 'C:\\Users\\alex\\.nuget', 3.1),
      node('Downloads', 'C:\\Users\\alex\\Downloads', 41, leaves('C:\\Users\\alex\\Downloads', ['ISOs', 'Installers', 'Archives', 'Papers'], 41)),
      node('Documents', 'C:\\Users\\alex\\Documents', 38),
      node('Pictures', 'C:\\Users\\alex\\Pictures', 64),
      node('Videos', 'C:\\Users\\alex\\Videos', 58),
      node('OneDrive', 'C:\\Users\\alex\\OneDrive', 13.9),
    ]),
    node('Public', 'C:\\Users\\Public', 4),
  ]);
  const root = node('C:', 'C:\\', 734, [
    node('Windows', 'C:\\Windows', 38, leaves('C:\\Windows', ['WinSxS', 'System32', 'SoftwareDistribution', 'Installer', 'SysWOW64'], 38)),
    node('Program Files', 'C:\\Program Files', 46, leaves('C:\\Program Files', ['Docker', 'NVIDIA Corporation', 'Microsoft Office', 'PowerShell', 'Google'], 46)),
    node('Program Files (x86)', 'C:\\Program Files (x86)', 22, leaves('C:\\Program Files (x86)', ['Steam', 'Microsoft Visual Studio', 'Windows Kits'], 22)),
    node('ProgramData', 'C:\\ProgramData', 18),
    users,
    dev,
    node('pagefile.sys', 'C:\\pagefile.sys', 24),
    node('hiberfil.sys', 'C:\\hiberfil.sys', 12.8),
    node('$Recycle.Bin', 'C:\\$Recycle.Bin', 3.4),
  ]);
  if (!path) return root;
  const find = (n: SizeNode): SizeNode | null => (n.path === path ? n : n.children.map(find).find((x) => x) ?? null);
  return find(root) ?? root;
}
