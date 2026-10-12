use std::collections::{BTreeSet, HashSet};

use chrono::{DateTime, Duration, FixedOffset, NaiveDate, NaiveDateTime, Utc};

use crate::ical::{Component, Prop};
use crate::join::find_join_url;
use crate::model::{plain_text, Occurrence, Rsvp};
use crate::recur;
use crate::tz::{system_zone, Zone, Zones};

pub struct FeedMeta<'a> {
    pub index: usize,
    pub owner_email: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub enum IcalTime {
    Date(NaiveDate),
    Floating(NaiveDateTime),
    Utc(NaiveDateTime),
    Zoned(NaiveDateTime, String),
}

fn parse_naive(v: &str) -> Option<(NaiveDateTime, bool)> {
    let v = v.trim();
    let (body, z) = match v.strip_suffix('Z') {
        Some(b) => (b, true),
        None => (v, false),
    };
    if body.len() == 8 {
        let d = NaiveDate::parse_from_str(body, "%Y%m%d").ok()?;
        return Some((d.and_hms_opt(0, 0, 0)?, z));
    }
    NaiveDateTime::parse_from_str(body, "%Y%m%dT%H%M%S").ok().map(|n| (n, z))
}

pub fn parse_time_value(value: &str, tzid: Option<&str>, date_only: bool) -> Option<IcalTime> {
    let (n, z) = parse_naive(value)?;
    if date_only || value.trim().len() == 8 {
        return Some(IcalTime::Date(n.date()));
    }
    if z {
        return Some(IcalTime::Utc(n));
    }
    Some(match tzid {
        Some(t) => IcalTime::Zoned(n, t.to_string()),
        None => IcalTime::Floating(n),
    })
}

pub fn parse_time(p: &Prop) -> Option<IcalTime> {
    let date_only = p.param("VALUE").is_some_and(|v| v.eq_ignore_ascii_case("DATE"));
    parse_time_value(&p.value, p.param("TZID"), date_only)
}

impl IcalTime {
    fn local(&self) -> NaiveDateTime {
        match self {
            IcalTime::Date(d) => d.and_hms_opt(0, 0, 0).unwrap(),
            IcalTime::Floating(n) | IcalTime::Utc(n) | IcalTime::Zoned(n, _) => *n,
        }
    }

    fn zone(&self, zones: &Zones) -> Zone {
        match self {
            IcalTime::Date(_) | IcalTime::Floating(_) => zones.display.clone(),
            IcalTime::Utc(_) => Zone::Fixed(0),
            IcalTime::Zoned(_, id) => zones.get(id),
        }
    }

    fn to_utc(&self, zones: &Zones) -> NaiveDateTime {
        self.zone(zones).to_utc(self.local())
    }
}

fn parse_duration(v: &str) -> Option<Duration> {
    let v = v.trim();
    let (neg, v) = match v.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, v.strip_prefix('+').unwrap_or(v)),
    };
    let v = v.strip_prefix('P')?;
    let mut secs = 0i64;
    let mut num = String::new();
    let mut in_time = false;
    for c in v.chars() {
        match c {
            'T' => in_time = true,
            '0'..='9' => num.push(c),
            _ => {
                let n: i64 = num.parse().ok()?;
                num.clear();
                secs += match (c, in_time) {
                    ('W', _) => n * 604_800,
                    ('D', _) => n * 86_400,
                    ('H', true) => n * 3_600,
                    ('M', true) => n * 60,
                    ('S', true) => n,
                    _ => return None,
                };
            }
        }
    }
    Some(Duration::seconds(if neg { -secs } else { secs }))
}

struct Times {
    start: NaiveDateTime,
    end: NaiveDateTime,
    all_day: bool,
}

fn next_local_midnight(zones: &Zones, utc: NaiveDateTime, days: i64) -> NaiveDateTime {
    let d = zones.display.to_local(utc).date() + Duration::days(days);
    zones.display.to_utc(d.and_hms_opt(0, 0, 0).unwrap())
}

fn event_times(ev: &Component, zones: &Zones) -> Option<(Times, IcalTime)> {
    let raw = parse_time(ev.prop("DTSTART")?)?;
    let start = raw.to_utc(zones);
    let all_day = matches!(raw, IcalTime::Date(_));
    let end = ev
        .prop("DTEND")
        .and_then(parse_time)
        .map(|t| t.to_utc(zones))
        .or_else(|| ev.prop("DURATION").and_then(|p| parse_duration(&p.value)).map(|d| start + d))
        .unwrap_or_else(|| if all_day { next_local_midnight(zones, start, 1) } else { start });
    Some((Times { start, end: end.max(start), all_day }, raw))
}

fn rsvp_for(ev: &Component, owner: Option<&str>) -> Rsvp {
    let Some(owner) = owner else { return Rsvp::None };
    let owner = owner.to_ascii_lowercase();
    for a in ev.props_named("ATTENDEE") {
        if !a.value.to_ascii_lowercase().ends_with(&owner) {
            continue;
        }
        return match a.param("PARTSTAT").map(str::to_ascii_uppercase).as_deref() {
            Some("ACCEPTED") => Rsvp::Accepted,
            Some("DECLINED") => Rsvp::Declined,
            Some("TENTATIVE") => Rsvp::Tentative,
            Some("NEEDS-ACTION") => Rsvp::NeedsAction,
            _ => Rsvp::None,
        };
    }
    Rsvp::None
}

fn is_cancelled(ev: &Component) -> bool {
    ev.text("STATUS").is_some_and(|s| s.eq_ignore_ascii_case("CANCELLED"))
}

fn build(ev: &Component, meta: &FeedMeta, start: DateTime<FixedOffset>, end: DateTime<FixedOffset>, all_day: bool) -> Occurrence {
    let location = ev.text("LOCATION").unwrap_or_default();
    let raw_desc = ev.text("DESCRIPTION").unwrap_or_default();
    let conference = ev.prop("X-GOOGLE-CONFERENCE").map(|p| p.value.clone());
    let join_url = find_join_url(conference.as_deref(), &location, &raw_desc);
    Occurrence {
        feed: meta.index,
        uid: ev.text("UID").unwrap_or_default(),
        title: ev.text("SUMMARY").unwrap_or_default(),
        start,
        end,
        all_day,
        location,
        description: plain_text(&raw_desc),
        join_url,
        rsvp: rsvp_for(ev, meta.owner_email),
        busy: ev.text("TRANSP").is_none_or(|t| !t.eq_ignore_ascii_case("TRANSPARENT")),
    }
}

/// UTC instants listed in RDATE or EXDATE; a DATE value takes the series start's time of day.
fn date_list(ev: &Component, name: &str, raw: &IcalTime, zones: &Zones) -> Vec<NaiveDateTime> {
    let mut out = Vec::new();
    for prop in ev.props_named(name) {
        let date_only = prop.param("VALUE").is_some_and(|v| v.eq_ignore_ascii_case("DATE"));
        for part in prop.value.split(',').filter(|p| !p.contains('/')) {
            let Some(t) = parse_time_value(part, prop.param("TZID"), date_only) else { continue };
            let utc = match t {
                IcalTime::Date(d) => raw.zone(zones).to_utc(d.and_time(raw.local().time())),
                other => other.to_utc(zones),
            };
            out.push(utc);
        }
    }
    out
}

fn series_starts(ev: &Component, raw: &IcalTime, zones: &Zones, to: NaiveDateTime) -> Vec<NaiveDateTime> {
    let src = raw.zone(zones);
    let local = raw.local();
    let stop_after = src.to_local(to) + Duration::days(1);
    let mut starts: BTreeSet<NaiveDateTime> = BTreeSet::new();
    let rules: Option<Vec<recur::Rule>> = ev.props_named("RRULE").map(|r| recur::parse(&r.value)).collect();
    match rules {
        Some(rules) if !rules.is_empty() => {
            for rule in &rules {
                let to_utc = |l: NaiveDateTime| src.to_utc(l);
                starts.extend(recur::expand(rule, local, &to_utc, stop_after).into_iter().map(|l| src.to_utc(l)));
            }
        }
        _ => {
            starts.insert(src.to_utc(local));
        }
    }
    starts.extend(date_list(ev, "RDATE", raw, zones));
    for x in date_list(ev, "EXDATE", raw, zones) {
        starts.remove(&x);
    }
    starts.into_iter().collect()
}

fn in_window(start: NaiveDateTime, end: NaiveDateTime, from: NaiveDateTime, to: NaiveDateTime) -> bool {
    start < to && (end > from || start >= from)
}

pub fn expand(cal: &Component, meta: &FeedMeta, display: &Zone, from: DateTime<Utc>, to: DateTime<Utc>) -> Vec<Occurrence> {
    let system = system_zone;
    let zones = Zones::new(cal, display.clone(), &system);
    let (from, to) = (from.naive_utc(), to.naive_utc());
    let events = cal.events();
    let mut replaced: HashSet<(String, i64)> = HashSet::new();
    for ev in &events {
        let Some(rid) = ev.prop("RECURRENCE-ID").and_then(parse_time) else { continue };
        replaced.insert((ev.text("UID").unwrap_or_default(), rid.to_utc(&zones).and_utc().timestamp()));
    }

    let mut out = Vec::new();
    for ev in events {
        if is_cancelled(ev) {
            continue;
        }
        let Some((t, raw)) = event_times(ev, &zones) else { continue };
        let is_override = ev.prop("RECURRENCE-ID").is_some();
        let has_rule = ev.prop("RRULE").is_some() || ev.prop("RDATE").is_some();
        if is_override || !has_rule {
            if in_window(t.start, t.end, from, to) {
                out.push(build(ev, meta, display.display(t.start), display.display(t.end), t.all_day));
            }
            continue;
        }

        let uid = ev.text("UID").unwrap_or_default();
        let span = t.end - t.start;
        let days = (display.to_local(t.end).date() - display.to_local(t.start).date()).num_days().max(1);
        for s in series_starts(ev, &raw, &zones, to) {
            if replaced.contains(&(uid.clone(), s.and_utc().timestamp())) {
                continue;
            }
            let e = if t.all_day { next_local_midnight(&zones, s, days) } else { s + span };
            if in_window(s, e, from, to) {
                out.push(build(ev, meta, display.display(s), display.display(e), t.all_day));
            }
        }
    }
    out.sort_by_key(|o| (o.start.timestamp(), o.end.timestamp()));
    out
}
