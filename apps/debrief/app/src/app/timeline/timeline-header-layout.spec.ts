import { TestBed } from '@angular/core/testing';
import { provideTinyBackend, settle, tinyBackend } from '../../testing/helpers';
import { TimelinePanel } from './timeline-panel';

describe('TimelineHeader layout', () => {
  it('keeps a slot for the loading indicator, so the zoom buttons do not move when a load starts or ends', async () => {
    TestBed.configureTestingModule({ providers: [provideTinyBackend(tinyBackend())] });
    const fixture = TestBed.createComponent(TimelinePanel);
    await settle(fixture, 8);
    // Not loading: the slot is there (the header is full on a narrow pane, and the 22 px of a spinner and its gap that
    // appeared and went again moved the buttons between a mouse press and its release, so the click was lost).
    const root: HTMLElement = fixture.nativeElement;
    const slot = root.querySelector('.head .loading-slot');
    expect(slot).not.toBeNull();
    expect(slot?.querySelector('[role=status]')).toBeNull();
  });
});
