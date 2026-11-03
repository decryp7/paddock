import type { DiskChildren, DiskRings, DiskSettings, NotSaved, RingNode, DiskScanStatus, DiskTarget, ScanPhase, Volume } from './api.types';
import wire from './disk-map.wire.json';

/**
 * The wire contract of the disk map. `disk-map.wire.json` is written by the Rust test `tests/disk_wire.rs` from the engine's
 * own types; the values below are the same ones as TypeScript literals, so the compiler checks them against `api.types.ts`
 * and the test checks they equal what Rust produces. Change one side without the other and this fails.
 */
const internal: Volume = {
  id: 'Macintosh HD',
  name: 'Macintosh HD',
  mount: '/Volumes/Macintosh HD',
  fs: 'apfs',
  location: 'internal',
  capacity: 1000,
  free: 400,
  rotational: false,
  readOnly: false,
  reachable: true,
  wholeVolume: true,
};
const usb: Volume = { ...internal, id: 'Photos', name: 'Photos', mount: '/Volumes/Photos', location: { external: { bus: 'USB' } } };
const share: Volume = { ...internal, id: 'Media', name: 'Media', mount: '/Volumes/Media', location: { network: { protocol: 'webDav', host: 'nas.local' } }, wholeVolume: false };

const volumeTarget: DiskTarget = { volume: { id: 'Macintosh HD' } };
const status = (state: DiskScanStatus['state'], volume: Volume | null): DiskScanStatus => ({
  scan: 7,
  seq: 3,
  name: 'Macintosh HD',
  target: volumeTarget,
  volume,
  state,
  phase: 'readingFolders',
  progress: { bytes: 600, items: 12, folders: 3, totalBytes: 600, unreadable: 1 },
  truncated: false,
  unreadableFirst: ['/Users/alex/Library/Mail'],
  needsFullDiskAccess: true,
  itemsPerSecond: 1200,
  etaSeconds: null,
  hiddenBytes: 40,
  scannedAt: 1_700_000_100_000,
  origin: 'live',
  changedBytes: null,
  replaces: null,
  notSaved: null,
});

/** A finished scan read back from the cache, or a live one that could not be kept. */
const savedStatus = (scan: number, changedBytes: number | null, replaces: number | null, notSaved: NotSaved | null): DiskScanStatus => ({
  ...status('done', null),
  scan,
  origin: notSaved === null ? { saved: { savedAt: 1_700_000_200_000 } } : 'live',
  changedBytes,
  replaces,
  notSaved,
});

const phases: ScanPhase[] = ['readingVolumeFacts', 'readingFolders'];

const child = (id: number, name: string, kind: DiskChildren['children'][number]['kind'], bytes: number, logicalBytes: number, items: number, complete: boolean, newestChange: number, hasChildren = false, unreadableReason: DiskChildren['children'][number]['unreadableReason'] = null) => ({
  id,
  name,
  kind,
  bytes,
  logicalBytes,
  items,
  newestChange,
  complete,
  hasChildren,
  unreadableReason,
});

const children: DiskChildren = {
  node: { id: 0, name: 'Macintosh HD', kind: 'folder', bytes: 604, logicalBytes: 574, items: 6, newestChange: 1_700_000_000_000, complete: false, hasChildren: true, unreadableReason: null },
  children: [
    child(1, 'Users', 'folder', 500, 480, 1, true, 1_700_000_000_000, true),
    child(3, 'vm.img', 'file', 100, 90, 1, true, 1_650_000_000_000),
    child(4, 'Volumes', 'mountPoint', 0, 0, 0, true, 0),
    child(5, 'private', 'unreadable', 0, 0, 0, false, 0, false, 'needsFullDiskAccess'),
  ],
  other: { count: 0, bytes: 0, logicalBytes: 0, items: 0 },
  loose: { count: 1, bytes: 4, logicalBytes: 4, items: 1 },
  shown: 4,
  offset: 0,
  nextOffset: null,
};

const none = { count: 0, bytes: 0, logicalBytes: 0, items: 0 };
const ringNode = (node: DiskChildren['node'], kids: RingNode[] = [], loose = none): RingNode => ({ node, children: kids, other: none, loose });
const rings: DiskRings = {
  root: ringNode(
    { id: 0, name: 'Macintosh HD', kind: 'folder', bytes: 503, logicalBytes: 483, items: 4, newestChange: 1_700_000_000_000, complete: false, hasChildren: true, unreadableReason: null },
    [
      ringNode(
        { id: 1, name: 'Users', kind: 'folder', bytes: 503, logicalBytes: 483, items: 2, newestChange: 1_700_000_000_000, complete: true, hasChildren: true, unreadableReason: null },
        [ringNode({ id: 2, name: 'disk-image.dmg', kind: 'file', bytes: 500, logicalBytes: 480, items: 1, newestChange: 1_700_000_000_000, complete: true, hasChildren: false, unreadableReason: null })],
        { count: 1, bytes: 3, logicalBytes: 3, items: 1 },
      ),
      ringNode({ id: 3, name: 'private', kind: 'unreadable', bytes: 0, logicalBytes: 0, items: 0, newestChange: 0, complete: false, hasChildren: false, unreadableReason: 'needsFullDiskAccess' }),
    ],
  ),
  count: 4,
  capped: false,
};

const settings: DiskSettings = { scanFolders: 2_000_000, notableFileMib: 10, readThreads: 'auto', finderFiles: 500_000, finderDepth: 48, finderHashGib: 64 };

const expected = {
  requests: {
    diskVolumes: 'diskVolumes',
    startVolume: { startDiskScan: { target: volumeTarget, replaces: null } },
    rescanVolume: { startDiskScan: { target: volumeTarget, replaces: 7 } },
    startFolder: { startDiskScan: { target: { folder: { path: '/Users/alex' } } satisfies DiskTarget, replaces: null } },
    forgetSaved: { forgetSavedDiskMap: { scan: 7 } },
    forgetAllSaved: 'forgetSavedDiskMaps',
    cancel: { cancelDiskScan: { scan: 7 } },
    scans: 'diskScans',
    children: { diskChildren: { scan: 7, node: 4, offset: 0, limit: 48, minFraction: 0.004 } },
    childrenOfRoot: { diskChildren: { scan: 7, node: null, offset: 48, limit: 48, minFraction: 0 } },
    path: { diskPath: { scan: 7, node: 4 } },
    retry: { retryDiskScan: { scan: 7 } },
    rings: { diskRings: { scan: 7, node: 4, rings: 3, minShare: 0.01, maxNodes: 500 } },
    ringsOfRoot: { diskRings: { scan: 7, node: null, rings: 6, minShare: 0, maxNodes: 2000 } },
    openFullDiskAccess: 'openFullDiskAccessSettings',
    reveal: { diskReveal: { scan: 7, node: 4 } },
  },
  volumes: [internal, usb, share],
  statuses: [
    status('running', internal),
    status('done', null),
    status('cancelled', null),
    status({ failed: { failure: { volumeUnreachable: { detail: 'the share stopped answering' } } } }, null),
    status({ failed: { failure: { cannotStart: { reason: 'no' } } } }, null),
    status({ failed: { failure: { internal: { detail: 'bug' } } } }, null),
    savedStatus(8, -120, null, null),
    savedStatus(9, null, 8, 'tooBig'),
    savedStatus(10, 4096, null, 'noSpace'),
    savedStatus(11, null, null, 'failed'),
  ],
  settings,
  rings,
  phases,
  children,
};

describe('the disk map wire contract with the engine', () => {
  it('has the same values as the Rust types serialise to', () => {
    expect(expected).toEqual(wire);
  });

  it('covers every variant the engine can send', () => {
    const kinds = new Set(wire.children.children.map((c) => c.kind));
    expect([...kinds].sort()).toEqual(['file', 'folder', 'mountPoint', 'unreadable']);
    expect(wire.statuses.map((s) => (typeof s.state === 'string' ? s.state : Object.keys(s.state)[0]))).toEqual(['running', 'done', 'cancelled', 'failed', 'failed', 'failed', 'done', 'done', 'done', 'done']);
  });
});
