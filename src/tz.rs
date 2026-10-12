use std::collections::HashMap;
use std::rc::Rc;

use chrono::{DateTime, Duration, FixedOffset, Local, NaiveDate, NaiveDateTime, Offset, TimeZone};

use crate::ical::Component;
use crate::recur;

pub trait OffsetSource {
    /// Seconds east of UTC at this UTC instant.
    fn offset_at(&self, utc: NaiveDateTime) -> i32;
}

#[derive(Clone)]
pub enum Zone {
    Local,
    Fixed(i32),
    Rules(Rc<dyn OffsetSource>),
}

impl Zone {
    pub fn offset_at(&self, utc: NaiveDateTime) -> i32 {
        match self {
            Zone::Local => Local.offset_from_utc_datetime(&utc).fix().local_minus_utc(),
            Zone::Fixed(o) => *o,
            Zone::Rules(r) => r.offset_at(utc),
        }
    }

    /// Spring-forward gaps move forward by the gap; fall-back overlaps take the earlier instant.
    pub fn to_utc(&self, local: NaiveDateTime) -> NaiveDateTime {
        let before = self.offset_at(local - Duration::days(1));
        let after = self.offset_at(local + Duration::days(1));
        let valid: Vec<NaiveDateTime> = [before, after]
            .into_iter()
            .map(|o| local - Duration::seconds(o as i64))
            .filter(|u| {
                let o = self.offset_at(*u);
                *u + Duration::seconds(o as i64) == local
            })
            .collect();
        valid.into_iter().min().unwrap_or(local - Duration::seconds(before as i64))
    }

    pub fn to_local(&self, utc: NaiveDateTime) -> NaiveDateTime {
        utc + Duration::seconds(self.offset_at(utc) as i64)
    }

    pub fn display(&self, utc: NaiveDateTime) -> DateTime<FixedOffset> {
        let off = FixedOffset::east_opt(self.offset_at(utc)).unwrap_or(FixedOffset::east_opt(0).unwrap());
        off.from_utc_datetime(&utc)
    }
}

struct VTimezone {
    /// (UTC instant, offset in force from then on), sorted.
    transitions: Vec<(NaiveDateTime, i32)>,
    initial: i32,
}

impl OffsetSource for VTimezone {
    fn offset_at(&self, utc: NaiveDateTime) -> i32 {
        let i = self.transitions.partition_point(|(t, _)| *t <= utc);
        if i == 0 {
            self.initial
        } else {
            self.transitions[i - 1].1
        }
    }
}

pub fn parse_offset(v: &str) -> Option<i32> {
    let v = v.trim();
    let (sign, rest) = match v.as_bytes().first()? {
        b'+' => (1, &v[1..]),
        b'-' => (-1, &v[1..]),
        _ => (1, v),
    };
    if rest.len() < 4 || !rest.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let h: i32 = rest[0..2].parse().ok()?;
    let m: i32 = rest[2..4].parse().ok()?;
    let s: i32 = if rest.len() >= 6 { rest[4..6].parse().ok()? } else { 0 };
    Some(sign * (h * 3600 + m * 60 + s))
}

fn local_value(v: &str) -> Option<NaiveDateTime> {
    let v = v.trim().trim_end_matches('Z');
    if v.len() == 8 {
        return NaiveDate::parse_from_str(v, "%Y%m%d").ok()?.and_hms_opt(0, 0, 0);
    }
    NaiveDateTime::parse_from_str(v, "%Y%m%dT%H%M%S").ok()
}

const RULES_END_YEAR: i32 = 2100;

pub fn from_vtimezone(c: &Component) -> Option<Zone> {
    let horizon = NaiveDate::from_ymd_opt(RULES_END_YEAR, 1, 1)?.and_hms_opt(0, 0, 0)?;
    let mut transitions = Vec::new();
    let mut earliest: Option<(NaiveDateTime, i32)> = None;
    for obs in c.children.iter().filter(|o| o.name == "STANDARD" || o.name == "DAYLIGHT") {
        let from = parse_offset(&obs.prop("TZOFFSETFROM")?.value)?;
        let to = parse_offset(&obs.prop("TZOFFSETTO")?.value)?;
        let start = local_value(&obs.prop("DTSTART")?.value)?;
        let mut onsets = match obs.prop("RRULE").and_then(|r| recur::parse(&r.value)) {
            Some(rule) => recur::expand(&rule, start, &|l| l - Duration::seconds(from as i64), horizon),
            None => vec![start],
        };
        for rd in obs.props_named("RDATE") {
            onsets.extend(rd.value.split(',').filter_map(local_value));
        }
        for local in onsets {
            transitions.push((local - Duration::seconds(from as i64), to));
        }
        if earliest.is_none_or(|(s, _)| start < s) {
            earliest = Some((start, from));
        }
    }
    let (_, initial) = earliest?;
    transitions.sort_by_key(|t| t.0);
    Some(Zone::Rules(Rc::new(VTimezone { transitions, initial })))
}

/// Every `VTIMEZONE` block in the feed, by `TZID`.
pub fn feed_zones(cal: &Component) -> HashMap<String, Zone> {
    let mut out = HashMap::new();
    let mut stack = vec![cal];
    while let Some(c) = stack.pop() {
        for ch in &c.children {
            if ch.name == "VTIMEZONE" {
                if let (Some(id), Some(z)) = (ch.text("TZID"), from_vtimezone(ch)) {
                    out.insert(id, z);
                }
            } else {
                stack.push(ch);
            }
        }
    }
    out
}

/// Zone lookup in order: the feed's own `VTIMEZONE`, the system IANA database, then the display zone.
pub struct Zones<'a> {
    feed: HashMap<String, Zone>,
    system: &'a dyn Fn(&str) -> Option<Zone>,
    cache: std::cell::RefCell<HashMap<String, Zone>>,
    pub display: Zone,
}

impl<'a> Zones<'a> {
    pub fn new(cal: &Component, display: Zone, system: &'a dyn Fn(&str) -> Option<Zone>) -> Self {
        Zones { feed: feed_zones(cal), system, cache: Default::default(), display }
    }

    pub fn get(&self, tzid: &str) -> Zone {
        if let Some(z) = self.feed.get(tzid) {
            return z.clone();
        }
        if let Some(z) = self.cache.borrow().get(tzid) {
            return z.clone();
        }
        let z = (self.system)(tzid).unwrap_or_else(|| self.display.clone());
        self.cache.borrow_mut().insert(tzid.to_string(), z.clone());
        z
    }
}

pub fn system_zone(name: &str) -> Option<Zone> {
    #[cfg(windows)]
    {
        crate::icu::zone(name)
    }
    #[cfg(not(windows))]
    {
        let _ = name;
        None
    }
}
