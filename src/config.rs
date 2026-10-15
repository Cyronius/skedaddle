use chrono::Duration;
use serde::{Deserialize, Serialize};

use crate::alerts::Rules;
use crate::theme;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Config {
    pub week_start: String,
    pub clock: String,
    pub theme: String,
    pub text_size: String,
    pub day_start: u32,
    pub day_end: u32,
    pub alert: AlertCfg,
    pub feed: Vec<FeedCfg>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct AlertCfg {
    pub lead_minutes: i64,
    pub play_sound: bool,
    pub skip_free: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct FeedCfg {
    pub name: String,
    pub url: String,
    pub color: String,
    pub owner_email: Option<String>,
}

pub const TEXT_SIZES: [(&str, &str, i32); 3] = [("small", "Small", 100), ("medium", "Medium", 115), ("large", "Large", 130)];

pub fn text_scale(id: &str) -> Option<i32> {
    TEXT_SIZES.iter().find(|t| t.0.eq_ignore_ascii_case(id)).map(|t| t.2)
}

pub const MIN_DAY_HOURS: u32 = 4;

pub const POLL_MINUTES: u64 = 5;
pub const GRACE_MINUTES: i64 = 5;
pub const REPEAT_SECONDS: u32 = 60;
pub const REPEAT_UNTIL_MINUTES: i64 = 5;

impl Default for Config {
    fn default() -> Self {
        Self { week_start: "sunday".into(), clock: "12h".into(), theme: theme::AUTO.into(), text_size: "medium".into(), day_start: 7, day_end: 18, alert: AlertCfg::default(), feed: Vec::new() }
    }
}

impl Default for AlertCfg {
    fn default() -> Self {
        Self { lead_minutes: 1, play_sound: true, skip_free: false }
    }
}

impl Config {
    pub fn parse(text: &str) -> Result<Config, String> {
        toml::from_str(text).map_err(|e| e.to_string())
    }

    pub fn to_toml(&self) -> String {
        toml::to_string(self).unwrap_or_default()
    }

    pub fn validate(&self) -> Vec<String> {
        let mut errs = Vec::new();
        if !["sunday", "monday"].iter().any(|d| self.week_start.eq_ignore_ascii_case(d)) {
            errs.push("Week starts on: choose Sunday or Monday".into());
        }
        if !["12h", "24h"].iter().any(|c| self.clock.eq_ignore_ascii_case(c)) {
            errs.push("Clock: choose 12-hour or 24-hour".into());
        }
        if text_scale(&self.text_size).is_none() {
            errs.push("Text size: choose Small, Medium or Large".into());
        }
        if self.day_start > 23 || self.day_end > 24 || self.day_end < self.day_start + MIN_DAY_HOURS {
            errs.push(format!("Day view: show at least {MIN_DAY_HOURS} hours, between 12 AM and 12 AM"));
        }
        if !theme::is_known(&self.theme) {
            errs.push("Theme: choose one from the list".into());
        }
        if !(0..=60).contains(&self.alert.lead_minutes) {
            errs.push("Alert before start: choose 0 to 60 minutes".into());
        }
        for (i, f) in self.feed.iter().enumerate() {
            let label = if f.name.trim().is_empty() { format!("Calendar {}", i + 1) } else { f.name.clone() };
            let url = f.url.trim().to_ascii_lowercase();
            if !["https://", "http://", "webcal://"].iter().any(|p| url.starts_with(p)) {
                errs.push(format!("{label}: the address must start with https://, http:// or webcal://"));
            }
            if parse_color(&f.color).is_none() {
                errs.push(format!("{label}: color must look like #4285F4"));
            }
        }
        errs
    }

    pub fn rules(&self) -> Rules {
        Rules { lead: Duration::minutes(self.alert.lead_minutes), grace: Duration::minutes(GRACE_MINUTES), skip_free: self.alert.skip_free }
    }

    pub fn text_scale(&self) -> i32 {
        text_scale(&self.text_size).unwrap_or(115)
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
