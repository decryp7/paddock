import type { ApiFamily, ChosenBy } from '../../core/api.types';

export const API_LABEL: Record<ApiFamily, string> = {
  openai: 'OpenAI',
  anthropic: 'Anthropic',
  ollama: 'Ollama',
  extended: 'Pitwall /x',
};

export const CHOSEN_LABEL: Record<ChosenBy, string> = {
  explicit: 'Named',
  capabilities: 'Capabilities',
  default: 'Default',
};
