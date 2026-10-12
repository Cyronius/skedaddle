use std::cell::RefCell;

use chrono::{Datelike, Duration, Local, Months, NaiveDate, NaiveDateTime, Timelike, Utc};
use skedaddle::config::{parse_color, Config};
use skedaddle::layout;
use skedaddle::model::{fmt_time, local_midnight_utc, AlertKey, Occurrence, Rsvp};
use windows::core::{w, BOOL};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
use windows::Win32::UI::HiDpi::GetDpiForSystem;
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, VK_ESCAPE, VK_LEFT, VK_NEXT, VK_PRIOR, VK_RIGHT};
use windows::Win32::UI::WindowsAndMessaging::*;

use super::gdi::{self, Fonts};
use super::paths;
use super::shared;
use super::sync::{Cmd, Snapshot};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Day,
    Week,
    Month,
}

#[derive(Clone)]
enum Hit {
    Today,
    Prev,
    Next,
    Mode(Mode),
    Event(AlertKey),
    Day(NaiveDate),
    CloseDetail,
    Join(String),
    Nothing,
}

struct View {
    hwnd: isize,
    mode: Mode,
    anchor: NaiveDate,
    scroll: i32,
    autoscroll: Option<i32>,
    selected: Option<AlertKey>,
    hits: Vec<(RECT, Hit)>,
    fonts: Option<(u32, Fonts)>,
    dark: bool,
    max_scroll: i32,
    requested: Option<(NaiveDate, NaiveDate)>,
    pressed: Option<(RECT, Hit, bool)>,
}

thread_local! {
    static VIEW: RefCell<View> = RefCell::new(View {
        hwnd: 0,
        mode: Mode::Week,
        anchor: Local::now().date_naive(),
        scroll: 0,
        autoscroll: Some(8),
        selected: None,
        hits: Vec::new(),
        fonts: None,
        dark: false,
        max_scroll: 0,
        requested: None,
        pressed: None,
    });
}

#[derive(Clone, Copy)]
struct Pal {
    bg: u32,
    panel: u32,
    grid: u32,
    text: u32,
    dim: u32,
    accent: u32,
    now: u32,
}

fn palette(dark: bool) -> Pal {
    if dark {
        Pal { bg: 0x1E1F22, panel: 0x2B2D30, grid: 0x3A3C40, text: 0xE6E6E6, dim: 0x9AA0A6, accent: 0x5B9BFF, now: 0xFF6B5E }
    } else {
        Pal { bg: 0xFFFFFF, panel: 0xF4F5F7, grid: 0xE1E3E7, text: 0x1F2328, dim: 0x667085, accent: 0x1A73E8, now: 0xEA4335 }
    }
}

fn system_dark() -> bool {
    let mut data: u32 = 1;
    let mut size = 4u32;
    let sub = super::wide(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
    let rc = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            windows::core::PCWSTR(sub.as_ptr()),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut data as *mut u32 as *mut _),
            Some(&mut size),
        )
    };
    rc.is_ok() && data == 0
}

fn apply_title_bar(hwnd: HWND, dark: bool) {
    let v: BOOL = dark.into();
    unsafe {
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, &v as *const _ as *const _, 4);
    }
}

fn week_start(d: NaiveDate, monday_first: bool) -> NaiveDate {
    let off = if monday_first { d.weekday().num_days_from_monday() } else { d.weekday().num_days_from_sunday() };
    d - Duration::days(off as i64)
}

fn month_grid_start(anchor: NaiveDate, monday_first: bool) -> NaiveDate {
    week_start(anchor.with_day(1).unwrap(), monday_first)
}

fn month_rows(anchor: NaiveDate, monday_first: bool) -> i32 {
    let first = anchor.with_day(1).unwrap();
    let off = (first - month_grid_start(anchor, monday_first)).num_days() as i32;
    let next = first.checked_add_months(Months::new(1)).unwrap();
    let days = (next - first).num_days() as i32;
    (off + days + 6) / 7
}

fn visible_range(mode: Mode, anchor: NaiveDate, monday_first: bool) -> (NaiveDate, NaiveDate) {
    match mode {
        Mode::Day => (anchor, anchor),
        Mode::Week => {
            let s = week_start(anchor, monday_first);
            (s, s + Duration::days(6))
        }
        Mode::Month => {
            let s = month_grid_start(anchor, monday_first);
            (s, s + Duration::days(41))
        }
    }
}

fn range_label(mode: Mode, anchor: NaiveDate, monday_first: bool) -> String {
    match mode {
        Mode::Day => anchor.format("%A, %B %-d, %Y").to_string(),
        Mode::Month => anchor.format("%B %Y").to_string(),
        Mode::Week => {
            let (a, b) = visible_range(mode, anchor, monday_first);
            if a.month() == b.month() {
                format!("{} {} – {}, {}", a.format("%b"), a.day(), b.day(), b.year())
            } else if a.year() == b.year() {
                format!("{} – {}, {}", a.format("%b %-d"), b.format("%b %-d"), b.year())
            } else {
                format!("{} – {}", a.format("%b %-d, %Y"), b.format("%b %-d, %Y"))
            }
        }
    }
}

fn feed_color(cfg: &Config, feed: usize) -> u32 {
    cfg.feed.get(feed).and_then(|f| parse_color(&f.color)).unwrap_or(0x4285F4)
}

fn on_day(o: &Occurrence, d: NaiveDate) -> bool {
    let (s, e) = (o.start.naive_local(), o.end.naive_local());
    if o.all_day {
        return s.date() <= d && (d < e.date() || s.date() == d);
    }
    let ds = d.and_hms_opt(0, 0, 0).unwrap();
    let de = ds + Duration::days(1);
    s < de && (e > ds || (s == e && s >= ds))
}

fn compact_time(o: &Occurrence, h24: bool) -> String {
    let t = o.start.naive_local();
    if h24 {
        return t.format("%H:%M").to_string();
    }
    let suffix = if t.hour() < 12 { "a" } else { "p" };
    let h = match t.hour() % 12 {
        0 => 12,
        h => h,
    };
    if t.minute() == 0 {
        format!("{h}{suffix}")
    } else {
        format!("{h}:{:02}{suffix}", t.minute())
    }
}

fn hour_label(h: u32, h24: bool) -> String {
    if h24 {
        return format!("{h:02}:00");
    }
    match h {
        0 => "12 AM".into(),
        1..=11 => format!("{h} AM"),
        12 => "12 PM".into(),
        _ => format!("{} PM", h - 12),
    }
}

fn intersect(a: &RECT, b: &RECT) -> Option<RECT> {
    let r = gdi::rect(a.left.max(b.left), a.top.max(b.top), a.right.min(b.right), a.bottom.min(b.bottom));
    (r.right > r.left && r.bottom > r.top).then_some(r)
}

struct Cx<'a> {
    hdc: HDC,
    dpi: u32,
    pal: Pal,
    fonts: &'a Fonts,
    cfg: &'a Config,
    hits: Vec<(RECT, Hit)>,
    pressed: Option<RECT>,
    now: NaiveDateTime,
    today: NaiveDate,
}

impl Cx<'_> {
    fn sc(&self, v: i32) -> i32 {
        gdi::scale(v, self.dpi)
    }

    fn button(&mut self, r: RECT, label: &str, hit: Hit, active: bool, font: HFONT) {
        let down = self.pressed == Some(r);
        let (bg, fg) = match (active, down) {
            (true, false) => (self.pal.accent, 0xFFFFFF),
            (true, true) => (gdi::blend(0x000000, self.pal.accent, 0.25), 0xFFFFFF),
            (false, false) => (self.pal.bg, self.pal.text),
            (false, true) => (gdi::blend(self.pal.text, self.pal.bg, 0.16), self.pal.text),
        };
        gdi::round_fill(self.hdc, &r, if active { bg } else { self.pal.grid }, self.sc(6));
        if !active {
            let inner = gdi::rect(r.left + 1, r.top + 1, r.right - 1, r.bottom - 1);
            gdi::round_fill(self.hdc, &inner, bg, self.sc(6));
        }
        let label_rect = if down { gdi::rect(r.left, r.top + 1, r.right, r.bottom + 1) } else { r };
        gdi::text(self.hdc, label, label_rect, fg, font, DT_SINGLELINE | DT_CENTER | DT_VCENTER);
        self.hits.push((r, hit));
    }

    fn event_fill(&self, o: &Occurrence) -> (u32, u32, u32) {
        let c = feed_color(self.cfg, o.feed);
        let declined = o.rsvp == Rsvp::Declined;
        let t = if declined { 0.10 } else if self.pal.bg == 0xFFFFFF { 0.20 } else { 0.32 };
        let text = if declined { self.pal.dim } else { self.pal.text };
        (c, gdi::blend(c, self.pal.bg, t), text)
    }

    fn day_events<'o>(&self, occs: &'o [Occurrence], d: NaiveDate) -> (Vec<&'o Occurrence>, Vec<&'o Occurrence>) {
        let (mut all_day, mut timed) = (Vec::new(), Vec::new());
        for o in occs.iter().filter(|o| on_day(o, d)) {
            if o.all_day {
                all_day.push(o);
            } else {
                timed.push(o);
            }
        }
        (all_day, timed)
    }

    fn draw_toolbar(&mut self, rc: RECT, v: &View, snap: Option<&Snapshot>, loading: bool) -> i32 {
        let h = self.sc(52);
        gdi::fill(self.hdc, &gdi::rect(0, 0, rc.right, h), self.pal.panel);
        gdi::fill(self.hdc, &gdi::rect(0, h - 1, rc.right, h), self.pal.grid);
        let (bh, by) = (self.sc(32), self.sc(10));
        let mut x = self.sc(12);
        let tw = gdi::text_width(self.hdc, "Today", self.fonts.bold) + self.sc(28);
        self.button(gdi::rect(x, by, x + tw, by + bh), "Today", Hit::Today, false, self.fonts.bold);
        x += tw + self.sc(8);
        self.button(gdi::rect(x, by, x + self.sc(34), by + bh), "‹", Hit::Prev, false, self.fonts.arrow);
        x += self.sc(40);
        self.button(gdi::rect(x, by, x + self.sc(34), by + bh), "›", Hit::Next, false, self.fonts.arrow);
        x += self.sc(46);

        let mut mx = rc.right - self.sc(12);
        for (m, label) in [(Mode::Month, "Month"), (Mode::Week, "Week"), (Mode::Day, "Day")] {
            let r = gdi::rect(mx - self.sc(68), by, mx, by + bh);
            self.button(r, label, Hit::Mode(m), v.mode == m, self.fonts.bold);
            mx -= self.sc(74);
        }

        let now = Utc::now();
        let (status, color) = match snap {
            None => ("Loading…".to_string(), self.pal.dim),
            Some(s) if s.feeds.is_empty() => ("No feeds configured".to_string(), self.pal.now),
            Some(s) if s.is_stale(now) => {
                let since = s.oldest_sync().map(|t| t.with_timezone(&Local).format("%-I:%M %p").to_string()).unwrap_or_else(|| "start".into());
                let err = s.feeds.iter().find_map(|f| f.error.clone()).map(|e| format!(" ({e})")).unwrap_or_default();
                (format!("Not synced since {since}{err}"), self.pal.now)
            }
            Some(_) if loading => ("Loading…".to_string(), self.pal.dim),
            Some(s) => {
                let t = s.oldest_sync().map(|t| t.with_timezone(&Local).format("%-I:%M %p").to_string()).unwrap_or_default();
                (format!("Synced {t}"), self.pal.dim)
            }
        };
        let sw = gdi::text_width(self.hdc, &status, self.fonts.small) + self.sc(8);
        let sr = gdi::rect(mx - sw - self.sc(8), 0, mx, h);
        gdi::text(self.hdc, &status, sr, color, self.fonts.small, DT_SINGLELINE | DT_VCENTER | DT_RIGHT);

        let label = range_label(v.mode, v.anchor, self.cfg.monday_first());
        gdi::text(self.hdc, &label, gdi::rect(x, 0, sr.left - self.sc(8), h), self.pal.text, self.fonts.title, DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS);
        h
    }

    fn draw_grid(&mut self, area: RECT, days: &[NaiveDate], occs: &[Occurrence], v: &mut View) {
        let h24 = self.cfg.clock_24h();
        let gutter = self.sc(58);
        let n = days.len() as i32;
        let col_w = (area.right - area.left - gutter) / n;
        let head_h = self.sc(if n == 1 { 52 } else { 58 });
        let line_h = self.sc(20);

        let per_day: Vec<(Vec<&Occurrence>, Vec<&Occurrence>)> = days.iter().map(|d| self.day_events(occs, *d)).collect();
        let strip_rows = per_day.iter().map(|(a, _)| a.len()).max().unwrap_or(0) as i32;
        let strip_h = if strip_rows > 0 { strip_rows * line_h + self.sc(8) } else { 0 };

        let top = area.top + head_h + strip_h;
        let hour_h = self.sc(48);
        let visible = area.bottom - top;
        v.max_scroll = (24 * hour_h - visible).max(0);
        if let Some(hour) = v.autoscroll.take() {
            v.scroll = (hour - 1).max(0) * hour_h;
        }
        v.scroll = v.scroll.clamp(0, v.max_scroll);
        let clip = gdi::rect(area.left, top, area.right, area.bottom);

        // Grid area, clipped so scrolled content never covers the header.
        unsafe {
            SaveDC(self.hdc);
            IntersectClipRect(self.hdc, clip.left, clip.top, clip.right, clip.bottom);
        }
        for (i, d) in days.iter().enumerate() {
            let x = area.left + gutter + i as i32 * col_w;
            if *d == self.today {
                gdi::fill(self.hdc, &gdi::rect(x, top, x + col_w, area.bottom), gdi::blend(self.pal.accent, self.pal.bg, 0.06));
            }
        }
        for hr in 0..24u32 {
            let y = top + hr as i32 * hour_h - v.scroll;
            if hr > 0 {
                gdi::text(self.hdc, &hour_label(hr, h24), gdi::rect(area.left, y - self.sc(9), area.left + gutter - self.sc(8), y + self.sc(9)), self.pal.dim, self.fonts.small, DT_SINGLELINE | DT_RIGHT | DT_VCENTER);
            }
            gdi::fill(self.hdc, &gdi::rect(area.left + gutter, y, area.right, y + 1), self.pal.grid);
        }
        for i in 0..=n {
            let x = area.left + gutter + i * col_w;
            gdi::fill(self.hdc, &gdi::rect(x, top, x + 1, area.bottom), self.pal.grid);
        }

        for (i, d) in days.iter().enumerate() {
            let cx = area.left + gutter + i as i32 * col_w;
            let ds = d.and_hms_opt(0, 0, 0).unwrap();
            let timed = &per_day[i].1;
            let spans: Vec<(i64, i64)> = timed
                .iter()
                .map(|o| {
                    let s = (o.start.naive_local() - ds).num_minutes().clamp(0, 1439);
                    let e = (o.end.naive_local() - ds).num_minutes().clamp(0, 1440);
                    (s, e.max(s + 15))
                })
                .collect();
            for p in layout::columns(&spans) {
                let o = timed[p.idx];
                let (s, e) = spans[p.idx];
                let slot = (col_w - self.sc(6)) / p.cols as i32;
                let x = cx + self.sc(2) + p.col as i32 * slot;
                let y = top + (s as i32 * hour_h) / 60 - v.scroll;
                let hh = (((e - s) as i32 * hour_h) / 60 - 2).max(self.sc(20));
                let r = gdi::rect(x, y, x + slot - 1, y + hh);
                let (c, fill, tcol) = self.event_fill(o);
                gdi::round_fill(self.hdc, &r, fill, self.sc(4));
                gdi::fill(self.hdc, &gdi::rect(r.left, r.top + 2, r.left + self.sc(3), r.bottom - 2), c);
                self.event_text(&r, o, tcol, h24);
                if v.selected.as_ref() == Some(&o.key()) {
                    gdi::outline(self.hdc, &r, self.pal.accent, 2);
                }
                if let Some(hit) = intersect(&r, &clip) {
                    self.hits.push((hit, Hit::Event(o.key())));
                }
            }
            if *d == self.today {
                let y = top + (self.now.hour() as i32 * 60 + self.now.minute() as i32) * hour_h / 60 - v.scroll;
                gdi::fill(self.hdc, &gdi::rect(cx, y - 1, cx + col_w, y + 1), self.pal.now);
                gdi::round_fill(self.hdc, &gdi::rect(cx - self.sc(4), y - self.sc(4), cx + self.sc(4), y + self.sc(4)), self.pal.now, self.sc(4));
            }
        }
        unsafe {
            let _ = RestoreDC(self.hdc, -1);
        }

        // Header and all-day strip are drawn last so their hits sit above the grid hits.
        gdi::fill(self.hdc, &gdi::rect(area.left, area.top, area.right, top), self.pal.bg);
        for (i, d) in days.iter().enumerate() {
            let x = area.left + gutter + i as i32 * col_w;
            let is_today = *d == self.today;
            let hr = gdi::rect(x, area.top, x + col_w, area.top + head_h);
            let name = d.format("%a").to_string().to_uppercase();
            gdi::text(self.hdc, &name, gdi::rect(x, area.top + self.sc(6), x + col_w, area.top + self.sc(22)), if is_today { self.pal.accent } else { self.pal.dim }, self.fonts.small_bold, DT_SINGLELINE | DT_CENTER);
            let num = d.day().to_string();
            let circle = gdi::rect(x + col_w / 2 - self.sc(16), area.top + self.sc(22), x + col_w / 2 + self.sc(16), area.top + self.sc(22) + self.sc(32));
            if is_today {
                gdi::round_fill(self.hdc, &circle, self.pal.accent, self.sc(16));
            }
            gdi::text(self.hdc, &num, circle, if is_today { 0xFFFFFF } else { self.pal.text }, self.fonts.title, DT_SINGLELINE | DT_CENTER | DT_VCENTER);
            self.hits.push((hr, Hit::Day(*d)));
            gdi::fill(self.hdc, &gdi::rect(x, area.top, x + 1, top), self.pal.grid);
            for (k, o) in per_day[i].0.iter().enumerate() {
                let r = gdi::rect(x + self.sc(2), area.top + head_h + self.sc(4) + k as i32 * line_h, x + col_w - self.sc(2), area.top + head_h + self.sc(4) + k as i32 * line_h + line_h - 2);
                let (c, fill, tcol) = self.event_fill(o);
                gdi::round_fill(self.hdc, &r, fill, self.sc(3));
                gdi::fill(self.hdc, &gdi::rect(r.left, r.top + 1, r.left + self.sc(3), r.bottom - 1), c);
                gdi::text(self.hdc, &o.title, gdi::rect(r.left + self.sc(7), r.top, r.right - self.sc(3), r.bottom), tcol, self.fonts.small, DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS);
                if v.selected.as_ref() == Some(&o.key()) {
                    gdi::outline(self.hdc, &r, self.pal.accent, 2);
                }
                self.hits.push((r, Hit::Event(o.key())));
            }
        }
        if strip_rows > 0 {
            gdi::text(self.hdc, "all-day", gdi::rect(area.left, area.top + head_h, area.left + gutter - self.sc(8), area.top + head_h + line_h + self.sc(6)), self.pal.dim, self.fonts.small, DT_SINGLELINE | DT_RIGHT | DT_VCENTER);
        }
        gdi::fill(self.hdc, &gdi::rect(area.left, top - 1, area.right, top), self.pal.grid);
    }

    fn event_text(&self, r: &RECT, o: &Occurrence, color: u32, h24: bool) {
        let pad = self.sc(7);
        let inner_w = r.right - r.left - pad - self.sc(3);
        let h = r.bottom - r.top;
        let line = self.sc(16);
        if h < self.sc(34) {
            let label = format!("{}  {}", compact_time(o, h24), o.title);
            gdi::text(self.hdc, &label, gdi::rect(r.left + pad, r.top, r.right - self.sc(3), r.bottom), color, self.fonts.small, DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS);
            return;
        }
        let th = gdi::text_height(self.hdc, &o.title, inner_w, self.fonts.small_bold).min(line * 2);
        gdi::text(self.hdc, &o.title, gdi::rect(r.left + pad, r.top + 3, r.right - self.sc(3), r.top + 3 + th), color, self.fonts.small_bold, DT_WORDBREAK | DT_END_ELLIPSIS);
        if th + line + 4 <= h {
            let when = format!("{} – {}", fmt_time(&o.start, h24), fmt_time(&o.end, h24));
            gdi::text(self.hdc, &when, gdi::rect(r.left + pad, r.top + 3 + th, r.right - self.sc(3), r.top + 3 + th + line), self.pal.dim, self.fonts.small, DT_SINGLELINE | DT_END_ELLIPSIS);
        }
    }

    fn draw_month(&mut self, area: RECT, anchor: NaiveDate, occs: &[Occurrence], v: &View) {
        let h24 = self.cfg.clock_24h();
        let monday = self.cfg.monday_first();
        let rows = month_rows(anchor, monday);
        let head_h = self.sc(28);
        let cell_w = (area.right - area.left) / 7;
        let cell_h = (area.bottom - area.top - head_h) / rows;
        let start = month_grid_start(anchor, monday);
        let line_h = self.sc(19);

        for c in 0..7 {
            let d = start + Duration::days(c);
            let x = area.left + c as i32 * cell_w;
            gdi::text(self.hdc, &d.format("%a").to_string().to_uppercase(), gdi::rect(x, area.top, x + cell_w, area.top + head_h), self.pal.dim, self.fonts.small_bold, DT_SINGLELINE | DT_CENTER | DT_VCENTER);
        }
        for row in 0..rows {
            for c in 0..7i32 {
                let d = start + Duration::days((row * 7 + c) as i64);
                let x = area.left + c * cell_w;
                let y = area.top + head_h + row * cell_h;
                let cell = gdi::rect(x, y, x + cell_w, y + cell_h);
                let in_month = d.month() == anchor.month();
                gdi::fill(self.hdc, &cell, if in_month { self.pal.bg } else { self.pal.panel });
                gdi::outline(self.hdc, &cell, self.pal.grid, 1);
                self.hits.push((cell, Hit::Day(d)));

                let is_today = d == self.today;
                let label = if d.day() == 1 { d.format("%b %-d").to_string() } else { d.day().to_string() };
                let lw = (gdi::text_width(self.hdc, &label, self.fonts.small_bold) + self.sc(12)).max(self.sc(24));
                let nr = gdi::rect(x + self.sc(4), y + self.sc(3), x + self.sc(4) + lw, y + self.sc(3) + self.sc(22));
                if is_today {
                    gdi::round_fill(self.hdc, &nr, self.pal.accent, self.sc(11));
                }
                gdi::text(self.hdc, &label, nr, if is_today { 0xFFFFFF } else if in_month { self.pal.text } else { self.pal.dim }, self.fonts.small_bold, DT_SINGLELINE | DT_CENTER | DT_VCENTER);

                let (all_day, timed) = self.day_events(occs, d);
                let list: Vec<&Occurrence> = all_day.into_iter().chain(timed).collect();
                let avail = ((cell_h - self.sc(28)) / line_h).max(0) as usize;
                let (show, more) = if list.len() > avail && avail > 0 { (avail - 1, list.len() - (avail - 1)) } else { (list.len().min(avail), 0) };
                for (k, o) in list.iter().take(show).enumerate() {
                    let ly = y + self.sc(27) + k as i32 * line_h;
                    let r = gdi::rect(x + self.sc(3), ly, x + cell_w - self.sc(3), ly + line_h - 1);
                    let (col, fill, tcol) = self.event_fill(o);
                    if o.all_day {
                        gdi::round_fill(self.hdc, &r, fill, self.sc(3));
                        gdi::fill(self.hdc, &gdi::rect(r.left, r.top + 1, r.left + self.sc(3), r.bottom - 1), col);
                        gdi::text(self.hdc, &o.title, gdi::rect(r.left + self.sc(7), r.top, r.right - self.sc(2), r.bottom), tcol, self.fonts.small, DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS);
                    } else {
                        gdi::round_fill(self.hdc, &gdi::rect(r.left + self.sc(3), r.top + self.sc(6), r.left + self.sc(9), r.top + self.sc(12)), col, self.sc(3));
                        let label = format!("{} {}", compact_time(o, h24), o.title);
                        gdi::text(self.hdc, &label, gdi::rect(r.left + self.sc(13), r.top, r.right - self.sc(2), r.bottom), tcol, self.fonts.small, DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS);
                    }
                    if v.selected.as_ref() == Some(&o.key()) {
                        gdi::outline(self.hdc, &r, self.pal.accent, 2);
                    }
                    self.hits.push((r, Hit::Event(o.key())));
                }
                if more > 0 {
                    let ly = y + self.sc(27) + show as i32 * line_h;
                    let r = gdi::rect(x + self.sc(6), ly, x + cell_w - self.sc(3), ly + line_h - 1);
                    gdi::text(self.hdc, &format!("+{more} more"), r, self.pal.dim, self.fonts.small_bold, DT_SINGLELINE | DT_VCENTER);
                }
            }
        }
    }

    fn draw_detail(&mut self, area: RECT, o: &Occurrence) {
        let h24 = self.cfg.clock_24h();
        let pw = self.sc(340).min(area.right - area.left);
        let panel = gdi::rect(area.right - pw, area.top, area.right, area.bottom);
        gdi::fill(self.hdc, &panel, self.pal.panel);
        gdi::fill(self.hdc, &gdi::rect(panel.left, panel.top, panel.left + 1, panel.bottom), self.pal.grid);
        self.hits.push((panel, Hit::Nothing));

        let pad = self.sc(18);
        let x = panel.left + pad;
        let inner = panel.right - pad - x;
        let mut y = panel.top + pad;
        let color = feed_color(self.cfg, o.feed);
        gdi::fill(self.hdc, &gdi::rect(panel.left + 1, y, panel.left + self.sc(5), y + self.sc(28)), color);

        let close = gdi::rect(panel.right - self.sc(40), panel.top + self.sc(10), panel.right - self.sc(10), panel.top + self.sc(40));
        gdi::text(self.hdc, "×", close, self.pal.dim, self.fonts.title, DT_SINGLELINE | DT_CENTER | DT_VCENTER);

        let title = if o.title.is_empty() { "(no title)" } else { &o.title };
        let th = gdi::text_height(self.hdc, title, inner - self.sc(28), self.fonts.title);
        gdi::text(self.hdc, title, gdi::rect(x, y, x + inner - self.sc(28), y + th), self.pal.text, self.fonts.title, DT_WORDBREAK);
        y += th + self.sc(12);

        let date = o.start.naive_local().format("%A, %B %-d").to_string();
        gdi::text(self.hdc, &date, gdi::rect(x, y, x + inner, y + self.sc(20)), self.pal.text, self.fonts.regular, DT_SINGLELINE);
        y += self.sc(22);
        let when = if o.all_day { "All day".to_string() } else { format!("{} – {}", fmt_time(&o.start, h24), fmt_time(&o.end, h24)) };
        gdi::text(self.hdc, &when, gdi::rect(x, y, x + inner, y + self.sc(20)), self.pal.dim, self.fonts.regular, DT_SINGLELINE);
        y += self.sc(30);

        if let Some(f) = self.cfg.feed.get(o.feed) {
            gdi::round_fill(self.hdc, &gdi::rect(x, y + self.sc(6), x + self.sc(10), y + self.sc(16)), color, self.sc(5));
            gdi::text(self.hdc, &f.name, gdi::rect(x + self.sc(18), y, x + inner, y + self.sc(22)), self.pal.dim, self.fonts.regular, DT_SINGLELINE | DT_END_ELLIPSIS);
            y += self.sc(30);
        }
        if !o.location.is_empty() {
            let lh = gdi::text_height(self.hdc, &o.location, inner, self.fonts.regular).min(self.sc(60));
            gdi::text(self.hdc, &o.location, gdi::rect(x, y, x + inner, y + lh), self.pal.text, self.fonts.regular, DT_WORDBREAK | DT_END_ELLIPSIS);
            y += lh + self.sc(14);
        }
        if let Some(url) = &o.join_url {
            let b = gdi::rect(x, y, x + self.sc(120), y + self.sc(36));
            gdi::round_fill(self.hdc, &b, 0x1E8E3E, self.sc(6));
            gdi::text(self.hdc, "Join", b, 0xFFFFFF, self.fonts.bold, DT_SINGLELINE | DT_CENTER | DT_VCENTER);
            self.hits.push((b, Hit::Join(url.clone())));
            y += self.sc(50);
        }
        if !o.description.is_empty() && y < panel.bottom - self.sc(30) {
            gdi::fill(self.hdc, &gdi::rect(x, y, x + inner, y + 1), self.pal.grid);
            y += self.sc(10);
            gdi::text(self.hdc, &o.description, gdi::rect(x, y, x + inner, panel.bottom - pad), self.pal.dim, self.fonts.small, DT_WORDBREAK | DT_END_ELLIPSIS);
        }
        self.hits.push((close, Hit::CloseDetail));
    }
}

fn now_hour() -> Option<i32> {
    Some(Local::now().hour() as i32)
}

fn draw(hdc: HDC, rc: RECT, dpi: u32, v: &mut View, snap: Option<&Snapshot>, cfg: &Config) {
    let pal = palette(v.dark);
    if v.fonts.as_ref().map(|f| f.0) != Some(dpi) {
        if let Some((_, f)) = v.fonts.take() {
            f.destroy();
        }
        v.fonts = Some((dpi, Fonts::new(dpi)));
    }
    let (fdpi, fonts) = v.fonts.take().unwrap();
    let now = Local::now().naive_local();
    let mut cx = Cx { hdc, dpi, pal, fonts: &fonts, cfg, hits: Vec::new(), pressed: v.pressed.as_ref().filter(|p| p.2).map(|p| p.0), now, today: now.date() };
    gdi::fill(hdc, &rc, pal.bg);

    let monday = cfg.monday_first();
    let occs: &[Occurrence] = snap.map(|s| s.occs.as_slice()).unwrap_or(&[]);
    let (a, b) = visible_range(v.mode, v.anchor, monday);
    let loading = snap.is_some_and(|s| local_midnight_utc(a) < s.from || local_midnight_utc(b + Duration::days(1)) > s.to);
    let tb_h = cx.draw_toolbar(rc, v, snap, loading);
    let area = gdi::rect(0, tb_h, rc.right, rc.bottom);
    match v.mode {
        Mode::Day => cx.draw_grid(area, &[v.anchor], occs, v),
        Mode::Week => {
            let s = week_start(v.anchor, monday);
            let days: Vec<NaiveDate> = (0..7).map(|i| s + Duration::days(i)).collect();
            cx.draw_grid(area, &days, occs, v);
        }
        Mode::Month => cx.draw_month(area, v.anchor, occs, v),
    }
    if let Some(key) = v.selected.clone() {
        if let Some(o) = occs.iter().find(|o| o.key() == key) {
            cx.draw_detail(area, o);
        }
    }
    v.hits = cx.hits;
    v.fonts = Some((fdpi, fonts));
}

fn paint(hwnd: HWND) {
    let dpi = gdi::dpi_of(hwnd);
    let snap = shared::snapshot();
    let cfg = shared::config();
    gdi::paint_buffered(hwnd, |hdc, rc| {
        VIEW.with(|v| draw(hdc, rc, dpi, &mut v.borrow_mut(), snap.as_deref(), &cfg));
    });
}

fn shift(v: &mut View, dir: i32) {
    v.anchor = match v.mode {
        Mode::Day => v.anchor + Duration::days(dir as i64),
        Mode::Week => v.anchor + Duration::days(7 * dir as i64),
        Mode::Month if dir > 0 => v.anchor.checked_add_months(Months::new(1)).unwrap_or(v.anchor),
        Mode::Month => v.anchor.checked_sub_months(Months::new(1)).unwrap_or(v.anchor),
    };
}

fn ensure_range(cfg: &Config) {
    let Some(snap) = shared::snapshot() else { return };
    let cmd = VIEW.with(|v| {
        let mut v = v.borrow_mut();
        let (a, b) = visible_range(v.mode, v.anchor, cfg.monday_first());
        if local_midnight_utc(a) >= snap.from && local_midnight_utc(b + Duration::days(1)) <= snap.to {
            v.requested = None;
            return None;
        }
        if v.requested.is_some_and(|(ra, rb)| ra <= a && b <= rb) {
            return None;
        }
        let want = (a - Duration::weeks(8), b + Duration::weeks(8));
        v.requested = Some(want);
        Some(want)
    });
    if let Some((a, b)) = cmd {
        shared::send(Cmd::Range(Some((local_midnight_utc(a), local_midnight_utc(b + Duration::days(1))))));
    }
}

fn click(hwnd: HWND, x: i32, y: i32) {
    let hit = VIEW.with(|v| v.borrow().hits.iter().rev().find(|(r, _)| gdi::contains(r, x, y)).cloned());
    let Some((rect, hit)) = hit else { return };
    if matches!(hit, Hit::Today | Hit::Prev | Hit::Next | Hit::Mode(_)) {
        VIEW.with(|v| v.borrow_mut().pressed = Some((rect, hit, true)));
        unsafe {
            SetCapture(hwnd);
            let _ = InvalidateRect(Some(hwnd), None, false);
        }
        return;
    }
    perform(hwnd, hit);
}

fn release(hwnd: HWND, x: i32, y: i32) {
    let pressed = VIEW.with(|v| v.borrow_mut().pressed.take());
    unsafe {
        let _ = ReleaseCapture();
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
    if let Some((rect, hit, _)) = pressed {
        if gdi::contains(&rect, x, y) {
            perform(hwnd, hit);
        }
    }
}

fn drag(hwnd: HWND, x: i32, y: i32) {
    let changed = VIEW.with(|v| {
        let mut v = v.borrow_mut();
        let Some(p) = v.pressed.as_mut() else { return false };
        let inside = gdi::contains(&p.0, x, y);
        std::mem::replace(&mut p.2, inside) != inside
    });
    if changed {
        unsafe {
            let _ = InvalidateRect(Some(hwnd), None, false);
        }
    }
}

fn perform(hwnd: HWND, hit: Hit) {
    let today = Local::now().date_naive();
    let mut url = None;
    VIEW.with(|v| {
        let mut v = v.borrow_mut();
        match hit {
            Hit::Today => {
                v.anchor = today;
                v.autoscroll = now_hour();
            }
            Hit::Prev => shift(&mut v, -1),
            Hit::Next => shift(&mut v, 1),
            Hit::Mode(m) => {
                v.mode = m;
                v.autoscroll = now_hour();
            }
            Hit::Event(k) => v.selected = if v.selected.as_ref() == Some(&k) { None } else { Some(k) },
            Hit::Day(d) => {
                v.anchor = d;
                if v.mode != Mode::Day {
                    v.mode = Mode::Day;
                    v.autoscroll = now_hour();
                }
            }
            Hit::CloseDetail => v.selected = None,
            Hit::Join(u) => url = Some(u),
            Hit::Nothing => {}
        }
    });
    if let Some(u) = url {
        gdi::open_url(&u);
    }
    ensure_range(&shared::config());
    unsafe {
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
}

fn key_down(hwnd: HWND, vk: u32) {
    let mut hide_now = false;
    VIEW.with(|v| {
        let mut v = v.borrow_mut();
        match vk {
            k if k == VK_ESCAPE.0 as u32 => {
                if v.selected.is_some() {
                    v.selected = None;
                } else {
                    hide_now = true;
                }
            }
            k if k == VK_LEFT.0 as u32 || k == VK_PRIOR.0 as u32 => shift(&mut v, -1),
            k if k == VK_RIGHT.0 as u32 || k == VK_NEXT.0 as u32 => shift(&mut v, 1),
            0x44 => v.mode = Mode::Day,
            0x57 => v.mode = Mode::Week,
            0x4D => v.mode = Mode::Month,
            0x54 => {
                v.anchor = Local::now().date_naive();
                v.autoscroll = now_hour();
            }
            _ => {}
        }
    });
    if hide_now {
        hide();
        return;
    }
    ensure_range(&shared::config());
    unsafe {
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            paint(hwnd);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_SIZE => {
            let _ = InvalidateRect(Some(hwnd), None, false);
            LRESULT(0)
        }
        WM_GETMINMAXINFO => {
            let mm = &mut *(lp.0 as *mut MINMAXINFO);
            let dpi = gdi::dpi_of(hwnd);
            mm.ptMinTrackSize.x = gdi::scale(720, dpi);
            mm.ptMinTrackSize.y = gdi::scale(480, dpi);
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            click(hwnd, (lp.0 & 0xFFFF) as i16 as i32, ((lp.0 >> 16) & 0xFFFF) as i16 as i32);
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            release(hwnd, (lp.0 & 0xFFFF) as i16 as i32, ((lp.0 >> 16) & 0xFFFF) as i16 as i32);
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            drag(hwnd, (lp.0 & 0xFFFF) as i16 as i32, ((lp.0 >> 16) & 0xFFFF) as i16 as i32);
            LRESULT(0)
        }
        WM_CAPTURECHANGED => {
            VIEW.with(|v| v.borrow_mut().pressed = None);
            let _ = InvalidateRect(Some(hwnd), None, false);
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            let delta = ((wp.0 >> 16) & 0xFFFF) as i16 as i32;
            let hour_h = gdi::scale(48, gdi::dpi_of(hwnd));
            VIEW.with(|v| {
                let mut v = v.borrow_mut();
                if v.mode != Mode::Month {
                    v.scroll = (v.scroll - delta * hour_h * 3 / 240).clamp(0, v.max_scroll);
                }
            });
            let _ = InvalidateRect(Some(hwnd), None, false);
            LRESULT(0)
        }
        WM_KEYDOWN => {
            key_down(hwnd, wp.0 as u32);
            LRESULT(0)
        }
        WM_TIMER => {
            let _ = InvalidateRect(Some(hwnd), None, false);
            LRESULT(0)
        }
        WM_DPICHANGED => {
            let r = &*(lp.0 as *const RECT);
            let _ = SetWindowPos(hwnd, None, r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_NOACTIVATE);
            LRESULT(0)
        }
        WM_SETTINGCHANGE => {
            let dark = system_dark();
            VIEW.with(|v| v.borrow_mut().dark = dark);
            apply_title_bar(hwnd, dark);
            let _ = InvalidateRect(Some(hwnd), None, false);
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        WM_CLOSE => {
            hide();
            LRESULT(0)
        }
        WM_DESTROY => {
            VIEW.with(|v| v.borrow_mut().hwnd = 0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

pub fn register(hinst: HINSTANCE, big: HICON, small: HICON) {
    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(proc),
        hInstance: hinst,
        hIcon: big,
        hIconSm: small,
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW).unwrap_or_default() },
        lpszClassName: w!("skedaddle.view"),
        ..Default::default()
    };
    unsafe {
        RegisterClassExW(&wc);
    }
}

fn saved_rect() -> Option<RECT> {
    let text = std::fs::read_to_string(paths::view_state_file()).ok()?;
    let n: Vec<i32> = text.split_whitespace().filter_map(|t| t.parse().ok()).collect();
    if n.len() != 4 || n[2] < 300 || n[3] < 300 {
        return None;
    }
    let r = gdi::rect(n[0], n[1], n[0] + n[2], n[1] + n[3]);
    let on_screen = unsafe { !MonitorFromRect(&r, MONITOR_DEFAULTTONULL).is_invalid() };
    on_screen.then_some(r)
}

fn save_rect(hwnd: HWND) {
    unsafe {
        if IsIconic(hwnd).as_bool() || IsZoomed(hwnd).as_bool() {
            return;
        }
        let mut r = RECT::default();
        if GetWindowRect(hwnd, &mut r).is_ok() {
            if let Some(dir) = paths::view_state_file().parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(paths::view_state_file(), format!("{} {} {} {}", r.left, r.top, r.right - r.left, r.bottom - r.top));
        }
    }
}

fn ensure_window() -> HWND {
    let existing = VIEW.with(|v| v.borrow().hwnd);
    if existing != 0 {
        return HWND(existing as *mut _);
    }
    unsafe {
        let hinst: HINSTANCE = GetModuleHandleW(None).unwrap_or_default().into();
        let r = saved_rect().unwrap_or_else(|| {
            let work = gdi::cursor_work_area();
            let sys = GetDpiForSystem();
            let w = gdi::scale(1100, sys).min(work.right - work.left);
            let h = gdi::scale(760, sys).min(work.bottom - work.top);
            let x = work.left + (work.right - work.left - w) / 2;
            let y = work.top + (work.bottom - work.top - h) / 2;
            gdi::rect(x, y, x + w, y + h)
        });
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("skedaddle.view"),
            w!("skedaddle"),
            WS_OVERLAPPEDWINDOW,
            r.left,
            r.top,
            r.right - r.left,
            r.bottom - r.top,
            None,
            None,
            Some(hinst),
            None,
        )
        .unwrap_or_default();
        VIEW.with(|v| v.borrow_mut().hwnd = hwnd.0 as isize);
        hwnd
    }
}

pub fn is_visible() -> bool {
    let h = VIEW.with(|v| v.borrow().hwnd);
    h != 0 && unsafe { IsWindowVisible(HWND(h as *mut _)).as_bool() }
}

pub fn show() {
    let hwnd = ensure_window();
    if hwnd.0.is_null() {
        return;
    }
    let dark = system_dark();
    VIEW.with(|v| {
        let mut v = v.borrow_mut();
        v.dark = dark;
        v.anchor = Local::now().date_naive();
        v.autoscroll = now_hour();
        v.selected = None;
    });
    apply_title_bar(hwnd, dark);
    unsafe {
        let _ = ShowWindow(hwnd, if IsIconic(hwnd).as_bool() { SW_RESTORE } else { SW_SHOW });
        let _ = SetForegroundWindow(hwnd);
        SetTimer(Some(hwnd), 1, 60_000, None);
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
    ensure_range(&shared::config());
}

pub fn show_event(o: &Occurrence) {
    show();
    VIEW.with(|v| {
        let mut v = v.borrow_mut();
        v.anchor = o.start.naive_local().date();
        v.mode = Mode::Day;
        v.autoscroll = Some(o.start.naive_local().hour() as i32);
        v.selected = Some(o.key());
    });
    ensure_range(&shared::config());
    let h = VIEW.with(|v| v.borrow().hwnd);
    unsafe {
        let _ = InvalidateRect(Some(HWND(h as *mut _)), None, false);
    }
}

pub fn hide() {
    let h = VIEW.with(|v| v.borrow().hwnd);
    if h == 0 {
        return;
    }
    let hwnd = HWND(h as *mut _);
    save_rect(hwnd);
    unsafe {
        let _ = KillTimer(Some(hwnd), 1);
        let _ = ShowWindow(hwnd, SW_HIDE);
    }
    VIEW.with(|v| {
        let mut v = v.borrow_mut();
        v.selected = None;
        v.requested = None;
        if let Some((_, f)) = v.fonts.take() {
            f.destroy();
        }
    });
    shared::send(Cmd::Range(None));
    super::app::trim_memory();
}

pub fn toggle() {
    if is_visible() {
        hide();
    } else {
        show();
    }
}

pub fn on_snapshot() {
    if !is_visible() {
        return;
    }
    ensure_range(&shared::config());
    let h = VIEW.with(|v| v.borrow().hwnd);
    unsafe {
        let _ = InvalidateRect(Some(HWND(h as *mut _)), None, false);
    }
}
