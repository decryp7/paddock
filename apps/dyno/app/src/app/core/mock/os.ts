import type { AppInfo, DiskSample, NetworkSample } from '../api.types';
import { type AppSpec, APP_SPECS } from './processes';

const GB = 1024 ** 3;
const MB = 1024 ** 2;

/** The operating system the mock pretends to run on (`?mock=windows|macos|linux`). */
export type MockOs = AppInfo['os'];

const WIN_USER = 'DEV-LAPTOP\\alex';

type Spec = Omit<AppSpec, 'user'> & { user?: string };
const spec = (user: string, s: Spec): AppSpec => ({ user, ...s });

/** macOS: a developer's MacBook Pro in the middle of a `cargo build`. Parents come first. */
function macSpecs(): AppSpec[] {
  const u = 'alex';
  const app = (s: Spec) => spec(u, s);
  const sys = (s: Spec) => spec('root', { system: true, ...s });
  return [
    sys({ group: 'system', groupName: 'launchd', name: 'launchd', exe: '/sbin/launchd', count: 1, cpu: 0.2, memory: 14 * MB, disk: 0.1 * MB, ageSecs: 1_040_000 }),
    sys({ group: 'kernel', groupName: 'Kernel', name: 'kernel_task', exe: null, count: 1, cpu: 2.6, memory: 1.1 * GB, disk: 0, parent: 'system', swing: 0.5, ageSecs: 1_040_000 }),
    spec('_windowserver', { group: 'windowserver', groupName: 'WindowServer', name: 'WindowServer', exe: '/System/Library/PrivateFrameworks/SkyLight.framework/Resources/WindowServer', count: 1, cpu: 4.4, memory: 310 * MB, disk: 0, parent: 'system', system: true, swing: 0.6, ageSecs: 1_039_900 }),
    sys({ group: 'svc', groupName: 'macOS services', name: 'xpcproxy', exe: '/usr/libexec/xpcproxy', count: 64, cpu: 1.7, memory: 1.05 * GB, disk: 1.9 * MB, parent: 'system', ageSecs: 1_039_000 }),
    sys({ group: 'coreaudio', groupName: 'Core Audio', name: 'coreaudiod', exe: '/usr/sbin/coreaudiod', count: 1, cpu: 0.3, memory: 21 * MB, disk: 0, parent: 'system', ageSecs: 1_039_000 }),
    sys({ group: 'spotlight', groupName: 'Spotlight', name: 'mds_stores', exe: '/System/Library/Frameworks/CoreServices.framework/Frameworks/Metadata.framework/Support/mds_stores', count: 3, cpu: 1.6, memory: 340 * MB, disk: 3.1 * MB, parent: 'system', system: false, swing: 0.9, ageSecs: 640_000 }),
    sys({ group: 'xprotect', groupName: 'XProtect', name: 'XProtectService', exe: '/Library/Apple/System/Library/CoreServices/XProtect.app/Contents/MacOS/XProtectService', count: 2, cpu: 1.1, memory: 96 * MB, disk: 0.4 * MB, parent: 'system', ageSecs: 1_039_000 }),
    sys({ group: 'dyno-agent', groupName: 'Dyno agent', name: 'dyno-agent', exe: '/Applications/Dyno.app/Contents/MacOS/dyno-agent', count: 1, cpu: 0.4, memory: 38 * MB, disk: 0.05 * MB, parent: 'system', system: true, ageSecs: 3_000 }),
    app({ group: 'finder', groupName: 'Finder', name: 'Finder', exe: '/System/Library/CoreServices/Finder.app/Contents/MacOS/Finder', count: 1, cpu: 0.4, memory: 240 * MB, disk: 0.2 * MB, parent: 'system', ageSecs: 1_039_800 }),
    app({ group: 'dock', groupName: 'Dock and menu bar', name: 'Dock', exe: '/System/Library/CoreServices/Dock.app/Contents/MacOS/Dock', count: 4, cpu: 0.8, memory: 190 * MB, disk: 0, parent: 'system', system: true, ageSecs: 1_039_800 }),
    app({ group: 'chrome', groupName: 'Google Chrome', name: 'Google Chrome', exe: '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', count: 38, cpu: 6.4, memory: 4.3 * GB, disk: 1.8 * MB, parent: 'finder', swing: 0.8, ageSecs: 31_000 }),
    app({ group: 'vscode', groupName: 'Visual Studio Code', name: 'Code Helper', exe: '/Applications/Visual Studio Code.app/Contents/MacOS/Electron', count: 14, cpu: 3.2, memory: 2.2 * GB, disk: 0.9 * MB, parent: 'finder', swing: 0.7, ageSecs: 18_000 }),
    app({ group: 'terminal', groupName: 'Terminal', name: 'Terminal', exe: '/System/Applications/Utilities/Terminal.app/Contents/MacOS/Terminal', count: 1, cpu: 0.3, memory: 120 * MB, disk: 0, parent: 'finder', ageSecs: 189_000 }),
    app({ group: 'zsh', groupName: 'zsh', name: 'zsh', exe: '/bin/zsh', count: 3, cpu: 0.1, memory: 22 * MB, disk: 0, parent: 'terminal', ageSecs: 189_000 }),
    app({ group: 'cargo', groupName: 'Rust build (cargo)', name: 'cargo', exe: '/Users/alex/.cargo/bin/cargo', count: 1, cpu: 0.4, memory: 48 * MB, disk: 0.4 * MB, parent: 'zsh', ageSecs: 410 }),
    app({ group: 'cargo', groupName: 'Rust build (cargo)', name: 'rustc', exe: '/Users/alex/.rustup/toolchains/stable-aarch64-apple-darwin/bin/rustc', count: 6, cpu: 41, memory: 3.1 * GB, disk: 186 * MB, parent: 'cargo', swing: 0.25, ageSecs: 300 }),
    app({ group: 'node', groupName: 'Node.js (ng serve)', name: 'node', exe: '/opt/homebrew/bin/node', count: 3, cpu: 1.8, memory: 920 * MB, disk: 0.6 * MB, parent: 'zsh', swing: 0.7, ageSecs: 7_200 }),
    app({ group: 'docker', groupName: 'Docker Desktop', name: 'Docker Desktop', exe: '/Applications/Docker.app/Contents/MacOS/Docker Desktop', count: 4, cpu: 0.6, memory: 430 * MB, disk: 0.1 * MB, parent: 'finder', ageSecs: 302_400 }),
    app({ group: 'vm', groupName: 'Docker VM', name: 'com.apple.Virtualization.VirtualMachine', exe: '/System/Library/Frameworks/Virtualization.framework/Versions/A/XPCServices/com.apple.Virtualization.VirtualMachine.xpc/Contents/MacOS/com.apple.Virtualization.VirtualMachine', count: 1, cpu: 3.4, memory: 4.1 * GB, disk: 6.2 * MB, parent: 'docker', swing: 0.5, ageSecs: 302_300 }),
    app({ group: 'teams', groupName: 'Microsoft Teams', name: 'MSTeams', exe: '/Applications/Microsoft Teams.app/Contents/MacOS/MSTeams', count: 6, cpu: 1.9, memory: 1.25 * GB, disk: 0.4 * MB, parent: 'finder', swing: 0.8, ageSecs: 90_000 }),
    app({ group: 'slack', groupName: 'Slack', name: 'Slack', exe: '/Applications/Slack.app/Contents/MacOS/Slack', count: 5, cpu: 0.8, memory: 640 * MB, disk: 0.1 * MB, parent: 'finder', swing: 0.7, ageSecs: 86_000 }),
    app({ group: 'spotify', groupName: 'Spotify', name: 'Spotify', exe: '/Applications/Spotify.app/Contents/MacOS/Spotify', count: 5, cpu: 1.1, memory: 470 * MB, disk: 0.2 * MB, parent: 'finder', ageSecs: 26_000 }),
    app({ group: 'dyno', groupName: 'Dyno', name: 'Dyno', exe: '/Applications/Dyno.app/Contents/MacOS/Dyno', count: 2, cpu: 0.3, memory: 96 * MB, disk: 0, parent: 'finder', system: true, ageSecs: 3_000 }),
  ];
}

/** Linux: a developer's laptop on GNOME. */
function linuxSpecs(): AppSpec[] {
  const u = 'alex';
  const app = (s: Spec) => spec(u, s);
  const sys = (s: Spec) => spec('root', { system: true, ...s });
  return [
    sys({ group: 'system', groupName: 'systemd', name: 'systemd', exe: '/usr/lib/systemd/systemd', count: 1, cpu: 0.2, memory: 18 * MB, disk: 0.1 * MB, ageSecs: 1_040_000 }),
    sys({ group: 'svc', groupName: 'Linux services', name: 'systemd-udevd', exe: '/usr/lib/systemd/systemd-udevd', count: 42, cpu: 1.4, memory: 520 * MB, disk: 1.9 * MB, parent: 'system', ageSecs: 1_039_000 }),
    sys({ group: 'journald', groupName: 'Journal', name: 'systemd-journald', exe: '/usr/lib/systemd/systemd-journald', count: 1, cpu: 0.3, memory: 64 * MB, disk: 2.2 * MB, parent: 'system', ageSecs: 1_039_000 }),
    spec('alex', { group: 'gnome', groupName: 'GNOME Shell', name: 'gnome-shell', exe: '/usr/bin/gnome-shell', count: 1, cpu: 3.6, memory: 380 * MB, disk: 0, parent: 'system', system: true, swing: 0.6, ageSecs: 1_039_800 }),
    spec('alex', { group: 'pipewire', groupName: 'PipeWire audio', name: 'pipewire', exe: '/usr/bin/pipewire', count: 2, cpu: 0.4, memory: 28 * MB, disk: 0, parent: 'system', system: true, ageSecs: 1_039_000 }),
    sys({ group: 'netman', groupName: 'NetworkManager', name: 'NetworkManager', exe: '/usr/sbin/NetworkManager', count: 1, cpu: 0.1, memory: 24 * MB, disk: 0, parent: 'system', ageSecs: 1_039_000 }),
    app({ group: 'tracker', groupName: 'GNOME Tracker (indexing)', name: 'tracker-miner-fs-3', exe: '/usr/libexec/tracker-miner-fs-3', count: 2, cpu: 1.5, memory: 150 * MB, disk: 3.1 * MB, parent: 'system', swing: 0.9, ageSecs: 640_000 }),
    sys({ group: 'dyno-agent', groupName: 'Dyno agent', name: 'dyno-agent', exe: '/usr/bin/dyno-agent', count: 1, cpu: 0.4, memory: 38 * MB, disk: 0.05 * MB, parent: 'system', system: true, ageSecs: 3_000 }),
    app({ group: 'chrome', groupName: 'Google Chrome', name: 'chrome', exe: '/opt/google/chrome/chrome', count: 38, cpu: 6.4, memory: 4.3 * GB, disk: 1.8 * MB, parent: 'gnome', swing: 0.8, ageSecs: 31_000 }),
    app({ group: 'vscode', groupName: 'Visual Studio Code', name: 'code', exe: '/usr/share/code/code', count: 14, cpu: 3.2, memory: 2.2 * GB, disk: 0.9 * MB, parent: 'gnome', swing: 0.7, ageSecs: 18_000 }),
    app({ group: 'terminal', groupName: 'GNOME Terminal', name: 'gnome-terminal-server', exe: '/usr/libexec/gnome-terminal-server', count: 1, cpu: 0.3, memory: 96 * MB, disk: 0, parent: 'gnome', ageSecs: 189_000 }),
    app({ group: 'zsh', groupName: 'bash', name: 'bash', exe: '/usr/bin/bash', count: 3, cpu: 0.1, memory: 18 * MB, disk: 0, parent: 'terminal', ageSecs: 189_000 }),
    app({ group: 'cargo', groupName: 'Rust build (cargo)', name: 'cargo', exe: '/home/alex/.cargo/bin/cargo', count: 1, cpu: 0.4, memory: 48 * MB, disk: 0.4 * MB, parent: 'zsh', ageSecs: 410 }),
    app({ group: 'cargo', groupName: 'Rust build (cargo)', name: 'rustc', exe: '/home/alex/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin/rustc', count: 6, cpu: 41, memory: 3.1 * GB, disk: 186 * MB, parent: 'cargo', swing: 0.25, ageSecs: 300 }),
    app({ group: 'node', groupName: 'Node.js (ng serve)', name: 'node', exe: '/usr/bin/node', count: 3, cpu: 1.8, memory: 920 * MB, disk: 0.6 * MB, parent: 'zsh', swing: 0.7, ageSecs: 7_200 }),
    sys({ group: 'docker', groupName: 'Docker', name: 'dockerd', exe: '/usr/bin/dockerd', count: 3, cpu: 3.4, memory: 1.4 * GB, disk: 6.2 * MB, parent: 'system', system: false, swing: 0.5, ageSecs: 302_300 }),
    app({ group: 'slack', groupName: 'Slack', name: 'slack', exe: '/usr/lib/slack/slack', count: 5, cpu: 0.8, memory: 640 * MB, disk: 0.1 * MB, parent: 'gnome', swing: 0.7, ageSecs: 86_000 }),
    app({ group: 'spotify', groupName: 'Spotify', name: 'spotify', exe: '/usr/share/spotify/spotify', count: 5, cpu: 1.1, memory: 470 * MB, disk: 0.2 * MB, parent: 'gnome', ageSecs: 26_000 }),
    app({ group: 'dyno', groupName: 'Dyno', name: 'dyno', exe: '/usr/bin/dyno', count: 2, cpu: 0.3, memory: 96 * MB, disk: 0, parent: 'gnome', system: true, ageSecs: 3_000 }),
  ];
}

/** Process names the mock refers to by role (they differ per OS). */
export interface RoleNames {
  rustc: string;
  cargo: string;
  chrome: string;
  code: string;
  node: string;
  indexer: string;
  agent: string;
}

/** Everything about a mock machine that depends on the OS. */
export interface OsProfile {
  os: MockOs;
  specs: AppSpec[];
  rootPid: number;
  names: RoleNames;
  user: string;
  cores: number;
  /** Hybrid layout: efficiency and performance core counts and whether the E cores are numbered first (Apple silicon). */
  hybrid: { efficiency: number; performance: number; efficiencyFirst: boolean } | null;
  memoryTotal: number;
  cpuBaseMhz: number;
  cpuMaxMhz: number;
  gpuName: string;
  disks: (elapsedDays: number) => DiskSample[];
  network: (wobble: (salt: number, period: number) => number) => NetworkSample[];
  /** The label of the primary disk in diagnoses ("Drive C:" / "Macintosh HD"). */
  primaryDisk: string;
  bootLabel: string;
  cpuSensor: string;
  noBattery: string;
  noTemperatures: string;
  noFans: string;
  home: string;
  projectDir: string;
  libs: string[];
  openFilesDenied: string;
  modulesDenied: string;
  signer: string;
  kernelNote: string;
}

const MB1 = (v: number) => Math.round(v * MB);

export function osProfile(os: MockOs): OsProfile {
  if (os === 'macos') {
    return {
      os,
      specs: macSpecs(),
      rootPid: 1,
      names: { rustc: 'rustc', cargo: 'cargo', chrome: 'Google Chrome', code: 'Code Helper', node: 'node', indexer: 'mds_stores', agent: 'dyno-agent' },
      user: 'alex',
      cores: 14,
      hybrid: { efficiency: 4, performance: 10, efficiencyFirst: true },
      memoryTotal: 48 * GB,
      cpuBaseMhz: 1000,
      cpuMaxMhz: 4500,
      gpuName: 'Apple M4 Pro GPU (20 cores)',
      disks: (days) => [
        { id: 'macintosh-hd', name: 'Macintosh HD (APFS)', mount: '/', total: Math.round(994.7 * GB), free: Math.round((231 - days * 5.1) * GB), removable: false },
        { id: 't7', name: 'Samsung T7', mount: '/Volumes/T7', total: Math.round(931.5 * GB), free: Math.round(604 * GB), removable: true },
      ],
      network: (w) => [
        { name: 'Wi-Fi (en0)', rxBps: MB1(3.8 + 2.2 * w(810, 5)), txBps: MB1(0.6 + 0.4 * w(811, 5)) },
        { name: 'Thunderbolt Ethernet (en5)', rxBps: MB1(0.04 + 0.02 * w(812, 5)), txBps: MB1(0.01 + 0.01 * w(813, 5)) },
      ],
      primaryDisk: 'Macintosh HD',
      bootLabel: 'macOS started',
      cpuSensor: 'CPU die',
      noBattery: 'This Mac has no battery.',
      noTemperatures: 'macOS does not report temperatures to Dyno (Dyno installs no drivers).',
      noFans: 'This Mac reports no fan speeds to Dyno.',
      home: '/Users/alex',
      projectDir: '/Users/alex/dev/dyno',
      libs: ['/usr/lib/libSystem.B.dylib', '/usr/lib/system/libsystem_kernel.dylib', '/usr/lib/libc++.1.dylib', '/usr/lib/libobjc.A.dylib'],
      openFilesDenied: 'macOS does not list open files of system processes.',
      modulesDenied: 'macOS does not list the libraries of system processes.',
      signer: 'Apple Inc.',
      kernelNote: 'it is part of the macOS kernel',
    };
  }
  if (os === 'linux') {
    return {
      os,
      specs: linuxSpecs(),
      rootPid: 1,
      names: { rustc: 'rustc', cargo: 'cargo', chrome: 'chrome', code: 'code', node: 'node', indexer: 'tracker-miner-fs-3', agent: 'dyno-agent' },
      user: 'alex',
      cores: 16,
      hybrid: null,
      memoryTotal: 32 * GB,
      cpuBaseMhz: 3800,
      cpuMaxMhz: 4650,
      gpuName: 'NVIDIA GeForce RTX 4070 Laptop GPU',
      disks: (days) => [
        { id: 'nvme0n1p2', name: 'WD Blue SN570 (ext4)', mount: '/', total: Math.round(953 * GB), free: Math.round((210 - days * 5.1) * GB), removable: false },
        { id: 'sda1', name: 'Seagate BarraCuda (ext4)', mount: '/mnt/data', total: Math.round(1863 * GB), free: Math.round(1204 * GB), removable: false },
      ],
      network: (w) => [
        { name: 'enp5s0 (Ethernet)', rxBps: MB1(3.8 + 2.2 * w(810, 5)), txBps: MB1(0.6 + 0.4 * w(811, 5)) },
        { name: 'wlp4s0 (Wi-Fi)', rxBps: MB1(0.12 + 0.1 * w(812, 5)), txBps: MB1(0.03 + 0.02 * w(813, 5)) },
      ],
      primaryDisk: 'Root filesystem',
      bootLabel: 'Linux started',
      cpuSensor: 'CPU package',
      noBattery: 'This computer has no battery.',
      noTemperatures: 'No hwmon temperature sensors are readable (the kernel exposes none on this machine).',
      noFans: 'No fan speeds are exposed under /sys/class/hwmon.',
      home: '/home/alex',
      projectDir: '/home/alex/dev/dyno',
      libs: ['/lib/x86_64-linux-gnu/libc.so.6', '/lib/x86_64-linux-gnu/libm.so.6', '/lib/x86_64-linux-gnu/libpthread.so.0', '/lib/x86_64-linux-gnu/libdl.so.2'],
      openFilesDenied: 'Reading /proc/<pid>/fd of other users’ processes needs administrator rights.',
      modulesDenied: 'Reading /proc/<pid>/maps of other users’ processes needs administrator rights.',
      signer: 'Distribution package',
      kernelNote: 'it is a kernel thread',
    };
  }
  return {
    os: 'windows',
    specs: APP_SPECS,
    rootPid: 4,
    names: { rustc: 'rustc.exe', cargo: 'cargo.exe', chrome: 'chrome.exe', code: 'Code.exe', node: 'node.exe', indexer: 'SearchIndexer.exe', agent: 'dyno-agent.exe' },
    user: WIN_USER,
    cores: 16,
    hybrid: null,
    memoryTotal: Math.round(31.7 * GB),
    cpuBaseMhz: 3800,
    cpuMaxMhz: 4640,
    gpuName: 'NVIDIA GeForce RTX 4070 Laptop GPU',
    disks: (days) => [
      { id: 'C', name: 'Samsung PM9A1 NVMe', mount: 'C:\\', total: Math.round(953 * GB), free: Math.round((210 - days * 5.1) * GB), removable: false },
      { id: 'D', name: 'Seagate BarraCuda HDD', mount: 'D:\\', total: Math.round(1863 * GB), free: Math.round(1204 * GB), removable: false },
    ],
    network: (w) => [
      { name: 'Ethernet (USB-C dock)', rxBps: MB1(3.8 + 2.2 * w(810, 5)), txBps: MB1(0.6 + 0.4 * w(811, 5)) },
      { name: 'Wi-Fi', rxBps: MB1(0.12 + 0.1 * w(812, 5)), txBps: MB1(0.03 + 0.02 * w(813, 5)) },
    ],
    primaryDisk: 'Drive C:',
    bootLabel: 'Windows started',
    cpuSensor: 'CPU package',
    noBattery: 'This computer has no battery.',
    noTemperatures: 'The CPU does not report temperatures without a vendor driver (Dyno installs no drivers).',
    noFans: 'The motherboard does not expose fan speeds to Windows.',
    home: 'C:\\Users\\alex',
    projectDir: 'C:\\dev\\dyno',
    libs: ['ntdll.dll', 'KERNEL32.DLL', 'KERNELBASE.dll', 'ucrtbase.dll', 'USER32.dll', 'win32u.dll', 'GDI32.dll', 'ws2_32.dll', 'bcrypt.dll'],
    openFilesDenied: 'Windows does not list open files of protected system processes.',
    modulesDenied: 'Windows does not list modules of protected system processes.',
    signer: 'Microsoft Windows',
    kernelNote: 'it is part of the Windows kernel',
  };
}

/** The kind of each logical core for a hybrid layout (empty for a uniform CPU). */
export function coreKindsOf(hybrid: OsProfile['hybrid']): ('performance' | 'efficiency')[] {
  if (!hybrid) return [];
  const e = Array<'efficiency'>(hybrid.efficiency).fill('efficiency');
  const p = Array<'performance'>(hybrid.performance).fill('performance');
  return hybrid.efficiencyFirst ? [...e, ...p] : [...p, ...e];
}
