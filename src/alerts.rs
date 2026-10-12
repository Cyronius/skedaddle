use std::collections::HashSet;

use chrono::{DateTime, Duration, Utc};

use crate::model::{AlertKey, Occurrence, Rsvp};

#[derive(Clone, Copy, Debug)]
pub struct Rules {
    pub lead: Duration,
    pub grace: Duration,
    pub skip_free: bool,
}

pub fn eligible(o: &Occurrence, rules: &Rules) -> bool {
    !o.all_day && o.rsvp != Rsvp::Declined && (o.busy || !rules.skip_free)
}

pub fn due<'a>(occs: &'a [Occurrence], now: DateTime<Utc>, rules: &Rules, seen: &HashSet<AlertKey>) -> Vec<&'a Occurrence> {
    let mut v: Vec<&Occurrence> = occs
        .iter()
        .filter(|o| eligible(o, rules))
        .filter(|o| {
            let s = o.start_utc();
            s - rules.lead <= now && now <= s + rules.grace
        })
        .filter(|o| !seen.contains(&o.key()))
        .collect();
    v.sort_by_key(|o| o.start.timestamp());
    v
}

pub fn next_wake(occs: &[Occurrence], now: DateTime<Utc>, rules: &Rules, seen: &HashSet<AlertKey>) -> Option<DateTime<Utc>> {
    occs.iter()
        .filter(|o| eligible(o, rules) && !seen.contains(&o.key()))
        .map(|o| o.start_utc() - rules.lead)
        .filter(|t| *t > now)
        .min()
}
