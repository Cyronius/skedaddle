use std::cell::RefCell;

use windows::core::w;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, PAINTSTRUCT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::Win32::UI::WindowsAndMessaging::*;

use super::gdi;

const TOTAL_MS: u64 = 1200;
const PULSES: f32 = 1.0;
const MAX_ALPHA: f32 = 130.0;

struct State {
    color: u32,
    started: u64,
    wins: Vec<isize>,
    registered: bool,
}

thread_local! {
    static FLASH: RefCell<State> = const { RefCell::new(State { color: 0x2F80ED, started: 0, wins: Vec::new(), registered: false }) };
}

fn alpha_at(elapsed: u64) -> u8 {
    let phase = (elapsed as f32 / TOTAL_MS as f32 * PULSES).fract();
    let tri = if phase < 0.5 { phase * 2.0 } else { 2.0 - phase * 2.0 };
    (tri * MAX_ALPHA) as u8
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_TIMER => {
            let (started, gone) = FLASH.with(|f| {
                let f = f.borrow();
                (f.started, GetTickCount64().saturating_sub(f.started) >= TOTAL_MS)
            });
            if gone {
                let _ = KillTimer(Some(hwnd), 1);
                FLASH.with(|f| f.borrow_mut().wins.retain(|w| *w != hwnd.0 as isize));
                let _ = DestroyWindow(hwnd);
            } else {
                let a = alpha_at(GetTickCount64().saturating_sub(started));
                let _ = SetLayeredWindowAttributes(hwnd, windows::Win32::Foundation::COLORREF(0), a, LWA_ALPHA);
            }
            LRESULT(0)
        }
        WM_PAINT => {
            let color = FLASH.with(|f| f.borrow().color);
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            let mut rc = RECT::default();
            let _ = GetClientRect(hwnd, &mut rc);
            gdi::fill(hdc, &rc, color);
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_NCHITTEST => LRESULT(-1),
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

fn register(hinst: HINSTANCE) {
    let already = FLASH.with(|f| std::mem::replace(&mut f.borrow_mut().registered, true));
    if already {
        return;
    }
    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(proc),
        hInstance: hinst,
        lpszClassName: w!("skedaddle.flash"),
        ..Default::default()
    };
    unsafe {
        RegisterClassExW(&wc);
    }
}

pub fn start(color: u32) {
    unsafe {
        let hinst: HINSTANCE = GetModuleHandleW(None).unwrap_or_default().into();
        register(hinst);
        let old: Vec<isize> = FLASH.with(|f| std::mem::take(&mut f.borrow_mut().wins));
        for h in old {
            let _ = DestroyWindow(HWND(h as *mut _));
        }
        FLASH.with(|f| {
            let mut f = f.borrow_mut();
            f.color = color;
            f.started = GetTickCount64();
        });
        for r in gdi::monitor_rects() {
            let ex = WS_EX_TOPMOST | WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW;
            let Ok(hwnd) = CreateWindowExW(ex, w!("skedaddle.flash"), w!(""), WS_POPUP, r.left, r.top, r.right - r.left, r.bottom - r.top, None, None, Some(hinst), None) else {
                continue;
            };
            let _ = SetLayeredWindowAttributes(hwnd, windows::Win32::Foundation::COLORREF(0), 0, LWA_ALPHA);
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOACTIVATE | SWP_SHOWWINDOW);
            SetTimer(Some(hwnd), 1, 30, None);
            FLASH.with(|f| f.borrow_mut().wins.push(hwnd.0 as isize));
        }
    }
}
