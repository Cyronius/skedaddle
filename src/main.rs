#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod win;

fn main() {
    #[cfg(windows)]
    win::app::run();
    #[cfg(not(windows))]
    eprintln!("skedaddle supports Windows only");
}
