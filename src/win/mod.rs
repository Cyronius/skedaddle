pub mod app;
pub mod autostart;
pub mod flash;
pub mod gdi;
pub mod icon;
pub mod icon_art;
pub mod install;
pub mod modal;
pub mod paths;
pub mod settings;
pub mod shared;
pub mod sync;
pub mod theme;
pub mod tray;
pub mod view;

use windows::Win32::UI::WindowsAndMessaging::WM_APP;

pub const WM_APP_TRAY: u32 = WM_APP + 1;
pub const WM_APP_SNAPSHOT: u32 = WM_APP + 2;
pub const WM_APP_SHOW: u32 = WM_APP + 3;
pub const WM_APP_MODAL_CLOSED: u32 = WM_APP + 4;

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}
