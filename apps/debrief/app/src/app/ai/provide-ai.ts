import { EnvironmentProviders, inject, makeEnvironmentProviders } from '@angular/core';
import { DEBRIEF_RECORD_MENU_ITEMS, RecordMenuContribution } from '../context/record-actions';
import { DebriefView } from '../extensions/tokens';
import { UI_STATE_CONTRIBUTIONS } from '../lifecycle/ui-state';
import { aiActivityItems } from './ai-activity';
import { AI_CHAT_VIEW } from './ai.ids';
import { AI_COMMANDS_CONTRIBUTIONS } from './ai.contributions';
import { AiBackend, provideAiBackend } from './ai-backend';
import { AiChatPersistence } from './ai-chat-persistence';
import { KnowledgePersistence } from './knowledge/knowledge-persistence';
import { AI_IMAGE_VIEW, AiLauncher } from './ai-launcher';
import { AiStatusStore } from './ai-status.store';
import { contributeFrom } from '../extensions/contributions';

export { AI_CHAT_VIEW };

/** The chat, a tool window offered only while AI can be used in this run. */
export function aiViews(status: InstanceType<typeof AiStatusStore>): DebriefView[] {
  return [
    {
      id: AI_CHAT_VIEW,
      title: $localize`:@@ai.provide.view.title:AI assistant`,
      category: 'AI',
      icon: 'puzzle',
      defaultPosition: 'right',
      layout: { area: 'side', min: { width: 320, height: 240 }, pref: { width: 372, height: 400 }, priority: 70 },
      component: () => import('./chat/chat-panel').then((m) => m.AiChatPanel),
      enabled: () => status.available(),
    },
    {
      id: AI_IMAGE_VIEW,
      title: $localize`:@@ai.vision.view.title:Image`,
      category: 'AI',
      icon: 'image',
      defaultPosition: 'right',
      layout: { area: 'side', min: { width: 320, height: 240 }, pref: { width: 372, height: 400 }, priority: 45 },
      component: () => import('./vision/image-panel').then((m) => m.AiImagePanel),
      enabled: () => status.available(),
    },
  ];
}

/** "Explain this event" and "Suggest regex" in the grid and inspector context menu. */
export function aiRecordMenu(launcher: AiLauncher, status: InstanceType<typeof AiStatusStore>): RecordMenuContribution {
  return (record, target) => {
    if (!status.available()) return [];
    const lines = (target.records?.length ? target.records : [record]).map((r) => r.message);
    return [
      { label: $localize`:@@ai.provide.menu.explain:Explain this event (AI)…`, run: () => void launcher.explainEvent(record.id) },
      { label: $localize`:@@ai.provide.menu.regex:Suggest a regex for these lines (AI)…`, run: () => void launcher.suggestRegex(lines, { highlight: true }) },
    ];
  };
}

/** One line in `app.config.ts`: the AI chat view, its commands and menu items, saved chats, the backend. */
export function provideAi(): EnvironmentProviders {
  return makeEnvironmentProviders([
    ...provideAiBackend(),
    ...contributeFrom('views', () => aiViews(inject(AiStatusStore))),
    ...contributeFrom('activityItems', () => aiActivityItems(inject(AiStatusStore), AI_CHAT_VIEW)),
    // Stubs only: the commands' code loads when the window is idle or one runs.
    ...AI_COMMANDS_CONTRIBUTIONS,
    { provide: DEBRIEF_RECORD_MENU_ITEMS, multi: true, useFactory: () => aiRecordMenu(inject(AiLauncher), inject(AiStatusStore)) },
    { provide: UI_STATE_CONTRIBUTIONS, multi: true, useFactory: () => [inject(AiChatPersistence).contribution, inject(KnowledgePersistence).contribution] },
  ]);
}

export { AiBackend };
