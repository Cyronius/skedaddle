use chrono::{DateTime, FixedOffset, Local, NaiveDate, TimeZone, Utc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rsvp {
    None,
    Accepted,
    Declined,
    Tentative,
    NeedsAction,
}

#[derive(Clone, Debug)]
pub struct Occurrence {
    pub feed: usize,
    pub uid: String,
    pub title: String,
    pub start: DateTime<FixedOffset>,
    pub end: DateTime<FixedOffset>,
    pub all_day: bool,
    pub location: String,
    pub description: String,
    pub join_url: Option<String>,
    pub rsvp: Rsvp,
    pub busy: bool,
}

pub type AlertKey = (usize, String, i64);

impl Occurrence {
    pub fn key(&self) -> AlertKey {
        (self.feed, self.uid.clone(), self.start.timestamp())
    }

    pub fn start_utc(&self) -> DateTime<Utc> {
        self.start.with_timezone(&Utc)
    }
}

pub fn plain_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut chars = html.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '<' {
            out.push(c);
            continue;
        }
        let mut tag = String::new();
        for t in chars.by_ref() {
            if t == '>' {
                break;
            }
            tag.push(t);
        }
        let t = tag.trim_start_matches('/').to_ascii_lowercase();
        if t.starts_with("br") || t.starts_with("p") || t.starts_with("li") || t.starts_with("div") {
            if !out.ends_with('\n') {
                out.push('\n');
            }
        }
    }
    let out = out
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&");
    out.trim().to_string()
}

pub fn fmt_time(dt: &DateTime<FixedOffset>, h24: bool) -> String {
    if h24 {
        dt.format("%H:%M").to_string()
    } else {
        dt.format("%-I:%M %p").to_string()
    }
}

pub fn fmt_in(secs: i64) -> String {
    let mins = (secs + 59).div_euclid(60);
    match mins {
        m if m <= 0 => "now".into(),
        m if m < 60 => format!("in {m} min"),
        m => format!("in {} h {} min", m / 60, m % 60),
    }
}

pub fn local_midnight_utc(d: NaiveDate) -> DateTime<Utc> {
    let naive = d.and_hms_opt(0, 0, 0).unwrap();
    Local.from_local_datetime(&naive).earliest().map_or_else(|| Utc.from_utc_datetime(&naive), |l| l.with_timezone(&Utc))
}
