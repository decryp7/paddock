// C40: an app never listens to `paddock://jobs` itself. The shell's JobsStore (@decryp7/paddock-ui/shell/jobs) is the one
// listener of a window: it announces a job's end as a notice, and `JobFollow`/`JobsStore.follow` let a page or dialog wait
// for its own job. An app listener next to it announces the same end twice (the Spotter and Pitcrew shell moves both did).
// Only paddock-ui may name the event; the app waits through the shell's job follow.
import { error } from '../finding.mjs';

const EVENT = /paddock:\/\/jobs\b|\bJOBS_EVENT\b/;

export default {
  id: 'C40',
  title: 'Apps wait for jobs through the shell’s job follow, never a direct paddock://jobs listener',
  rule: 12,
  run(ctx) {
    const out = [];
    const root = `${ctx.config.appDir}/`;
    for (const f of ctx.files()) {
      if (!f.startsWith(root) || !/\.[mc]?tsx?$/.test(f) || /\.(?:spec|test)\.[mc]?tsx?$/.test(f) || f.includes('/node_modules/')) continue;
      const lines = (ctx.read(f) ?? '').split('\n');
      lines.forEach((line, i) => {
        if (!EVENT.test(line) || /^\s*(?:\/\/|\*|\/\*)/.test(line)) return;
        out.push(error('C40', 12, f, i + 1, 'the app listens to the shell’s jobs event (paddock://jobs) itself, so a job’s end is announced twice (its listener and the shell’s notice)', 'Start the work with JobFollow.run(...) / JobsStore.follow(id) from @decryp7/paddock-ui/shell/jobs and wait for its result; only paddock-ui names the event.'));
      });
    }
    return out;
  },
};
