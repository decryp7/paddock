#!/usr/bin/env python3
"""Stop hook (ADVISORY, never blocks): when a subagent report names a recurring class and nothing followed, tell the
model through hookSpecificOutput.additionalContext (ER-21).
Reads the transcript backwards just far enough to reach the last human message."""
import json, os, re, sys

NAME = "recurring-guard"
CHUNK = 2 * 1024 * 1024
MAX_SCAN = int(os.environ.get("RECURRING_GUARD_MAX_SCAN", 96 * 1024 * 1024))
HUMAN = re.compile(rb'"kind"\s*:\s*"human"')
CAND = re.compile(rb'Subagent hand-back|<agent-message|"name"\s*:\s*"(?:AskUserQuestion|Agent|SendMessage)"')
LINE = re.compile(r"^\s*(?:[-*+]\s*)?(?:\*\*|__)?\s*Recurring\s*(?::\s*(?:\*\*|__)?|(?:\*\*|__)\s*:?)\s*(.*)$", re.I)
NONE = re.compile(r"^(none|no new|no\b|n/?a\b|nothing)", re.I)
FOLLOW = {"AskUserQuestion", "Agent", "SendMessage"}


class TooLarge(Exception):
    pass


def tail_from_last_human(path):
    """Bytes of the transcript from the last human message line to the end."""
    size = os.path.getsize(path)
    want = min(CHUNK, MAX_SCAN)
    with open(path, "rb") as f:
        while True:
            start = max(0, size - want)
            f.seek(start)
            data = f.read(size - start)
            for m in reversed(list(HUMAN.finditer(data))):
                ls = data.rfind(b"\n", 0, m.start()) + 1
                le = data.find(b"\n", m.end())
                line = data[ls:(len(data) if le < 0 else le)]
                try:
                    o = json.loads(line)
                except ValueError:
                    continue
                if o.get("type") == "user" and (o.get("origin") or {}).get("kind") == "human":
                    return data[ls:]
            if start == 0:
                return data
            if want >= MAX_SCAN:
                raise TooLarge("no human message in the last %d MB" % (MAX_SCAN >> 20))
            want *= 2


def text_of(o):
    if o.get("type") == "queue-operation":
        return o.get("content") if isinstance(o.get("content"), str) else ""
    c = (o.get("message") or {}).get("content")
    if isinstance(c, str):
        return c
    if isinstance(c, list):
        return "\n".join(b.get("text", "") for b in c if isinstance(b, dict) and b.get("type") == "text")
    return ""


def recurring_value(report):
    """The class named by the LAST `Recurring:` line outside code fences; '' when none."""
    lines, fenced, found = report.split("\n"), False, None
    for k, line in enumerate(lines):
        if line.strip().startswith("```"):
            fenced = not fenced
            continue
        if fenced:
            continue
        m = LINE.match(line)
        if m:
            val = m.group(1).strip()
            if not val:
                nxt = next((x.strip() for x in lines[k + 1:k + 4] if x.strip()), "")
                val = "" if nxt.startswith("```") else nxt
            found = val
    if found is None:
        return ""
    v = re.sub(r"^[\s*_`\"'(\[]+", "", found)
    if not v or NONE.match(v):
        return ""
    return found.strip("*_` ")


def decide(data):
    tp = data.get("transcript_path")
    if not tp or not os.path.isfile(tp):
        sys.stderr.write("%s: no readable transcript_path; allowing\n" % NAME)
        return None
    blob = tail_from_last_human(tp)
    events, seen = [], set()      # ("report", class) or ("follow",)
    for raw in blob.split(b"\n"):
        if not raw:
            continue
        if not (raw.startswith(b"{") and raw.rstrip().endswith(b"}")):
            sys.stderr.write("%s: a transcript line is not a JSON object; allowing\n" % NAME)
            return None
        if not CAND.search(raw):
            continue
        try:
            o = json.loads(raw)
        except ValueError:
            sys.stderr.write("%s: a transcript line did not parse; allowing\n" % NAME)
            return None
        t = o.get("type")
        if t == "assistant":
            for b in (o.get("message") or {}).get("content") or []:
                if isinstance(b, dict) and b.get("type") == "tool_use" and b.get("name") in FOLLOW:
                    events.append(("follow",))
        elif t in ("queue-operation", "user") and not (t == "user" and (o.get("origin") or {}).get("kind") == "human"):
            if t == "queue-operation" and o.get("operation") != "enqueue":
                continue
            txt = text_of(o)
            if "[Subagent hand-back]" not in txt and "<agent-message" not in txt:
                continue
            if txt in seen:
                continue
            seen.add(txt)
            events.append(("report", recurring_value(txt)))
    for k, e in enumerate(events):
        if e[0] == "report" and e[1]:
            if not any(x[0] == "follow" for x in events[k + 1:]):
                return ("Notice: a report named a recurring class (%s) — propose its fix now (ER-21): ask the user or start it "
                        "under an earlier approval." % e[1][:80])
    return None


def main():
    try:
        data = json.load(sys.stdin)
        if data.get("stop_hook_active") or data.get("agent_id"):
            return
        reason = decide(data)
    except TooLarge as e:
        sys.stderr.write("%s: transcript too large to scan (%s); allowing\n" % (NAME, e))
        return
    except Exception as e:  # never fail closed
        sys.stderr.write("%s: could not read the transcript (%s); allowing\n" % (NAME, e))
        return
    if reason:
        # Advisory: no `decision: block`; Claude Code shows this as Stop hook feedback, not as a hook error.
        print(json.dumps({"hookSpecificOutput": {"hookEventName": "Stop", "additionalContext": reason}}, ensure_ascii=False))


if __name__ == "__main__":
    main()
