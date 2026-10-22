import { defaultTrayItems, layoutFromPreset } from './widget-presets';
import type {
  ActivityEntry,
  Alert,
  Anomaly,
  CleanupCategory,
  CleanupItem,
  HealthReason,
  PluginInfo,
  Profile,
  Rule,
  Settings,
  SizeNode,
  StartupEntry,
  TuningSuggestion,
} from '../api.types';
import { prng } from './noise';

const GB = 1024 ** 3;
const MB = 1024 ** 2;
const MIN = 60_000;
const HOUR = 60 * MIN;
const DAY = 24 * HOUR;

// ------------------------------------------------------------------------------------------------------- rules

export const PROFILES: Profile[] = [
  { id: 'normal', name: 'Normal', description: 'Balanced: tame long CPU hogs, never touch what you are using.' },
  { id: 'gaming', name: 'Gaming', description: 'Keep the game smooth: background apps get lower priority and efficiency mode.' },
  { id: 'meeting', name: 'Meeting', description: 'Calls first: cap builds, sync and indexing so audio and video never stutter.' },
  { id: 'onBattery', name: 'On battery', description: 'Save power: efficiency mode for background apps, cap anything above 25%.' },
  { id: 'buildingCode', name: 'Building code', description: 'Compilers get the CPU: browsers, chat and indexing step aside.' },
];

export const BUILT_IN_PROTECTED = [
  'System',
  'Registry',
  'smss.exe',
  'csrss.exe',
  'wininit.exe',
  'services.exe',
  'lsass.exe',
  'winlogon.exe',
  'dwm.exe',
  'svchost.exe',
  'MsMpEng.exe',
  'MemCompression',
  'fontdrvhost.exe',
  'audiodg.exe',
  'dyno-agent.exe',
  'Dyno.exe',
];

export function demoRules(): Rule[] {
  return [
    {
      id: 'tame-long-hogs',
      name: 'Tame long CPU hogs',
      enabled: true,
      condition: { target: 'anyApp', cpuAbove: 40, forSecs: 300, when: ['userActive'] },
      ladder: [
        { action: 'lowerPriority', afterSecs: 0 },
        { action: 'efficiencyMode', afterSecs: 120 },
        { action: { cpuCap: 50 }, afterSecs: 300 },
        { action: 'notify', afterSecs: 0 },
      ],
      releaseAfterSecs: 60,
      profiles: ['normal', 'meeting'],
    },
    {
      id: 'browser-background',
      name: 'Browsers in the background',
      enabled: true,
      condition: { target: { apps: ['chrome.exe', 'msedge.exe', 'firefox.exe'] }, cpuAbove: 15, forSecs: 120, when: ['userActive'] },
      ladder: [
        { action: 'efficiencyMode', afterSecs: 0 },
        { action: { cpuCap: 20 }, afterSecs: 180 },
      ],
      releaseAfterSecs: 30,
      profiles: ['gaming', 'meeting', 'buildingCode'],
    },
    {
      id: 'meeting-builds',
      name: 'Builds step aside during calls',
      enabled: true,
      condition: { target: { apps: ['rustc.exe', 'cargo.exe', 'node.exe', 'MSBuild.exe'] }, cpuAbove: 30, forSecs: 20, when: ['userActive'] },
      ladder: [
        { action: 'lowerPriority', afterSecs: 0 },
        { action: { cpuCap: 40 }, afterSecs: 30 },
      ],
      releaseAfterSecs: 20,
      profiles: ['meeting'],
    },
    {
      id: 'battery-saver',
      name: 'Save battery',
      enabled: true,
      condition: { target: 'anyApp', cpuAbove: 25, forSecs: 60, when: ['onBattery'] },
      ladder: [
        { action: 'efficiencyMode', afterSecs: 0 },
        { action: { cpuCap: 25 }, afterSecs: 120 },
        { action: 'notify', afterSecs: 0 },
      ],
      releaseAfterSecs: 60,
      profiles: ['onBattery'],
    },
    {
      id: 'indexer-idle',
      name: 'Indexing only when idle',
      enabled: false,
      condition: { target: { apps: ['SearchIndexer.exe', 'SearchProtocolHost.exe'] }, cpuAbove: 5, forSecs: 30, when: ['userActive'] },
      ladder: [{ action: 'efficiencyMode', afterSecs: 0 }],
      releaseAfterSecs: 120,
      profiles: [],
    },
  ];
}

// ----------------------------------------------------------------------------------------------------- cleanup

export const CLEANUP_CATEGORIES: CleanupCategory[] = [
  { id: 'system-temp', name: 'Temporary files', description: 'Windows and app temp folders older than 2 days.', group: 'system', risk: 'safe', scheduled: true },
  { id: 'windows-update', name: 'Windows Update leftovers', description: 'Downloaded update packages that are already installed.', group: 'system', risk: 'safe', scheduled: true },
  { id: 'crash-dumps', name: 'Crash dumps', description: 'Memory dumps and error reports from crashed apps.', group: 'system', risk: 'safe', scheduled: true },
  { id: 'thumbnails', name: 'Thumbnail cache', description: 'Explorer rebuilds thumbnails when it needs them.', group: 'system', risk: 'safe', scheduled: false },
  { id: 'old-logs', name: 'Old logs', description: 'App and setup logs older than 30 days.', group: 'system', risk: 'safe', scheduled: false },
  { id: 'chrome-cache', name: 'Chrome cache', description: 'Cached pages and media. Sign-ins, history and passwords stay.', group: 'browser', risk: 'safe', scheduled: false },
  { id: 'edge-cache', name: 'Edge cache', description: 'Cached pages and media. Sign-ins, history and passwords stay.', group: 'browser', risk: 'safe', scheduled: false },
  { id: 'rust-target', name: 'Rust target/ folders', description: 'Build output of projects untouched for 30+ days. Rebuilt on the next cargo build.', group: 'developer', risk: 'review', scheduled: false },
  { id: 'node-modules', name: 'node_modules folders', description: 'Dependencies of projects untouched for 30+ days. npm install restores them.', group: 'developer', risk: 'review', scheduled: false },
  { id: 'cargo-cache', name: 'Cargo registry cache', description: 'Downloaded crate archives; cargo downloads them again when needed.', group: 'developer', risk: 'safe', scheduled: false },
  { id: 'npm-cache', name: 'npm cache', description: 'Cached package tarballs.', group: 'developer', risk: 'safe', scheduled: false },
  { id: 'nuget-cache', name: 'NuGet cache', description: 'Cached .NET packages.', group: 'developer', risk: 'safe', scheduled: false },
  { id: 'pip-cache', name: 'pip cache', description: 'Cached Python wheels.', group: 'developer', risk: 'safe', scheduled: false },
  { id: 'old-toolchains', name: 'Old Rust toolchains', description: 'Toolchains no project uses (rustup can install them again).', group: 'developer', risk: 'review', scheduled: false },
  { id: 'downloads-installers', name: 'Installers in Downloads', description: 'Setup files (.exe, .msi) in your Downloads folder. Personal files: never selected for you.', group: 'downloads', risk: 'personal', scheduled: false },
  { id: 'docker-images', name: 'Unused Docker images', description: 'Dangling images and build cache (docker system prune).', group: 'docker', risk: 'review', scheduled: false },
  { id: 'plugin-gradle', name: 'Gradle caches', description: 'From the Example plugin: Gradle build caches older than 30 days.', group: 'plugin', risk: 'safe', scheduled: false },
];

/** Items per category: [path, bytes, ageDays, folder, reason]. */
const ITEM_SPECS: Record<string, [string, number, number, boolean, string][]> = {
  'system-temp': [
    ['C:\\Users\\alex\\AppData\\Local\\Temp\\rustc-tmp-8f2c', 1.1 * GB, 9, true, 'Temp folder, 9 days old'],
    ['C:\\Users\\alex\\AppData\\Local\\Temp\\chrome_BITS_19640', 640 * MB, 14, true, 'Temp folder, 14 days old'],
    ['C:\\Windows\\Temp\\MpCmdRun.log.bak', 82 * MB, 21, false, 'Temp file, 21 days old'],
    ['C:\\Users\\alex\\AppData\\Local\\Temp\\vscode-update-x64', 410 * MB, 6, true, 'Leftover update download'],
    ['C:\\Users\\alex\\AppData\\Local\\Temp\\npm-41120-3c1', 960 * MB, 4, true, 'Temp folder, 4 days old'],
  ],
  'windows-update': [
    ['C:\\Windows\\SoftwareDistribution\\Download', 5.2 * GB, 12, true, 'Installed update packages'],
    ['C:\\Windows\\WinSxS\\Backup', 1.6 * GB, 40, true, 'Superseded component backups'],
  ],
  'crash-dumps': [
    ['C:\\Users\\alex\\AppData\\Local\\CrashDumps\\ms-teams.exe.21344.dmp', 1.2 * GB, 3, false, 'Crash dump, 3 days old'],
    ['C:\\Users\\alex\\AppData\\Local\\CrashDumps\\com.docker.backend.exe.9120.dmp', 640 * MB, 1, false, 'Crash dump, 1 day old'],
    ['C:\\ProgramData\\Microsoft\\Windows\\WER\\ReportArchive', 280 * MB, 18, true, 'Sent error reports'],
  ],
  thumbnails: [['C:\\Users\\alex\\AppData\\Local\\Microsoft\\Windows\\Explorer\\thumbcache_*.db', 380 * MB, 0, false, 'Rebuilt on demand']],
  'old-logs': [
    ['C:\\ProgramData\\NVIDIA Corporation\\NvBackend\\logs', 220 * MB, 45, true, 'Logs older than 30 days'],
    ['C:\\Windows\\Logs\\CBS\\CbsPersist_*.log', 160 * MB, 60, false, 'Logs older than 30 days'],
  ],
  'chrome-cache': [
    ['C:\\Users\\alex\\AppData\\Local\\Google\\Chrome\\User Data\\Default\\Cache', 1.4 * GB, 0, true, 'Browser cache'],
    ['C:\\Users\\alex\\AppData\\Local\\Google\\Chrome\\User Data\\Default\\Code Cache', 520 * MB, 0, true, 'Script cache'],
  ],
  'edge-cache': [['C:\\Users\\alex\\AppData\\Local\\Microsoft\\Edge\\User Data\\Default\\Cache', 640 * MB, 2, true, 'Browser cache']],
  'rust-target': [
    ['C:\\dev\\old-raytracer\\target', 7.8 * GB, 142, true, 'Untouched for 142 days'],
    ['C:\\dev\\aoc-2025\\target', 4.6 * GB, 290, true, 'Untouched for 290 days'],
    ['C:\\dev\\tauri-playground\\target', 3.9 * GB, 61, true, 'Untouched for 61 days'],
    ['C:\\dev\\wgpu-experiments\\target', 2.1 * GB, 38, true, 'Untouched for 38 days'],
  ],
  'node-modules': [
    ['C:\\dev\\portfolio-site\\node_modules', 1.9 * GB, 210, true, 'Untouched for 210 days'],
    ['C:\\dev\\angular-sandbox\\node_modules', 1.6 * GB, 88, true, 'Untouched for 88 days'],
    ['C:\\dev\\electron-test\\node_modules', 2.7 * GB, 330, true, 'Untouched for 330 days'],
  ],
  'cargo-cache': [['C:\\Users\\alex\\.cargo\\registry\\cache', 2.3 * GB, 0, true, 'Downloaded crate archives']],
  'npm-cache': [['C:\\Users\\alex\\AppData\\Local\\npm-cache\\_cacache', 1.8 * GB, 0, true, 'Cached tarballs']],
  'nuget-cache': [['C:\\Users\\alex\\.nuget\\packages', 3.1 * GB, 0, true, 'Cached packages']],
  'pip-cache': [['C:\\Users\\alex\\AppData\\Local\\pip\\Cache', 420 * MB, 0, true, 'Cached wheels']],
  'old-toolchains': [
    ['C:\\Users\\alex\\.rustup\\toolchains\\1.79.0-x86_64-pc-windows-msvc', 1.3 * GB, 120, true, 'No project uses 1.79.0'],
    ['C:\\Users\\alex\\.rustup\\toolchains\\nightly-2025-11-02-x86_64-pc-windows-msvc', 1.3 * GB, 90, true, 'Old nightly'],
  ],
  'downloads-installers': [
    ['C:\\Users\\alex\\Downloads\\DockerDesktopInstaller.exe', 610 * MB, 75, false, 'Installer, 75 days old'],
    ['C:\\Users\\alex\\Downloads\\VSCodeUserSetup-x64-1.94.2.exe', 98 * MB, 40, false, 'Installer, 40 days old'],
    ['C:\\Users\\alex\\Downloads\\rustup-init.exe', 9 * MB, 300, false, 'Installer, 300 days old'],
    ['C:\\Users\\alex\\Downloads\\Win11_24H2_English_x64.iso', 5.4 * GB, 120, false, 'Disk image, 120 days old'],
    ['C:\\Users\\alex\\Downloads\\node-v22.9.0-x64.msi', 31 * MB, 33, false, 'Installer, 33 days old'],
  ],
  'docker-images': [
    ['postgres:15 (dangling layers)', 1.2 * GB, 60, false, 'Not used by any container'],
    ['Build cache', 6.4 * GB, 20, false, 'Build cache older than 7 days'],
    ['node:18-alpine', 2.2 * GB, 140, false, 'Not used by any container'],
  ],
  'plugin-gradle': [['C:\\Users\\alex\\.gradle\\caches\\build-cache-1', 940 * MB, 50, true, 'Older than 30 days']],
};

/** The items a scan finds in a category (relative to `now`); `personal` categories are never pre-selected. */
export function cleanupItems(categoryId: string, now: number): CleanupItem[] {
  const personal = CLEANUP_CATEGORIES.find((c) => c.id === categoryId)?.risk === 'personal';
  return (ITEM_SPECS[categoryId] ?? []).map(([path, bytes, ageDays, folder, reason], i) => ({
    id: `${categoryId}:${i}`,
    path,
    bytes: Math.round(bytes),
    modified: now - ageDays * DAY - i * 3_600_000,
    folder,
    reason,
    selected: !personal,
  }));
}

// -------------------------------------------------------------------------------------------- health and tuning

export function healthReasons(): HealthReason[] {
  return [
    {
      id: 'low-disk-c',
      title: 'Drive C: is 78% full',
      detail: '210 GB free of 953 GB, and build output fills about 5 GB a day: full in about 41 days. 63 GB can be freed safely.',
      impact: 8,
      severity: 'warning',
      fix: {
        label: 'Review cleanup',
        explanation: 'Scan the cleanup categories you trust and review what would be removed before anything is deleted.',
        preview: ['Opens Cleanup and scans the enabled categories', 'Nothing is deleted until you confirm the preview'],
        needsAdmin: false,
        undoable: true,
        kind: 'openCleanup',
      },
    },
    {
      id: 'uptime',
      title: 'Not restarted for 12 days',
      detail: 'Windows installed an update that finishes on restart, and long uptimes let memory fragment.',
      impact: 6,
      severity: 'warning',
      fix: {
        label: 'Restart when convenient',
        explanation: 'Restarting finishes the pending Windows update and frees memory held by long-running services.',
        preview: ['Save your work first', 'Restart from the Start menu when it suits you'],
        needsAdmin: false,
        undoable: false,
        kind: 'restart',
      },
    },
    {
      id: 'defender-dev',
      title: 'Defender scans your build folders',
      detail: 'MsMpEng.exe spends about 4% CPU scanning files cargo writes to target/ folders during builds.',
      impact: 5,
      severity: 'info',
      fix: {
        label: 'Exclude dev build folders',
        explanation:
          'Adds Microsoft Defender exclusions for build output only (target\\ and node_modules\\ under C:\\dev). Source code stays scanned. Builds get about 20% faster.',
        preview: ['Add exclusion: C:\\dev\\**\\target', 'Add exclusion: C:\\dev\\**\\node_modules', 'Defender keeps scanning everything else'],
        needsAdmin: true,
        undoable: true,
        kind: { tuning: 'defender-dev-exclusions' },
      },
    },
    {
      id: 'startup-apps',
      title: '9 apps start at sign-in',
      detail: 'Teams, Docker Desktop and Spotify have high impact: the desktop takes 48 s to settle after sign-in.',
      impact: 4,
      severity: 'info',
      fix: {
        label: 'Review startup apps',
        explanation: 'Disable or delay apps you do not need right after signing in. You can turn them back on any time.',
        preview: ['Opens the startup manager', 'Nothing changes until you choose'],
        needsAdmin: false,
        undoable: true,
        kind: 'openStartup',
      },
    },
    {
      id: 'battery-wear',
      title: 'Battery health 87%',
      detail: 'Full charge holds 87% of the design capacity after 412 cycles. Normal for a two-year-old laptop.',
      impact: 2,
      severity: 'info',
      fix: null,
    },
    {
      id: 'search-index',
      title: 'Search indexes build folders',
      detail: 'Windows Search re-indexes thousands of files in target/ and node_modules/ after every build.',
      impact: 1,
      severity: 'info',
      fix: {
        label: 'Stop indexing build folders',
        explanation: 'Removes build output folders from the Windows Search index. Your code and documents stay searchable.',
        preview: ['Exclude C:\\dev\\**\\target from indexing', 'Exclude C:\\dev\\**\\node_modules from indexing'],
        needsAdmin: false,
        undoable: true,
        kind: { tuning: 'search-index-dev' },
      },
    },
  ];
}

export function demoTuning(): TuningSuggestion[] {
  return [
    {
      id: 'power-plan',
      title: 'Best performance while plugged in',
      explanation: 'Your laptop uses the Balanced power mode on AC. Best performance lets the CPU boost sooner; Dyno switches back on battery.',
      current: 'Balanced',
      proposed: 'Best performance (on AC)',
      needsAdmin: false,
      applied: false,
    },
    {
      id: 'defender-dev-exclusions',
      title: 'Defender exclusions for build output',
      explanation: 'Microsoft Defender scans every file a compiler writes. Excluding build output (not source) makes builds about 20% faster.',
      current: 'No exclusions',
      proposed: 'Exclude C:\\dev\\**\\target and node_modules',
      needsAdmin: true,
      applied: false,
    },
    {
      id: 'search-index-dev',
      title: 'Stop indexing build folders',
      explanation: 'Windows Search re-indexes build output after every build. Code and documents stay searchable.',
      current: 'Indexed',
      proposed: 'Excluded',
      needsAdmin: false,
      applied: false,
    },
    {
      id: 'background-apps',
      title: 'Fewer background apps',
      explanation: '12 Store apps may run in the background. Most only need to while open.',
      current: '12 allowed',
      proposed: '4 allowed (Teams, Outlook, Phone Link, Clock)',
      needsAdmin: false,
      applied: false,
    },
    {
      id: 'visual-effects',
      title: 'Lighter visual effects',
      explanation: 'Keeps smooth fonts and thumbnails but turns off animations and shadows under windows.',
      current: 'Let Windows choose',
      proposed: 'Custom (animations off)',
      needsAdmin: false,
      applied: true,
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
    needsAdmin: location === 'machineRunKey' || location === 'commonStartupFolder' || location === 'scheduledTask',
  });
  return [
    e('teams', 'Microsoft Teams', '"C:\\Program Files\\WindowsApps\\MSTeams\\ms-teams.exe" --process-start-reason=StartupTask', 'userRunKey', 'Microsoft Corporation', 9.8, 310, 'high'),
    e('docker', 'Docker Desktop', '"C:\\Program Files\\Docker\\Docker\\Docker Desktop.exe" -Autostart', 'userRunKey', 'Docker Inc.', 14.2, 520, 'high'),
    e('spotify', 'Spotify', '"C:\\Users\\alex\\AppData\\Roaming\\Spotify\\Spotify.exe" --autostart --minimized', 'userRunKey', 'Spotify AB', 4.1, 180, 'medium'),
    e('onedrive', 'Microsoft OneDrive', '"C:\\Program Files\\Microsoft OneDrive\\OneDrive.exe" /background', 'userRunKey', 'Microsoft Corporation', 3.2, 140, 'medium'),
    e('slack', 'Slack', '"C:\\Users\\alex\\AppData\\Local\\slack\\slack.exe" --startup', 'userRunKey', 'Slack Technologies, LLC', 2.6, 96, 'low', { delayed: 60 }),
    e('nvidia', 'NVIDIA Broadcast', '"C:\\Program Files\\NVIDIA Corporation\\NVIDIA Broadcast\\NVIDIA Broadcast UI.exe"', 'machineRunKey', 'NVIDIA Corporation', 2.1, 88, 'low'),
    e('security', 'Windows Security notification icon', '"C:\\Windows\\System32\\SecurityHealthSystray.exe"', 'machineRunKey', 'Microsoft Corporation', 0.3, 4, 'low'),
    e('logi', 'Logi Options+', '"C:\\Program Files\\LogiOptionsPlus\\logioptionsplus_agent.exe"', 'commonStartupFolder', 'Logitech Inc', 1.9, 60, 'low', { delayed: 30 }),
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
    { id: 'al-5', at: now - 3 * HOUR, kind: 'longHighCpu', severity: 'info', title: 'rustc.exe used over 50% CPU for 12 minutes', detail: 'The "Tame long CPU hogs" rule lowered its priority.' },
    { id: 'al-4', at: now - 27 * HOUR, kind: 'memoryPressure', severity: 'warning', title: 'Memory pressure high', detail: 'Commit reached 92% of the limit while Docker and two browsers were open.' },
    { id: 'al-3', at: now - 2 * DAY - 4 * HOUR, kind: 'lowDisk', severity: 'warning', title: 'Drive C: below 25% free', detail: '210 GB free of 953 GB.' },
    { id: 'al-2', at: now - 4 * DAY, kind: 'overheating', severity: 'critical', title: 'CPU reached 98 °C', detail: 'Thermal throttling for 3 minutes during a build with the lid closed.' },
    { id: 'al-1', at: now - 6 * DAY, kind: 'longHighCpu', severity: 'info', title: 'MsMpEng.exe used 25% CPU for 9 minutes', detail: 'A scheduled Defender scan.' },
  ];
}

export function demoActivity(now: number): ActivityEntry[] {
  const a = (id: string, ago: number, kind: ActivityEntry['kind'], title: string, detail: string, undo: ActivityEntry['undo'], bytes: number | null = null): ActivityEntry => ({
    id,
    at: now - ago,
    kind,
    title,
    detail,
    undo,
    bytes,
  });
  return [
    a('act-12', 4 * MIN, 'rule', 'Lowered priority of rustc.exe', 'Tame long CPU hogs: above 40% for 5 min', 'available'),
    a('act-11', 2 * MIN, 'manual', 'Efficiency mode on for Docker Desktop.exe (6)', 'From the task manager', 'available'),
    a('act-10', 38 * MIN, 'release', 'Released node.exe', 'Below the threshold for 60 s', 'notUndoable'),
    a('act-9', 1 * HOUR + 12 * MIN, 'cleanup', 'Cleaned 3 categories', 'Temporary files, crash dumps, npm cache to the 7-day quarantine', 'available', 4.6 * GB),
    a('act-8', 3 * HOUR, 'rule', 'Capped rustc.exe at 50% CPU', 'Tame long CPU hogs, step 3', 'expired'),
    a('act-7', 5 * HOUR, 'tuning', 'Lighter visual effects', 'Animations and window shadows off', 'available'),
    a('act-6', 9 * HOUR, 'startup', 'Delayed Slack by 60 s at sign-in', 'Startup manager', 'available'),
    a('act-5', 22 * HOUR, 'manual', 'Ended SearchProtocolHost.exe', 'From the task manager', 'notUndoable'),
    a('act-4', 26 * HOUR, 'alert', 'Memory pressure high', 'Commit reached 92% of the limit', 'notUndoable'),
    a('act-3', 2 * DAY, 'cleanup', 'Scheduled cleanup', 'Temporary files, Windows Update leftovers to the Recycle Bin', 'expired', 6.1 * GB),
    a('act-2', 3 * DAY, 'rule', 'Efficiency mode for Teams', 'Save battery', 'expired'),
    a('act-1', 12 * DAY, 'system', 'Dyno started', 'Sampling every 1 s, processes every 2 s', 'notUndoable'),
  ].map((e) => ({ ...e, bytes: e.bytes === null ? null : Math.round(e.bytes) }));
}

export function demoPlugins(): PluginInfo[] {
  return [
    { id: 'dyno-plugin-example', name: 'Example plugin', version: '0.1.0', state: 'running', error: null, contributes: ['Sensor: uptime', 'Cleanup: Gradle caches'] },
    { id: 'gpu-fans', name: 'GPU fan sensor', version: '0.3.2', state: 'quarantined', error: 'Crashed 3 times in 10 minutes (access violation reading the vendor API).', contributes: ['Sensor: GPU fans'] },
  ];
}

export function demoSettings(): Settings {
  return {
    version: 1,
    theme: 'system',
    launchAtLogin: true,
    sampling: { intervalMs: 1000, processIntervalMs: 2000 },
    retention: { rawHours: 24, minuteDays: 14, quarterHourDays: 90, hourDays: 730 },
    tray: { metrics: ['cpu', 'memory', 'disk'], style: 'graph' },
    alerts: { enabled: true, lowDiskPercent: 10, overheatCelsius: 95, longHighCpuMinutes: 10 },
    cleanup: { schedule: 'weekly', categories: CLEANUP_CATEGORIES.filter((c) => c.scheduled).map((c) => c.id), mode: 'quarantine', devProjectAgeDays: 30 },
    webApi: { enabled: false, port: 7878, lan: false },
    dashboard: layoutFromPreset('overview'),
    ipLookup: { ipTemplate: 'https://ipinfo.io/{ip}', asnTemplate: '' },
    trayItems: defaultTrayItems(),
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
