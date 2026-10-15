use std::path::PathBuf;

fn base(var: &str) -> PathBuf {
    let root = std::env::var_os(var).map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    root.join("skedaddle")
}

pub fn config_file() -> PathBuf {
    base("APPDATA").join("config.toml")
}

pub fn cache_dir() -> PathBuf {
    base("LOCALAPPDATA").join("cache")
}

pub fn alerted_file() -> PathBuf {
    base("LOCALAPPDATA").join("alerted.txt")
}

pub fn view_state_file() -> PathBuf {
    base("LOCALAPPDATA").join("view.txt")
}

pub fn install_dir() -> PathBuf {
    let root = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    root.join("Programs").join("Skedaddle")
}

pub fn installed_exe() -> PathBuf {
    install_dir().join("skedaddle.exe")
}

pub fn install_declined_file() -> PathBuf {
    base("LOCALAPPDATA").join("install-declined.txt")
}
