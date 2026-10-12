use chrono::{NaiveDateTime, TimeZone, Utc};
use skedaddle::expand::{expand, FeedMeta};
use skedaddle::ical::parse;
use skedaddle::tz::{feed_zones, system_zone, Zone};

fn dt(s: &str) -> NaiveDateTime {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M").unwrap()
}

const US_EASTERN: &str = "BEGIN:VTIMEZONE\nTZID:Eastern Standard Time\nBEGIN:STANDARD\nDTSTART:16010101T020000\nTZOFFSETFROM:-0400\nTZOFFSETTO:-0500\nRRULE:FREQ=YEARLY;INTERVAL=1;BYDAY=1SU;BYMONTH=11\nEND:STANDARD\nBEGIN:DAYLIGHT\nDTSTART:16010101T020000\nTZOFFSETFROM:-0500\nTZOFFSETTO:-0400\nRRULE:FREQ=YEARLY;INTERVAL=1;BYDAY=2SU;BYMONTH=3\nEND:DAYLIGHT\nEND:VTIMEZONE\n";

fn starts_utc(ics: &str) -> Vec<String> {
    let meta = FeedMeta { index: 0, owner_email: None };
    let from = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
    let to = Utc.with_ymd_and_hms(2027, 1, 1, 0, 0, 0).unwrap();
    expand(&parse(ics), &meta, &Zone::Fixed(0), from, to).iter().map(|o| o.start_utc().format("%Y-%m-%d %H:%M").to_string()).collect()
}

// Traces: CAL-TZ
#[test]
fn zone_defined_only_by_vtimezone() {
    let ics = "BEGIN:VCALENDAR\nBEGIN:VTIMEZONE\nTZID:Test Zone\nBEGIN:STANDARD\nDTSTART:19700101T000000\nTZOFFSETFROM:+0530\nTZOFFSETTO:+0530\nEND:STANDARD\nEND:VTIMEZONE\nBEGIN:VEVENT\nUID:t\nSUMMARY:T\nDTSTART;TZID=Test Zone:20260310T090000\nDTEND;TZID=Test Zone:20260310T100000\nEND:VEVENT\nEND:VCALENDAR\n";
    assert_eq!(starts_utc(ics), ["2026-03-10 03:30"]);
}

// Traces: CAL-TZ
#[test]
fn outlook_windows_zone_name_uses_its_vtimezone() {
    let ics = format!(
        "BEGIN:VCALENDAR\n{US_EASTERN}BEGIN:VEVENT\nUID:w\nSUMMARY:Winter\nDTSTART;TZID=Eastern Standard Time:20260105T093000\nDTEND;TZID=Eastern Standard Time:20260105T100000\nEND:VEVENT\nBEGIN:VEVENT\nUID:s\nSUMMARY:Summer\nDTSTART;TZID=Eastern Standard Time:20260706T093000\nDTEND;TZID=Eastern Standard Time:20260706T100000\nEND:VEVENT\nEND:VCALENDAR\n"
    );
    assert_eq!(starts_utc(&ics), ["2026-01-05 14:30", "2026-07-06 13:30"]);
}

// Traces: CAL-TZ
#[test]
fn iana_zone_without_vtimezone_comes_from_windows_icu() {
    let london = system_zone("Europe/London").expect("Windows ICU knows Europe/London");
    assert_eq!(london.offset_at(dt("2026-01-15T12:00")), 0);
    assert_eq!(london.offset_at(dt("2026-07-01T12:00")), 3600);
    let kolkata = system_zone("Asia/Kolkata").unwrap();
    assert_eq!(kolkata.offset_at(dt("2026-07-01T12:00")), 19800);
    assert!(system_zone("Not/AZone").is_none());

    let ics = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:l\nSUMMARY:L\nDTSTART;TZID=Europe/London:20260710T150000\nDTEND;TZID=Europe/London:20260710T160000\nEND:VEVENT\nEND:VCALENDAR\n";
    assert_eq!(starts_utc(ics), ["2026-07-10 14:00"]);
}

// Traces: CAL-TZ
#[test]
fn gap_moves_forward_and_overlap_takes_earlier_instant() {
    let cal = parse(&format!("BEGIN:VCALENDAR\n{US_EASTERN}END:VCALENDAR\n"));
    let east = feed_zones(&cal)["Eastern Standard Time"].clone();
    assert_eq!(east.to_utc(dt("2026-03-08T02:30")), dt("2026-03-08T07:30"));
    assert_eq!(east.display(dt("2026-03-08T07:30")).format("%H:%M").to_string(), "03:30");
    assert_eq!(east.to_utc(dt("2026-11-01T01:30")), dt("2026-11-01T05:30"));
    assert_eq!(east.to_utc(dt("2026-11-01T03:00")), dt("2026-11-01T08:00"));
}
