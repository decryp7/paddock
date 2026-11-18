//! One-off measurement for paddock-git (never merged): the same reads three ways on a synthetic repository made with one
//! `git fast-import` stream.
//!
//! - `per-call`: one git process per read (`git ls-tree -r -z <commit>`, `git cat-file blob <oid>`), as paddock-dev does today;
//! - `batch`: one long-lived `git cat-file --batch` process, trees parsed in process;
//! - `gix`: gix in process, no git process at all.
//!
//! Usage: `git-read-spike [--files N] [--commits C] [--blobs B] [--reps R] [--dir D]`. Prints one line per variant and rep, then a
//! summary table (median ms per variant) as Markdown rows.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::Instant;

fn git(dir: &Path) -> Command {
    let mut c = Command::new("git");
    c.arg("-C")
        .arg(dir)
        .args(["-c", "core.fsmonitor=false", "-c", "core.quotePath=false"])
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env(
            "GIT_CONFIG_GLOBAL",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        );
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    c
}

fn out(mut c: Command) -> Vec<u8> {
    let o = c.output().expect("git starts");
    assert!(
        o.status.success(),
        "{c:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    o.stdout
}

/// A repository of `files` files in 40 folders and `commits` commits (each later commit changes 20 files), in one fast-import.
fn make_repo(dir: &Path, files: usize, commits: usize) -> Vec<String> {
    std::fs::create_dir_all(dir).unwrap();
    out({
        let mut c = git(dir);
        c.args(["init", "-q", "-b", "main"]);
        c
    });
    let mut s = String::new();
    for k in 0..commits {
        s.push_str(&format!(
            "commit refs/heads/main\ncommitter T <t@example.invalid> {} +0000\ndata 3\nc{:<1}\n",
            1_700_000_000 + k,
            k % 10
        ));
        let changed: Vec<usize> = if k == 0 {
            (0..files).collect()
        } else {
            (0..20).map(|j| (k * 37 + j * 101) % files).collect()
        };
        for f in changed {
            let body = format!(
                "file {f} rev {k}\n{}\n",
                "line of text ".repeat(20 + f % 30)
            );
            s.push_str(&format!(
                "M 100644 inline d{}/sub{}/f{f}.txt\ndata {}\n{body}\n",
                f % 40,
                f % 3,
                body.len()
            ));
        }
    }
    let mut c = git(dir);
    c.args(["fast-import", "--quiet"]).stdin(Stdio::piped());
    let mut child = c.spawn().unwrap();
    child.stdin.take().unwrap().write_all(s.as_bytes()).unwrap();
    assert!(child.wait().unwrap().success());
    String::from_utf8(out({
        let mut c = git(dir);
        c.args(["rev-list", "main"]);
        c
    }))
    .unwrap()
    .lines()
    .map(str::to_string)
    .collect()
}

/// Result of one variant: files listed over all commits, bytes of blobs read.
#[derive(Debug, PartialEq, Eq)]
struct Got {
    entries: usize,
    bytes: usize,
}

fn per_call(dir: &Path, commits: &[String], blobs: &[String]) -> Got {
    let mut entries = 0;
    for c in commits {
        let o = out({
            let mut g = git(dir);
            g.args(["ls-tree", "-r", "-z", c]);
            g
        });
        entries += o.split(|b| *b == 0).filter(|e| !e.is_empty()).count();
    }
    let mut bytes = 0;
    for b in blobs {
        bytes += out({
            let mut g = git(dir);
            g.args(["cat-file", "blob", b]);
            g
        })
        .len();
    }
    Got { entries, bytes }
}

struct Batch {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}

impl Batch {
    fn new(dir: &Path) -> Batch {
        let mut c = git(dir);
        c.args(["cat-file", "--batch"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped());
        let mut child = c.spawn().unwrap();
        let input = child.stdin.take().unwrap();
        let output = BufReader::new(child.stdout.take().unwrap());
        Batch {
            child,
            input,
            output,
        }
    }

    /// (type, content) of `spec`.
    fn get(&mut self, spec: &str) -> (String, Vec<u8>) {
        writeln!(self.input, "{spec}").unwrap();
        self.input.flush().unwrap();
        let mut header = String::new();
        self.output.read_line(&mut header).unwrap();
        let parts: Vec<&str> = header.trim_end().split(' ').collect();
        assert_eq!(parts.len(), 3, "{header}");
        let size: usize = parts[2].parse().unwrap();
        let mut data = vec![0; size + 1];
        self.output.read_exact(&mut data).unwrap();
        data.pop();
        (parts[1].to_string(), data)
    }

    /// Files under the tree `spec`, recursively (sha1 trees: "<mode> <name>\0<20 bytes>").
    fn walk(&mut self, spec: &str) -> usize {
        let (kind, data) = self.get(spec);
        assert_eq!(kind, "tree");
        let mut n = 0;
        let mut i = 0;
        let mut subtrees = Vec::new();
        while i < data.len() {
            let sp = data[i..].iter().position(|b| *b == b' ').unwrap() + i;
            let nul = data[sp..].iter().position(|b| *b == 0).unwrap() + sp;
            let oid: String = data[nul + 1..nul + 21]
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            if &data[i..sp] == b"40000" {
                subtrees.push(oid);
            } else {
                n += 1;
            }
            i = nul + 21;
        }
        for t in subtrees {
            n += self.walk(&t);
        }
        n
    }
}

impl Drop for Batch {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn batch(dir: &Path, commits: &[String], blobs: &[String]) -> Got {
    let mut b = Batch::new(dir);
    let entries = commits
        .iter()
        .map(|c| b.walk(&format!("{c}^{{tree}}")))
        .sum();
    let bytes = blobs.iter().map(|o| b.get(o).1.len()).sum();
    Got { entries, bytes }
}

fn with_gix(dir: &Path, commits: &[String], blobs: &[String]) -> Got {
    let repo = gix::open(dir).unwrap();
    fn walk(repo: &gix::Repository, tree: gix::ObjectId) -> usize {
        let t = repo.find_object(tree).unwrap().into_tree();
        let mut n = 0;
        let mut subtrees = Vec::new();
        for e in t.iter() {
            let e = e.unwrap();
            if e.mode().is_tree() {
                subtrees.push(e.oid().to_owned());
            } else {
                n += 1;
            }
        }
        for s in subtrees {
            n += walk(repo, s);
        }
        n
    }
    let mut entries = 0;
    for c in commits {
        let id = gix::ObjectId::from_hex(c.as_bytes()).unwrap();
        let tree = repo
            .find_object(id)
            .unwrap()
            .into_commit()
            .tree_id()
            .unwrap()
            .detach();
        entries += walk(&repo, tree);
    }
    let mut bytes = 0;
    for o in blobs {
        bytes += repo
            .find_object(gix::ObjectId::from_hex(o.as_bytes()).unwrap())
            .unwrap()
            .data
            .len();
    }
    Got { entries, bytes }
}

fn arg(args: &[String], name: &str, default: usize) -> usize {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let files = arg(&args, "--files", 2000);
    let n_commits = arg(&args, "--commits", 20);
    let n_blobs = arg(&args, "--blobs", 300);
    let reps = arg(&args, "--reps", 3);
    let base = args
        .iter()
        .position(|a| a == "--dir")
        .and_then(|i| args.get(i + 1))
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let dir = base.join(format!("git-read-spike-{}", std::process::id()));
    let t = Instant::now();
    let commits = make_repo(&dir, files, n_commits);
    println!(
        "repo: {files} files, {} commits, made in {:.0} ms (one fast-import)",
        commits.len(),
        t.elapsed().as_secs_f64() * 1000.0
    );
    let listing = out({
        let mut g = git(&dir);
        g.args(["ls-tree", "-r", "-z", &commits[0]]);
        g
    });
    let blobs: Vec<String> = listing
        .split(|b| *b == 0)
        .filter(|e| !e.is_empty())
        .filter_map(|e| {
            std::str::from_utf8(e)
                .ok()?
                .split(' ')
                .nth(2)
                .map(|o| o[..40].to_string())
        })
        .take(n_blobs)
        .collect();
    type Variant = fn(&Path, &[String], &[String]) -> Got;
    let variants: [(&str, Variant); 3] =
        [("per-call", per_call), ("batch", batch), ("gix", with_gix)];
    let mut times: Vec<Vec<f64>> = vec![Vec::new(); variants.len()];
    let mut expected = None;
    for r in 0..reps {
        for k in 0..variants.len() {
            let v = (k + r) % variants.len();
            let t = Instant::now();
            let got = (variants[v].1)(&dir, &commits, &blobs);
            let ms = t.elapsed().as_secs_f64() * 1000.0;
            println!("rep {r} {}: {ms:.1} ms ({got:?})", variants[v].0);
            match &expected {
                None => expected = Some(got),
                Some(e) => assert_eq!(e, &got, "{} disagrees", variants[v].0),
            }
            times[v].push(ms);
        }
    }
    println!("| variant | median ms | min ms | reads |");
    println!("|---|---:|---:|---:|");
    for (k, (name, _)) in variants.iter().enumerate() {
        let mut v = times[k].clone();
        v.sort_by(f64::total_cmp);
        println!(
            "| {name} | {:.1} | {:.1} | {} trees listed + {} blobs |",
            v[v.len() / 2],
            v[0],
            commits.len(),
            blobs.len()
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}
