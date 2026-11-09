/** Plausible data for the scripted AI mock: proposals and answers. */
import {
  ExtractorsProposal,
  MarkersProposal,
  PipelineProposal,
  ParserProposal,
  RegexProposal,
} from '../../ai/ai.types';
import { generateDemo } from './mock-data';
import { MOCK_PROFILE_TOML } from './mock-profile';

let demoIds: { error: number; fatal: number; warning: number } | null = null;

/** Ids of real records of the demo workspace (same seed as the mock backend), so citations jump somewhere. */
export function demoRecordIds(): { error: number; fatal: number; warning: number } {
  if (!demoIds) {
    // Ids as the mock engine assigns them: the position in time order (the demo profile drops nothing).
    const records = [...generateDemo().records].sort((a, b) => (a.ts ?? Number.MAX_SAFE_INTEGER) - (b.ts ?? Number.MAX_SAFE_INTEGER) || a.source - b.source || a.line - b.line);
    const first = (level: string, fallback: number) => {
      const i = records.findIndex((r) => r.level === level);
      return i >= 0 ? i : fallback;
    };
    demoIds = { error: first('Error', 1), fatal: first('Fatal', 2), warning: first('Warning', 3) };
  }
  return demoIds;
}

export function mockRegexProposal(lines: string[]): RegexProposal {
  const pattern = String.raw`^(?<timestamp>\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}(?:\.\d+)?) \[(?<level>\w+)\] (?<message>.*)$`;
  const re = new RegExp(pattern.replace(/\(\?<(\w+)>/g, '('));
  const matches = lines.map((l) => re.test(l));
  return {
    kind: 'regex',
    pattern,
    source: 'model',
    groups: [
      { name: 'timestamp', kind: 'timestamp', examples: ['2026-10-14 09:00:02'] },
      { name: 'level', kind: 'word', examples: ['INF', 'ERR'] },
      { name: 'message', kind: 'text', examples: ['Order 1182 accepted'] },
    ],
    matches,
    matchCount: matches.filter(Boolean).length,
    total: lines.length,
    explanation: [
      { depth: 0, text: 'Start of the line', start: 0, end: 1 },
      { depth: 0, text: 'Group "timestamp": a date and time', start: 1, end: 60 },
      { depth: 0, text: 'Group "level" in square brackets', start: 61, end: 75 },
      { depth: 0, text: 'Group "message": the rest of the line', start: 76, end: 92 },
    ],
    rationale: 'Each line starts with a timestamp, then the level in brackets, then the message.',
  };
}

/** The parser the mock AI proposes for the demo's worker log (invented). */
const MOCK_PROPOSED_PARSER = String.raw`schema = 1
id = "worker-text"
name = "Worker text logs"
version = "1.0.0"
description = "Plain text with an ISO timestamp and a bracketed level, proposed by the local model."
category = "Custom"

[match]
files = ["worker-*.log"]

[format]
kind = "text"
regexes = ['^(?P<timestamp>\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}(?:\.\d+)?(?: [+-]\d{2}:\d{2})?) \[(?P<level>[A-Z]{3})\] (?P<message>.*)$']
`;

export function mockParserProposal(): ParserProposal {
  return {
    kind: 'parser',
    name: 'Worker text logs',
    // A parser for the library (ADR 0026); empty when the library already reads the file.
    toml: MOCK_PROPOSED_PARSER,
    importer: 'text',
    parsed: 58,
    withTimestamp: 58,
    preview: [
      { line: 1, ts: Date.UTC(2026, 9, 14, 9, 0, 2), level: 'Information', message: 'Worker started', exception: null, fields: { Worker: 'sync' }, markers: ['start'] },
      { line: 2, ts: Date.UTC(2026, 9, 14, 9, 0, 3), level: 'Warning', message: 'Queue depth 812 above 500', exception: null, fields: {}, markers: ['warning'] },
    ],
    rationale: 'The sample is plain text with an ISO timestamp and a bracketed level.',
    // The engine's verdict and the rounds (ADR 0032): plausible invented numbers; Rust computes the real ones.
    status: 'done',
    verdict: {
      score: 0.995,
      lines: 64,
      readable: 63,
      events: 58,
      withTimestamp: 58,
      withLevel: 58,
      readsLevels: true,
      failures: [{ line: 41, fragment: '-- worker paused --', rule: 'format', reason: 'no line pattern reads this line' }],
      timestamps: { accepted: true, timed: 58, events: 58, backwards: 0, first: Date.UTC(2026, 9, 14, 9, 0, 2), last: Date.UTC(2026, 9, 14, 9, 14, 40), plausible: true, reason: null, suggestedFormat: null },
      zone: { offset: '+02:00', source: 'modification-time', confidence: 0.6, detail: "the last event's clock is +02:00 against the file's modification time (within 1 min)" },
      identity: [
        { field: 'Worker', coverage: 1, distinct: 3, accepted: true, reason: '3 value(s) on 100% of the events' },
        { field: 'RequestId', coverage: 1, distinct: 58, accepted: false, reason: '58 values in 58 events: too many to tell processes apart' },
      ],
      metrics: [{ name: 'QueueDepth', unit: 'count', matches: 12, plausible: 1, min: 40, max: 812, accepted: true, reason: '12 events, 40..812 ' }],
      error: null,
    },
    rounds: [
      { round: 1, score: 0.62, timestampsAccepted: false, failures: 8, note: '' },
      { round: 2, score: 0.995, timestampsAccepted: true, failures: 1, note: '' },
    ],
    chartsToml: '[[chart]]\nid = "metric-queuedepth"\ntitle = "QueueDepth per instance"\ny = "avg(QueueDepth)"\nbucket = "auto"\nsplit_by = ["@group"]\ntype = "line"\nunit = ""\n',
  };
}

export function mockPipelineProposal(goal: string): PipelineProposal {
  const toml = `[[pipeline]]\nstep = "drop"\nwhen = [{ field = "@message", op = "contains", value = "health check" }]\n`;
  return {
    kind: 'pipeline',
    toml,
    profileToml: `${MOCK_PROFILE_TOML}\n${toml}`,
    steps: [{ index: 0, step: 'drop: @message contains "health check"', input: 60, output: 52 }],
    sampleTotal: 60,
    kept: 52,
    dropped: 8,
    changed: 0,
    examples: [{ before: 'GET /health check ok in 2 ms', after: '(dropped)' }],
    rationale: `Health checks are noise for "${goal}"; they are dropped before analysis.`,
  };
}

export function mockMarkersProposal(goal: string): MarkersProposal {
  const toml = `[[marker]]\nkind = "timeout"\nlabel = "Timeout"\ncolor = "#E9AE3F"\nwhen = [{ field = "@message", op = "contains", value = "timed out" }]\n`;
  return {
    kind: 'markers',
    toml,
    profileToml: `${MOCK_PROFILE_TOML}\n${toml}`,
    markers: [{ kind: 'timeout', label: 'Timeout', matches: 6 }],
    sampled: 60,
    rationale: `Timeouts are what "${goal}" is about; they become markers on the timeline.`,
  };
}

export function mockExtractorsProposal(goal: string): ExtractorsProposal {
  const toml = `[[extract]]\nname = "elapsed"\nfrom = "@message"\nregex = 'in (?P<elapsed>\\d+) ms'\n[extract.fields]\nelapsed = { type = "duration", unit = "ms" }\n`;
  return {
    kind: 'extractors',
    toml,
    profileToml: `${MOCK_PROFILE_TOML}\n${toml}`,
    extractors: [{ name: 'elapsed', fields: ['elapsed'], matches: 31, examples: [{ elapsed: 120 }, { elapsed: 2300 }] }],
    sampled: 60,
    rationale: `Request durations answer "${goal}".`,
  };
}
