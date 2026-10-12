use std::collections::HashSet;

use chrono::{DateTime, Duration, NaiveDate, TimeZone, Utc};
use skedaddle::alerts::{due, next_wake, Rules};
use skedaddle::config::{parse_color, Config, FeedCfg, TEMPLATE};
use skedaddle::expand::{expand, FeedMeta};
use skedaddle::ical::{parse, parse_line, unescape, unfold};
use skedaddle::join::find_join_url;
use skedaddle::model::{Occurrence, Rsvp, Tz};

const FEED: &str = include_str!("fixtures/google-like.ics");
const NY: Tz = Tz::America__New_York;

fn utc(y: i32, m: u32, d: u32, h: u32, mi: u32, s: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(y, m, d, h, mi, s).unwrap()
}

fn load() -> Vec<Occurrence> {
    let cal = parse(FEED);
    let meta = FeedMeta { index: 0, owner_email: Some("me@example.com") };
    expand(&cal, &meta, NY, utc(2026, 1, 1, 0, 0, 0), utc(2026, 4, 30, 0, 0, 0))
}

fn local(o: &Occurrence) -> String {
    o.start.format("%Y-%m-%d %H:%M").to_string()
}

fn by_uid<'a>(occs: &'a [Occurrence], uid: &str) -> Vec<&'a Occurrence> {
    occs.iter().filter(|o| o.uid == uid).collect()
}

// Traces: CAL-ICS-PARSE
#[test]
fn unfolds_lines_and_reads_quoted_params() {
    assert_eq!(unfold("A:b\r\n c\r\nD:e"), vec!["A:bc", "D:e"]);
    let p = parse_line(r#"ATTENDEE;CN="Doe; John:X";PARTSTAT=DECLINED:mailto:a@b.com"#).unwrap();
    assert_eq!(p.name, "ATTENDEE");
    assert_eq!(p.param("CN"), Some("Doe; John:X"));
    assert_eq!(p.param("PARTSTAT"), Some("DECLINED"));
    assert_eq!(p.value, "mailto:a@b.com");
    assert_eq!(unescape(r"a\, b\;c\nd\\e"), "a, b;c\nd\\e");
}

// Traces: CAL-ICS-PARSE
#[test]
fn reads_text_fields_from_fixture() {
    let occs = load();
    let v = by_uid(&occs, "utc@google.com");
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].location, "Room 4, Floor 2");
    assert!(v[0].description.starts_with("Agenda: pricing, terms\nJoin at https://zoom.us/j/123456789?pwd=abc"));
    assert_eq!(v[0].join_url.as_deref(), Some("https://zoom.us/j/123456789?pwd=abc"));
}

// Traces: CAL-RECUR
#[test]
fn weekly_series_applies_exdate_override_and_cancel() {
    let occs = load();
    let got: Vec<(String, String)> =
        by_uid(&occs, "standup@google.com").iter().map(|o| (local(o), o.title.clone())).collect();
    let expect: Vec<(&str, &str)> = vec![
        ("2026-01-05 09:30", "Standup"),
        ("2026-01-12 09:30", "Standup"),
        ("2026-01-26 11:00", "Standup (moved)"),
        ("2026-02-09 09:30", "Standup"),
        ("2026-02-16 09:30", "Standup"),
        ("2026-02-23 09:30", "Standup"),
        ("2026-03-02 09:30", "Standup"),
        ("2026-03-09 09:30", "Standup"),
        ("2026-03-16 09:30", "Standup"),
        ("2026-03-23 09:30", "Standup"),
        ("2026-03-30 09:30", "Standup"),
    ];
    let expect: Vec<(String, String)> = expect.into_iter().map(|(a, b)| (a.into(), b.into())).collect();
    assert_eq!(got, expect);
}

// Traces: CAL-RECUR
#[test]
fn cancelled_event_is_dropped() {
    let occs = load();
    assert!(by_uid(&occs, "cancelled@google.com").is_empty());
}

// Traces: CAL-RECUR
#[test]
fn window_limits_expansion() {
    let cal = parse(FEED);
    let meta = FeedMeta { index: 0, owner_email: None };
    let v = expand(&cal, &meta, NY, utc(2026, 2, 8, 0, 0, 0), utc(2026, 2, 15, 0, 0, 0));
    let titles: Vec<&str> = v.iter().map(|o| o.title.as_str()).collect();
    assert_eq!(titles, vec!["Standup", "Offsite", "Vendor call"]);
}

// Traces: CAL-TZ
#[test]
fn zones_and_dst_resolve_to_local_wall_time() {
    let occs = load();
    let standup = by_uid(&occs, "standup@google.com");
    let mar2 = standup.iter().find(|o| local(o) == "2026-03-02 09:30").unwrap();
    let mar9 = standup.iter().find(|o| local(o) == "2026-03-09 09:30").unwrap();
    assert_eq!(mar2.start_utc(), utc(2026, 3, 2, 14, 30, 0));
    assert_eq!(mar9.start_utc(), utc(2026, 3, 9, 13, 30, 0));

    let london = by_uid(&occs, "london@google.com");
    assert_eq!(local(london[0]), "2026-03-10 11:00");
    let vendor = by_uid(&occs, "utc@google.com");
    assert_eq!(local(vendor[0]), "2026-02-12 13:00");
}

// Traces: CAL-TZ
#[test]
fn all_day_event_spans_local_midnights() {
    let occs = load();
    let off = by_uid(&occs, "offsite@google.com");
    assert_eq!(off.len(), 1);
    assert!(off[0].all_day);
    assert_eq!(local(off[0]), "2026-02-10 00:00");
    assert_eq!(off[0].end.format("%Y-%m-%d %H:%M").to_string(), "2026-02-11 00:00");
    assert_eq!(off[0].end.date_naive(), NaiveDate::from_ymd_opt(2026, 2, 11).unwrap());
}

fn rules() -> Rules {
    Rules { lead: Duration::minutes(1), grace: Duration::minutes(5), skip_free: false }
}

// Traces: CAL-ALERT-SELECT
#[test]
fn alert_fires_inside_lead_and_grace_window_only() {
    let occs = load();
    let seen = HashSet::new();
    let start = utc(2026, 1, 5, 14, 30, 0);
    assert!(due(&occs, start - Duration::seconds(61), &rules(), &seen).is_empty());
    assert_eq!(due(&occs, start - Duration::seconds(59), &rules(), &seen).len(), 1);
    assert_eq!(due(&occs, start + Duration::minutes(5), &rules(), &seen).len(), 1);
    assert!(due(&occs, start + Duration::minutes(5) + Duration::seconds(1), &rules(), &seen).is_empty());
}

// Traces: CAL-ALERT-SELECT
#[test]
fn alert_fires_once_per_occurrence() {
    let occs = load();
    let now = utc(2026, 1, 5, 14, 30, 0);
    let mut seen = HashSet::new();
    let first = due(&occs, now, &rules(), &seen);
    assert_eq!(first.len(), 1);
    seen.insert(first[0].key());
    assert!(due(&occs, now, &rules(), &seen).is_empty());
    let next = next_wake(&occs, now, &rules(), &seen).unwrap();
    assert_eq!(next, utc(2026, 1, 12, 14, 29, 0));
}

// Traces: CAL-ALERT-SELECT
#[test]
fn alert_skips_declined_all_day_and_optionally_free() {
    let occs = load();
    let seen = HashSet::new();
    let review = by_uid(&occs, "review@google.com")[0];
    assert_eq!(review.rsvp, Rsvp::Declined);
    assert!(due(&occs, review.start_utc(), &rules(), &seen).is_empty());

    let off = by_uid(&occs, "offsite@google.com")[0];
    assert!(due(&occs, off.start_utc(), &rules(), &seen).is_empty());

    let mut free = occs.iter().find(|o| o.uid == "utc@google.com").unwrap().clone();
    free.busy = false;
    let list = vec![free.clone()];
    assert_eq!(due(&list, free.start_utc(), &rules(), &seen).len(), 1);
    let skip = Rules { skip_free: true, ..rules() };
    assert!(due(&list, free.start_utc(), &skip, &seen).is_empty());
}

// Traces: CAL-ALERT-SELECT
#[test]
fn next_wake_ignores_declined_events() {
    let occs = load();
    let seen = HashSet::new();
    let t = next_wake(&occs, utc(2026, 2, 20, 0, 0, 0), &rules(), &seen).unwrap();
    assert_eq!(t, utc(2026, 2, 23, 14, 29, 0));
}

// Traces: CAL-JOIN-URL
#[test]
fn join_url_prefers_conference_then_location_then_description() {
    let meet = "https://meet.google.com/abc-defg-hij";
    let zoom = "https://us02web.zoom.us/j/99?pwd=x";
    assert_eq!(find_join_url(Some(meet), zoom, "https://webex.com/x").as_deref(), Some(meet));
    assert_eq!(find_join_url(None, &format!("Zoom: {zoom}."), "https://webex.com/x").as_deref(), Some(zoom));
    assert_eq!(find_join_url(None, "Room 4", "see <a href=\"https://teams.microsoft.com/l/m\">link</a>").as_deref(), Some("https://teams.microsoft.com/l/m"));
}

// Traces: CAL-JOIN-URL
#[test]
fn join_url_ignores_unlisted_domains_and_lookalikes() {
    assert_eq!(find_join_url(None, "https://example.com/agenda", "https://notzoom.us/j/1"), None);
    assert_eq!(find_join_url(None, "", "https://zoom.us.evil.com/j/1"), None);
}

// Traces: CAL-FEED-CONFIG
#[test]
fn config_template_parses_and_owner_comes_from_google_url() {
    let c = Config::parse(TEMPLATE).unwrap();
    assert_eq!(c.alert.lead_minutes, 1);
    assert!(c.feed.is_empty());
    let c = Config::parse("[[feed]]\nname='W'\nurl='https://calendar.google.com/calendar/ical/me%40example.com/private-abc/basic.ics'").unwrap();
    assert_eq!(c.feed[0].owner().as_deref(), Some("me@example.com"));
    let f = FeedCfg { url: "https://x/y.ics".into(), owner_email: Some("a@b.c".into()), ..Default::default() };
    assert_eq!(f.owner().as_deref(), Some("a@b.c"));
    assert!(Config::parse("poll_minutes = [").is_err());
    assert_eq!(parse_color("#2F80ED"), Some(0x2F80ED));
    assert_eq!(parse_color("red"), None);
}

// Traces: CAL-RECUR
#[test]
fn weekly_all_day_series_with_date_until_expands_in_local_zone() {
    let ics = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:ooo\nSUMMARY:Out\nDTSTART;VALUE=DATE:20260302\nDTEND;VALUE=DATE:20260303\nRRULE:FREQ=WEEKLY;UNTIL=20260316\nEND:VEVENT\nEND:VCALENDAR\n";
    let cal = parse(ics);
    let meta = FeedMeta { index: 0, owner_email: None };
    let v = expand(&cal, &meta, Tz::LOCAL, utc(2026, 2, 20, 0, 0, 0), utc(2026, 4, 1, 0, 0, 0));
    let days: Vec<String> = v.iter().map(|o| o.start.format("%Y-%m-%d").to_string()).collect();
    assert_eq!(days, vec!["2026-03-02", "2026-03-09", "2026-03-16"]);
    assert!(v.iter().all(|o| o.all_day));
}
