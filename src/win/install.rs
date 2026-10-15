use std::cell::RefCell;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};

use windows::core::{w, Interface, PCWSTR};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::AdjustWindowRectExForDpi;
use windows::Win32::UI::Input::KeyboardAndMouse::{TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT, VIRTUAL_KEY, VK_ESCAPE, VK_RETURN};
use windows::Win32::System::Com::{CoCreateInstance, IPersistFile, CLSCTX_INPROC_SERVER};
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteKeyW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_WRITE, REG_DWORD, REG_OPTION_NON_VOLATILE, REG_SZ,
};
use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
use windows::Win32::UI::WindowsAndMessaging::*;

use super::gdi::{self, Fonts};
use super::theme::{self, Pal};
use super::{autostart, paths, wide};

const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Skedaddle";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

fn shortcut_path() -> PathBuf {
    let appdata = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    appdata.join(r"Microsoft\Windows\Start Menu\Programs\Skedaddle.lnk")
}

pub fn is_installed() -> bool {
    std::env::current_exe().is_ok_and(|exe| same_file(&exe, &paths::installed_exe()))
}

fn message(title: &str, text: &str, flags: MESSAGEBOX_STYLE) -> MESSAGEBOX_RESULT {
    let (t, x) = (wide(title), wide(text));
    unsafe { MessageBoxW(None, PCWSTR(x.as_ptr()), PCWSTR(t.as_ptr()), flags | MB_SETFOREGROUND) }
}

pub fn error(text: &str) {
    message("Skedaddle", text, MB_OK | MB_ICONERROR);
}

fn make_shortcut(target: &Path, lnk: &Path) -> windows::core::Result<()> {
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        let exe = wide(&target.display().to_string());
        link.SetPath(PCWSTR(exe.as_ptr()))?;
        let dir = wide(&target.parent().map(|d| d.display().to_string()).unwrap_or_default());
        link.SetWorkingDirectory(PCWSTR(dir.as_ptr()))?;
        link.SetDescription(w!("Skedaddle"))?;
        let file: IPersistFile = link.cast()?;
        let path = wide(&lnk.display().to_string());
        file.Save(PCWSTR(path.as_ptr()), true)
    }
}

fn write_uninstall_entry(exe: &Path) {
    let mut key = HKEY::default();
    let path = wide(UNINSTALL_KEY);
    unsafe {
        if RegCreateKeyExW(HKEY_CURRENT_USER, PCWSTR(path.as_ptr()), None, PCWSTR::null(), REG_OPTION_NON_VOLATILE, KEY_WRITE, None, &mut key, None).is_err() {
            return;
        }
        let exe_s = exe.display().to_string();
        let text = [
            ("DisplayName", "Skedaddle".to_string()),
            ("DisplayVersion", env!("CARGO_PKG_VERSION").to_string()),
            ("Publisher", "Skedaddle".to_string()),
            ("InstallLocation", exe.parent().map(|d| d.display().to_string()).unwrap_or_default()),
            ("DisplayIcon", exe_s.clone()),
            ("UninstallString", format!("\"{exe_s}\" --uninstall")),
        ];
        for (name, value) in text {
            let (n, v) = (wide(name), wide(&value));
            let bytes = std::slice::from_raw_parts(v.as_ptr() as *const u8, v.len() * 2);
            let _ = RegSetValueExW(key, PCWSTR(n.as_ptr()), None, REG_SZ, Some(bytes));
        }
        for name in ["NoModify", "NoRepair"] {
            let n = wide(name);
            let _ = RegSetValueExW(key, PCWSTR(n.as_ptr()), None, REG_DWORD, Some(&1u32.to_le_bytes()));
        }
        let _ = RegCloseKey(key);
    }
}

pub fn install_now() -> Result<(), String> {
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let target = paths::installed_exe();
    std::fs::create_dir_all(paths::install_dir()).map_err(|e| format!("Could not create {}: {e}", paths::install_dir().display()))?;
    if !same_file(&me, &target) {
        std::fs::copy(&me, &target).map_err(|e| format!("Could not copy Skedaddle to {}: {e}", target.display()))?;
    }
    if let Some(dir) = shortcut_path().parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    make_shortcut(&target, &shortcut_path()).map_err(|e| format!("Could not create the Start Menu shortcut: {e}"))?;
    write_uninstall_entry(&target);
    autostart::repoint(&target);
    let _ = std::fs::remove_file(paths::install_declined_file());
    Ok(())
}

pub fn launch_installed() {
    // The new copy waits for this one to release the single-instance lock.
    let _ = std::process::Command::new(paths::installed_exe()).arg("--relaunch").spawn();
}

// True when Skedaddle installed itself and started the installed copy, so this process should exit.
pub fn offer() -> bool {
    if is_installed() || paths::installed_exe().exists() || paths::install_declined_file().exists() {
        return false;
    }
    match ask() {
        Choice::Install => match install_now() {
            Ok(()) => {
                launch_installed();
                return true;
            }
            Err(e) => error(&e),
        },
        Choice::Never => {
            if let Some(dir) = paths::install_declined_file().parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(paths::install_declined_file(), "declined");
        }
        Choice::Later => {}
    }
    false
}

#[derive(Clone, Copy, PartialEq)]
enum Choice {
    Install,
    Never,
    Later,
}

struct Dlg {
    pal: Pal,
    fonts: Fonts,
    dpi: u32,
    body: String,
    hover: Option<usize>,
    tracking: bool,
    choice: Option<Choice>,
}

thread_local! {
    static DLG: RefCell<Option<Dlg>> = const { RefCell::new(None) };
}

const HEADING: &str = "Install Skedaddle for your account?";
const WM_MOUSELEAVE: u32 = 0x02A3;
const CLIENT_W: i32 = 460;
const PAD: i32 = 24;
const BTN_H: i32 = 34;
const LABELS: [&str; 2] = ["Install", "No thanks"];

fn buttons(dpi: u32, w: i32, h: i32) -> [RECT; 2] {
    let s = |v| gdi::scale(v, dpi);
    let (bw, gap) = (s(120), s(10));
    let top = h - s(20) - s(BTN_H);
    let right = w - s(PAD);
    [gdi::rect(right - bw * 2 - gap, top, right - bw - gap, top + s(BTN_H)), gdi::rect(right - bw, top, right, top + s(BTN_H))]
}

fn ask() -> Choice {
    let body = format!(
        "It copies itself to {} and adds a Start Menu shortcut. No admin rights needed, and you can uninstall it from Windows Settings > Apps.\n\n\"No thanks\" runs Skedaddle from where it is and does not ask again. You can still install later from the tray menu.",
        paths::install_dir().display()
    );
    let pal = theme::pal_for(skedaddle::theme::AUTO, theme::system_dark());
    unsafe {
        let hinst: HINSTANCE = GetModuleHandleW(None).unwrap_or_default().into();
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(proc),
            hInstance: hinst,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: w!("skedaddle.install"),
            ..Default::default()
        };
        RegisterClassExW(&wc);
        let style = WS_CAPTION | WS_SYSMENU;
        let Ok(hwnd) = CreateWindowExW(WINDOW_EX_STYLE(0), w!("skedaddle.install"), w!("Skedaddle"), style, 0, 0, 10, 10, None, None, Some(hinst), None) else {
            return Choice::Later;
        };
        let dpi = gdi::dpi_of(hwnd);
        let fonts = Fonts::new(dpi, pal.font, 100);
        let s = |v| gdi::scale(v, dpi);
        let dc = GetDC(Some(hwnd));
        let tw = s(CLIENT_W - PAD * 2);
        let ch = s(PAD) + gdi::text_height(dc, HEADING, tw, fonts.title) + s(10) + gdi::text_height(dc, &body, tw, fonts.regular) + s(24) + s(BTN_H) + s(20);
        ReleaseDC(Some(hwnd), dc);
        let mut r = gdi::rect(0, 0, s(CLIENT_W), ch);
        let _ = AdjustWindowRectExForDpi(&mut r, style, false, WINDOW_EX_STYLE(0), dpi);
        let (w, h) = (r.right - r.left, r.bottom - r.top);
        let work = gdi::cursor_work_area();
        let (x, y) = (work.left + (work.right - work.left - w) / 2, work.top + (work.bottom - work.top - h) / 3);
        DLG.with(|d| *d.borrow_mut() = Some(Dlg { pal, fonts, dpi, body, hover: None, tracking: false, choice: None }));
        theme::apply_title_bar(hwnd, pal.dark);
        let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), x, y, w, h, SWP_SHOWWINDOW);
        let _ = SetForegroundWindow(hwnd);
        let mut msg = MSG::default();
        while DLG.with(|d| d.borrow().as_ref().is_some_and(|d| d.choice.is_none())) && GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        let _ = DestroyWindow(hwnd);
        let d = DLG.with(|d| d.borrow_mut().take());
        d.map_or(Choice::Later, |d| {
            d.fonts.destroy();
            d.choice.unwrap_or(Choice::Later)
        })
    }
}

fn finish(c: Choice) {
    DLG.with(|d| {
        if let Some(d) = d.borrow_mut().as_mut() {
            d.choice = Some(c);
        }
    });
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let pt = ((lp.0 & 0xFFFF) as i16 as i32, ((lp.0 >> 16) & 0xFFFF) as i16 as i32);
    let hit = |dpi: u32| {
        let mut rc = RECT::default();
        let _ = GetClientRect(hwnd, &mut rc);
        buttons(dpi, rc.right, rc.bottom).iter().position(|r| gdi::contains(r, pt.0, pt.1))
    };
    match msg {
        WM_PAINT => {
            gdi::paint_buffered(hwnd, |hdc, rc| {
                DLG.with(|d| {
                    let d = d.borrow();
                    let Some(d) = d.as_ref() else { return };
                    let s = |v| gdi::scale(v, d.dpi);
                    gdi::fill(hdc, &rc, d.pal.bg);
                    let tw = s(CLIENT_W - PAD * 2);
                    let head_h = gdi::text_height(hdc, HEADING, tw, d.fonts.title);
                    let top = s(PAD);
                    gdi::text(hdc, HEADING, gdi::rect(s(PAD), top, s(PAD) + tw, top + head_h), d.pal.text, d.fonts.title, DT_WORDBREAK);
                    let by = top + head_h + s(10);
                    gdi::text(hdc, &d.body, gdi::rect(s(PAD), by, s(PAD) + tw, rc.bottom), d.pal.dim, d.fonts.regular, DT_WORDBREAK);
                    for (i, r) in buttons(d.dpi, rc.right, rc.bottom).iter().enumerate() {
                        let hot = d.hover == Some(i);
                        let rad = s(6);
                        if i == 0 {
                            gdi::round_fill(hdc, r, if hot { gdi::blend(d.pal.on_accent, d.pal.accent, 0.15) } else { d.pal.accent }, rad);
                        } else {
                            gdi::round_fill(hdc, r, d.pal.line, rad);
                            let inner = gdi::rect(r.left + 1, r.top + 1, r.right - 1, r.bottom - 1);
                            gdi::round_fill(hdc, &inner, if hot { gdi::blend(d.pal.text, d.pal.bg, 0.10) } else { d.pal.bg }, rad);
                        }
                        let fg = if i == 0 { d.pal.on_accent } else { d.pal.text };
                        gdi::text(hdc, LABELS[i], *r, fg, d.fonts.bold, DT_SINGLELINE | DT_CENTER | DT_VCENTER);
                    }
                });
            });
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_MOUSEMOVE => {
            let track = DLG.with(|d| {
                let mut d = d.borrow_mut();
                let Some(d) = d.as_mut() else { return false };
                let now = hit(d.dpi);
                let changed = now != d.hover;
                d.hover = now;
                if changed {
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                !std::mem::replace(&mut d.tracking, true)
            });
            if track {
                let mut t = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                let _ = TrackMouseEvent(&mut t);
            }
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            DLG.with(|d| {
                if let Some(d) = d.borrow_mut().as_mut() {
                    d.hover = None;
                    d.tracking = false;
                }
            });
            let _ = InvalidateRect(Some(hwnd), None, false);
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let dpi = DLG.with(|d| d.borrow().as_ref().map_or(96, |d| d.dpi));
            match hit(dpi) {
                Some(0) => finish(Choice::Install),
                Some(_) => finish(Choice::Never),
                None => {}
            }
            LRESULT(0)
        }
        WM_KEYDOWN => {
            match VIRTUAL_KEY(wp.0 as u16) {
                VK_ESCAPE => finish(Choice::Later),
                VK_RETURN => finish(Choice::Install),
                _ => {}
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            finish(Choice::Later);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

pub fn uninstall() {
    if message("Uninstall Skedaddle", "Uninstall Skedaddle?", MB_YESNO | MB_ICONQUESTION) != IDYES {
        return;
    }
    let delete_settings = message("Uninstall Skedaddle", "Also delete your settings and cached calendars?", MB_YESNO | MB_ICONQUESTION) == IDYES;
    unsafe {
        if let Ok(h) = FindWindowW(w!("skedaddle.main"), None) {
            let _ = PostMessageW(Some(h), WM_CLOSE, WPARAM(0), LPARAM(0));
            std::thread::sleep(std::time::Duration::from_millis(900));
        }
        let key = wide(UNINSTALL_KEY);
        let _ = RegDeleteKeyW(HKEY_CURRENT_USER, PCWSTR(key.as_ptr()));
    }
    autostart::set(false);
    let _ = std::fs::remove_file(shortcut_path());
    if delete_settings {
        for dir in [paths::config_file().parent(), paths::cache_dir().parent()].into_iter().flatten() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
    message("Skedaddle", "Skedaddle was uninstalled.", MB_OK | MB_ICONINFORMATION);
    // A running exe cannot delete itself, so a hidden shell does it after this process exits.
    let cmd = format!("ping 127.0.0.1 -n 3 >nul & rmdir /s /q \"{}\"", paths::install_dir().display());
    let _ = std::process::Command::new("cmd.exe").raw_arg(format!("/C {cmd}")).creation_flags(CREATE_NO_WINDOW).spawn();
}
