use chrono::Duration;
use serde::Deserialize;

use crate::alerts::Rules;

#[derive(Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Config {
    pub poll_minutes: u64,
    pub week_start: String,
    pub clock: String,
    pub alert: AlertCfg,
    pub feed: Vec<FeedCfg>,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(default)]
pub struct AlertCfg {
    pub lead_minutes: i64,
    pub grace_minutes: i64,
    pub repeat_seconds: u32,
    pub repeat_until_minutes: i64,
    pub flash: bool,
    pub flash_color: String,
    pub sound: String,
    pub skip_free: bool,
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct FeedCfg {
    pub name: String,
    pub url: String,
    pub color: String,
    pub owner_email: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            poll_minutes: 5,
            week_start: "sunday".into(),
            clock: "12h".into(),
            alert: AlertCfg::default(),
            feed: Vec::new(),
        }
    }
}

impl Default for AlertCfg {
    fn default() -> Self {
        Self {
            lead_minutes: 1,
            grace_minutes: 5,
            repeat_seconds: 60,
            repeat_until_minutes: 5,
            flash: true,
            flash_color: "#2F80ED".into(),
            sound: "default".into(),
            skip_free: false,
        }
    }
}

impl Config {
    pub fn parse(text: &str) -> Result<Config, String> {
        let mut c: Config = toml::from_str(text).map_err(|e| e.to_string())?;
        c.poll_minutes = c.poll_minutes.clamp(1, 120);
        Ok(c)
    }

    pub fn rules(&self) -> Rules {
        Rules {
            lead: Duration::minutes(self.alert.lead_minutes),
            grace: Duration::minutes(self.alert.grace_minutes),
            skip_free: self.alert.skip_free,
        }
    }

    pub fn monday_first(&self) -> bool {
        self.week_start.eq_ignore_ascii_case("monday")
    }

    pub fn clock_24h(&self) -> bool {
        self.clock.eq_ignore_ascii_case("24h")
    }
}

impl FeedCfg {
    pub fn owner(&self) -> Option<String> {
        if let Some(o) = &self.owner_email {
            return Some(o.clone());
        }
        let seg = self.url.split("/ical/").nth(1)?.split('/').next()?;
        let decoded = seg.replace("%40", "@");
        decoded.contains('@').then_some(decoded)
    }
}

pub fn parse_color(s: &str) -> Option<u32> {
    let h = s.trim().strip_prefix('#')?;
    if h.len() != 6 {
        return None;
    }
    u32::from_str_radix(h, 16).ok()
}

pub const TEMPLATE: &str = r##"# skedaddle config. Saved changes load on the next sync (or use "Refresh now" in the tray menu).
poll_minutes = 5
week_start = "sunday"   # or "monday"
clock = "12h"           # or "24h"

[alert]
lead_minutes = 1
grace_minutes = 5
repeat_seconds = 60          # 0 = flash one time only
repeat_until_minutes = 5
flash = true
flash_color = "#2F80ED"
sound = "default"            # "none" | "default" | "C:\\path\\to.wav"
skip_free = false

# Google Calendar: Settings > your calendar > Integrate calendar > "Secret address in iCal format".
# The URL is a credential. Do not share it.
# [[feed]]
# name = "Work"
# url = "https://calendar.google.com/calendar/ical/.../private-.../basic.ics"
# color = "#4285F4"
# owner_email = "me@example.com"   # only needed for non-Google feeds
"##;
