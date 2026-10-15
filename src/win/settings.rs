use std::cell::RefCell;

use skedaddle::config::{parse_color, Config, FeedCfg, MIN_DAY_HOURS, TEXT_SIZES};
use skedaddle::theme::{self, Theme, Tone, THEMES};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::AdjustWindowRectExForDpi;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetFocus, SetFocus, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT, VK_ESCAPE, VK_RETURN};
use windows::Win32::UI::WindowsAndMessaging::*;

use super::gdi::{self, Fonts};
use super::sync::Cmd;
use super::theme::{apply_title_bar, pal_for, system_dark, Pal};
use super::{paths, shared, wide};

const W: i32 = 880;
const H: i32 = 600;
const LX: i32 = 24;
const LW: i32 = 396;
const RX: i32 = 468;
const RW: i32 = 388;
const CX: i32 = RX + 172;
const CHIP_W: i32 = 190;
const ROW: i32 = 34;
const LIST_ROWS: usize = 6;
const SWATCHES: [u32; 8] = [0x4285F4, 0xDB4437, 0xF4B400, 0x0F9D58, 0xAB47BC, 0x00ACC1, 0xFF7043, 0x78909C];

const NAME: usize = 0;
const URL: usize = 1;
const OWNER: usize = 2;
const N_EDITS: usize = 3;
const ID_BASE: usize = 100;

const WM_MOUSELEAVE: u32 = 0x02A3;
const EM_SETLIMITTEXT: u32 = 0xC5;
const EN_SETFOCUS: u32 = 0x0100;
const EN_KILLFOCUS: u32 = 0x0200;
const EN_CHANGE: u32 = 0x0300;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tog {
    Sound,
    SkipFree,
}

const LEADS: [i64; 4] = [1, 2, 5, 10];

#[derive(Clone, Copy)]
enum Hit {
    Feed(usize),
    Add,
    Remove,
    TextSize(&'static str),
    Hour(bool, i32),
    Toggle(Tog),
    Monday(bool),
    Clock24(bool),
    Lead(i64),
    Theme(&'static str),
    Tone(usize),
    Save,
    Cancel,
}

struct State {
    hwnd: HWND,
    dpi: u32,
    fonts: Fonts,
    pal: Pal,
    brush: HBRUSH,
    cfg: Config,
    edits: [HWND; N_EDITS],
    sel: Option<usize>,
    scroll: usize,
    theme_fonts: Vec<HFONT>,
    loading: bool,
    error: Option<String>,
    hits: Vec<(RECT, Hit)>,
    hover: Option<RECT>,
    tracking: bool,
}

thread_local! {
    static ST: RefCell<Option<State>> = const { RefCell::new(None) };
}

// try_borrow: SetWindowText fires EN_CHANGE synchronously, so a nested access must be a no-op, not a panic.
fn with_st<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    ST.with(|s| s.try_borrow_mut().ok().and_then(|mut s| s.as_mut().map(f)))
}

fn sc(v: i32, dpi: u32) -> i32 {
    gdi::scale(v, dpi)
}

fn get_text(h: HWND) -> String {
    unsafe {
        let n = GetWindowTextLengthW(h);
        if n <= 0 {
            return String::new();
        }
        let mut buf = vec![0u16; n as usize + 1];
        let got = GetWindowTextW(h, &mut buf);
        String::from_utf16_lossy(&buf[..got.max(0) as usize])
    }
}

fn set_text(h: HWND, s: &str) {
    let t = wide(s);
    unsafe {
        let _ = SetWindowTextW(h, PCWSTR(t.as_ptr()));
    }
}

fn edit_field(i: usize, dpi: u32) -> RECT {
    let s = |v| sc(v, dpi);
    match i {
        NAME => gdi::rect(s(LX + 100), s(304), s(LX + LW), s(334)),
        URL => gdi::rect(s(LX + 100), s(340), s(LX + LW), s(370)),
        _ => gdi::rect(s(LX + 100), s(376), s(LX + LW), s(406)),
    }
}

fn edit_rect(i: usize, dpi: u32) -> RECT {
    let f = edit_field(i, dpi);
    let h = sc(22, dpi);
    let pad = sc(8, dpi);
    let top = f.top + (f.bottom - f.top - h) / 2;
    gdi::rect(f.left + pad, top, f.right - pad, top + h)
}

fn visible(st: &State, _i: usize) -> bool {
    st.sel.is_some()
}

fn next_color(cfg: &Config) -> String {
    let used: Vec<u32> = cfg.feed.iter().filter_map(|f| parse_color(&f.color)).collect();
    let c = SWATCHES.iter().find(|c| !used.contains(c)).copied().unwrap_or(SWATCHES[cfg.feed.len() % SWATCHES.len()]);
    format!("#{c:06X}")
}

fn load_config() -> Config {
    let mut c: Config = std::fs::read_to_string(paths::config_file()).ok().and_then(|t| Config::parse(&t).ok()).unwrap_or_default();
    if !theme::is_known(&c.theme) {
        c.theme = theme::AUTO.into();
    }
    c
}

fn load_feed_edits(edits: &[HWND; N_EDITS], feed: Option<&FeedCfg>) {
    let f = feed.cloned().unwrap_or_default();
    set_text(edits[NAME], &f.name);
    set_text(edits[URL], &f.url);
    set_text(edits[OWNER], f.owner_email.as_deref().unwrap_or(""));
}

fn refresh_edits() {
    let Some((edits, cfg, sel)) = with_st(|s| {
        s.loading = true;
        (s.edits, s.cfg.clone(), s.sel)
    }) else {
        return;
    };
    load_feed_edits(&edits, sel.and_then(|i| cfg.feed.get(i)));
    with_st(|s| s.loading = false);
    layout_edits();
}

fn layout_edits() {
    let Some((edits, dpi, vis)) = with_st(|s| (s.edits, s.dpi, std::array::from_fn::<bool, N_EDITS, _>(|i| visible(s, i)))) else {
        return;
    };
    for (i, h) in edits.iter().enumerate() {
        let r = edit_rect(i, dpi);
        unsafe {
            let _ = SetWindowPos(*h, None, r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_NOACTIVATE);
            let _ = ShowWindow(*h, if vis[i] { SW_SHOW } else { SW_HIDE });
        }
    }
    invalidate();
}

fn invalidate() {
    if let Some(h) = with_st(|s| s.hwnd) {
        unsafe {
            let _ = InvalidateRect(Some(h), None, false);
        }
    }
}

fn commit_feed() {
    let Some((edits, sel, loading)) = with_st(|s| (s.edits, s.sel, s.loading)) else { return };
    let Some(i) = sel.filter(|_| !loading) else { return };
    let (name, url, owner) = (get_text(edits[NAME]), get_text(edits[URL]), get_text(edits[OWNER]));
    with_st(|s| {
        if let Some(f) = s.cfg.feed.get_mut(i) {
            f.name = name.trim().into();
            f.url = url.trim().into();
            f.owner_email = Some(owner.trim().to_string()).filter(|o| !o.is_empty());
        }
    });
    invalidate();
}

fn collect() -> Vec<String> {
    commit_feed();
    let cfg = with_st(|s| {
        // A calendar the user never filled in is not an error.
        s.cfg.feed.retain(|f| !(f.name.is_empty() && f.url.is_empty()));
        s.cfg.clone()
    });
    cfg.map(|c| c.validate()).unwrap_or_default()
}

fn save() {
    let errs = collect();
    if let Some(first) = errs.first() {
        let msg = if errs.len() > 1 { format!("{first} (and {} more)", errs.len() - 1) } else { first.clone() };
        with_st(|s| s.error = Some(msg));
        invalidate();
        return;
    }
    let Some(cfg) = with_st(|s| s.cfg.clone()) else { return };
    let path = paths::config_file();
    let written = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|_| {
            let tmp = path.with_extension("toml.tmp");
            std::fs::write(&tmp, cfg.to_toml())?;
            std::fs::rename(&tmp, &path)
        });
    match written {
        Ok(()) => {
            shared::send(Cmd::Refresh);
            close();
        }
        Err(e) => {
            with_st(|s| s.error = Some(format!("Could not save: {e}")));
            invalidate();
        }
    }
}

fn close() {
    if let Some(h) = with_st(|s| s.hwnd) {
        unsafe {
            let _ = DestroyWindow(h);
        }
    }
}

fn select(i: Option<usize>) {
    with_st(|s| {
        s.sel = i;
        if let Some(i) = i {
            if i < s.scroll {
                s.scroll = i;
            } else if i >= s.scroll + LIST_ROWS {
                s.scroll = i + 1 - LIST_ROWS;
            }
        }
    });
    refresh_edits();
}

fn perform(hit: Hit) {
    match hit {
        Hit::Feed(i) => select(Some(i)),
        Hit::Add => {
            let n = with_st(|s| {
                let color = next_color(&s.cfg);
                s.cfg.feed.push(FeedCfg { color, ..Default::default() });
                s.cfg.feed.len() - 1
            });
            select(n);
            if let Some(h) = with_st(|s| s.edits[URL]) {
                unsafe {
                    let _ = SetFocus(Some(h));
                }
            }
        }
        Hit::Remove => {
            with_st(|s| {
                if let Some(i) = s.sel.filter(|i| *i < s.cfg.feed.len()) {
                    s.cfg.feed.remove(i);
                    s.sel = if s.cfg.feed.is_empty() { None } else { Some(i.min(s.cfg.feed.len() - 1)) };
                    s.scroll = s.scroll.min(s.cfg.feed.len().saturating_sub(LIST_ROWS));
                }
            });
            refresh_edits();
        }
        Hit::Hour(is_end, delta) => {
            with_st(|s| {
                let c = &mut s.cfg;
                if is_end {
                    c.day_end = (c.day_end as i32 + delta).clamp((c.day_start + MIN_DAY_HOURS) as i32, 24) as u32;
                } else {
                    c.day_start = (c.day_start as i32 + delta).clamp(0, c.day_end as i32 - MIN_DAY_HOURS as i32) as u32;
                }
            });
            invalidate();
        }
        Hit::TextSize(id) => {
            with_st(|s| s.cfg.text_size = id.into());
            apply_theme();
        }
        Hit::Toggle(t) => {
            with_st(|s| match t {
                Tog::Sound => s.cfg.alert.play_sound = !s.cfg.alert.play_sound,
                Tog::SkipFree => s.cfg.alert.skip_free = !s.cfg.alert.skip_free,
            });
            invalidate();
        }
        Hit::Monday(m) => {
            with_st(|s| s.cfg.week_start = if m { "monday" } else { "sunday" }.into());
            invalidate();
        }
        Hit::Clock24(h24) => {
            with_st(|s| s.cfg.clock = if h24 { "24h" } else { "12h" }.into());
            invalidate();
        }
        Hit::Lead(m) => {
            with_st(|s| s.cfg.alert.lead_minutes = m);
            invalidate();
        }
        Hit::Theme(id) => {
            with_st(|s| s.cfg.theme = id.into());
            apply_theme();
        }
        Hit::Tone(i) => {
            if with_st(|s| tone_index(&s.cfg.theme)) != Some(i) {
                with_st(|s| s.cfg.theme = tone_default(i).into());
                apply_theme();
            }
        }
        Hit::Save => save(),
        Hit::Cancel => close(),
    }
}

fn pill(hdc: HDC, r: RECT, hex: u32, radius: i32) {
    gdi::round_fill(hdc, &r, hex, radius);
}

fn field_box(hdc: HDC, st: &State, r: &RECT, focused: bool) {
    let rad = sc(5, st.dpi);
    pill(hdc, *r, if focused { st.pal.accent } else { st.pal.line }, rad);
    pill(hdc, gdi::rect(r.left + 1, r.top + 1, r.right - 1, r.bottom - 1), st.pal.panel, rad);
}

fn draw_button(hdc: HDC, st: &State, hits: &mut Vec<(RECT, Hit)>, r: RECT, label: &str, hit: Hit, primary: bool) {
    let rad = sc(6, st.dpi);
    let hot = st.hover == Some(r);
    if primary {
        pill(hdc, r, if hot { gdi::blend(st.pal.on_accent, st.pal.accent, 0.15) } else { st.pal.accent }, rad);
    } else {
        pill(hdc, r, st.pal.line, rad);
        let bg = if hot { gdi::blend(st.pal.text, st.pal.bg, 0.10) } else { st.pal.bg };
        pill(hdc, gdi::rect(r.left + 1, r.top + 1, r.right - 1, r.bottom - 1), bg, rad);
    }
    let fg = if primary { st.pal.on_accent } else { st.pal.text };
    gdi::text(hdc, label, r, fg, st.fonts.bold, DT_SINGLELINE | DT_CENTER | DT_VCENTER);
    hits.push((r, hit));
}

fn draw_toggle(hdc: HDC, st: &State, hits: &mut Vec<(RECT, Hit)>, x: i32, y: i32, on: bool, tog: Tog) {
    let s = |v| sc(v, st.dpi);
    let track = gdi::rect(s(x), s(y + 6), s(x + 40), s(y + 28));
    let hit = gdi::rect(s(x), s(y), s(x + 40), s(y + 34));
    let base = if on { st.pal.accent } else { st.pal.line };
    pill(hdc, track, if st.hover == Some(hit) { gdi::blend(st.pal.text, base, 0.18) } else { base }, s(11));
    let kx = if on { x + 40 - 20 } else { x + 2 };
    gdi::ellipse(hdc, &gdi::rect(s(kx), s(y + 8), s(kx + 18), s(y + 26)), if on { st.pal.on_accent } else { st.pal.panel });
    hits.push((hit, Hit::Toggle(tog)));
}

fn draw_segments(hdc: HDC, st: &State, hits: &mut Vec<(RECT, Hit)>, x: i32, y: i32, seg_w: i32, labels: &[&str], active: usize, make: impl Fn(usize) -> Hit) {
    let s = |v| sc(v, st.dpi);
    let whole = gdi::rect(s(x), s(y + 2), s(x + seg_w * labels.len() as i32), s(y + 32));
    pill(hdc, whole, st.pal.line, s(6));
    pill(hdc, gdi::rect(whole.left + 1, whole.top + 1, whole.right - 1, whole.bottom - 1), st.pal.panel, s(6));
    for (i, label) in labels.iter().enumerate() {
        let r = gdi::rect(s(x + seg_w * i as i32), s(y + 2), s(x + seg_w * (i as i32 + 1)), s(y + 32));
        let inner = gdi::rect(r.left + 2, r.top + 2, r.right - 2, r.bottom - 2);
        if i == active {
            let c = if st.hover == Some(r) { gdi::blend(st.pal.on_accent, st.pal.accent, 0.15) } else { st.pal.accent };
            pill(hdc, inner, c, s(5));
        } else if st.hover == Some(r) {
            pill(hdc, inner, gdi::blend(st.pal.text, st.pal.panel, 0.12), s(5));
        }
        let fg = if i == active { st.pal.on_accent } else { st.pal.text };
        gdi::text(hdc, label, r, fg, st.fonts.bold, DT_SINGLELINE | DT_CENTER | DT_VCENTER);
        hits.push((r, make(i)));
    }
}

fn label(hdc: HDC, st: &State, x: i32, y: i32, text: &str, dim: bool) {
    let s = |v| sc(v, st.dpi);
    let fg = if dim { st.pal.dim } else { st.pal.text };
    gdi::text(hdc, text, gdi::rect(s(x), s(y), s(x + 190), s(y + ROW)), fg, st.fonts.regular, DT_SINGLELINE | DT_VCENTER);
}

fn header(hdc: HDC, st: &State, x: i32, y: i32, w: i32, text: &str) {
    let s = |v| sc(v, st.dpi);
    gdi::text(hdc, text, gdi::rect(s(x), s(y), s(x + w), s(y + 24)), st.pal.text, st.fonts.title, DT_SINGLELINE | DT_VCENTER);
    gdi::fill(hdc, &gdi::rect(s(x), s(y + 28), s(x + w), s(y + 29)), st.pal.grid);
}

fn draw(hdc: HDC, rc: RECT, st: &mut State) {
    let s = |v| sc(v, st.dpi);
    let mut hits: Vec<(RECT, Hit)> = Vec::new();
    gdi::fill(hdc, &rc, st.pal.bg);
    let focus = unsafe { GetFocus() };

    header(hdc, st, LX, 14, LW, "Calendars");
    header(hdc, st, RX, 14, RW, "Preferences");
    header(hdc, st, RX, 312, RW, "Theme");

    let list = gdi::rect(s(LX), s(46), s(LX + LW), s(46 + ROW * LIST_ROWS as i32 + 8));
    field_box(hdc, st, &list, false);
    for (row, i) in (st.scroll..st.cfg.feed.len().min(st.scroll + LIST_ROWS)).enumerate() {
        let f = &st.cfg.feed[i];
        let r = gdi::rect(list.left + s(4), list.top + s(4) + s(ROW) * row as i32, list.right - s(4), list.top + s(4) + s(ROW) * (row as i32 + 1));
        if st.sel == Some(i) {
            pill(hdc, r, gdi::blend(st.pal.accent, st.pal.panel, 0.22), s(5));
        } else if st.hover == Some(r) {
            pill(hdc, r, gdi::blend(st.pal.text, st.pal.panel, 0.08), s(5));
        }
        let dot = parse_color(&f.color).unwrap_or(st.pal.grid);
        let cy = (r.top + r.bottom) / 2;
        gdi::ellipse(hdc, &gdi::rect(r.left + s(10), cy - s(6), r.left + s(22), cy + s(6)), dot);
        let (name, fg) = if f.name.is_empty() { ("New calendar", st.pal.dim) } else { (f.name.as_str(), st.pal.text) };
        gdi::text(hdc, name, gdi::rect(r.left + s(32), r.top, r.right - s(8), r.bottom), fg, st.fonts.regular, DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS);
        hits.push((r, Hit::Feed(i)));
    }
    if st.cfg.feed.is_empty() {
        gdi::text(hdc, "No calendars yet", list, st.pal.dim, st.fonts.regular, DT_SINGLELINE | DT_CENTER | DT_VCENTER);
    }
    draw_button(hdc, st, &mut hits, gdi::rect(s(LX), s(264), s(LX + 96), s(294)), "Add", Hit::Add, false);
    if st.sel.is_some() {
        draw_button(hdc, st, &mut hits, gdi::rect(s(LX + 104), s(264), s(LX + 200), s(294)), "Remove", Hit::Remove, false);
    }

    if st.sel.is_some() {
        for (y, text) in [(302, "Name"), (338, "Address"), (374, "Your email")] {
            gdi::text(hdc, text, gdi::rect(s(LX), s(y), s(LX + 100), s(y + 34)), st.pal.text, st.fonts.regular, DT_SINGLELINE | DT_VCENTER);
        }
        for i in [NAME, URL, OWNER] {
            field_box(hdc, st, &edit_field(i, st.dpi), focus == st.edits[i]);
        }
    }
    let help = "Google Calendar: open Settings, pick your calendar, then Integrate calendar, and copy the Secret address in iCal format. Anyone with the address can read your calendar, so keep it private.";
    gdi::text(hdc, help, gdi::rect(s(LX), s(426), s(LX + LW), s(536)), st.pal.dim, st.fonts.regular, DT_WORDBREAK);

    label(hdc, st, RX, 48, "Week starts on", false);
    draw_segments(hdc, st, &mut hits, CX, 48, 84, &["Sunday", "Monday"], st.cfg.monday_first() as usize, |i| Hit::Monday(i == 1));
    label(hdc, st, RX, 84, "Clock", false);
    draw_segments(hdc, st, &mut hits, CX, 84, 84, &["12-hour", "24-hour"], st.cfg.clock_24h() as usize, |i| Hit::Clock24(i == 1));
    label(hdc, st, RX, 120, "Minutes before start", false);
    let lead = LEADS.iter().position(|m| *m == st.cfg.alert.lead_minutes).unwrap_or(usize::MAX);
    draw_segments(hdc, st, &mut hits, CX, 120, 52, &["1", "2", "5", "10"], lead, |i| Hit::Lead(LEADS[i]));
    label(hdc, st, RX, 156, "Play a sound", false);
    draw_toggle(hdc, st, &mut hits, CX, 156, st.cfg.alert.play_sound, Tog::Sound);
    label(hdc, st, RX, 192, "Skip free events", false);
    draw_toggle(hdc, st, &mut hits, CX, 192, st.cfg.alert.skip_free, Tog::SkipFree);
    label(hdc, st, RX, 264, "Day view", false);
    let h24 = st.cfg.clock_24h();
    let mut x = RX + 84;
    for (is_end, hour) in [(false, st.cfg.day_start), (true, st.cfg.day_end)] {
        draw_button(hdc, st, &mut hits, gdi::rect(s(x), s(266), s(x + 30), s(296)), "\u{2212}", Hit::Hour(is_end, -1), false);
        gdi::text(hdc, &hour_label(hour, h24), gdi::rect(s(x + 30), s(266), s(x + 30 + 62), s(296)), st.pal.text, st.fonts.bold, DT_SINGLELINE | DT_CENTER | DT_VCENTER);
        draw_button(hdc, st, &mut hits, gdi::rect(s(x + 92), s(266), s(x + 122), s(296)), "+", Hit::Hour(is_end, 1), false);
        x += 122;
        if !is_end {
            gdi::text(hdc, "to", gdi::rect(s(x), s(266), s(x + 30), s(296)), st.pal.dim, st.fonts.regular, DT_SINGLELINE | DT_CENTER | DT_VCENTER);
            x += 30;
        }
    }
    label(hdc, st, RX, 228, "Text size", false);
    let size = TEXT_SIZES.iter().position(|t| t.0.eq_ignore_ascii_case(&st.cfg.text_size)).unwrap_or(usize::MAX);
    let size_labels: Vec<&str> = TEXT_SIZES.iter().map(|t| t.1).collect();
    draw_segments(hdc, st, &mut hits, RX + 84, 228, 90, &size_labels, size, |i| Hit::TextSize(TEXT_SIZES[i].0));

    let shown = tone_index(&st.cfg.theme);
    draw_segments(hdc, st, &mut hits, RX, 350, RW / 4, &["Auto", "Light", "Midtone", "Dark"], shown, Hit::Tone);
    if shown == 0 {
        let r = gdi::rect(s(RX), s(396), s(RX + RW), s(456));
        gdi::text(hdc, "Follows your Windows light or dark setting.", r, st.pal.dim, st.fonts.regular, DT_WORDBREAK);
    }
    for (i, c) in chips().iter().enumerate().filter(|(_, c)| shown != 0 && TONES[shown - 1] == c.theme.tone) {
        let r = chip_rect(c, st.dpi);
        let selected = c.id.eq_ignore_ascii_case(&st.cfg.theme);
        pill(hdc, r, if selected { st.pal.accent } else { st.pal.line }, s(5));
        let inset = if selected { 2 } else { 1 };
        let fill = if st.hover == Some(r) { gdi::blend(st.pal.text, st.pal.panel, 0.10) } else { st.pal.panel };
        pill(hdc, gdi::rect(r.left + inset, r.top + inset, r.right - inset, r.bottom - inset), fill, s(4));
        let (colors, widths) = stripes(c);
        let (mut x, top) = (r.left + s(9), r.top + s(5));
        gdi::fill(hdc, &gdi::rect(x - 1, top - 1, x + s(34) + 1, top + s(16) + 1), st.pal.line);
        for (color, wd) in colors.iter().zip(widths) {
            gdi::fill(hdc, &gdi::rect(x, top, x + s(wd), top + s(16)), *color);
            x += s(wd);
        }
        let font = st.theme_fonts.get(i).copied().unwrap_or(st.fonts.regular);
        gdi::text(hdc, c.label, gdi::rect(r.left + s(52), r.top, r.right - s(6), r.bottom), st.pal.text, font, DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS);
        hits.push((r, Hit::Theme(c.id)));
    }

    gdi::fill(hdc, &gdi::rect(0, s(540), rc.right, s(541)), st.pal.line);
    if let Some(e) = &st.error {
        gdi::text(hdc, e, gdi::rect(s(LX), s(548), s(640), s(592)), st.pal.now, st.fonts.regular, DT_WORDBREAK | DT_VCENTER);
    }
    draw_button(hdc, st, &mut hits, gdi::rect(s(W - 24 - 96 - 8 - 96), s(552), s(W - 24 - 96 - 8), s(584)), "Cancel", Hit::Cancel, false);
    draw_button(hdc, st, &mut hits, gdi::rect(s(W - 24 - 96), s(552), s(W - 24), s(584)), "Save", Hit::Save, true);
    st.hits = hits;
}

fn paint(hwnd: HWND) {
    gdi::paint_buffered(hwnd, |hdc, rc| {
        with_st(|st| draw(hdc, rc, st));
    });
}

fn click(x: i32, y: i32) {
    let hit = with_st(|s| s.hits.iter().rev().find(|(r, _)| gdi::contains(r, x, y)).map(|(_, h)| *h)).flatten();
    if let Some(h) = hit {
        perform(h);
    }
}

fn on_command(wp: WPARAM) {
    let id = (wp.0 & 0xFFFF).wrapping_sub(ID_BASE);
    let code = ((wp.0 >> 16) & 0xFFFF) as u32;
    if id >= N_EDITS {
        return;
    }
    match code {
        EN_CHANGE if matches!(id, NAME | URL | OWNER) => commit_feed(),
        EN_CHANGE | EN_SETFOCUS | EN_KILLFOCUS => invalidate(),
        _ => {}
    }
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            paint(hwnd);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_COMMAND => {
            on_command(wp);
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let (x, y) = ((lp.0 & 0xFFFF) as i16 as i32, ((lp.0 >> 16) & 0xFFFF) as i16 as i32);
            let moved = with_st(|s| {
                let now = s.hits.iter().rev().find(|(r, _)| gdi::contains(r, x, y)).map(|(r, _)| *r);
                let changed = s.hover != now;
                s.hover = now;
                (changed, !std::mem::replace(&mut s.tracking, true))
            });
            if let Some((changed, track)) = moved {
                if track {
                    let mut t = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                    let _ = TrackMouseEvent(&mut t);
                }
                if changed {
                    invalidate();
                }
            }
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            with_st(|s| {
                s.hover = None;
                s.tracking = false;
            });
            invalidate();
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            click((lp.0 & 0xFFFF) as i16 as i32, ((lp.0 >> 16) & 0xFFFF) as i16 as i32);
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            let up = (wp.0 >> 16) as i16 > 0;
            with_st(|s| {
                let max = s.cfg.feed.len().saturating_sub(LIST_ROWS);
                s.scroll = if up { s.scroll.saturating_sub(1) } else { (s.scroll + 1).min(max) };
            });
            invalidate();
            LRESULT(0)
        }
        WM_CTLCOLOREDIT => {
            match with_st(|s| (s.pal, s.brush)) {
                Some((pal, brush)) => {
                    let hdc = HDC(wp.0 as *mut _);
                    SetTextColor(hdc, gdi::rgb(pal.text));
                    SetBkColor(hdc, gdi::rgb(pal.panel));
                    LRESULT(brush.0 as isize)
                }
                None => DefWindowProcW(hwnd, msg, wp, lp),
            }
        }
        WM_SETTINGCHANGE => {
            apply_theme();
            LRESULT(0)
        }
        WM_DPICHANGED => {
            let r = &*(lp.0 as *const RECT);
            let _ = SetWindowPos(hwnd, None, r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_NOACTIVATE);
            let dpi = gdi::dpi_of(hwnd);
            let face = with_st(|s| s.pal.font).unwrap_or("Segoe UI");
            let (fonts, theme_fonts) = (Fonts::new(dpi, face, with_st(|s| s.cfg.text_scale()).unwrap_or(100)), build_theme_fonts(dpi));
            let old = with_st(|s| {
                s.dpi = dpi;
                (std::mem::replace(&mut s.fonts, fonts), std::mem::replace(&mut s.theme_fonts, theme_fonts))
            });
            if let Some((old, old_theme)) = old {
                old.destroy();
                for f in old_theme {
                    unsafe {
                        let _ = DeleteObject(HGDIOBJ(f.0));
                    }
                }
            }
            apply_fonts();
            layout_edits();
            LRESULT(0)
        }
        WM_CLOSE => {
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            let st = ST.with(|s| s.borrow_mut().take());
            if let Some(st) = st {
                st.fonts.destroy();
                for f in st.theme_fonts {
                    let _ = DeleteObject(HGDIOBJ(f.0));
                }
                let _ = DeleteObject(HGDIOBJ(st.brush.0));
            }
            super::app::trim_memory();
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

fn apply_theme() {
    let Some(id) = with_st(|s| s.cfg.theme.clone()) else { return };
    let pal = pal_for(&id, system_dark());
    let rebuilt = with_st(|s| s.fonts.face != pal.font || s.fonts.scale != s.cfg.text_scale()).unwrap_or(false).then(|| with_st(|s| Fonts::new(s.dpi, pal.font, s.cfg.text_scale()))).flatten();
    let old = with_st(|s| {
        s.pal = pal;
        let old_fonts = rebuilt.map(|f| std::mem::replace(&mut s.fonts, f));
        (std::mem::replace(&mut s.brush, unsafe { CreateSolidBrush(gdi::rgb(pal.panel)) }), old_fonts)
    });
    if let Some(((old_brush, old_fonts), hwnd)) = old.zip(with_st(|s| s.hwnd)) {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(old_brush.0));
        }
        if let Some(f) = old_fonts {
            f.destroy();
        }
        apply_fonts();
        apply_title_bar(hwnd, pal.dark);
    }
    invalidate();
}

struct Chip {
    id: &'static str,
    label: &'static str,
    font: &'static str,
    theme: &'static Theme,
    col: i32,
    row: i32,
}

const TONES: [Tone; 3] = [Tone::Light, Tone::Midtone, Tone::Dark];

fn chips() -> Vec<Chip> {
    let mut v = Vec::new();
    for tone in TONES {
        for (k, t) in THEMES.iter().filter(|t| t.tone == tone).enumerate() {
            v.push(Chip { id: t.id, label: t.label, font: t.font, theme: t, col: (k % 2) as i32, row: (k / 2) as i32 });
        }
    }
    v
}

// 0 = Auto, 1..=3 = Light, Midtone, Dark.
fn tone_index(id: &str) -> usize {
    match theme::find(id) {
        None => 0,
        Some(t) => TONES.iter().position(|x| *x == t.tone).map_or(0, |i| i + 1),
    }
}

fn tone_default(i: usize) -> &'static str {
    match i {
        1 => "light",
        2 => THEMES.iter().find(|t| t.tone == Tone::Midtone).map_or("light", |t| t.id),
        3 => "dark",
        _ => theme::AUTO,
    }
}

fn build_theme_fonts(dpi: u32) -> Vec<HFONT> {
    chips().iter().map(|c| gdi::themed_font(c.font, 15, 500, dpi)).collect()
}

fn chip_rect(c: &Chip, dpi: u32) -> RECT {
    let x = RX + c.col * (CHIP_W + 8);
    let y = 396 + c.row * 28;
    gdi::rect(sc(x, dpi), sc(y, dpi), sc(x + CHIP_W, dpi), sc(y + 26, dpi))
}

fn stripes(c: &Chip) -> ([u32; 4], [i32; 4]) {
    let t = c.theme;
    ([t.bg, t.panel, t.accent, t.ok], [12, 10, 6, 6])
}

fn apply_fonts() {
    if let Some((edits, font)) = with_st(|s| (s.edits, s.fonts.regular)) {
        for h in edits {
            unsafe {
                SendMessageW(h, WM_SETFONT, Some(WPARAM(font.0 as usize)), Some(LPARAM(1)));
            }
        }
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
        lpszClassName: w!("skedaddle.settings"),
        ..Default::default()
    };
    unsafe {
        RegisterClassExW(&wc);
    }
}

pub fn is_open() -> bool {
    ST.with(|s| s.borrow().is_some())
}

pub fn show() {
    if let Some(h) = with_st(|s| s.hwnd) {
        unsafe {
            let _ = ShowWindow(h, if IsIconic(h).as_bool() { SW_RESTORE } else { SW_SHOW });
            let _ = SetForegroundWindow(h);
        }
        return;
    }
    if is_open() {
        return;
    }
    unsafe {
        let hinst: HINSTANCE = GetModuleHandleW(None).unwrap_or_default().into();
        let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU;
        let hwnd = CreateWindowExW(WINDOW_EX_STYLE(0), w!("skedaddle.settings"), w!("Skedaddle settings"), style, 0, 0, 100, 100, None, None, Some(hinst), None).unwrap_or_default();
        if hwnd.0.is_null() {
            return;
        }
        let dpi = gdi::dpi_of(hwnd);
        let mut outer = gdi::rect(0, 0, sc(W, dpi), sc(H, dpi));
        let _ = AdjustWindowRectExForDpi(&mut outer, style, false, WINDOW_EX_STYLE(0), dpi);
        let (ow, oh) = (outer.right - outer.left, outer.bottom - outer.top);
        let work = gdi::cursor_work_area();
        let x = work.left + ((work.right - work.left - ow) / 2).max(0);
        let y = work.top + ((work.bottom - work.top - oh) / 2).max(0);
        let _ = SetWindowPos(hwnd, None, x, y, ow, oh, SWP_NOZORDER | SWP_NOACTIVATE);

        let mut cfg = load_config();
        if cfg.feed.is_empty() {
            let color = next_color(&cfg);
            cfg.feed.push(FeedCfg { color, ..Default::default() });
        }
        let dark = system_dark();
        let pal = pal_for(&cfg.theme, dark);
        let mut edits = [HWND::default(); N_EDITS];
        for (i, slot) in edits.iter_mut().enumerate() {
            let es = WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(ES_AUTOHSCROLL as u32);
            let r = edit_rect(i, dpi);
            *slot = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("EDIT"),
                PCWSTR::null(),
                es,
                r.left,
                r.top,
                r.right - r.left,
                r.bottom - r.top,
                Some(hwnd),
                Some(HMENU((ID_BASE + i) as *mut _)),
                Some(hinst),
                None,
            )
            .unwrap_or_default();
            let limit = 2048;
            SendMessageW(*slot, EM_SETLIMITTEXT, Some(WPARAM(limit)), Some(LPARAM(0)));
        }
        let sel = Some(0);
        let theme_fonts = build_theme_fonts(dpi);
        ST.with(|s| {
            *s.borrow_mut() = Some(State {
                hwnd,
                dpi,
                fonts: Fonts::new(dpi, pal.font, cfg.text_scale()),
                pal,
                brush: CreateSolidBrush(gdi::rgb(pal.panel)),
                cfg,
                edits,
                sel,
                scroll: 0,
                theme_fonts,
                loading: false,
                error: None,
                hits: Vec::new(),
                hover: None,
                tracking: false,
            })
        });
        apply_fonts();
        refresh_edits();
        apply_title_bar(hwnd, pal.dark);
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
        if let Some(h) = with_st(|s| s.edits[URL]) {
            let _ = SetFocus(Some(h));
        }
    }
}

// Returns true when the message was consumed (Tab, Enter and Esc inside the settings window).
pub fn pre_translate(msg: &MSG) -> bool {
    let Some(hwnd) = ST.with(|s| s.try_borrow().ok().and_then(|s| s.as_ref().map(|s| s.hwnd))) else {
        return false;
    };
    unsafe {
        if msg.hwnd != hwnd && !IsChild(hwnd, msg.hwnd).as_bool() {
            return false;
        }
        if msg.message == WM_KEYDOWN {
            if msg.wParam.0 == VK_RETURN.0 as usize {
                save();
                return true;
            }
            if msg.wParam.0 == VK_ESCAPE.0 as usize {
                close();
                return true;
            }
        }
        IsDialogMessageW(hwnd, msg).as_bool()
    }
}

fn hour_label(h: u32, h24: bool) -> String {
    if h24 {
        return format!("{:02}:00", h % 24);
    }
    let (n, ap) = (if h % 12 == 0 { 12 } else { h % 12 }, if h % 24 < 12 { "AM" } else { "PM" });
    format!("{n} {ap}")
}
