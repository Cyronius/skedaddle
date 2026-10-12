# Plan: drop chrono-tz and the regex engine

Status: done. Exe went from 2.9 MiB to 0.92 MiB (961,024 bytes). All 30 tests pass, including the 28-rule differential test against `rrule`.

## Goal

Cut the exe from 2.9 MiB to about 1.0–1.1 MiB. To do this, remove `chrono-tz` (~1.2 MiB of time zone tables) and `rrule`. `rrule` is the only crate that uses `regex` (~0.45 MiB of code). `chrono` stays, but without time zone data.

RAM does not change much. This is a download-size change.

## Spec impact

### Modified: CAL-TZ

New rule for how a `TZID` turns into a time zone, in this order:

1. A `VTIMEZONE` block in the same feed with that `TZID`. RFC 5545 requires one for every `TZID` that a feed uses. Outlook zone names such as "Eastern Standard Time" work this way. This removes the README limitation for Outlook feeds.
2. The IANA time zone database that ships with Windows, through ICU (`icu.dll`, Windows 10 1903 and later). Some Google feeds use a `TZID` with no `VTIMEZONE` block. This step covers them.
3. If neither one knows the zone, the time is read as local time (same as today).

New acceptance criteria:
- A zone that exists only as a `VTIMEZONE` (an invented name, for example "Test Zone" at UTC+5:30 with no DST) resolves to the offsets in its block.
- An Outlook-style feed (`TZID=Eastern Standard Time` plus its `VTIMEZONE`) puts a 09:30 meeting at 13:30Z in summer and 14:30Z in winter.
- An IANA `TZID` with no `VTIMEZONE` (`Europe/London`) resolves through Windows ICU.
- A local time in the spring-forward gap moves forward by the gap. A local time in the fall-back overlap takes the first (earlier) instant.

### Modified: CAL-RECUR

We write our own RRULE engine. It supports:
- `FREQ` = DAILY, WEEKLY, MONTHLY, YEARLY
- `INTERVAL`, `COUNT`, `UNTIL` (date, local date-time, or UTC)
- `BYDAY` (with ordinals such as `2TU` and `-1FR`), `BYMONTHDAY` (also negative), `BYMONTH`, `BYSETPOS`, `WKST`

Rules that use other parts (`BYWEEKNO`, `BYYEARDAY`, `BYHOUR`/`BYMINUTE`/`BYSECOND`, or a FREQ shorter than DAILY) show the first instance only. Google and Outlook do not create such rules from their normal UI.

`DTSTART` is always the first instance. Invalid dates are skipped (for example, the 31st in a 30-day month), as RFC 5545 says. `COUNT` counts before `EXDATE` removes instances.

New acceptance criteria (each case is a test with a hand-written list of expected dates):
- `FREQ=MONTHLY;BYDAY=2TU`: the second Tuesday of each month
- `FREQ=MONTHLY;BYDAY=-1FR`: the last Friday
- `FREQ=MONTHLY;BYMONTHDAY=-1`: the last day of each month
- `FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=-1`: the last weekday of each month (Outlook uses this)
- `FREQ=MONTHLY` starting on the 31st: months with 31 days only
- `FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,WE;WKST=SU`: every other week, on two days
- `FREQ=YEARLY;BYMONTH=3;BYDAY=2SU`: the US DST rule (the same engine reads `VTIMEZONE` rules)
- `COUNT=5` with one `EXDATE`: four instances
- `UNTIL` as a date, and as UTC

Differential test: about 30 rules over 3 years, compared with the `rrule` crate as a **dev-dependency only** (not in the exe). This catches cases that the hand-written lists miss.

### No change

CAL-ICS-PARSE, CAL-ALERT-SELECT, CAL-JOIN-URL, CAL-FEED-CONFIG and the manual requirements.

## Design

New modules in the core library:

| File | What it does |
|---|---|
| `src/recur.rs` | RRULE parse and expansion, in local wall-clock time. Returns local date-times in a window. |
| `src/tz.rs` | `Zone`: maps a UTC instant to an offset and a local time to UTC (gap and overlap rules above). Sources: `VTIMEZONE` rules, the Windows local zone, or an external source. `VTIMEZONE` parsing uses `recur.rs` for its yearly rules. |
| `src/icu.rs` (Windows only) | Loads `icu.dll` at run time (`LoadLibraryW` + `GetProcAddress`; no link-time dependency) and calls `ucal_open` / `ucal_setMillis` / `ucal_get` for zone and DST offsets. If `icu.dll` is missing, step 2 is skipped. |

Changes to existing code:
- `model.rs`: `Occurrence.start` and `.end` become `DateTime<FixedOffset>`, in the display zone's offset at that instant. All view and alert code already uses `naive_local()`, `timestamp()`, `format()` and `with_timezone(&Utc)`, which `FixedOffset` supports.
- `expand.rs`: builds a `TZID → Zone` table from the feed's `VTIMEZONE` blocks, then ICU, then local. It takes the display zone as a `Zone`. Tests pass a `Zone` built from the fixture's New York `VTIMEZONE`, so they do not depend on the PC's time zone.
- `Cargo.toml`: remove `chrono-tz` and `rrule`. Add `rrule` under `[dev-dependencies]` for the differential test only.
- `win/sync.rs`, `win/app.rs`: use `Zone::Local` and `FixedOffset` instead of `rrule::Tz`.
- README: new size figure, remove the Outlook time zone limitation, say "Windows 10 version 1903 or later".

## Steps

1. Write the new CAL-RECUR and CAL-TZ tests first, against the new API. They fail to compile (red).
2. `recur.rs`. Then the differential test against `rrule`.
3. `tz.rs` and the `VTIMEZONE` parser.
4. `icu.rs`.
5. Rewire `expand.rs`, `model.rs` and the Windows shell. Remove the dependencies.
6. All tests green. Run `cargo bloat` and record the new size. Smoke-test the app against the local test feed and an Outlook-style feed.
7. Update the README and `spec.md`. Move this plan to `archive/`.

## Risks

- **RRULE edge cases.** This is the main risk. The hand-written cases plus the differential test cover it. Rules we do not support show one instance; they never crash and never loop forever (there is an iteration cap).
- **ICU on old Windows.** Builds before Windows 10 1903 have no `icu.dll`. On those builds, IANA zones with no `VTIMEZONE` fall back to local time. Windows 10 1809 and older are out of support, so this is acceptable.
- **Bad `VTIMEZONE` data in a feed.** The app trusts the feed's own rules, as RFC 5545 says. If a feed's block is wrong, its times are wrong. This is the same behaviour as Outlook and Thunderbird.

## Size estimate

| | Now | After |
|---|---|---|
| Time zone tables | ~1.2 MiB | 0 |
| Regex stack | ~0.45 MiB | 0 |
| `rrule` code | ~40 KiB | replaced by ~15–25 KiB of our own code |
| **exe** | **2.9 MiB** | **~1.0–1.1 MiB** (estimate; measured in step 6) |
