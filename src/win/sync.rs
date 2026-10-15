use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::Read;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::time::{Duration as StdDuration, SystemTime};

use chrono::{DateTime, Duration, Local, NaiveDate, Utc};
use skedaddle::config::{Config, FeedCfg};
use skedaddle::expand::{expand, FeedMeta};
use skedaddle::ical;
use skedaddle::model::{local_midnight_utc, Occurrence};
use skedaddle::tz::Zone;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

use super::{paths, WM_APP_SNAPSHOT};

pub enum Cmd {
    Refresh,
    Range(Option<(DateTime<Utc>, DateTime<Utc>)>),
    Quit,
}

#[derive(Clone)]
pub struct FeedStatus {
    pub last_ok: Option<DateTime<Utc>>,
    pub error: Option<String>,
}

pub struct Snapshot {
    pub occs: Arc<Vec<Occurrence>>,
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
    pub feeds: Vec<FeedStatus>,
    pub config: Arc<Config>,
    pub config_error: Option<String>,
}

#[derive(Default)]
struct FeedState {
    etag: Option<String>,
    last_modified: Option<String>,
    hash: u64,
    last_ok: Option<DateTime<Utc>>,
    error: Option<String>,
}

struct Worker {
    hwnd: isize,
    config: Arc<Config>,
    config_error: Option<String>,
    config_mtime: Option<Option<SystemTime>>,
    states: Vec<FeedState>,
    agent: Option<ureq::Agent>,
    range: Option<(DateTime<Utc>, DateTime<Utc>)>,
    occs: Arc<Vec<Occurrence>>,
    window: (DateTime<Utc>, DateTime<Utc>),
    built_day: Option<NaiveDate>,
}

pub fn spawn(hwnd: HWND, rx: Receiver<Cmd>) {
    let raw = hwnd.0 as isize;
    std::thread::Builder::new()
        .name("sync".into())
        .spawn(move || Worker::new(raw).run(rx))
        .expect("spawn sync thread");
}

fn fnv(s: &str) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn cache_file(feed: &FeedCfg) -> std::path::PathBuf {
    paths::cache_dir().join(format!("{:016x}.ics", fnv(&feed.url)))
}

impl Worker {
    fn new(hwnd: isize) -> Self {
        let now = Utc::now();
        Worker {
            hwnd,
            config: Arc::new(Config::default()),
            config_error: None,
            config_mtime: None,
            states: Vec::new(),
            agent: None,
            range: None,
            occs: Arc::new(Vec::new()),
            window: (now, now),
            built_day: None,
        }
    }

    fn run(mut self, rx: Receiver<Cmd>) {
        self.reload_config();
        self.load_cache_times();
        self.rebuild();
        self.post();
        loop {
            let mut changed = self.reload_config();
            changed |= self.fetch_all();
            if changed || self.built_day != Some(Local::now().date_naive()) {
                self.rebuild();
            }
            self.post();
            let failing = self.states.iter().any(|s| s.error.is_some());
            let wait = if failing { StdDuration::from_secs(60) } else { StdDuration::from_secs(skedaddle::config::POLL_MINUTES * 60) };
            let deadline = std::time::Instant::now() + wait;
            loop {
                let left = deadline.saturating_duration_since(std::time::Instant::now());
                match rx.recv_timeout(left) {
                    Ok(Cmd::Refresh) | Err(RecvTimeoutError::Timeout) => break,
                    Ok(Cmd::Range(r)) => {
                        self.range = r;
                        self.rebuild();
                        self.post();
                    }
                    Ok(Cmd::Quit) | Err(RecvTimeoutError::Disconnected) => return,
                }
            }
        }
    }

    fn reload_config(&mut self) -> bool {
        let path = paths::config_file();
        let mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        if self.config_mtime == Some(mtime) {
            return false;
        }
        self.config_mtime = Some(mtime);
        if mtime.is_none() {
            self.config_error = None;
            self.config = Arc::new(Config::default());
            self.states = Vec::new();
            return true;
        }
        match std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|t| Config::parse(&t)) {
            Ok(c) => {
                self.config_error = None;
                self.config = Arc::new(c);
                self.states = self.config.feed.iter().map(|_| FeedState::default()).collect();
                self.load_cache_times();
            }
            Err(e) => {
                self.config_error = Some(e);
            }
        }
        true
    }

    fn load_cache_times(&mut self) {
        for (i, f) in self.config.feed.iter().enumerate() {
            if let Some(st) = self.states.get_mut(i) {
                if st.last_ok.is_none() {
                    st.last_ok = std::fs::metadata(cache_file(f)).and_then(|m| m.modified()).ok().map(DateTime::<Utc>::from);
                }
            }
        }
    }

    fn agent(&mut self) -> Result<&ureq::Agent, String> {
        if self.agent.is_none() {
            let tls = native_tls::TlsConnector::new().map_err(|e| e.to_string())?;
            self.agent = Some(
                ureq::AgentBuilder::new()
                    .tls_connector(Arc::new(tls))
                    .timeout(StdDuration::from_secs(30))
                    .user_agent("skedaddle/0.1")
                    .build(),
            );
        }
        Ok(self.agent.as_ref().unwrap())
    }

    fn fetch_all(&mut self) -> bool {
        let mut changed = false;
        let feeds = self.config.feed.clone();
        for (i, feed) in feeds.iter().enumerate() {
            if feed.url.trim().is_empty() {
                continue;
            }
            let mut st = std::mem::take(&mut self.states[i]);
            match self.fetch(feed, &mut st) {
                Ok(c) => {
                    changed |= c;
                    st.last_ok = Some(Utc::now());
                    st.error = None;
                }
                Err(e) => {
                    let url = feed.url.trim();
                    st.error = Some(e.replace(url, "<feed url>").replace(&url.replacen("webcal://", "https://", 1), "<feed url>"));
                }
            }
            self.states[i] = st;
        }
        changed
    }

    fn fetch(&mut self, feed: &FeedCfg, st: &mut FeedState) -> Result<bool, String> {
        let url = feed.url.trim().replacen("webcal://", "https://", 1);
        let agent = self.agent()?.clone();
        let mut req = agent.get(&url);
        if let Some(e) = &st.etag {
            req = req.set("If-None-Match", e);
        }
        if let Some(m) = &st.last_modified {
            req = req.set("If-Modified-Since", m);
        }
        let resp = match req.call() {
            Ok(r) => r,
            Err(ureq::Error::Status(304, _)) => return Ok(false),
            Err(ureq::Error::Status(code, _)) => return Err(format!("HTTP {code}")),
            Err(e) => return Err(e.to_string()),
        };
        st.etag = resp.header("ETag").map(str::to_string);
        st.last_modified = resp.header("Last-Modified").map(str::to_string);
        let mut body = Vec::new();
        resp.into_reader().take(64 << 20).read_to_end(&mut body).map_err(|e| e.to_string())?;
        if !body.windows(15).any(|w| w == b"BEGIN:VCALENDAR") {
            return Err("response is not an iCalendar feed".into());
        }
        let mut h = DefaultHasher::new();
        body.hash(&mut h);
        let hash = h.finish();
        if hash == st.hash {
            return Ok(false);
        }
        let path = cache_file(feed);
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        std::fs::write(&path, &body).map_err(|e| e.to_string())?;
        st.hash = hash;
        Ok(true)
    }

    fn rebuild(&mut self) {
        let today = Local::now().date_naive();
        let mut from = local_midnight_utc(today - Duration::weeks(6));
        let mut to = local_midnight_utc(today + Duration::weeks(13));
        if let Some((a, b)) = self.range {
            from = from.min(a);
            to = to.max(b);
        }
        let mut all: Vec<Occurrence> = Vec::new();
        for (i, f) in self.config.feed.iter().enumerate() {
            let Ok(bytes) = std::fs::read(cache_file(f)) else { continue };
            let cal = ical::parse(&String::from_utf8_lossy(&bytes));
            let owner = f.owner();
            let meta = FeedMeta { index: i, owner_email: owner.as_deref() };
            all.extend(expand(&cal, &meta, &Zone::Local, from, to));
        }
        all.sort_by_key(|o| (o.start.timestamp(), o.end.timestamp()));
        self.occs = Arc::new(all);
        self.window = (from, to);
        self.built_day = Some(today);
    }

    fn post(&self) {
        let feeds = self
            .config
            .feed
            .iter()
            .zip(self.states.iter())
            .map(|(_, s)| FeedStatus { last_ok: s.last_ok, error: s.error.clone() })
            .collect();
        let snap = Box::new(Snapshot {
            occs: self.occs.clone(),
            from: self.window.0,
            to: self.window.1,
            feeds,
            config: self.config.clone(),
            config_error: self.config_error.clone(),
        });
        let ptr = Box::into_raw(snap);
        let ok = unsafe { PostMessageW(Some(HWND(self.hwnd as *mut _)), WM_APP_SNAPSHOT, WPARAM(0), LPARAM(ptr as isize)) };
        if ok.is_err() {
            drop(unsafe { Box::from_raw(ptr) });
        }
    }
}

impl Snapshot {
    pub fn is_stale(&self, now: DateTime<Utc>) -> bool {
        self.feeds.iter().any(|f| f.last_ok.is_none_or(|t| now - t > Duration::minutes(30)))
    }

    pub fn oldest_sync(&self) -> Option<DateTime<Utc>> {
        self.feeds.iter().filter_map(|f| f.last_ok).min()
    }
}
