import { MENU_MODE } from '@decryp7/paddock-ui/shell/menu';
import { menuModeProviders } from './menu-mode';

describe('menuModeProviders', () => {
  it('provides the native menu mode in the browser on ?menu=native', () => {
    expect(menuModeProviders(false, '?latency=0&menu=native')).toEqual([{ provide: MENU_MODE, useValue: 'native' }]);
  });
  it('provides nothing without the flag, with another value, or in the desktop app', () => {
    expect(menuModeProviders(false, '')).toEqual([]);
    expect(menuModeProviders(false, '?menu=in-app')).toEqual([]);
    expect(menuModeProviders(true, '?menu=native')).toEqual([]);
  });
});
