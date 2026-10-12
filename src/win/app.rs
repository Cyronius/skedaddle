use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::mpsc;
use std::sync::Arc;

use chrono::{Duration, Local, Utc};
use skedaddle::alerts;
use skedaddle::config::{parse_color, TEMPLATE};
use skedaddle::model::{fmt_in, fmt_time, AlertKey, Occurrence, Rsvp};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS, HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_FILENAME, SND_NODEFAULT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Diagnostics::Debug::MessageBeep;
use windows::Win32::System::Threading::{CreateMutexW, GetCurrentProcess, SetProcessWorkingSetSize};
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::WindowsAndMessaging::*;

use super::sync::{self, Cmd, Snapshot};
use super::{autostart, flash, gdi, icon, modal, paths, shared, tray, view, wide};
use super::{WM_APP_MODAL_CLOSED, WM_APP_SHOW, WM_APP_SNAPSHOT, WM_APP_TRAY};

const ID_ALERT: usize = 1;
const ID_TICK: usize = 2;
const ID_REPEAT: usize = 3;
const ID_TRIM: usize = 4;

const MENU_OPEN: usize = 1;
const MENU_REFRESH: usize = 2;
const MENU_TEST: usize = 3;
const MENU_CONFIG: usize = 4;
const MENU_AUTOSTART: usize = 5;
const MENU_QUIT: usize = 6;

struct App {
    seen: HashSet<AlertKey>,
    icon_ok: HICON,
    icon_warn: HICON,
    stale: bool,
    tip: String,
    taskbar_created: u32,
    first_snapshot: bool,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|a| a.borrow_mut().as_mut().map(f))
}

pub fn trim_memory() {
    unsafe {
        let _ = SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX);
    }
}

fn load_seen() -> HashSet<AlertKey> {
    let cutoff = (Utc::now() - Duration::days(2)).timestamp();
    let text = std::fs::read_to_string(paths::alerted_file()).unwrap_or_default();
    text.lines()
        .filter_map(|l| {
            let mut p = l.splitn(3, '\t');
            let feed = p.next()?.parse().ok()?;
            let ts: i64 = p.next()?.parse().ok()?;
            let uid = p.next()?.to_string();
            (ts > cutoff).then_some((feed, uid, ts))
        })
        .collect()
}

fn save_seen(seen: &HashSet<AlertKey>) {
    let cutoff = (Utc::now() - Duration::days(2)).timestamp();
    let body: String = seen.iter().filter(|k| k.2 > cutoff).map(|(f, u, t)| format!("{f}\t{t}\t{u}\n")).collect();
    if let Some(dir) = paths::alerted_file().parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(paths::alerted_file(), body);
}

fn tooltip(snap: Option<&Snapshot>) -> (String, bool) {
    let Some(s) = snap else { return ("skedaddle: starting".into(), false) };
    if s.config_error.is_some() {
        return ("skedaddle: config error. Right-click > Open config file".into(), false);
    }
    if s.feeds.is_empty() {
        return ("skedaddle: no calendar feeds. Right-click > Open config file".into(), false);
    }
    let now = Utc::now();
    let stale = s.is_stale(now);
    let h24 = s.config.clock_24h();
    let mut tip = String::new();
    if stale {
        let since = s.oldest_sync().map(|t| fmt_time(&t.with_timezone(&Local).fixed_offset(), h24)).unwrap_or_else(|| "start".into());
        tip.push_str(&format!("Not synced since {since}\n"));
    }
    let live = |o: &&Occurrence| !o.all_day && o.rsvp != Rsvp::Declined && o.end.with_timezone(&Utc) > now;
    let upcoming = s.occs.iter().filter(live).find(|o| o.start_utc() > now);
    if let Some(o) = upcoming {
        let day = if o.start.date_naive() == Local::now().date_naive() { String::new() } else { format!("{} ", o.start.format("%a")) };
        tip.push_str(&format!("Next: {day}{} {} ({})", fmt_time(&o.start, h24), o.title, fmt_in((o.start_utc() - now).num_seconds())));
    } else if let Some(o) = s.occs.iter().filter(live).next() {
        tip.push_str(&format!("Now: {} (until {})", o.title, fmt_time(&o.end, h24)));
    } else {
        tip.push_str("No upcoming events");
    }
    (tip.chars().take(127).collect(), stale)
}

fn refresh_tray(hwnd: HWND) {
    let (tip, stale) = tooltip(shared::snapshot().as_deref());
    let changed = with_app(|a| {
        let changed = a.tip != tip || a.stale != stale;
        a.tip = tip.clone();
        a.stale = stale;
        (changed, if stale { a.icon_warn } else { a.icon_ok })
    });
    if let Some((true, ic)) = changed {
        tray::update(hwnd, ic, &tip);
    }
}

fn play_sound(spec: &str) {
    match spec {
        "none" => {}
        "" | "default" => unsafe {
            let _ = MessageBeep(MB_ICONEXCLAMATION);
        },
        path => {
            let p = wide(path);
            unsafe {
                let _ = PlaySoundW(PCWSTR(p.as_ptr()), None, SND_FILENAME | SND_ASYNC | SND_NODEFAULT);
            }
        }
    }
}

fn show_alert(hwnd: HWND, items: Vec<Occurrence>) {
    let cfg = shared::config();
    if cfg.alert.flash {
        flash::start(parse_color(&cfg.alert.flash_color).unwrap_or(0x2F80ED));
    }
    play_sound(&cfg.alert.sound);
    modal::show(hwnd, items, &cfg);
    if cfg.alert.repeat_seconds > 0 {
        unsafe {
            SetTimer(Some(hwnd), ID_REPEAT, cfg.alert.repeat_seconds * 1000, None);
        }
    }
}

fn repeat(hwnd: HWND) {
    let cfg = shared::config();
    let until = modal::latest_start().map(|s| s + Duration::minutes(cfg.alert.repeat_until_minutes));
    if modal::is_open() && until.is_some_and(|u| Utc::now() < u) {
        if cfg.alert.flash {
            flash::start(parse_color(&cfg.alert.flash_color).unwrap_or(0x2F80ED));
        }
    } else {
        unsafe {
            let _ = KillTimer(Some(hwnd), ID_REPEAT);
        }
    }
}

fn reschedule(hwnd: HWND) {
    let Some(snap) = shared::snapshot() else { return };
    let now = Utc::now();
    let rules = snap.config.rules();
    let due: Vec<Occurrence> = with_app(|a| alerts::due(&snap.occs, now, &rules, &a.seen).into_iter().cloned().collect()).unwrap_or_default();
    if !due.is_empty() {
        with_app(|a| {
            a.seen.extend(due.iter().map(|o| o.key()));
            save_seen(&a.seen);
        });
        show_alert(hwnd, due);
    }
    let next = with_app(|a| alerts::next_wake(&snap.occs, now, &rules, &a.seen)).flatten();
    unsafe {
        match next {
            Some(t) => {
                let ms = ((t - Utc::now()).num_milliseconds() + 30).clamp(30, 3_600_000) as u32;
                SetTimer(Some(hwnd), ID_ALERT, ms, None);
            }
            None => {
                let _ = KillTimer(Some(hwnd), ID_ALERT);
            }
        }
    }
}

fn on_snapshot(hwnd: HWND, snap: Arc<Snapshot>) {
    shared::set_snapshot(snap);
    refresh_tray(hwnd);
    view::on_snapshot();
    reschedule(hwnd);
    if with_app(|a| std::mem::replace(&mut a.first_snapshot, false)) == Some(true) {
        unsafe {
            SetTimer(Some(hwnd), ID_TRIM, 4000, None);
        }
    }
}

fn test_alert(hwnd: HWND) {
    let now = Local::now().fixed_offset();
    let o = Occurrence {
        feed: 0,
        uid: format!("test-{}", now.timestamp()),
        title: "Test alert".into(),
        start: now,
        end: now + Duration::minutes(30),
        all_day: false,
        location: "Conference room".into(),
        description: "This is a test of the skedaddle alert.".into(),
        join_url: Some("https://meet.google.com/".into()),
        rsvp: Rsvp::None,
        busy: true,
    };
    show_alert(hwnd, vec![o]);
}

fn menu(hwnd: HWND) {
    unsafe {
        let Ok(m) = CreatePopupMenu() else { return };
        let _ = AppendMenuW(m, MF_STRING, MENU_OPEN, w!("Open calendar"));
        let _ = AppendMenuW(m, MF_STRING, MENU_REFRESH, w!("Refresh now"));
        let _ = AppendMenuW(m, MF_STRING, MENU_TEST, w!("Test alert"));
        let _ = AppendMenuW(m, MF_STRING, MENU_CONFIG, w!("Open config file"));
        let auto = if autostart::is_enabled() { MF_STRING | MF_CHECKED } else { MF_STRING };
        let _ = AppendMenuW(m, auto, MENU_AUTOSTART, w!("Start with Windows"));
        let _ = AppendMenuW(m, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(m, MF_STRING, MENU_QUIT, w!("Quit"));
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let _ = SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenu(m, TPM_RIGHTBUTTON | TPM_RETURNCMD | TPM_BOTTOMALIGN, pt.x, pt.y, Some(0), hwnd, None);
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(m);
        match cmd.0 as usize {
            MENU_OPEN => view::show(),
            MENU_REFRESH => shared::send(Cmd::Refresh),
            MENU_TEST => test_alert(hwnd),
            MENU_CONFIG => gdi::open_file_in_notepad(&paths::config_file()),
            MENU_AUTOSTART => autostart::set(!autostart::is_enabled()),
            MENU_QUIT => {
                let _ = DestroyWindow(hwnd);
            }
            _ => {}
        }
    }
}

unsafe extern "system" fn main_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_APP_TRAY => {
            match (lp.0 & 0xFFFF) as u32 {
                WM_LBUTTONUP => view::toggle(),
                WM_RBUTTONUP => menu(hwnd),
                _ => {}
            }
            LRESULT(0)
        }
        WM_APP_SNAPSHOT => {
            let snap = Arc::new(*Box::from_raw(lp.0 as *mut Snapshot));
            on_snapshot(hwnd, snap);
            LRESULT(0)
        }
        WM_APP_SHOW => {
            view::show();
            LRESULT(0)
        }
        WM_APP_MODAL_CLOSED => {
            let _ = KillTimer(Some(hwnd), ID_REPEAT);
            LRESULT(0)
        }
        WM_TIMER => {
            match wp.0 {
                ID_ALERT => {
                    let _ = KillTimer(Some(hwnd), ID_ALERT);
                    reschedule(hwnd);
                }
                ID_TICK => {
                    refresh_tray(hwnd);
                    reschedule(hwnd);
                }
                ID_REPEAT => repeat(hwnd),
                ID_TRIM => {
                    let _ = KillTimer(Some(hwnd), ID_TRIM);
                    trim_memory();
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_POWERBROADCAST => {
            if wp.0 as u32 == PBT_APMRESUMEAUTOMATIC || wp.0 as u32 == PBT_APMRESUMESUSPEND {
                shared::send(Cmd::Refresh);
                reschedule(hwnd);
            }
            LRESULT(1)
        }
        WM_TIMECHANGE => {
            reschedule(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            tray::remove(hwnd);
            shared::send(Cmd::Quit);
            PostQuitMessage(0);
            LRESULT(0)
        }
        m if Some(m) == with_app(|a| a.taskbar_created) => {
            if let Some((ic, tip)) = with_app(|a| (if a.stale { a.icon_warn } else { a.icon_ok }, a.tip.clone())) {
                tray::add(hwnd, ic, &tip);
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

pub fn run() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let mutex_name = wide(r"Local\skedaddle.single");
        let _guard = CreateMutexW(None, false, PCWSTR(mutex_name.as_ptr()));
        if GetLastError() == ERROR_ALREADY_EXISTS {
            if let Ok(h) = FindWindowW(w!("skedaddle.main"), None) {
                let _ = PostMessageW(Some(h), WM_APP_SHOW, WPARAM(0), LPARAM(0));
            }
            return;
        }

        let cfg_path = paths::config_file();
        if !cfg_path.exists() {
            if let Some(dir) = cfg_path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if std::fs::write(&cfg_path, TEMPLATE).is_ok() {
                gdi::open_file_in_notepad(&cfg_path);
            }
        }

        let hinst: HINSTANCE = GetModuleHandleW(None).unwrap_or_default().into();
        let small = GetSystemMetrics(SM_CXSMICON).max(16);
        let icon_ok = icon::make(small, false);
        let icon_warn = icon::make(small, true);
        let big = icon::make(32, false);

        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(main_proc),
            hInstance: hinst,
            lpszClassName: w!("skedaddle.main"),
            ..Default::default()
        };
        RegisterClassExW(&wc);
        view::register(hinst, big, icon_ok);

        let created = CreateWindowExW(WS_EX_TOOLWINDOW, w!("skedaddle.main"), w!("skedaddle"), WS_OVERLAPPED, 0, 0, 0, 0, None, None, Some(hinst), None);
        let Ok(hwnd) = created else { return };

        APP.with(|a| {
            *a.borrow_mut() = Some(App {
                seen: load_seen(),
                icon_ok,
                icon_warn,
                stale: false,
                tip: "skedaddle: starting".into(),
                taskbar_created: RegisterWindowMessageW(w!("TaskbarCreated")),
                first_snapshot: true,
            })
        });
        tray::add(hwnd, icon_ok, "skedaddle: starting");

        let (tx, rx) = mpsc::channel::<Cmd>();
        shared::set_sender(tx);
        sync::spawn(hwnd, rx);
        SetTimer(Some(hwnd), ID_TICK, 60_000, None);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}
