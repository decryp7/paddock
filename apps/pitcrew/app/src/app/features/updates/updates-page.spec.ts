import { TestBed } from '@angular/core/testing';
import { MockBackend } from '../../core/mock-backend';
import { appProviders, makeMock, settle, text } from '../../../testing/test-helpers';
import { UpdatesPage } from './updates-page';

describe('UpdatesPage', () => {
  let mock: MockBackend;
  afterEach(() => mock.dispose());

  async function mount() {
    localStorage.clear();
    mock = makeMock();
    TestBed.configureTestingModule({ providers: appProviders(mock) });
    const f = TestBed.createComponent(UpdatesPage);
    await settle(f, 8);
    return { f, el: f.nativeElement as HTMLElement };
  }

  it('lists every runner with its target as a neutral pill, never an accent badge', async () => {
    const { el } = await mount();
    expect(el.querySelectorAll('article.runner').length).toBe(4);
    expect(el.querySelector('.badge.update')?.textContent).toContain('5.3.0');
    expect(el.querySelector('.badge.accent')).toBeNull();
  });

  it('counts down to the owner\'s own deadline and explains that no server enforces one', async () => {
    const { el } = await mount();
    const ui = Array.from(el.querySelectorAll('article.runner')).find((a) => text(a).includes('ui-tests'))!;
    expect(text(ui)).toContain('Due in 4 days');
    expect(text(ui)).toContain('Neither Gitea nor Forgejo enforces a deadline');
    expect(ui.querySelector('.badge.warning')).not.toBeNull();
  });

  it('shows an update loop with its cause', async () => {
    const { el } = await mount();
    const docker = Array.from(el.querySelectorAll('article.runner')).find((a) => text(a).includes('docker-lab'))!;
    expect(text(docker.querySelector('.loop'))).toContain('Updating to 5.3.0 keeps failing');
    expect(text(docker.querySelector('.loop'))).toContain('checksums');
  });

  it('words overdue and due-today deadlines', async () => {
    const { f } = await mount();
    const page = f.componentInstance as unknown as { deadline(r: unknown): { text: string; warn: boolean } | null };
    const base = { updateTarget: '5.3.0' };
    const d = (daysLeft: number, state: string) => ({ ...base, deadline: { behindSince: 0, dueAt: 0, daysLeft, state } });
    expect(page.deadline(d(-3, 'overdue'))).toEqual({ text: 'Overdue by 3 days', warn: true });
    expect(page.deadline(d(0, 'soon'))?.text).toBe('Due today');
    expect(page.deadline(d(20, 'ok'))).toEqual({ text: 'Due in 20 days', warn: false });
    expect(page.deadline({ ...base, deadline: null })).toBeNull();
    expect(page.deadline({ updateTarget: null, deadline: { behindSince: 0, dueAt: 0, daysLeft: 1, state: 'soon' } })).toBeNull();
  });
});
