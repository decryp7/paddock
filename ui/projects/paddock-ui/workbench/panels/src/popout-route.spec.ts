import { parsePopoutRoute, popoutHash } from './popout-route';

describe('pop-out route', () => {
  it('round-trips a panel, including an instance key with special characters', () => {
    const route = parsePopoutRoute('#/popout?panel=pn.code%23src%2Fa%20b.rs&host=w-2&label=p-w-2-3')!;
    expect(route).toEqual({ panel: { panel: 'pn.code#src/a b.rs', view: 'pn.code', key: 'src/a b.rs' }, host: 'w-2', label: 'p-w-2-3' });
    expect(parsePopoutRoute(popoutHash(route))).toEqual(route);
    expect(popoutHash(route)).toBe('#/popout?panel=pn.code%23src%2Fa+b.rs&host=w-2&label=p-w-2-3');
  });

  it.each([
    null,
    undefined,
    '',
    '#/dialog?id=a.b&host=main&label=d-main-1',
    '#/popout',
    '#/popout?panel=code&host=main&label=p-main-1',
    '#/popout?panel=pn.code&host=p-main-1&label=p-main-1',
    '#/popout?panel=pn.code&host=main&label=p-w-2-1',
    '#/popout?panel=pn.code&host=main&label=d-main-1',
    '#/popout?panel=pn.code&host=x&label=p-x-1',
  ])('is not a pop-out route: %j', (hash) => {
    expect(parsePopoutRoute(hash)).toBeNull();
  });
});
