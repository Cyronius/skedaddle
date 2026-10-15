use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, POINT, RECT, TRUE};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, SW_SHOWNORMAL};

use super::wide;

pub fn rgb(hex: u32) -> COLORREF {
    COLORREF(((hex & 0xFF) << 16) | (hex & 0xFF00) | ((hex >> 16) & 0xFF))
}

pub fn blend(top: u32, under: u32, t: f32) -> u32 {
    let ch = |shift: u32| {
        let a = ((top >> shift) & 0xFF) as f32;
        let b = ((under >> shift) & 0xFF) as f32;
        ((a * t + b * (1.0 - t)).round() as u32) & 0xFF
    };
    (ch(16) << 16) | (ch(8) << 8) | ch(0)
}

pub fn scale(v: i32, dpi: u32) -> i32 {
    v * dpi as i32 / 96
}

pub fn rect(l: i32, t: i32, r: i32, b: i32) -> RECT {
    RECT { left: l, top: t, right: r, bottom: b }
}

pub fn contains(r: &RECT, x: i32, y: i32) -> bool {
    x >= r.left && x < r.right && y >= r.top && y < r.bottom
}

pub fn fill(hdc: HDC, r: &RECT, hex: u32) {
    unsafe {
        SetDCBrushColor(hdc, rgb(hex));
        FillRect(hdc, r, HBRUSH(GetStockObject(DC_BRUSH).0));
    }
}

pub fn round_fill(hdc: HDC, r: &RECT, hex: u32, radius: i32) {
    unsafe {
        SetDCBrushColor(hdc, rgb(hex));
        let old_brush = SelectObject(hdc, GetStockObject(DC_BRUSH));
        let old_pen = SelectObject(hdc, GetStockObject(NULL_PEN));
        let _ = RoundRect(hdc, r.left, r.top, r.right + 1, r.bottom + 1, radius * 2, radius * 2);
        SelectObject(hdc, old_pen);
        SelectObject(hdc, old_brush);
    }
}

pub fn outline(hdc: HDC, r: &RECT, hex: u32, thick: i32) {
    fill(hdc, &rect(r.left, r.top, r.right, r.top + thick), hex);
    fill(hdc, &rect(r.left, r.bottom - thick, r.right, r.bottom), hex);
    fill(hdc, &rect(r.left, r.top, r.left + thick, r.bottom), hex);
    fill(hdc, &rect(r.right - thick, r.top, r.right, r.bottom), hex);
}

pub struct Fonts {
    pub face: &'static str,
    pub scale: i32,
    pub regular: HFONT,
    pub bold: HFONT,
    pub small: HFONT,
    pub small_bold: HFONT,
    pub event: HFONT,
    pub event_bold: HFONT,
    pub title: HFONT,
    pub big: HFONT,
    pub arrow: HFONT,
}

fn font(px: i32, weight: i32, dpi: u32) -> HFONT {
    unsafe {
        CreateFontW(
            -scale(px, dpi),
            0,
            0,
            0,
            weight,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            0,
            w!("Segoe UI"),
        )
    }
}

pub fn themed_font(face: &str, px: i32, weight: i32, dpi: u32) -> HFONT {
    let name = wide(face);
    unsafe {
        let f = CreateFontW(-scale(px, dpi), 0, 0, 0, weight, 0, 0, 0, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY, 0, PCWSTR(name.as_ptr()));
        let hdc = GetDC(None);
        let old = SelectObject(hdc, HGDIOBJ(f.0));
        let mut buf = [0u16; 64];
        let n = GetTextFaceW(hdc, Some(&mut buf)).max(0) as usize;
        SelectObject(hdc, old);
        ReleaseDC(None, hdc);
        let got = String::from_utf16_lossy(&buf[..n.min(63)]);
        if got.trim_end_matches('\0').eq_ignore_ascii_case(face) {
            f
        } else {
            let _ = DeleteObject(HGDIOBJ(f.0));
            font(px, weight, dpi)
        }
    }
}

impl Fonts {
    pub fn new(dpi: u32, face: &'static str, scale: i32) -> Fonts {
        let f = |px: i32, weight| themed_font(face, (px * scale + 50) / 100, weight, dpi);
        Fonts { face, scale, regular: f(14, 400), bold: f(14, 600), small: f(12, 400), small_bold: f(12, 600), event: f(14, 400), event_bold: f(14, 600), title: f(20, 600), big: f(26, 400), arrow: f(30, 700) }
    }

    pub fn destroy(&self) {
        for f in [self.regular, self.bold, self.small, self.small_bold, self.event, self.event_bold, self.title, self.big, self.arrow] {
            unsafe {
                let _ = DeleteObject(HGDIOBJ(f.0));
            }
        }
    }
}

pub fn text(hdc: HDC, s: &str, mut r: RECT, hex: u32, font: HFONT, flags: DRAW_TEXT_FORMAT) {
    // DrawTextW faults on a zero-length buffer with a dangling pointer.
    if s.is_empty() {
        return;
    }
    let mut buf: Vec<u16> = s.encode_utf16().collect();
    unsafe {
        SelectObject(hdc, HGDIOBJ(font.0));
        SetTextColor(hdc, rgb(hex));
        SetBkMode(hdc, TRANSPARENT);
        let noclip = if flags.0 & DT_WORDBREAK.0 == 0 { DT_NOCLIP } else { DRAW_TEXT_FORMAT(0) };
        DrawTextW(hdc, &mut buf, &mut r, flags | DT_NOPREFIX | noclip);
    }
}

pub fn text_height(hdc: HDC, s: &str, width: i32, font: HFONT) -> i32 {
    let mut buf: Vec<u16> = s.encode_utf16().collect();
    let mut r = rect(0, 0, width, 0);
    unsafe {
        SelectObject(hdc, HGDIOBJ(font.0));
        DrawTextW(hdc, &mut buf, &mut r, DT_CALCRECT | DT_WORDBREAK | DT_NOPREFIX);
    }
    r.bottom - r.top
}

pub fn text_width(hdc: HDC, s: &str, font: HFONT) -> i32 {
    let mut buf: Vec<u16> = s.encode_utf16().collect();
    let mut r = rect(0, 0, 0, 0);
    unsafe {
        SelectObject(hdc, HGDIOBJ(font.0));
        DrawTextW(hdc, &mut buf, &mut r, DT_CALCRECT | DT_SINGLELINE | DT_NOPREFIX);
    }
    r.right - r.left
}

pub fn paint_buffered(hwnd: HWND, draw: impl FnOnce(HDC, RECT)) {
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let hdc = BeginPaint(hwnd, &mut ps);
        let mut rc = RECT::default();
        let _ = windows::Win32::UI::WindowsAndMessaging::GetClientRect(hwnd, &mut rc);
        let (w, h) = (rc.right - rc.left, rc.bottom - rc.top);
        if w > 0 && h > 0 {
            let mem = CreateCompatibleDC(Some(hdc));
            let bmp = CreateCompatibleBitmap(hdc, w, h);
            let old = SelectObject(mem, HGDIOBJ(bmp.0));
            draw(mem, rc);
            let _ = BitBlt(hdc, 0, 0, w, h, Some(mem), 0, 0, SRCCOPY);
            SelectObject(mem, old);
            let _ = DeleteObject(HGDIOBJ(bmp.0));
            let _ = DeleteDC(mem);
        }
        let _ = EndPaint(hwnd, &ps);
    }
}

pub fn dpi_of(hwnd: HWND) -> u32 {
    let d = unsafe { GetDpiForWindow(hwnd) };
    if d == 0 {
        96
    } else {
        d
    }
}

pub fn monitor_rects() -> Vec<RECT> {
    unsafe extern "system" fn cb(_: HMONITOR, _: HDC, r: *mut RECT, data: LPARAM) -> windows::core::BOOL {
        let v = &mut *(data.0 as *mut Vec<RECT>);
        v.push(*r);
        TRUE
    }
    let mut v: Vec<RECT> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(cb), LPARAM(&mut v as *mut _ as isize));
    }
    v
}

/// Work area of the monitor that holds the mouse cursor.
pub fn cursor_work_area() -> RECT {
    unsafe {
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let mon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(mon, &mut mi);
        mi.rcWork
    }
}

pub fn open_url(url: &str) {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return;
    }
    let u = wide(url);
    unsafe {
        ShellExecuteW(None, w!("open"), PCWSTR(u.as_ptr()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
    }
}

pub fn ellipse(hdc: HDC, r: &RECT, hex: u32) {
    unsafe {
        SetDCBrushColor(hdc, rgb(hex));
        let old_brush = SelectObject(hdc, GetStockObject(DC_BRUSH));
        let old_pen = SelectObject(hdc, GetStockObject(NULL_PEN));
        let _ = Ellipse(hdc, r.left, r.top, r.right + 1, r.bottom + 1);
        SelectObject(hdc, old_pen);
        SelectObject(hdc, old_brush);
    }
}

pub fn gear(hdc: HDC, cx: i32, cy: i32, radius: i32, hex: u32, hole_hex: u32) {
    const TEETH: usize = 8;
    let step = std::f32::consts::TAU / TEETH as f32;
    let (inner, outer) = (radius as f32 * 0.74, radius as f32);
    let mut pts = Vec::with_capacity(TEETH * 4);
    for k in 0..TEETH {
        let a = k as f32 * step - step / 2.0;
        for (r, off) in [(inner, 0.5 - 0.30), (outer, 0.5 - 0.17), (outer, 0.5 + 0.17), (inner, 0.5 + 0.30)] {
            let t = a + step * off;
            pts.push(POINT { x: cx + (r * t.cos()).round() as i32, y: cy + (r * t.sin()).round() as i32 });
        }
    }
    unsafe {
        SetDCBrushColor(hdc, rgb(hex));
        let old_brush = SelectObject(hdc, GetStockObject(DC_BRUSH));
        let old_pen = SelectObject(hdc, GetStockObject(NULL_PEN));
        let _ = Polygon(hdc, &pts);
        SelectObject(hdc, old_pen);
        SelectObject(hdc, old_brush);
    }
    let hole = (radius as f32 * 0.32).round() as i32;
    ellipse(hdc, &rect(cx - hole, cy - hole, cx + hole, cy + hole), hole_hex);
}

pub fn chevron(hdc: HDC, cx: i32, cy: i32, half_h: i32, left: bool, hex: u32, thick: i32) {
    let w = half_h * 6 / 10;
    let (x_tip, x_end) = if left { (cx - w / 2, cx + w / 2) } else { (cx + w / 2, cx - w / 2) };
    let pts = [POINT { x: x_end, y: cy - half_h }, POINT { x: x_tip, y: cy }, POINT { x: x_end, y: cy + half_h }];
    unsafe {
        let pen = CreatePen(PS_SOLID, thick, rgb(hex));
        let old = SelectObject(hdc, HGDIOBJ(pen.0));
        let _ = Polyline(hdc, &pts);
        SelectObject(hdc, old);
        let _ = DeleteObject(HGDIOBJ(pen.0));
    }
}
