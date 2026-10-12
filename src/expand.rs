use std::collections::HashSet;
use std::str::FromStr;

use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, TimeZone, Utc};
use rrule::RRuleSet;

use crate::ical::{Component, Prop};
use crate::join::find_join_url;
use crate::model::{plain_text, Occurrence, Rsvp, Tz};

pub struct FeedMeta<'a> {
    pub index: usize,
    pub owner_email: Option<&'a str>,
}

#[derive(Debug, Clone, Copy)]
pub enum IcalTime {
    Date(NaiveDate),
    Floating(NaiveDateTime),
    Utc(NaiveDateTime),
    Zoned(NaiveDateTime, chrono_tz::Tz),
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
    match tzid.and_then(|t| chrono_tz::Tz::from_str(t).ok()) {
        Some(tz) => Some(IcalTime::Zoned(n, tz)),
        None => Some(IcalTime::Floating(n)),
    }
}

pub fn parse_time(p: &Prop) -> Option<IcalTime> {
    let date_only = p.param("VALUE").is_some_and(|v| v.eq_ignore_ascii_case("DATE"));
    parse_time_value(&p.value, p.param("TZID"), date_only)
}

fn resolve(n: NaiveDateTime, tz: &Tz) -> Option<DateTime<Tz>> {
    tz.from_local_datetime(&n).earliest().or_else(|| tz.from_local_datetime(&(n + Duration::hours(1))).earliest())
}

impl IcalTime {
    pub fn to_dt(self, display: &Tz) -> Option<DateTime<Tz>> {
        match self {
            IcalTime::Date(d) => resolve(d.and_hms_opt(0, 0, 0)?, display),
            IcalTime::Floating(n) => resolve(n, display),
            IcalTime::Utc(n) => Some(Utc.from_utc_datetime(&n).with_timezone(display)),
            IcalTime::Zoned(n, z) => resolve(n, &Tz::from(z)).map(|d| d.with_timezone(display)),
        }
    }

    fn rrule_parts(self, display: &Tz) -> (String, String) {
        const F: &str = "%Y%m%dT%H%M%S";
        let floating = |n: NaiveDateTime| match display {
            Tz::Tz(t) => (format!(";TZID={}", t.name()), n.format(F).to_string()),
            _ => (String::new(), n.format(F).to_string()),
        };
        match self {
            IcalTime::Date(d) => floating(d.and_hms_opt(0, 0, 0).unwrap()),
            IcalTime::Floating(n) => floating(n),
            IcalTime::Utc(n) => (String::new(), format!("{}Z", n.format(F))),
            IcalTime::Zoned(n, z) => (format!(";TZID={}", z.name()), n.format(F).to_string()),
        }
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
    start: DateTime<Tz>,
    end: DateTime<Tz>,
    all_day: bool,
}

fn event_times(ev: &Component, display: &Tz) -> Option<(Times, IcalTime)> {
    let raw = parse_time(ev.prop("DTSTART")?)?;
    let start = raw.to_dt(display)?;
    let all_day = matches!(raw, IcalTime::Date(_));
    let end = ev
        .prop("DTEND")
        .and_then(parse_time)
        .and_then(|t| t.to_dt(display))
        .or_else(|| ev.prop("DURATION").and_then(|p| parse_duration(&p.value)).map(|d| start + d))
        .unwrap_or_else(|| {
            if all_day {
                resolve(start.naive_local() + Duration::days(1), display).unwrap_or(start)
            } else {
                start
            }
        });
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

fn build(ev: &Component, meta: &FeedMeta, start: DateTime<Tz>, end: DateTime<Tz>, all_day: bool) -> Occurrence {
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

fn rrule_source(ev: &Component, raw_start: IcalTime, display: &Tz) -> Option<String> {
    let has_rule = ev.prop("RRULE").is_some() || ev.prop("RDATE").is_some();
    if !has_rule {
        return None;
    }
    let (p, v) = raw_start.rrule_parts(display);
    let mut s = format!("DTSTART{p}:{v}");
    let date_start = matches!(raw_start, IcalTime::Date(_));
    for r in ev.props_named("RRULE") {
        s.push_str("\nRRULE:");
        s.push_str(&normalize_until(&r.value, date_start));
    }
    for name in ["RDATE", "EXDATE"] {
        for prop in ev.props_named(name) {
            let date_only = prop.param("VALUE").is_some_and(|v| v.eq_ignore_ascii_case("DATE"));
            for part in prop.value.split(',') {
                if part.contains('/') {
                    continue;
                }
                if let Some(t) = parse_time_value(part, prop.param("TZID"), date_only) {
                    let (p, v) = t.rrule_parts(display);
                    s.push_str(&format!("\n{name}{p}:{v}"));
                }
            }
        }
    }
    Some(s)
}

fn normalize_until(rule: &str, date_start: bool) -> String {
    rule.split(';')
        .map(|part| match part.strip_prefix("UNTIL=") {
            Some(u) if u.len() == 8 => format!("UNTIL={u}T235959"),
            Some(u) if date_start && u.ends_with('Z') => format!("UNTIL={}", u.trim_end_matches('Z')),
            _ => part.to_string(),
        })
        .collect::<Vec<_>>()
        .join(";")
}

const MAX_INSTANCES: u16 = 5000;

fn instances(source: &str, display: &Tz, lo: DateTime<Utc>, hi: DateTime<Utc>) -> Option<Vec<DateTime<Tz>>> {
    let set = RRuleSet::from_str(source).ok()?;
    let r = set.after(lo.with_timezone(display)).before(hi.with_timezone(display)).all(MAX_INSTANCES);
    Some(r.dates.into_iter().map(|d| d.with_timezone(display)).collect())
}

fn in_window(start: DateTime<Tz>, end: DateTime<Tz>, from: DateTime<Utc>, to: DateTime<Utc>) -> bool {
    start.with_timezone(&Utc) < to && (end.with_timezone(&Utc) > from || start.with_timezone(&Utc) >= from)
}

pub fn expand(cal: &Component, meta: &FeedMeta, display: Tz, from: DateTime<Utc>, to: DateTime<Utc>) -> Vec<Occurrence> {
    let events = cal.events();
    let mut replaced: HashSet<(String, i64)> = HashSet::new();
    for ev in &events {
        let Some(rid) = ev.prop("RECURRENCE-ID").and_then(parse_time).and_then(|t| t.to_dt(&display)) else { continue };
        replaced.insert((ev.text("UID").unwrap_or_default(), rid.timestamp()));
    }

    let mut out = Vec::new();
    for ev in events {
        if is_cancelled(ev) {
            continue;
        }
        let Some((t, raw)) = event_times(ev, &display) else { continue };
        let is_override = ev.prop("RECURRENCE-ID").is_some();
        let uid = ev.text("UID").unwrap_or_default();

        let source = if is_override { None } else { rrule_source(ev, raw, &display) };
        let Some(source) = source else {
            if in_window(t.start, t.end, from, to) {
                out.push(build(ev, meta, t.start, t.end, t.all_day));
            }
            continue;
        };

        let span = t.end - t.start;
        let days = (t.end.date_naive() - t.start.date_naive()).num_days();
        let starts = match instances(&source, &display, from - span - Duration::days(1), to) {
            Some(v) => v,
            None => vec![t.start],
        };
        for s in starts {
            if replaced.contains(&(uid.clone(), s.timestamp())) {
                continue;
            }
            let e = if t.all_day {
                resolve(s.naive_local() + Duration::days(days.max(1)), &display).unwrap_or(s)
            } else {
                s + span
            };
            if in_window(s, e, from, to) {
                out.push(build(ev, meta, s, e, t.all_day));
            }
        }
    }
    out.sort_by_key(|o| (o.start.timestamp(), o.end.timestamp()));
    out
}
