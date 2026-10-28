import type { AppGroup, ProcessSample, RunState } from '../api.types';
import { clamp, hash01, seedOf, wobble } from './noise';

const GB = 1000 ** 3;
const MB = 1000 ** 2;

/** One application in the mock catalogue: how many processes, and the totals they share. */
export interface AppSpec {
  group: string;
  groupName: string;
  name: string;
  exe: string | null;
  user: string;
  count: number;
  /** Average total CPU % of the machine. */
  cpu: number;
  /** Total working set. */
  memory: number;
  /** Total disk throughput (read + write). */
  disk: number;
  /** How much the CPU swings around its average (0–1). */
  swing?: number;
  /** The group whose first process is this app's parent. */
  parent?: string;
  protectedReason?: string;
  /** Started this many seconds before the mock's start. */
  ageSecs: number;
}

const SYS = 'Protected: a Windows system process';
const SEC = 'Protected: Microsoft Defender (tamper protection)';
const ME = 'Protected: Dyno itself';
const USER = 'DEV-LAPTOP\\alex';

/** A developer's Windows 11 laptop during a `cargo build`, ~200 processes. Order matters: parents come first. */
export const APP_SPECS: AppSpec[] = [
  { group: 'system', groupName: 'System', name: 'System', exe: null, user: 'SYSTEM', count: 1, cpu: 0.6, memory: 6 * MB, disk: 1.2 * MB, protectedReason: SYS, ageSecs: 1_040_000 },
  { group: 'registry', groupName: 'Registry', name: 'Registry', exe: null, user: 'SYSTEM', count: 1, cpu: 0, memory: 74 * MB, disk: 0, parent: 'system', protectedReason: SYS, ageSecs: 1_040_000 },
  { group: 'memcompression', groupName: 'Memory Compression', name: 'MemCompression', exe: null, user: 'SYSTEM', count: 1, cpu: 0.2, memory: 820 * MB, disk: 0, parent: 'system', protectedReason: SYS, ageSecs: 1_040_000 },
  { group: 'smss', groupName: 'Session Manager', name: 'smss.exe', exe: 'C:\\Windows\\System32\\smss.exe', user: 'SYSTEM', count: 1, cpu: 0, memory: 1 * MB, disk: 0, parent: 'system', protectedReason: SYS, ageSecs: 1_040_000 },
  { group: 'csrss', groupName: 'Client Server Runtime', name: 'csrss.exe', exe: 'C:\\Windows\\System32\\csrss.exe', user: 'SYSTEM', count: 2, cpu: 0.1, memory: 12 * MB, disk: 0, parent: 'smss', protectedReason: SYS, ageSecs: 1_039_990 },
  { group: 'wininit', groupName: 'Windows Start-Up', name: 'wininit.exe', exe: 'C:\\Windows\\System32\\wininit.exe', user: 'SYSTEM', count: 1, cpu: 0, memory: 7 * MB, disk: 0, parent: 'smss', protectedReason: SYS, ageSecs: 1_039_990 },
  { group: 'services', groupName: 'Services and Controller', name: 'services.exe', exe: 'C:\\Windows\\System32\\services.exe', user: 'SYSTEM', count: 1, cpu: 0.1, memory: 11 * MB, disk: 0, parent: 'wininit', protectedReason: SYS, ageSecs: 1_039_985 },
  { group: 'lsass', groupName: 'Local Security Authority', name: 'lsass.exe', exe: 'C:\\Windows\\System32\\lsass.exe', user: 'SYSTEM', count: 1, cpu: 0.1, memory: 28 * MB, disk: 0, parent: 'wininit', protectedReason: SYS, ageSecs: 1_039_985 },
  { group: 'winlogon', groupName: 'Windows Logon', name: 'winlogon.exe', exe: 'C:\\Windows\\System32\\winlogon.exe', user: 'SYSTEM', count: 1, cpu: 0, memory: 9 * MB, disk: 0, parent: 'smss', protectedReason: SYS, ageSecs: 1_039_980 },
  { group: 'dwm', groupName: 'Desktop Window Manager', name: 'dwm.exe', exe: 'C:\\Windows\\System32\\dwm.exe', user: 'DWM-1', count: 1, cpu: 1.4, memory: 142 * MB, disk: 0, parent: 'winlogon', protectedReason: SYS, swing: 0.6, ageSecs: 1_039_980 },
  { group: 'fontdrvhost', groupName: 'Font Driver Host', name: 'fontdrvhost.exe', exe: 'C:\\Windows\\System32\\fontdrvhost.exe', user: 'UMFD-0', count: 2, cpu: 0, memory: 8 * MB, disk: 0, parent: 'wininit', protectedReason: SYS, ageSecs: 1_039_980 },
  { group: 'svchost', groupName: 'Windows services', name: 'svchost.exe', exe: 'C:\\Windows\\System32\\svchost.exe', user: 'SYSTEM', count: 70, cpu: 2.1, memory: 1.35 * GB, disk: 2.4 * MB, parent: 'services', protectedReason: SYS, swing: 0.5, ageSecs: 1_039_970 },
  { group: 'spoolsv', groupName: 'Print Spooler', name: 'spoolsv.exe', exe: 'C:\\Windows\\System32\\spoolsv.exe', user: 'SYSTEM', count: 1, cpu: 0, memory: 14 * MB, disk: 0, parent: 'services', ageSecs: 1_039_960 },
  { group: 'defender', groupName: 'Microsoft Defender', name: 'MsMpEng.exe', exe: 'C:\\ProgramData\\Microsoft\\Windows Defender\\Platform\\4.18.24090.11-0\\MsMpEng.exe', user: 'SYSTEM', count: 1, cpu: 4.1, memory: 386 * MB, disk: 9.5 * MB, parent: 'services', protectedReason: SEC, swing: 0.5, ageSecs: 1_039_950 },
  { group: 'search', groupName: 'Windows Search', name: 'SearchIndexer.exe', exe: 'C:\\Windows\\System32\\SearchIndexer.exe', user: 'SYSTEM', count: 3, cpu: 1.6, memory: 96 * MB, disk: 3.1 * MB, parent: 'services', swing: 0.9, ageSecs: 1_039_950 },
  { group: 'nvidia', groupName: 'NVIDIA Container', name: 'NVDisplay.Container.exe', exe: 'C:\\Windows\\System32\\DriverStore\\FileRepository\\nv_dispi.inf_amd64\\Display.NvContainer\\NVDisplay.Container.exe', user: 'SYSTEM', count: 3, cpu: 0.2, memory: 64 * MB, disk: 0, parent: 'services', ageSecs: 1_039_940 },
  { group: 'wmi', groupName: 'WMI Provider Host', name: 'WmiPrvSE.exe', exe: 'C:\\Windows\\System32\\wbem\\WmiPrvSE.exe', user: 'NETWORK SERVICE', count: 2, cpu: 0.3, memory: 22 * MB, disk: 0, parent: 'services', ageSecs: 640_000 },
  { group: 'audiodg', groupName: 'Windows Audio', name: 'audiodg.exe', exe: 'C:\\Windows\\System32\\audiodg.exe', user: 'LOCAL SERVICE', count: 1, cpu: 0.3, memory: 18 * MB, disk: 0, parent: 'services', protectedReason: SYS, ageSecs: 9_000 },
  { group: 'dyno-agent', groupName: 'Dyno agent', name: 'dyno-agent.exe', exe: 'C:\\Program Files\\Dyno\\dyno-agent.exe', user: 'SYSTEM', count: 1, cpu: 0.4, memory: 38 * MB, disk: 0.05 * MB, parent: 'services', protectedReason: ME, swing: 0.2, ageSecs: 1_039_900 },
  { group: 'explorer', groupName: 'Windows Explorer', name: 'explorer.exe', exe: 'C:\\Windows\\explorer.exe', user: USER, count: 1, cpu: 0.6, memory: 212 * MB, disk: 0.2 * MB, parent: 'winlogon', ageSecs: 1_039_800 },
  { group: 'shell', groupName: 'Windows shell', name: 'StartMenuExperienceHost.exe', exe: 'C:\\Windows\\SystemApps\\Microsoft.Windows.StartMenuExperienceHost_cw5n1h2txyewy\\StartMenuExperienceHost.exe', user: USER, count: 5, cpu: 0.2, memory: 310 * MB, disk: 0, parent: 'svchost', ageSecs: 1_039_790 },
  { group: 'runtimebroker', groupName: 'Runtime Broker', name: 'RuntimeBroker.exe', exe: 'C:\\Windows\\System32\\RuntimeBroker.exe', user: USER, count: 6, cpu: 0.1, memory: 96 * MB, disk: 0, parent: 'svchost', ageSecs: 1_039_700 },
  { group: 'dllhost', groupName: 'COM Surrogate', name: 'dllhost.exe', exe: 'C:\\Windows\\System32\\dllhost.exe', user: USER, count: 3, cpu: 0, memory: 26 * MB, disk: 0, parent: 'svchost', ageSecs: 400_000 },
  { group: 'chrome', groupName: 'Google Chrome', name: 'chrome.exe', exe: 'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe', user: USER, count: 38, cpu: 6.4, memory: 4.3 * GB, disk: 1.8 * MB, parent: 'explorer', swing: 0.6, ageSecs: 302_000 },
  { group: 'vscode', groupName: 'Visual Studio Code', name: 'Code.exe', exe: 'C:\\Users\\alex\\AppData\\Local\\Programs\\Microsoft VS Code\\Code.exe', user: USER, count: 14, cpu: 3.2, memory: 2.2 * GB, disk: 0.9 * MB, parent: 'explorer', swing: 0.5, ageSecs: 210_000 },
  { group: 'cargo', groupName: 'Rust build (cargo)', name: 'cargo.exe', exe: 'C:\\Users\\alex\\.cargo\\bin\\cargo.exe', user: USER, count: 1, cpu: 0.4, memory: 48 * MB, disk: 0.4 * MB, parent: 'terminal', ageSecs: 410 },
  { group: 'cargo', groupName: 'Rust build (cargo)', name: 'rustc.exe', exe: 'C:\\Users\\alex\\.rustup\\toolchains\\stable-x86_64-pc-windows-msvc\\bin\\rustc.exe', user: USER, count: 6, cpu: 62.8, memory: 3.1 * GB, disk: 186 * MB, parent: 'cargo-first', swing: 0.15, ageSecs: 380 },
  { group: 'terminal', groupName: 'Windows Terminal', name: 'WindowsTerminal.exe', exe: 'C:\\Program Files\\WindowsApps\\Microsoft.WindowsTerminal_1.21.2911.0_x64__8wekyb3d8bbwe\\WindowsTerminal.exe', user: USER, count: 1, cpu: 0.3, memory: 118 * MB, disk: 0, parent: 'explorer', ageSecs: 190_000 },
  { group: 'pwsh', groupName: 'PowerShell', name: 'pwsh.exe', exe: 'C:\\Program Files\\PowerShell\\7\\pwsh.exe', user: USER, count: 2, cpu: 0.1, memory: 142 * MB, disk: 0, parent: 'terminal', ageSecs: 189_000 },
  { group: 'conhost', groupName: 'Console Window Host', name: 'conhost.exe', exe: 'C:\\Windows\\System32\\conhost.exe', user: USER, count: 6, cpu: 0, memory: 42 * MB, disk: 0, parent: 'terminal', ageSecs: 189_000 },
  { group: 'node', groupName: 'Node.js (ng serve)', name: 'node.exe', exe: 'C:\\Program Files\\nodejs\\node.exe', user: USER, count: 3, cpu: 1.8, memory: 920 * MB, disk: 0.6 * MB, parent: 'pwsh', swing: 0.7, ageSecs: 7_200 },
  { group: 'docker', groupName: 'Docker Desktop', name: 'Docker Desktop.exe', exe: 'C:\\Program Files\\Docker\\Docker\\Docker Desktop.exe', user: USER, count: 4, cpu: 0.6, memory: 430 * MB, disk: 0.1 * MB, parent: 'explorer', ageSecs: 302_400 },
  { group: 'docker', groupName: 'Docker Desktop', name: 'com.docker.backend.exe', exe: 'C:\\Program Files\\Docker\\Docker\\resources\\com.docker.backend.exe', user: USER, count: 2, cpu: 0.4, memory: 160 * MB, disk: 0.3 * MB, parent: 'docker-first', ageSecs: 3_100 },
  { group: 'vmmem', groupName: 'WSL / Docker VM', name: 'vmmem', exe: null, user: USER, count: 1, cpu: 3.4, memory: 4.1 * GB, disk: 6.2 * MB, parent: 'svchost', swing: 0.5, ageSecs: 302_300 },
  { group: 'teams', groupName: 'Microsoft Teams', name: 'ms-teams.exe', exe: 'C:\\Program Files\\WindowsApps\\MSTeams_24277.3507.3205.5228_x64__8wekyb3d8bbwe\\ms-teams.exe', user: USER, count: 6, cpu: 1.9, memory: 1.25 * GB, disk: 0.3 * MB, parent: 'explorer', swing: 0.6, ageSecs: 1_039_700 },
  { group: 'slack', groupName: 'Slack', name: 'slack.exe', exe: 'C:\\Users\\alex\\AppData\\Local\\slack\\app-4.41.97\\slack.exe', user: USER, count: 5, cpu: 0.8, memory: 640 * MB, disk: 0.1 * MB, parent: 'explorer', swing: 0.7, ageSecs: 1_039_700 },
  { group: 'outlook', groupName: 'Outlook', name: 'OUTLOOK.EXE', exe: 'C:\\Program Files\\Microsoft Office\\root\\Office16\\OUTLOOK.EXE', user: USER, count: 1, cpu: 0.7, memory: 390 * MB, disk: 0.4 * MB, parent: 'explorer', ageSecs: 302_000 },
  { group: 'spotify', groupName: 'Spotify', name: 'Spotify.exe', exe: 'C:\\Users\\alex\\AppData\\Roaming\\Spotify\\Spotify.exe', user: USER, count: 5, cpu: 1.1, memory: 470 * MB, disk: 0.2 * MB, parent: 'explorer', ageSecs: 26_000 },
  { group: 'onedrive', groupName: 'OneDrive', name: 'OneDrive.exe', exe: 'C:\\Program Files\\Microsoft OneDrive\\OneDrive.exe', user: USER, count: 1, cpu: 0.2, memory: 120 * MB, disk: 0.5 * MB, parent: 'explorer', ageSecs: 1_039_700 },
  { group: 'dyno', groupName: 'Dyno', name: 'Dyno.exe', exe: 'C:\\Program Files\\Dyno\\Dyno.exe', user: USER, count: 2, cpu: 0.3, memory: 96 * MB, disk: 0, parent: 'explorer', protectedReason: ME, ageSecs: 3_000 },
];

/** A process in the mock with the state Dyno's actions change. */
export interface MockProcess {
  pid: number;
  parent: number | null;
  name: string;
  exe: string | null;
  user: string;
  group: string;
  groupName: string;
  /** This process's average CPU share and memory. */
  cpu: number;
  memory: number;
  disk: number;
  swing: number;
  seed: number;
  started: number;
  protectedReason: string | null;
  state: RunState;
}

/** Builds the process table once (deterministic pids and parents) relative to `start`. */
export function buildProcesses(start: number, specs: readonly AppSpec[] = APP_SPECS, rootPid = 4): MockProcess[] {
  const out: MockProcess[] = [];
  const firstOf = new Map<string, number>();
  let nextPid = rootPid + 4;
  const root = specs[0]!;
  for (const spec of specs) {
    // Split the totals unevenly: the first process of a group does most of the work (the browser's main process).
    const weights = Array.from({ length: spec.count }, (_, i) => (i === 0 ? 3 : 0.4 + hash01(i, seedOf(spec.name)) * 1.6));
    const sum = weights.reduce((a, b) => a + b, 0);
    const parentKey = spec.parent?.replace(/-first$/, '');
    for (let i = 0; i < spec.count; i++) {
      const pid = spec === root ? rootPid : nextPid;
      nextPid += 4 + Math.floor(hash01(nextPid, 7) * 12) * 4;
      const ownParent = i > 0 && (spec.user === USER || spec.user === 'alex') && !['conhost', 'pwsh', 'zsh', 'bash', 'runtimebroker', 'dllhost', 'dock'].includes(spec.group) ? firstOf.get(`${spec.group}:${spec.name}`) : undefined;
      const parent = ownParent ?? (parentKey ? (firstOf.get(parentKey) ?? null) : null);
      if (i === 0) {
        if (!firstOf.has(spec.group)) firstOf.set(spec.group, pid);
        firstOf.set(`${spec.group}:${spec.name}`, pid);
      }
      const w = weights[i]! / sum;
      out.push({
        pid,
        parent: spec === root ? null : parent,
        name: spec.name,
        exe: spec.exe,
        user: spec.user,
        group: spec.group,
        groupName: spec.groupName,
        cpu: spec.cpu * w,
        memory: spec.memory * w,
        disk: spec.disk * w,
        swing: spec.swing ?? 0.3,
        seed: seedOf(`${spec.name}#${i}`),
        started: start - (spec.ageSecs - i * 3) * 1000,
        protectedReason: spec.protectedReason ?? null,
        state: 'running',
      });
    }
  }
  return out;
}

/** The live sample of a process at second `t`; suspended processes use no CPU and capped ones stay under the cap. */
export function sampleProcess(p: MockProcess, t: number, cap: number | null, cpuScale: number): ProcessSample {
  const suspended = p.state === 'suspended';
  let cpu = suspended ? 0 : Math.max(0, p.cpu * cpuScale * (1 + p.swing * wobble(t, p.seed, 6)));
  if (cap !== null) cpu = Math.min(cpu, cap);
  const memory = p.memory * (1 + 0.03 * wobble(t, p.seed + 3, 40));
  const disk = suspended ? 0 : Math.max(0, p.disk * (1 + 0.8 * wobble(t, p.seed + 5, 4)));
  return {
    pid: p.pid,
    parent: p.parent,
    name: p.name,
    exe: p.exe,
    user: p.user,
    cpu: round(clamp(cpu, 0, 100), 2),
    memory: Math.round(memory),
    diskReadBps: Math.round(disk * 0.35),
    diskWriteBps: Math.round(disk * 0.65),
    started: p.started,
    state: suspended ? 'suspended' : cpu < 0.05 ? 'sleeping' : 'running',
    protectedReason: p.protectedReason,
  };
}

/** Groups samples by app as the engine does (the mock knows each process's group; the engine derives it). */
export function groupSamples(processes: MockProcess[], samples: ProcessSample[]): AppGroup[] {
  const groups = new Map<string, AppGroup>();
  samples.forEach((s, i) => {
    const p = processes[i]!;
    let g = groups.get(p.group);
    if (!g) {
      g = { id: p.group, name: p.groupName, exe: p.exe, pids: [], cpu: 0, memory: 0, diskBps: 0 };
      groups.set(p.group, g);
    }
    g.pids.push(s.pid);
    g.cpu += s.cpu;
    g.memory += s.memory;
    g.diskBps += s.diskReadBps + s.diskWriteBps;
  });
  return [...groups.values()].map((g) => ({ ...g, cpu: round(g.cpu, 2) }));
}

export function round(v: number, digits: number): number {
  const f = 10 ** digits;
  return Math.round(v * f) / f;
}
