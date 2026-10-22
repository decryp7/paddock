//! Runs a real Chromium-family browser when one is installed (skipped cleanly otherwise) against a local server.
//! Profiles live in temp folders; nothing touches a user profile or a real host.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use spotter_render::discover::{find_installed, Browser};
use spotter_render::launch::{Chromium, EndedBy, LaunchOptions, Mode};
use spotter_render::process::processes_mentioning;
use spotter_render::{ChromiumRenderer, PageRenderer, RenderError};
use std::path::Path;
use std::time::{Duration, Instant};
use support::{serve, Reply};

const PAGE: &str = "<!doctype html><html><head><title>Rendered</title></head><body><div id=v>loading</div><ul id=r></ul>\
<script>setTimeout(()=>{document.getElementById('v').textContent='version 1.2.3';\
for(let i=1;i<=3;i++){const l=document.createElement('li');l.className='rel';l.textContent='item '+i;r.appendChild(l);}},400)</script></body></html>";

fn browser() -> Option<Browser> {
    let b = find_installed();
    if b.is_none() {
        eprintln!("skipped: no Chromium-based browser installed");
    }
    b
}

async fn site() -> u16 {
    serve(|p| match p {
        "/hang" => Reply::Hang,
        "/captcha" => Reply::Html("<title>Just a moment...</title><body>Checking your browser before accessing the site.</body>".into()),
        "/login" => Reply::Html(
            "<title>Sign in</title><body><h1>Sign in</h1><form>Email <input> Password <input type=password></form></body>".into(),
        ),
        "/redirect" => Reply::Html("<script>location.replace('/final')</script>".into()),
        "/final" => Reply::Html("<title>Final</title><p id=done>arrived</p>".into()),
        _ => Reply::Html(PAGE.into()),
    })
    .await
}

fn renderer(b: Browser, base: &Path) -> ChromiumRenderer {
    ChromiumRenderer::new(base, base).with_browser(b).with_mode(Mode::Headless)
}

/// No browser process mentions the folder any more (allowing a moment for the OS to reap).
fn assert_no_leftovers(marker: &str) {
    let end = Instant::now() + Duration::from_secs(10);
    while Instant::now() < end {
        if processes_mentioning(marker).is_empty() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("browser processes are left over for {marker}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn renders_a_scripted_page_and_leaves_nothing_behind() {
    let Some(b) = browser() else { return };
    let port = site().await;
    let base = tempfile::tempdir().unwrap();
    let marker = base.path().to_string_lossy().to_string();
    let r = renderer(b, base.path());
    let page = r
        .render(&format!("http://127.0.0.1:{port}/"), Some("li.rel"), Duration::from_secs(40))
        .await
        .unwrap();
    assert!(page.html.contains("version 1.2.3"), "script output missing: {}", page.html);
    assert!(page.html.contains("item 3"));
    assert_eq!(page.final_url, format!("http://127.0.0.1:{port}/"));
    assert_eq!(std::fs::read_dir(base.path()).unwrap().count(), 0, "throwaway profile not deleted");
    assert_no_leftovers(&marker);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn off_screen_headful_mode_and_redirects() {
    let Some(b) = browser() else { return };
    let port = site().await;
    let base = tempfile::tempdir().unwrap();
    let marker = base.path().to_string_lossy().to_string();
    let r = ChromiumRenderer::new(base.path(), base.path()).with_browser(b);
    let page = r
        .render(&format!("http://127.0.0.1:{port}/redirect"), Some("#done"), Duration::from_secs(40))
        .await
        .unwrap();
    assert_eq!(page.final_url, format!("http://127.0.0.1:{port}/final"));
    assert!(page.html.contains("arrived"));
    assert_no_leftovers(&marker);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn challenges_become_needs_you() {
    let Some(b) = browser() else { return };
    let port = site().await;
    let base = tempfile::tempdir().unwrap();
    let r = renderer(b, base.path());
    for path in ["captcha", "login"] {
        let e = r
            .render(&format!("http://127.0.0.1:{port}/{path}"), Some("#never"), Duration::from_secs(40))
            .await
            .unwrap_err();
        assert!(matches!(e, RenderError::NeedsYou(_)), "{path}: {e:?}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_missing_element_times_out_as_failed() {
    let Some(b) = browser() else { return };
    let port = site().await;
    let base = tempfile::tempdir().unwrap();
    let marker = base.path().to_string_lossy().to_string();
    let r = renderer(b, base.path());
    let e = r
        .render(&format!("http://127.0.0.1:{port}/"), Some("#absent"), Duration::from_secs(15))
        .await
        .unwrap_err();
    // On a loaded runner starting the browser can use up the whole budget before the wait begins; either way the
    // render fails (it is never reported as ready) and says why.
    let expected = |m: &str| m.contains("did not appear") || m.contains("did not finish in time");
    assert!(matches!(&e, RenderError::Failed(m) if expected(m)), "{e:?}");
    assert_no_leftovers(&marker);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_server_that_never_answers_is_stopped_at_the_timeout() {
    let Some(b) = browser() else { return };
    let port = site().await;
    let base = tempfile::tempdir().unwrap();
    let marker = base.path().to_string_lossy().to_string();
    let r = renderer(b, base.path());
    let t = Instant::now();
    let e = r
        .render(&format!("http://127.0.0.1:{port}/hang"), None, Duration::from_secs(6))
        .await
        .unwrap_err();
    assert!(matches!(e, RenderError::Failed(_)), "{e:?}");
    // Start-up has its own limit; the hang itself must end at the 6 s page budget, not at the watchdog.
    assert!(
        t.elapsed() < spotter_render::launch::lifetime_for(Duration::from_secs(6)),
        "took {:?}",
        t.elapsed()
    );
    assert_eq!(std::fs::read_dir(base.path()).unwrap().count(), 0);
    assert_no_leftovers(&marker);
}

#[test]
fn the_watchdog_kills_the_whole_tree_at_the_wall_clock_limit() {
    let Some(b) = browser() else { return };
    let base = tempfile::tempdir().unwrap();
    let mut opts = LaunchOptions::new(base.path());
    opts.mode = Mode::Headless;
    opts.max_lifetime = Duration::from_secs(4);
    let mut c = Chromium::launch(&b, &opts).unwrap();
    let dir = c.profile_dir().to_path_buf();
    assert!(c.is_running());
    let marker = dir.to_string_lossy().to_string();
    assert!(!processes_mentioning(&marker).is_empty());
    let end = Instant::now() + Duration::from_secs(20);
    while c.is_running() && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(!c.is_running(), "still running after the limit");
    assert_eq!(c.ended_by(), EndedBy::TimeLimit);
    assert_no_leftovers(&marker);
    drop(c);
    assert!(!dir.exists(), "profile not removed");
}

#[test]
fn the_memory_cap_kills_the_tree() {
    let Some(b) = browser() else { return };
    let base = tempfile::tempdir().unwrap();
    let mut opts = LaunchOptions::new(base.path());
    opts.mode = Mode::Headless;
    opts.memory_cap = Some(1);
    let marker = base.path().to_string_lossy().to_string();
    // The cap also applies while the browser starts: it may be stopped before the connection is up, or after.
    if let Ok(mut c) = Chromium::launch(&b, &opts) {
        let end = Instant::now() + Duration::from_secs(20);
        while c.is_running() && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(100));
        }
        assert!(!c.is_running());
        assert_eq!(c.ended_by(), EndedBy::MemoryCap);
    }
    assert_no_leftovers(&marker);
}

#[test]
fn dropping_a_running_browser_kills_it_and_removes_the_profile() {
    let Some(b) = browser() else { return };
    let base = tempfile::tempdir().unwrap();
    let mut opts = LaunchOptions::new(base.path());
    opts.mode = Mode::Headless;
    let c = Chromium::launch(&b, &opts).unwrap();
    let dir = c.profile_dir().to_path_buf();
    let marker = dir.to_string_lossy().to_string();
    drop(c);
    assert!(!dir.exists());
    assert_no_leftovers(&marker);
}
