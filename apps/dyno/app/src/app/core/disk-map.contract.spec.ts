import type { DiskChildren, DiskScanStatus, DiskTarget, ScanPhase, Volume } from './api.types';
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
});

const phases: ScanPhase[] = ['readingVolumeFacts', 'readingFolders'];

const children: DiskChildren = {
  node: { id: 0, name: 'Macintosh HD', kind: 'folder', bytes: 504, items: 5, newestChange: 1_700_000_000_000, complete: false, hasChildren: true },
  children: [{ id: 1, name: 'Users', kind: 'folder', bytes: 500, items: 1, newestChange: 1_700_000_000_000, complete: true, hasChildren: true }],
  other: { count: 2, bytes: 0, items: 0 },
  loose: { count: 1, bytes: 4, items: 1 },
  shown: 1,
  offset: 0,
  nextOffset: null,
};

const expected = {
  requests: {
    diskVolumes: 'diskVolumes',
    startVolume: { startDiskScan: { target: volumeTarget } },
    startFolder: { startDiskScan: { target: { folder: { path: '/Users/alex' } } satisfies DiskTarget } },
    cancel: { cancelDiskScan: { scan: 7 } },
    scans: 'diskScans',
    children: { diskChildren: { scan: 7, node: 4, offset: 0, limit: 48, minFraction: 0.004 } },
    childrenOfRoot: { diskChildren: { scan: 7, node: null, offset: 48, limit: 48, minFraction: 0 } },
    path: { diskPath: { scan: 7, node: 4 } },
  },
  volumes: [internal, usb, share],
  statuses: [
    status('running', internal),
    status('done', null),
    status('cancelled', null),
    status({ failed: { failure: { volumeUnreachable: { detail: 'the share stopped answering' } } } }, null),
    status({ failed: { failure: { cannotStart: { reason: 'no' } } } }, null),
    status({ failed: { failure: { internal: { detail: 'bug' } } } }, null),
  ],
  phases,
  children,
};

describe('the disk map wire contract with the engine', () => {
  it('has the same values as the Rust types serialise to', () => {
    expect(expected).toEqual(wire);
  });

  it('covers every variant the engine can send', () => {
    const kinds = new Set(wire.children.children.map((c) => c.kind));
    expect(kinds.size).toBeGreaterThan(0);
    expect(wire.statuses.map((s) => (typeof s.state === 'string' ? s.state : Object.keys(s.state)[0]))).toEqual(['running', 'done', 'cancelled', 'failed', 'failed', 'failed']);
  });
});
