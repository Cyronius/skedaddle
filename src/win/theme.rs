use skedaddle::theme::{self, Theme};
use windows::core::{w, BOOL};
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE};
use super::shared;
use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};

#[derive(Clone, Copy)]
pub struct Pal {
    pub bg: u32,
    pub panel: u32,
    pub line: u32,
    pub grid: u32,
    pub text: u32,
    pub dim: u32,
    pub accent: u32,
    pub now: u32,
    pub ok: u32,
    pub on_accent: u32,
    pub on_ok: u32,
    pub dark: bool,
    pub font: &'static str,
}

impl Pal {
    pub fn of(t: &Theme) -> Pal {
        Pal {
            bg: t.bg,
            panel: t.panel,
            line: t.line,
            grid: t.grid,
            text: t.text,
            dim: t.dim,
            accent: t.accent,
            now: t.err,
            ok: t.ok,
            on_accent: t.on_accent(),
            on_ok: t.on_ok(),
            dark: t.is_dark(),
            font: t.font,
        }
    }
}

pub fn pal_for(id: &str, system_dark: bool) -> Pal {
    Pal::of(theme::resolve(id, system_dark))
}

pub fn current() -> Pal {
    pal_for(&shared::config().theme, system_dark())
}

pub fn system_dark() -> bool {
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

pub fn apply_title_bar(hwnd: HWND, dark: bool) {
    let v: BOOL = dark.into();
    unsafe {
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, &v as *const _ as *const _, 4);
    }
}
