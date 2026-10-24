//! Output of child processes (sidecars, runners, elevated helpers) into the log. A child's standard error is read line by
//! line with a bound, so a child that prints a gigabyte without a newline cannot exhaust memory, and every line becomes
//! an event with `Child = <name>` and the child's `SourceContext`. The daily budget caps the volume.

use std::io::{BufRead, BufReader, Read};

/// Longest child line kept (bytes); the rest of a longer line is dropped and the event says `Truncated = true`.
pub const MAX_CHILD_LINE: usize = 4096;
/// Longest child name (characters).
const MAX_CHILD_NAME: usize = 64;

/// Reads `reader` to its end and logs each line at Info as `child:<name>`; returns the number of lines logged.
/// Blocks: run it on its own thread (see [`spawn_child_capture`]). Invalid UTF-8 is replaced, not an error.
pub fn pump_child_lines(name: &str, reader: impl Read) -> u64 {
    let name: String = name.chars().take(MAX_CHILD_NAME).collect();
    let target = format!("child:{name}");
    let mut reader = BufReader::new(reader);
    let mut lines = 0;
    let mut buf: Vec<u8> = Vec::new();
    loop {
        buf.clear();
        let mut truncated = false;
        let mut got_any = false;
        loop {
            let (done, used) = match reader.fill_buf() {
                Ok([]) => break,
                Ok(chunk) => {
                    got_any = true;
                    match chunk.iter().position(|b| *b == b'\n') {
                        Some(i) => {
                            keep(&mut buf, &chunk[..i], &mut truncated);
                            (true, i + 1)
                        }
                        None => {
                            keep(&mut buf, chunk, &mut truncated);
                            (false, chunk.len())
                        }
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            };
            reader.consume(used);
            if done {
                break;
            }
        }
        if !got_any {
            return lines;
        }
        let text = String::from_utf8_lossy(&buf);
        let text = text.trim_end_matches('\r');
        tracing::info!(target: "child", Child = name.as_str(), Truncated = truncated, SourceContext = target.as_str(), "{}", text);
        lines += 1;
    }
}

fn keep(buf: &mut Vec<u8>, part: &[u8], truncated: &mut bool) {
    let room = MAX_CHILD_LINE.saturating_sub(buf.len());
    if part.len() > room {
        *truncated = true;
    }
    buf.extend_from_slice(&part[..part.len().min(room)]);
}

/// Starts a thread named `log-child-<name>` that runs [`pump_child_lines`]; join it to get the line count.
pub fn spawn_child_capture(
    name: &str,
    reader: impl Read + Send + 'static,
) -> std::io::Result<std::thread::JoinHandle<u64>> {
    let name = name.to_string();
    std::thread::Builder::new()
        .name(format!("log-child-{name}"))
        .spawn(move || pump_child_lines(&name, reader))
}
