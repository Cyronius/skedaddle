use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, Weekday};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Freq {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Until {
    Date(NaiveDate),
    Local(NaiveDateTime),
    Utc(NaiveDateTime),
}

#[derive(Clone, Debug)]
pub struct Rule {
    pub freq: Freq,
    pub interval: u32,
    pub count: Option<u32>,
    pub until: Option<Until>,
    pub by_day: Vec<(i32, Weekday)>,
    pub by_month_day: Vec<i32>,
    pub by_month: Vec<u32>,
    pub by_set_pos: Vec<i32>,
    pub wkst: Weekday,
}

fn weekday(s: &str) -> Option<Weekday> {
    Some(match s {
        "MO" => Weekday::Mon,
        "TU" => Weekday::Tue,
        "WE" => Weekday::Wed,
        "TH" => Weekday::Thu,
        "FR" => Weekday::Fri,
        "SA" => Weekday::Sat,
        "SU" => Weekday::Sun,
        _ => return None,
    })
}

fn ints(v: &str) -> Option<Vec<i32>> {
    v.split(',').map(|p| p.trim().trim_start_matches('+').parse().ok()).collect()
}

fn parse_until(v: &str) -> Option<Until> {
    let (body, utc) = match v.strip_suffix('Z') {
        Some(b) => (b, true),
        None => (v, false),
    };
    if body.len() == 8 {
        return NaiveDate::parse_from_str(body, "%Y%m%d").ok().map(Until::Date);
    }
    let n = NaiveDateTime::parse_from_str(body, "%Y%m%dT%H%M%S").ok()?;
    Some(if utc { Until::Utc(n) } else { Until::Local(n) })
}

/// Returns `None` for rules this engine does not support; callers then show the first instance only.
pub fn parse(s: &str) -> Option<Rule> {
    let mut rule = Rule {
        freq: Freq::Daily,
        interval: 1,
        count: None,
        until: None,
        by_day: Vec::new(),
        by_month_day: Vec::new(),
        by_month: Vec::new(),
        by_set_pos: Vec::new(),
        wkst: Weekday::Mon,
    };
    let mut has_freq = false;
    for part in s.trim().split(';').filter(|p| !p.is_empty()) {
        let (k, v) = part.split_once('=')?;
        match k.trim().to_ascii_uppercase().as_str() {
            "FREQ" => {
                rule.freq = match v.to_ascii_uppercase().as_str() {
                    "DAILY" => Freq::Daily,
                    "WEEKLY" => Freq::Weekly,
                    "MONTHLY" => Freq::Monthly,
                    "YEARLY" => Freq::Yearly,
                    _ => return None,
                };
                has_freq = true;
            }
            "INTERVAL" => rule.interval = v.parse().ok().filter(|n| *n > 0)?,
            "COUNT" => rule.count = Some(v.parse().ok()?),
            "UNTIL" => rule.until = Some(parse_until(v)?),
            "BYDAY" => {
                for d in v.split(',') {
                    let d = d.trim().to_ascii_uppercase();
                    let (n, wd) = d.split_at(d.len().checked_sub(2)?);
                    let n = if n.is_empty() { 0 } else { n.trim_start_matches('+').parse().ok()? };
                    rule.by_day.push((n, weekday(wd)?));
                }
            }
            "BYMONTHDAY" => rule.by_month_day = ints(v)?,
            "BYMONTH" => rule.by_month = ints(v)?.into_iter().map(|m| m as u32).collect(),
            "BYSETPOS" => rule.by_set_pos = ints(v)?,
            "WKST" => rule.wkst = weekday(&v.to_ascii_uppercase())?,
            "BYWEEKNO" | "BYYEARDAY" | "BYHOUR" | "BYMINUTE" | "BYSECOND" | "RSCALE" | "SKIP" => return None,
            _ => {}
        }
    }
    has_freq.then_some(rule)
}

fn days_in_month(y: i32, m: u32) -> u32 {
    let next = if m == 12 { NaiveDate::from_ymd_opt(y + 1, 1, 1) } else { NaiveDate::from_ymd_opt(y, m + 1, 1) };
    next.map_or(31, |n| n.pred_opt().map_or(31, |d| d.day()))
}

fn month_day(y: i32, m: u32, md: i32) -> Option<NaiveDate> {
    let len = days_in_month(y, m) as i32;
    let d = if md > 0 { md } else { len + md + 1 };
    if d < 1 || d > len {
        return None;
    }
    NaiveDate::from_ymd_opt(y, m, d as u32)
}

/// All `wd` days from `first` to `last` inclusive, or only the nth one (negative counts from the end).
fn nth_weekday(first: NaiveDate, last: NaiveDate, n: i32, wd: Weekday) -> Vec<NaiveDate> {
    let mut all = Vec::new();
    let mut d = first + Duration::days(((7 + wd.num_days_from_monday() as i64 - first.weekday().num_days_from_monday() as i64) % 7) as i64);
    while d <= last {
        all.push(d);
        d += Duration::days(7);
    }
    match n {
        0 => all,
        n if n > 0 => all.get(n as usize - 1).copied().into_iter().collect(),
        n => all.len().checked_sub((-n) as usize).and_then(|i| all.get(i).copied()).into_iter().collect(),
    }
}

fn day_matches(rule: &Rule, d: NaiveDate) -> bool {
    rule.by_day.is_empty() || rule.by_day.iter().any(|(_, wd)| *wd == d.weekday())
}

fn month_set(rule: &Rule, y: i32, m: u32, start: NaiveDate) -> Vec<NaiveDate> {
    let first = NaiveDate::from_ymd_opt(y, m, 1).unwrap();
    let last = first + Duration::days(days_in_month(y, m) as i64 - 1);
    if !rule.by_month_day.is_empty() {
        return rule.by_month_day.iter().filter_map(|md| month_day(y, m, *md)).filter(|d| day_matches(rule, *d)).collect();
    }
    if !rule.by_day.is_empty() {
        return rule.by_day.iter().flat_map(|(n, wd)| nth_weekday(first, last, *n, *wd)).collect();
    }
    month_day(y, m, start.day() as i32).into_iter().collect()
}

fn period_dates(rule: &Rule, start: NaiveDate, k: i64) -> (NaiveDate, Vec<NaiveDate>) {
    let step = k * rule.interval as i64;
    match rule.freq {
        Freq::Daily => {
            let d = start + Duration::days(step);
            let ok = (rule.by_month.is_empty() || rule.by_month.contains(&d.month()))
                && (rule.by_month_day.is_empty() || rule.by_month_day.iter().any(|md| month_day(d.year(), d.month(), *md) == Some(d)))
                && day_matches(rule, d);
            (d, if ok { vec![d] } else { vec![] })
        }
        Freq::Weekly => {
            let back = (7 + start.weekday().num_days_from_monday() as i64 - rule.wkst.num_days_from_monday() as i64) % 7;
            let week = start - Duration::days(back) + Duration::weeks(step);
            let days: Vec<Weekday> = if rule.by_day.is_empty() { vec![start.weekday()] } else { rule.by_day.iter().map(|(_, wd)| *wd).collect() };
            let set = (0..7)
                .map(|i| week + Duration::days(i))
                .filter(|d| days.contains(&d.weekday()))
                .filter(|d| rule.by_month.is_empty() || rule.by_month.contains(&d.month()))
                .collect();
            (week, set)
        }
        Freq::Monthly => {
            let idx = start.year() as i64 * 12 + start.month0() as i64 + step;
            let (y, m) = (idx.div_euclid(12) as i32, idx.rem_euclid(12) as u32 + 1);
            let first = NaiveDate::from_ymd_opt(y, m, 1).unwrap();
            if !rule.by_month.is_empty() && !rule.by_month.contains(&m) {
                return (first, vec![]);
            }
            (first, month_set(rule, y, m, start))
        }
        Freq::Yearly => {
            let y = start.year() + step as i32;
            let jan1 = NaiveDate::from_ymd_opt(y, 1, 1).unwrap();
            let set = if !rule.by_month.is_empty() {
                rule.by_month.iter().flat_map(|m| month_set(rule, y, *m, start)).collect()
            } else if !rule.by_month_day.is_empty() {
                (1..=12).flat_map(|m| month_set(rule, y, m, start)).collect()
            } else if !rule.by_day.is_empty() {
                let dec31 = NaiveDate::from_ymd_opt(y, 12, 31).unwrap();
                rule.by_day.iter().flat_map(|(n, wd)| nth_weekday(jan1, dec31, *n, *wd)).collect()
            } else {
                NaiveDate::from_ymd_opt(y, start.month(), start.day()).into_iter().collect()
            };
            (jan1, set)
        }
    }
}

fn apply_set_pos(rule: &Rule, mut set: Vec<NaiveDate>) -> Vec<NaiveDate> {
    set.sort();
    set.dedup();
    if rule.by_set_pos.is_empty() {
        return set;
    }
    let len = set.len() as i32;
    let mut out: Vec<NaiveDate> = rule
        .by_set_pos
        .iter()
        .filter_map(|p| {
            let i = if *p > 0 { p - 1 } else { len + p };
            (0..len).contains(&i).then(|| set[i as usize])
        })
        .collect();
    out.sort();
    out.dedup();
    out
}

const MAX_PERIODS: i64 = 200_000;

/// Instances in local wall-clock time, from `dtstart` up to `stop_after`. `to_utc` is needed for a UTC `UNTIL`.
pub fn expand(rule: &Rule, dtstart: NaiveDateTime, to_utc: &dyn Fn(NaiveDateTime) -> NaiveDateTime, stop_after: NaiveDateTime) -> Vec<NaiveDateTime> {
    let within_until = |t: NaiveDateTime| match rule.until {
        None => true,
        Some(Until::Date(d)) => t.date() <= d,
        Some(Until::Local(u)) => t <= u,
        Some(Until::Utc(u)) => to_utc(t) <= u,
    };
    let mut out = vec![dtstart];
    if !within_until(dtstart) || dtstart > stop_after {
        return if dtstart > stop_after { vec![] } else { out };
    }
    let mut count = 1u32;
    let time = dtstart.time();
    let start = dtstart.date();
    for k in 0..MAX_PERIODS {
        if rule.count.is_some_and(|c| count >= c) {
            break;
        }
        let (period_start, set) = period_dates(rule, start, k);
        if period_start > stop_after.date() {
            break;
        }
        for d in apply_set_pos(rule, set) {
            let t = d.and_time(time);
            if t <= dtstart {
                continue;
            }
            if !within_until(t) || t > stop_after {
                return out;
            }
            out.push(t);
            count += 1;
            if rule.count.is_some_and(|c| count >= c) {
                return out;
            }
        }
    }
    out
}
