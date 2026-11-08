//! A scripted run of the IP lists, the map and the firewall against a REAL running `dyno-agent` (not the mock), for a disposable VM
//! (ER-14): `real_run prepare <data dir>` stages a tiny location database for a host the VM can reach; `real_run check <data dir>`
//! talks to the running agent over its real IPC and prints what it answers as one JSON line per step. It never contacts the
//! publishers of any list or database: both network switches are turned off first.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::print_stdout, clippy::print_stderr)]

use dyno_agent::cli::{ask, ask_within, connect};
use dyno_agent::geo_db::testdb::sample_country;
use dyno_agent::geo_fetch::civil;
use dyno_agent::paths::Dirs;
use dyno_core::intel_lists::{IntelEdit, NetSwitches, NewSource};
use dyno_core::network::NetworkQuery;
use dyno_core::protocol::{Reply, Request};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const HOST: &str = "example.com:443";

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
}

fn show(step: &str, r: &Result<Reply, String>) {
    match r {
        Ok(reply) => println!("{}", serde_json::json!({ "step": step, "reply": reply })),
        Err(e) => println!("{}", serde_json::json!({ "step": step, "error": e })),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (cmd, dir) = (args.get(1).map(String::as_str).unwrap_or(""), args.get(2).expect("data dir"));
    let dirs = Dirs::at(dir);
    match cmd {
        "prepare" => {
            // A location database that puts the /24 of the test host in "Testland", written before the agent starts.
            let addr = HOST
                .to_socket_addrs()
                .expect("resolve")
                .find(|a| a.is_ipv4())
                .expect("an IPv4 address")
                .ip();
            let std::net::IpAddr::V4(v4) = addr else { unreachable!() };
            let o = v4.octets();
            let base = std::net::IpAddr::from([o[0], o[1], o[2], 0]);
            let built = now_secs() - 3600;
            let bytes = dyno_agent::geo_db::testdb::build(
                &dyno_agent::geo_db::testdb::Meta {
                    database_type: "DBIP-Country-Lite".into(),
                    build_epoch: built,
                    ip_version: 6,
                },
                &[(base, 24, dyno_agent::geo_db::testdb::country("XA", "Testland"))],
            );
            let _ = sample_country; // the fixed sample is for the unit tests
            let (y, m, _) = civil(now_secs());
            let cache = dirs.intel_cache("geo-country");
            std::fs::create_dir_all(&cache).unwrap();
            std::fs::write(cache.join(format!("current-{y:04}-{m:02}.mmdb")), bytes).unwrap();
            std::fs::write(dir.to_string() + "/host.txt", addr.to_string()).unwrap();
            println!(
                "{}",
                serde_json::json!({ "step": "prepare", "host": HOST, "range": format!("{base}/24") })
            );
        }
        "check" => {
            let (peer, _) = connect(&dirs).expect("the agent is running");
            let go = |step: &str, request: Request| {
                let started = Instant::now();
                let r = ask_within(&peer, request, Duration::from_secs(20));
                eprintln!("{step}: {} ms", started.elapsed().as_millis());
                show(step, &r);
                r
            };
            // Nothing may reach a publisher: both switches off first.
            go(
                "switches-off",
                Request::IntelEdit {
                    edit: IntelEdit::SetSwitches {
                        switches: NetSwitches {
                            location_databases: false,
                            own_location: false,
                            first_run_note_seen: true,
                        },
                    },
                },
            )
            .unwrap();
            go("status-at-start", Request::IntelStatus).unwrap();

            // Hold a real connection to the test host, then list a range that contains it.
            let target: std::net::IpAddr = std::fs::read_to_string(dir.to_string() + "/host.txt")
                .unwrap()
                .trim()
                .parse()
                .unwrap();
            let _held = TcpStream::connect_timeout(&std::net::SocketAddr::new(target, 443), Duration::from_secs(10))
                .map_err(|e| eprintln!("no public connection could be opened: {e}"))
                .ok();
            // The range is the one of the connection that is actually held (a name can answer with different addresses each time).
            let host_ip = _held.as_ref().expect("a connection is held").peer_addr().unwrap().ip();
            let std::net::IpAddr::V4(v4) = host_ip else { unreachable!() };
            let o = v4.octets();
            let range = format!("{}.{}.{}.0/24 # the test host's range", o[0], o[1], o[2]);
            let edit = |e: IntelEdit| go("edit", Request::IntelEdit { edit: e });
            edit(IntelEdit::AddList {
                name: "Work".into(),
                source: NewSource::Manual {
                    text: format!("{range}\n8.8.8.8\n8.8.8.8\nevil.example"),
                },
            })
            .unwrap();
            edit(IntelEdit::DedupePreview { id: "list-1".into() }).unwrap();
            edit(IntelEdit::SetAllow {
                text: "9.9.9.9 # mine".into(),
            })
            .unwrap();
            // A local plain-http address is refused at the door; a local https one is accepted as an address but never reached.
            let _ = edit(IntelEdit::AddList {
                name: "Local http".into(),
                source: NewSource::Url {
                    url: "http://127.0.0.1:8099/list.txt".into(),
                    every_hours: None,
                },
            });
            let added = edit(IntelEdit::AddList {
                name: "Local https".into(),
                source: NewSource::Url {
                    url: "https://127.0.0.1:8099/list.txt".into(),
                    every_hours: None,
                },
            });
            if added.is_ok() {
                // The agent starts the first fetch itself; wait for the job to end and read the page again.
                for _ in 0..40 {
                    let jobs = ask(&peer, Request::Jobs);
                    let active = matches!(&jobs, Ok(Reply::Jobs(j)) if j.iter().any(|s| !s.is_terminal()));
                    if !active {
                        show("jobs", &jobs);
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(250));
                }
            }
            std::thread::sleep(Duration::from_secs(2)); // the rebuild job after the edits
            go("status-after-edits", Request::IntelStatus).unwrap();
            go("geo-summary", Request::GeoSummary).unwrap();
            go(
                "network-filtered-to-XA",
                Request::Network {
                    query: NetworkQuery {
                        show_loopback: true,
                        country: Some("XA".into()),
                        ..Default::default()
                    },
                },
            )
            .unwrap();
            go("firewall", Request::Firewall).ok();
        }
        other => {
            eprintln!("usage: real_run prepare|check <data dir> (got {other:?})");
            std::process::exit(2);
        }
    }
}
