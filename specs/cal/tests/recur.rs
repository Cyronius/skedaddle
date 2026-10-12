use chrono::{Duration, NaiveDateTime, TimeZone};
use skedaddle::expand::{expand, FeedMeta};
use skedaddle::ical::parse as parse_ics;
use skedaddle::recur::{expand as recur_expand, parse};
use skedaddle::tz::Zone;

fn dt(s: &str) -> NaiveDateTime {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M").unwrap()
}

fn run(rule: &str, start: &str) -> Vec<String> {
    let r = parse(rule).expect("rule should be supported");
    recur_expand(&r, dt(start), &|l| l, dt("2031-01-01T00:00")).iter().map(|d| d.format("%Y-%m-%d").to_string()).collect()
}

// Traces: CAL-RECUR
#[test]
fn second_tuesday_of_month() {
    assert_eq!(run("FREQ=MONTHLY;BYDAY=2TU;COUNT=5", "2026-01-13T10:00"), ["2026-01-13", "2026-02-10", "2026-03-10", "2026-04-14", "2026-05-12"]);
}

// Traces: CAL-RECUR
#[test]
fn last_friday_of_month() {
    assert_eq!(run("FREQ=MONTHLY;BYDAY=-1FR;COUNT=5", "2026-01-30T10:00"), ["2026-01-30", "2026-02-27", "2026-03-27", "2026-04-24", "2026-05-29"]);
}

// Traces: CAL-RECUR
#[test]
fn last_day_of_month() {
    assert_eq!(run("FREQ=MONTHLY;BYMONTHDAY=-1;COUNT=5", "2026-01-31T10:00"), ["2026-01-31", "2026-02-28", "2026-03-31", "2026-04-30", "2026-05-31"]);
}

// Traces: CAL-RECUR
#[test]
fn last_weekday_of_month_with_setpos() {
    assert_eq!(
        run("FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=-1;COUNT=5", "2026-01-30T10:00"),
        ["2026-01-30", "2026-02-27", "2026-03-31", "2026-04-30", "2026-05-29"]
    );
}

// Traces: CAL-RECUR
#[test]
fn monthly_on_the_31st_skips_short_months() {
    assert_eq!(run("FREQ=MONTHLY;COUNT=4", "2026-01-31T10:00"), ["2026-01-31", "2026-03-31", "2026-05-31", "2026-07-31"]);
}

// Traces: CAL-RECUR
#[test]
fn every_other_week_on_two_days() {
    assert_eq!(
        run("FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,WE;WKST=SU;COUNT=6", "2026-01-05T10:00"),
        ["2026-01-05", "2026-01-07", "2026-01-19", "2026-01-21", "2026-02-02", "2026-02-04"]
    );
}

// Traces: CAL-RECUR
#[test]
fn us_dst_start_rule() {
    assert_eq!(run("FREQ=YEARLY;BYMONTH=3;BYDAY=2SU;COUNT=3", "2026-03-08T02:00"), ["2026-03-08", "2027-03-14", "2028-03-12"]);
}

// Traces: CAL-RECUR
#[test]
fn until_as_date_and_as_utc() {
    assert_eq!(run("FREQ=DAILY;UNTIL=20260108", "2026-01-05T09:00"), ["2026-01-05", "2026-01-06", "2026-01-07", "2026-01-08"]);
    let ny_winter = |l: NaiveDateTime| l + Duration::hours(5);
    let weeks = |rule: &str| recur_expand(&parse(rule).unwrap(), dt("2026-01-05T09:00"), &ny_winter, dt("2031-01-01T00:00")).len();
    assert_eq!(weeks("FREQ=WEEKLY;UNTIL=20260119T140000Z"), 3);
    assert_eq!(weeks("FREQ=WEEKLY;UNTIL=20260119T135959Z"), 2);
}

// Traces: CAL-RECUR
#[test]
fn count_is_taken_before_exdate() {
    let ics = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:x\nSUMMARY:X\nDTSTART:20260105T090000Z\nDTEND:20260105T093000Z\nRRULE:FREQ=DAILY;COUNT=5\nEXDATE:20260107T090000Z\nEND:VEVENT\nEND:VCALENDAR\n";
    let meta = FeedMeta { index: 0, owner_email: None };
    let from = chrono::Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
    let to = chrono::Utc.with_ymd_and_hms(2026, 2, 1, 0, 0, 0).unwrap();
    let days: Vec<String> = expand(&parse_ics(ics), &meta, &Zone::Fixed(0), from, to).iter().map(|o| o.start.format("%d").to_string()).collect();
    assert_eq!(days, ["05", "06", "08", "09"]);
}

// Traces: CAL-RECUR
#[test]
fn unsupported_rules_are_rejected_and_impossible_rules_terminate() {
    assert!(parse("FREQ=YEARLY;BYWEEKNO=20").is_none());
    assert!(parse("FREQ=HOURLY").is_none());
    assert!(parse("COUNT=3").is_none());
    assert_eq!(run("FREQ=MONTHLY;BYMONTH=2;BYMONTHDAY=30", "2026-01-30T10:00"), ["2026-01-30"]);
}

fn oracle(rule: &str, start: NaiveDateTime, stop: NaiveDateTime) -> Vec<NaiveDateTime> {
    let src = format!("DTSTART:{}Z\nRRULE:{}", start.format("%Y%m%dT%H%M%S"), rule);
    let set: rrule::RRuleSet = src.parse().unwrap();
    let stop = rrule::Tz::UTC.from_utc_datetime(&stop);
    set.before(stop).all(5000).dates.into_iter().map(|d| d.naive_utc()).collect()
}

// Traces: CAL-RECUR
#[test]
fn matches_rrule_crate_on_common_rules() {
    let cases = [
        ("FREQ=DAILY;COUNT=10", "2026-01-05T09:00"),
        ("FREQ=DAILY;INTERVAL=3;UNTIL=20260401T000000Z", "2026-01-05T09:00"),
        ("FREQ=DAILY;BYDAY=MO,WE,FR;COUNT=30", "2026-01-05T09:00"),
        ("FREQ=DAILY;BYMONTH=1,7;COUNT=40", "2026-01-05T09:00"),
        ("FREQ=WEEKLY;COUNT=20", "2026-01-05T09:00"),
        ("FREQ=WEEKLY;BYDAY=TU,TH;COUNT=30", "2026-01-06T09:00"),
        ("FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,FR;WKST=SU;COUNT=30", "2026-01-05T09:00"),
        ("FREQ=WEEKLY;INTERVAL=3;BYDAY=SA,SU;WKST=MO;COUNT=20", "2026-01-03T09:00"),
        ("FREQ=MONTHLY;COUNT=24", "2026-01-15T09:00"),
        ("FREQ=MONTHLY;BYMONTHDAY=1,15;COUNT=24", "2026-01-01T09:00"),
        ("FREQ=MONTHLY;BYMONTHDAY=-1;COUNT=24", "2026-01-31T09:00"),
        ("FREQ=MONTHLY;BYMONTHDAY=-3;COUNT=12", "2026-01-29T09:00"),
        ("FREQ=MONTHLY;BYMONTHDAY=31;COUNT=7", "2026-01-31T09:00"),
        ("FREQ=MONTHLY;BYDAY=1MO;COUNT=24", "2026-01-05T09:00"),
        ("FREQ=MONTHLY;BYDAY=-1SU;COUNT=24", "2026-01-25T09:00"),
        ("FREQ=MONTHLY;BYDAY=-2WE;COUNT=12", "2026-01-21T09:00"),
        ("FREQ=MONTHLY;BYDAY=TU;COUNT=30", "2026-01-06T09:00"),
        ("FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=1;COUNT=24", "2026-01-01T09:00"),
        ("FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=-1;COUNT=24", "2026-01-30T09:00"),
        ("FREQ=MONTHLY;INTERVAL=2;BYDAY=3TH;COUNT=12", "2026-01-15T09:00"),
        ("FREQ=MONTHLY;BYDAY=FR;BYMONTHDAY=13;COUNT=5", "2026-02-13T09:00"),
        ("FREQ=YEARLY;COUNT=5", "2026-03-10T09:00"),
        ("FREQ=YEARLY;BYMONTH=3;BYDAY=2SU;COUNT=10", "2026-03-08T02:00"),
        ("FREQ=YEARLY;BYMONTH=11;BYDAY=1SU;COUNT=10", "2026-11-01T02:00"),
        ("FREQ=YEARLY;BYMONTH=10;BYDAY=-1SU;COUNT=10", "2026-10-25T01:00"),
        ("FREQ=YEARLY;BYMONTH=1,6;BYMONTHDAY=1;COUNT=10", "2026-01-01T09:00"),
        ("FREQ=YEARLY;BYDAY=20MO;COUNT=5", "2026-05-18T09:00"),
        ("FREQ=YEARLY;INTERVAL=4;BYMONTH=2;BYMONTHDAY=29;COUNT=3", "2028-02-29T09:00"),
    ];
    let stop = dt("2034-12-31T23:59");
    for (rule, start) in cases {
        let ours = recur_expand(&parse(rule).unwrap(), dt(start), &|l| l, stop);
        assert_eq!(ours, oracle(rule, dt(start), stop), "rule {rule} from {start}");
    }
}
