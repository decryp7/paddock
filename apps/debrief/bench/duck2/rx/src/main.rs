//! Captures of the log4net-pattern parser's regex over N lines of a file: the shipped engine against alternatives.
use regex_automata::{dfa::onepass, nfa::thompson, util::captures::Captures as ACaps, Anchored, Input};
use std::time::Instant;
const LOG4NET: &str = r"^(?P<timestamp>\d{4}-\d{2}-\d{2}[ T]\d{2}:\d{2}:\d{2}(?:[.,]\d{1,9})?)\s+\[(?P<Thread>[^\]]+)\]\s+(?P<level>[A-Za-z]+)\s+(?P<Logger>\S+)\s+(?:\[(?P<Ndc>[^\]]*)\]\s+)?-\s(?P<message>.*)$";
fn main() {
    let path = std::env::args().nth(1).expect("file");
    let n: usize = std::env::args().nth(2).map_or(1_000_000, |v| v.parse().unwrap());
    let text = std::fs::read_to_string(&path).unwrap();
    let lines: Vec<&str> = text.lines().take(n).collect();
    let bytes: usize = lines.iter().map(|l| l.len()).sum();
    let report = |name: &str, t: f64, hits: usize, sum: usize| {
        println!("{name:44} {:7.0} ns/line {:6.0} MB/s  hits {hits} check {sum}", t * 1e9 / lines.len() as f64, bytes as f64 / t / 1e6)
    };
    let ascii = LOG4NET.replace(r"\d", "[0-9]").replace(r"\S", r"[^ \t]").replace(r"\s", r"[ \t]").replace("(?P<message>.*)", "(?P<message>(?s-u:.)*)");
    for (label, pat) in [("shipped (regex::Regex captures)", LOG4NET.to_string()), ("ASCII classes (regex::Regex captures)", ascii.clone())] {
        let re = regex::RegexBuilder::new(&pat).size_limit(8 << 20).dfa_size_limit(8 << 20).build();
        let Ok(re) = re else { println!("{label}: {}", re.unwrap_err()); continue };
        let mut caps = re.capture_locations();
        let t = Instant::now();
        let (mut hits, mut sum) = (0, 0);
        for l in &lines {
            if re.captures_read(&mut caps, l).is_some() {
                hits += 1;
                sum += caps.get(6).map_or(0, |(s, e)| e - s);
            }
        }
        report(label, t.elapsed().as_secs_f64(), hits, sum);
        let t = Instant::now();
        let hits = lines.iter().filter(|l| re.is_match(l)).count();
        report(&format!("{label}: is_match only"), t.elapsed().as_secs_f64(), hits, 0);
    }
    {
        // Tail split: a pattern ending in `(?P<message>.*)$` captures its prefix only; the group is the rest of the line.
        let prefix = LOG4NET.strip_suffix("(?P<message>.*)$").unwrap();
        let re = regex::RegexBuilder::new(prefix).size_limit(8 << 20).dfa_size_limit(8 << 20).build().unwrap();
        let mut caps = re.capture_locations();
        let t = Instant::now();
        let (mut hits, mut sum) = (0, 0);
        for l in &lines {
            if let Some(m) = re.captures_read(&mut caps, l) {
                hits += 1;
                sum += l.len() - m.end();
            }
        }
        report("tail split (prefix captures, rest = message)", t.elapsed().as_secs_f64(), hits, sum);
        let full = regex::Regex::new(LOG4NET).unwrap();
        let mut c2 = full.capture_locations();
        let mut diff = 0;
        for l in &lines {
            let a = full.captures_read(&mut c2, l).map(|_| (0..6).map(|g| c2.get(g)).collect::<Vec<_>>());
            let b = re.captures_read(&mut caps, l).map(|m| { let mut v: Vec<_> = (0..6).map(|g| caps.get(g)).collect(); v[0] = Some((0, l.len())); v });
            let mut a = a; if let Some(v) = a.as_mut() { v[0] = Some((0, l.len())); }
            if a != b { diff += 1; }
        }
        println!("tail split differs from the full pattern on {diff} lines");
    }
    for (label, pat) in [("one-pass DFA, shipped pattern", LOG4NET.to_string()), ("one-pass DFA, ASCII classes", ascii)] {
        let dfa = onepass::DFA::builder()
            .thompson(thompson::Config::new().utf8(false))
            .configure(onepass::Config::new().size_limit(Some(8 << 20)))
            .build(&pat);
        let dfa = match dfa { Ok(d) => d, Err(e) => { println!("{label}: does not build: {e}"); continue } };
        let mut cache = dfa.create_cache();
        let mut caps: ACaps = dfa.create_captures();
        let t = Instant::now();
        let (mut hits, mut sum) = (0, 0);
        for l in &lines {
            dfa.captures(&mut cache, Input::new(*l).anchored(Anchored::Yes), &mut caps);
            if caps.is_match() {
                hits += 1;
                sum += caps.get_group(6).map_or(0, |s| s.end - s.start);
            }
        }
        report(label, t.elapsed().as_secs_f64(), hits, sum);
    }
}
