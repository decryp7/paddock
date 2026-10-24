import { parseHealthSnapshot, parseNotice, parseNoticesEvent, parseNoticesSnapshot } from './wire';

const service = { service: 'store', label: 'the log store', state: 'down', cause: { message: 'm', hint: 'h', detail: null, code: 'c', raiseCap: { target: 'storeWorker', suggestedMb: 512 }, settings: null }, repair: 'restart' };
const notice = {
  id: 1, source: 'health', severity: 'error', title: 't', body: 'b', code: null, at: 2, firstAt: 1, count: 3, read: false,
  primary: { type: 'repairService', service: 'store' }, secondary: null, tertiary: null, link: { window: null, jobId: null, path: null }, cause: 'health:store', scope: 'app',
};

describe('parseHealthSnapshot', () => {
  it('reads a snapshot, with optional fields missing', () => {
    expect(parseHealthSnapshot({ revision: 2, services: [service] })).toEqual({ revision: 2, services: [service] });
    expect(parseHealthSnapshot({ revision: 1, services: [{ service: 's', label: 'l', state: 'ok' }] })).toEqual({ revision: 1, services: [{ service: 's', label: 'l', state: 'ok', cause: null, repair: null }] });
    expect(parseHealthSnapshot({ revision: 1, services: [{ ...service, cause: { message: 'only' } }] })?.services[0]?.cause).toEqual({ message: 'only', hint: null, detail: null, code: null, raiseCap: null, settings: null });
  });

  it.each([
    ['not an object', 3],
    ['an array', []],
    ['no services', { revision: 1 }],
    ['no revision', { services: [] }],
    ['a bad state', { revision: 1, services: [{ ...service, state: 'gone' }] }],
    ['a bad repair', { revision: 1, services: [{ ...service, repair: 'pray' }] }],
    ['a bad cause', { revision: 1, services: [{ ...service, cause: 'x' }] }],
    ['a cause without a message', { revision: 1, services: [{ ...service, cause: { hint: 'h' } }] }],
    ['a bad hint', { revision: 1, services: [{ ...service, cause: { message: 'm', hint: 3 } }] }],
    ['a bad cap', { revision: 1, services: [{ ...service, cause: { message: 'm', raiseCap: { target: 1 } } }] }],
    ['a cap that is not an object', { revision: 1, services: [{ ...service, cause: { message: 'm', raiseCap: 'big' } }] }],
    ['a service that is not an object', { revision: 1, services: [null] }],
    ['no label', { revision: 1, services: [{ service: 's', state: 'ok' }] }],
  ])('refuses %s', (_what, v) => {
    expect(parseHealthSnapshot(v)).toBeNull();
  });
});

describe('notices', () => {
  it('reads a notice, a list and an event', () => {
    expect(parseNotice(notice)).toEqual(notice);
    expect(parseNotice({ ...notice, link: undefined, code: undefined })?.link).toEqual({ window: null, jobId: null, path: null });
    expect(parseNoticesSnapshot({ notices: [notice], unread: 1 })).toEqual({ notices: [notice], unread: 1 });
    expect(parseNoticesEvent({ notices: [notice], unread: 1, posted: notice })).toEqual({ posted: notice, notices: [notice], unread: 1 });
    expect(parseNoticesEvent({ notices: [], unread: 0 })).toEqual({ posted: null, notices: [], unread: 0 });
  });

  it.each([
    ['not an object', null],
    ['a bad severity', { ...notice, severity: 'loud' }],
    ['a bad scope', { ...notice, scope: 'world' }],
    ['no read flag', { ...notice, read: 'no' }],
    ['a bad action', { ...notice, primary: { kind: 'x' } }],
    ['a bad code', { ...notice, code: 4 }],
    ['no count', { ...notice, count: undefined }],
    ['no id', { ...notice, id: 'one' }],
  ])('refuses a notice with %s', (_what, v) => {
    expect(parseNotice(v)).toBeNull();
  });

  it('refuses malformed lists and events', () => {
    expect(parseNoticesSnapshot(null)).toBeNull();
    expect(parseNoticesSnapshot({ notices: 'x', unread: 0 })).toBeNull();
    expect(parseNoticesSnapshot({ notices: [{}], unread: 0 })).toBeNull();
    expect(parseNoticesSnapshot({ notices: [], unread: 'x' })).toBeNull();
    expect(parseNoticesEvent({ notices: [], unread: 0, posted: { id: 1 } })).toBeNull();
    expect(parseNoticesEvent(null)).toBeNull();
  });
});
