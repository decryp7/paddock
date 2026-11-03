import type {
  ActivityEntry,
  Alert,
  Anomaly,
  HealthReason,
  PluginInfo,
  Settings,
  StartupEntry,
} from '../api.types';

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
      detail: 'macOS installed an update that finishes on restart, and long uptimes let memory fragment.',
      impact: 6,
      severity: 'warning',
    },
    {
      id: 'startup-apps',
      title: '9 apps start at sign-in',
      detail: 'Teams, VirtualBox and Spotify have high impact: the desktop takes 48 s to settle after sign-in.',
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

/** The startup items of the pretend machine: Windows ones (registry Run keys, startup folders, tasks) only on Windows. */
export function demoStartup(os: 'windows' | 'macos' | 'linux' = 'windows'): StartupEntry[] {
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
  if (os !== 'windows') {
    const mac = os === 'macos';
    const agent = mac ? 'userLaunchAgent' : 'userStartupFolder';
    const home = mac ? '/Users/alex' : '/home/alex';
    return [
      e('teams', 'Microsoft Teams', mac ? '/Applications/Microsoft Teams.app/Contents/MacOS/MSTeams --background' : '/opt/teams/teams --background', mac ? 'loginItem' : agent, 'Microsoft Corporation', 9.8, 310, 'high'),
      e('docker', 'VirtualBox', mac ? '/Applications/VirtualBox.app/Contents/MacOS/VirtualBox --autostart' : '/opt/VirtualBox/VirtualBox --autostart', mac ? 'loginItem' : agent, 'Oracle Corporation', 14.2, 520, 'high'),
      e('spotify', 'Spotify', mac ? '/Applications/Spotify.app/Contents/MacOS/Spotify --minimized' : '/usr/bin/spotify --minimized', mac ? 'loginItem' : agent, 'Spotify AB', 4.1, 180, 'medium'),
      e('onedrive', 'Microsoft OneDrive', mac ? '/Applications/OneDrive.app/Contents/MacOS/OneDrive --background' : '/usr/bin/onedrive --monitor', mac ? 'userLaunchAgent' : agent, 'Microsoft Corporation', 3.2, 140, 'medium'),
      e('slack', 'Slack', mac ? '/Applications/Slack.app/Contents/MacOS/Slack --startup' : '/usr/bin/slack --startup', mac ? 'loginItem' : agent, 'Slack Technologies, LLC', 2.6, 96, 'low'),
      e('logi', 'Logi Options+', mac ? '/Library/Application Support/Logitech.localized/LogiOptionsPlus/logioptionsplus_agent' : `${home}/.local/bin/logiops`, mac ? 'systemLaunchAgent' : agent, 'Logitech Inc', 1.9, 60, 'low'),
      e('steam', 'Steam', mac ? '/Applications/Steam.app/Contents/MacOS/steam_osx -silent' : '/usr/bin/steam -silent', mac ? 'loginItem' : agent, 'Valve Corp.', 0, 0, 'notMeasured', 'disabled'),
      e('system', mac ? 'Background Task Management' : 'Desktop services', mac ? '/System/Library/CoreServices/backgroundtaskmanagementagent' : '/usr/libexec/gvfsd', mac ? 'systemLaunchAgent' : agent, mac ? 'Apple Inc.' : null, 0.3, 4, 'low'),
      e('dyno', 'Dyno', mac ? '/Applications/Dyno.app/Contents/MacOS/dyno-app --hidden' : '/usr/bin/dyno-app --hidden', mac ? 'loginItem' : agent, 'Dyno contributors', 0.4, 12, 'low'),
    ];
  }
  return [
    e('teams', 'Microsoft Teams', '"C:\\Program Files\\WindowsApps\\MSTeams\\ms-teams.exe" --process-start-reason=StartupTask', 'userRunKey', 'Microsoft Corporation', 9.8, 310, 'high'),
    e('docker', 'VirtualBox', '"C:\\Program Files\\Oracle\\VirtualBox\\VirtualBox.exe" -Autostart', 'userRunKey', 'Oracle Corporation', 14.2, 520, 'high'),
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
      app: 'VirtualBox',
      since: now - 2 * HOUR,
      title: 'VirtualBox service restarted 6 times in 2 hours',
      explanation: 'VBoxSVC.exe crashes and restarts. Its last crash dump points at the virtual machine it manages.',
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
    { id: 'al-6', at: now - 52 * MIN, kind: 'crashLoop', severity: 'warning', title: 'VirtualBox service keeps restarting', detail: '6 restarts in 2 hours.' },
    { id: 'al-5', at: now - 3 * HOUR, kind: 'longHighCpu', severity: 'info', title: 'rustc.exe used over 50% CPU for 12 minutes', detail: 'It stayed busy for a long time.' },
    { id: 'al-4', at: now - 27 * HOUR, kind: 'memoryPressure', severity: 'warning', title: 'Memory pressure high', detail: 'Commit reached 92% of the limit while VirtualBox and two browsers were open.' },
    { id: 'al-3', at: now - 2 * DAY - 4 * HOUR, kind: 'lowDisk', severity: 'warning', title: 'Drive C: below 25% free', detail: '210 GB free of 953 GB.' },
    { id: 'al-2', at: now - 4 * DAY, kind: 'overheating', severity: 'critical', title: 'CPU reached 98 °C', detail: 'Thermal throttling for 3 minutes during a build with the lid closed.' },
    { id: 'al-1', at: now - 6 * DAY, kind: 'longHighCpu', severity: 'info', title: 'MsMpEng.exe used 25% CPU for 9 minutes', detail: 'A scheduled Defender scan.' },
  ];
}

export function demoActivity(now: number): ActivityEntry[] {
  const a = (id: string, ago: number, kind: ActivityEntry['kind'], title: string, detail: string): ActivityEntry => ({ id, at: now - ago, kind, title, detail });
  return [
    a('act-4', 26 * HOUR, 'alert', 'Memory pressure high', 'Commit reached 92% of the limit'),
    a('act-2', 3 * DAY, 'alert', 'VirtualBox service keeps restarting', '6 restarts in 2 hours.'),
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
    alerts: { enabled: true, lowDiskPercent: 10, overheatCelsius: 95, longHighCpuMinutes: 10 },
    // What the engine's DiskSettings::default() gives (crates/dyno-core/src/disk/settings.rs).
    disk: { scanFolders: 2_000_000, notableFileMib: 10, readThreads: 'auto', finderFiles: 500_000, finderDepth: 48, finderHashGib: 64, savedMaps: { enabled: true, maxMb: 1024, refreshOnOpen: 'off' } },
    jobs: { workers: 'auto' },
    webApi: { enabled: false, port: 7878, lan: false },
    ipLookup: { ipTemplate: 'https://ipinfo.io/{ip}', asnTemplate: '' },
    alarms: { quiet: { enabled: false, startMinute: 22 * 60, endMinute: 7 * 60, criticalBypass: true }, chatterTransitions: 6, chatterWindowMinutes: 30, maxAlarms: 500, keepClearedDays: 14 },
    flightRecorder: true,
    customAlerts: [{ id: 'gpu-load', name: 'GPU load', metric: 'gpuLoad', op: 'above', limit: 90, forSecs: 120, severity: 'warning', enabled: true }],
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

