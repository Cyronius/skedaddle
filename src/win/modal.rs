use std::cell::RefCell;

use chrono::{DateTime, Utc};
use skedaddle::config::{parse_color, Config};
use skedaddle::model::{fmt_in, fmt_time, Occurrence};
use windows::core::w;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;

use super::gdi::{self, Fonts};
use super::{view, WM_APP_MODAL_CLOSED};

const MAX_ROWS: usize = 5;
const BG: u32 = 0x1F2023;
const ROW_BG: u32 = 0x2A2C30;
const TEXT: u32 = 0xF2F2F2;
const DIM: u32 = 0xA9ADB3;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Hit {
    Join(usize),
    Details,
    Dismiss,
}

struct Modal {
    hwnd: isize,
    main: isize,
    items: Vec<Occurrence>,
    accent: u32,
    h24: bool,
    feed_colors: Vec<u32>,
    hits: Vec<(RECT, Hit)>,
    hover: Option<Hit>,
    dpi: u32,
    fonts: Option<Fonts>,
}

thread_local! {
    static MODAL: RefCell<Option<Modal>> = const { RefCell::new(None) };
    static REGISTERED: RefCell<bool> = const { RefCell::new(false) };
}

fn size_for(count: usize, dpi: u32) -> (i32, i32) {
    let rows = count.min(MAX_ROWS) as i32;
    let more = if count > MAX_ROWS { 28 } else { 0 };
    let h = 52 + rows * 64 + more + 64;
    (gdi::scale(500, dpi), gdi::scale(h, dpi))
}

pub fn is_open() -> bool {
    MODAL.with(|m| m.borrow().is_some())
}

pub fn latest_start() -> Option<DateTime<Utc>> {
    MODAL.with(|m| m.borrow().as_ref().and_then(|m| m.items.iter().map(|o| o.start_utc()).max()))
}

pub fn show(main: HWND, items: Vec<Occurrence>, cfg: &Config) {
    let existing = MODAL.with(|m| {
        let mut m = m.borrow_mut();
        let Some(modal) = m.as_mut() else { return None };
        for o in &items {
            if !modal.items.iter().any(|x| x.key() == o.key()) {
                modal.items.push(o.clone());
            }
        }
        modal.items.sort_by_key(|o| o.start.timestamp());
        Some((modal.hwnd, modal.items.len()))
    });
    if let Some((h, count)) = existing {
        let hwnd = HWND(h as *mut _);
        unsafe {
            let dpi = gdi::dpi_of(hwnd);
            let (w, hh) = size_for(count, dpi);
            let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, w, hh, SWP_NOMOVE | SWP_NOACTIVATE);
            let _ = InvalidateRect(Some(hwnd), None, false);
        }
        return;
    }

    let work = gdi::cursor_work_area();
    unsafe {
        let hinst: HINSTANCE = GetModuleHandleW(None).unwrap_or_default().into();
        register(hinst);
        let mut items = items;
        items.sort_by_key(|o| o.start.timestamp());
        let state = Modal {
            hwnd: 0,
            main: main.0 as isize,
            items,
            accent: parse_color(&cfg.alert.flash_color).unwrap_or(0x2F80ED),
            h24: cfg.clock_24h(),
            feed_colors: cfg.feed.iter().map(|f| parse_color(&f.color).unwrap_or(0x4285F4)).collect(),
            hits: Vec::new(),
            hover: None,
            dpi: 96,
            fonts: None,
        };
        MODAL.with(|m| *m.borrow_mut() = Some(state));
        let ex = WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW;
        let created = CreateWindowExW(ex, w!("skedaddle.modal"), w!("skedaddle alert"), WS_POPUP, work.left, work.top, 10, 10, None, None, Some(hinst), None);
        let Ok(hwnd) = created else {
            MODAL.with(|m| *m.borrow_mut() = None);
            return;
        };
        let dpi = gdi::dpi_of(hwnd);
        let count = MODAL.with(|m| {
            let mut m = m.borrow_mut();
            let m = m.as_mut().unwrap();
            m.hwnd = hwnd.0 as isize;
            m.dpi = dpi;
            m.items.len()
        });
        let (w, h) = size_for(count, dpi);
        let x = work.left + (work.right - work.left - w) / 2;
        let y = work.top + (work.bottom - work.top - h) / 3;
        let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), x, y, w, h, SWP_NOACTIVATE | SWP_SHOWWINDOW);
        SetTimer(Some(hwnd), 1, 1000, None);
    }
}

fn register(hinst: HINSTANCE) {
    if REGISTERED.with(|r| std::mem::replace(&mut *r.borrow_mut(), true)) {
        return;
    }
    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: CS_DROPSHADOW,
        lpfnWndProc: Some(proc),
        hInstance: hinst,
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW).unwrap_or_default() },
        lpszClassName: w!("skedaddle.modal"),
        ..Default::default()
    };
    unsafe {
        RegisterClassExW(&wc);
    }
}

fn header_text(items: &[Occurrence]) -> String {
    let now = Utc::now();
    let first = items.iter().map(|o| o.start_utc()).min().unwrap_or(now);
    let secs = (first - now).num_seconds();
    if secs > 0 {
        format!("Starts {}", fmt_in(secs))
    } else if secs > -60 {
        "Starting now".to_string()
    } else {
        format!("Started {} min ago", (-secs) / 60)
    }
}

fn paint(hdc: HDC, rc: RECT, m: &mut Modal) {
    let dpi = m.dpi;
    let sc = |v: i32| gdi::scale(v, dpi);
    let fonts = m.fonts.get_or_insert_with(|| Fonts::new(dpi));
    let (w, h) = (rc.right, rc.bottom);
    gdi::fill(hdc, &rc, BG);
    m.hits.clear();

    let head = gdi::rect(0, 0, w, sc(52));
    gdi::fill(hdc, &head, m.accent);
    gdi::text(hdc, &header_text(&m.items), gdi::rect(sc(20), 0, w - sc(20), sc(52)), 0xFFFFFF, fonts.title, DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS);

    let mut y = sc(52);
    let shown = m.items.len().min(MAX_ROWS);
    for (i, o) in m.items.iter().take(shown).enumerate() {
        let row = gdi::rect(sc(12), y + sc(6), w - sc(12), y + sc(62));
        gdi::round_fill(hdc, &row, ROW_BG, sc(6));
        let color = m.feed_colors.get(o.feed).copied().unwrap_or(0x4285F4);
        gdi::fill(hdc, &gdi::rect(row.left, row.top + sc(6), row.left + sc(4), row.bottom - sc(6)), color);

        let btn_w = if o.join_url.is_some() { sc(84) } else { 0 };
        let text_r = row.right - sc(12) - if btn_w > 0 { btn_w + sc(8) } else { 0 };
        gdi::text(hdc, &o.title, gdi::rect(row.left + sc(16), row.top + sc(8), text_r, row.top + sc(30)), TEXT, fonts.bold, DT_SINGLELINE | DT_END_ELLIPSIS);
        let mut when = format!("{} – {}", fmt_time(&o.start, m.h24), fmt_time(&o.end, m.h24));
        if !o.location.is_empty() {
            when.push_str(" · ");
            when.push_str(o.location.lines().next().unwrap_or(""));
        }
        gdi::text(hdc, &when, gdi::rect(row.left + sc(16), row.top + sc(30), text_r, row.bottom - sc(6)), DIM, fonts.small, DT_SINGLELINE | DT_END_ELLIPSIS);

        if btn_w > 0 {
            let b = gdi::rect(row.right - sc(12) - btn_w, row.top + sc(12), row.right - sc(12), row.bottom - sc(12));
            let hot = m.hover == Some(Hit::Join(i));
            gdi::round_fill(hdc, &b, if hot { 0x34A853 } else { 0x1E8E3E }, sc(5));
            gdi::text(hdc, "Join", b, 0xFFFFFF, fonts.bold, DT_SINGLELINE | DT_CENTER | DT_VCENTER);
            m.hits.push((b, Hit::Join(i)));
        }
        y += sc(64);
    }
    if m.items.len() > shown {
        let r = gdi::rect(sc(20), y + sc(4), w - sc(20), y + sc(26));
        gdi::text(hdc, &format!("+{} more", m.items.len() - shown), r, DIM, fonts.small, DT_SINGLELINE | DT_VCENTER);
    }

    let fy = h - sc(60);
    let dismiss = gdi::rect(w - sc(12) - sc(110), fy + sc(12), w - sc(12), fy + sc(48));
    let details = gdi::rect(dismiss.left - sc(8) - sc(100), fy + sc(12), dismiss.left - sc(8), fy + sc(48));
    let hot_d = m.hover == Some(Hit::Dismiss);
    gdi::round_fill(hdc, &dismiss, if hot_d { 0xE5E7EB } else { 0xFFFFFF }, sc(5));
    gdi::text(hdc, "Dismiss", dismiss, 0x1F2023, fonts.bold, DT_SINGLELINE | DT_CENTER | DT_VCENTER);
    let hot_t = m.hover == Some(Hit::Details);
    gdi::round_fill(hdc, &details, if hot_t { 0x3A3D42 } else { 0x2F3236 }, sc(5));
    gdi::text(hdc, "Details", details, TEXT, fonts.bold, DT_SINGLELINE | DT_CENTER | DT_VCENTER);
    m.hits.push((dismiss, Hit::Dismiss));
    m.hits.push((details, Hit::Details));
}

fn hit_at(x: i32, y: i32) -> Option<Hit> {
    MODAL.with(|m| m.borrow().as_ref().and_then(|m| m.hits.iter().find(|(r, _)| gdi::contains(r, x, y)).map(|(_, h)| *h)))
}

fn lparam_xy(lp: LPARAM) -> (i32, i32) {
    ((lp.0 & 0xFFFF) as i16 as i32, ((lp.0 >> 16) & 0xFFFF) as i16 as i32)
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            gdi::paint_buffered(hwnd, |hdc, rc| {
                MODAL.with(|m| {
                    if let Some(m) = m.borrow_mut().as_mut() {
                        paint(hdc, rc, m);
                    }
                })
            });
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_TIMER => {
            let _ = InvalidateRect(Some(hwnd), None, false);
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let (x, y) = lparam_xy(lp);
            let hit = hit_at(x, y);
            let changed = MODAL.with(|m| {
                let mut m = m.borrow_mut();
                let Some(m) = m.as_mut() else { return false };
                let c = m.hover != hit;
                m.hover = hit;
                c
            });
            if changed {
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            LRESULT(0)
        }
        WM_SETCURSOR => {
            let (x, y) = {
                let mut pt = windows::Win32::Foundation::POINT::default();
                let _ = GetCursorPos(&mut pt);
                let _ = ScreenToClient(hwnd, &mut pt);
                (pt.x, pt.y)
            };
            let cur = if hit_at(x, y).is_some() { IDC_HAND } else { IDC_ARROW };
            SetCursor(LoadCursorW(None, cur).ok());
            LRESULT(1)
        }
        WM_LBUTTONUP => {
            let (x, y) = lparam_xy(lp);
            match hit_at(x, y) {
                Some(Hit::Dismiss) => {
                    let _ = DestroyWindow(hwnd);
                }
                Some(Hit::Details) => {
                    let first = MODAL.with(|m| m.borrow().as_ref().and_then(|m| m.items.first().cloned()));
                    let _ = DestroyWindow(hwnd);
                    if let Some(o) = first {
                        view::show_event(&o);
                    }
                }
                Some(Hit::Join(i)) => {
                    let (url, left) = MODAL.with(|m| {
                        let mut m = m.borrow_mut();
                        let Some(m) = m.as_mut() else { return (None, 0) };
                        if i >= m.items.len() {
                            return (None, m.items.len());
                        }
                        let o = m.items.remove(i);
                        (o.join_url, m.items.len())
                    });
                    if let Some(u) = url {
                        gdi::open_url(&u);
                    }
                    if left == 0 {
                        let _ = DestroyWindow(hwnd);
                    } else {
                        let dpi = gdi::dpi_of(hwnd);
                        let (w, h) = size_for(left, dpi);
                        let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, w, h, SWP_NOMOVE | SWP_NOACTIVATE);
                        let _ = InvalidateRect(Some(hwnd), None, false);
                    }
                }
                None => {}
            }
            LRESULT(0)
        }
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_DESTROY => {
            let main = MODAL.with(|m| {
                let mut m = m.borrow_mut();
                let main = m.as_ref().map(|m| m.main);
                if let Some(modal) = m.as_ref() {
                    if let Some(f) = &modal.fonts {
                        f.destroy();
                    }
                }
                *m = None;
                main
            });
            let _ = KillTimer(Some(hwnd), 1);
            if let Some(main) = main {
                let _ = PostMessageW(Some(HWND(main as *mut _)), WM_APP_MODAL_CLOSED, WPARAM(0), LPARAM(0));
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}
