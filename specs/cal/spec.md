# skedaddle calendar (prefix CAL)

Home repo: skedaddle. Windows tray app, read-only calendar from iCalendar (RFC 5545) feed URLs, with loud meeting alerts.

## Parsing and time

### CAL-ICS-PARSE: iCalendar content lines
**Applies to:** skedaddle
**Verification:** test

The app unfolds lines, reads quoted parameters, unescapes text, and reads DATE, DATE-TIME, UTC and TZID values.

**Acceptance criteria:**
- `A:b` + CRLF + space + `c` unfolds to `A:bc`.
- `CN="Doe; John:X"` reads as one parameter value.
- `\,` `\;` `\n` `\\` unescape to `,` `;` newline `\`.

### CAL-RECUR: Recurrence expansion
**Applies to:** skedaddle
**Verification:** test

RRULE and RDATE expand inside the requested window. EXDATE removes instances. A RECURRENCE-ID event replaces its instance. A cancelled event or cancelled override is not shown.

**Acceptance criteria:**
- Weekly series with one EXDATE, one moved instance and one cancelled instance yields the expected instance list.
- An expansion window returns only events that overlap it.

### CAL-TZ: Time zones
**Applies to:** skedaddle
**Verification:** test

Events show in the display zone. A recurring event keeps its wall-clock time across DST in its own zone. An unknown TZID is read as a floating time in the display zone.

**Acceptance criteria:**
- Weekly 09:30 New York event is 14:30Z before US DST and 13:30Z after.
- `Europe/London` 15:00 on 2026-03-10 is 11:00 New York.
- All-day event runs from local midnight to the next local midnight.

## Alerts

### CAL-ALERT-SELECT: Which events alert
**Applies to:** skedaddle
**Verification:** test

An event alerts from `lead` before start until `grace` after start. The event is timed, not cancelled, and not declined by the feed owner. With `skip_free`, events marked TRANSPARENT do not alert. Each (feed, uid, start) alerts one time.

**Acceptance criteria:**
- Not due at start − 61 s. Due at start − 59 s. Due at start + 5 min. Not due after start + 5 min + 1 s.
- After the key is in the seen set, the event is not due again, and `next_wake` moves to the next event.
- Declined and all-day events never alert. A free event alerts, unless `skip_free` is set.

### CAL-JOIN-URL: Join link
**Applies to:** skedaddle
**Verification:** test

The join link comes from `X-GOOGLE-CONFERENCE`, then `LOCATION`, then `DESCRIPTION`. For the last two, only meet.google.com, zoom.us, teams.microsoft.com and webex.com (and subdomains) count.

**Acceptance criteria:**
- Order is conference, location, description.
- `zoom.us.evil.com` and `notzoom.us` do not match.
- Trailing punctuation is not part of the link.

### CAL-FEED-CONFIG: Config
**Applies to:** skedaddle
**Verification:** test

The config is TOML. The built-in template parses. A Google feed URL gives the owner email. `owner_email` overrides it. Bad TOML is an error.

**Acceptance criteria:**
- Template parses with no feeds.
- `.../ical/me%40example.com/private-x/basic.ics` gives `me@example.com`.
- `poll_minutes = [` is an error.

### CAL-FLASH: Screen flash
**Applies to:** skedaddle
**Verification:** manual

**Verification (manual):** Tray menu "Test alert" with two or more monitors connected. Every monitor shows one blue pulse (default colour) for about 1.2 s. Click and type through the pulse; focus and input do not change. With Focus Assist on "Alarms only", the flash and modal still show.

### CAL-MODAL: Alert popup
**Applies to:** skedaddle
**Verification:** manual

**Verification (manual):** "Test alert" opens a topmost popup on the monitor with the cursor. It does not take focus: type in another window while it shows and no key reaches it. It has Join (when a link exists), Details and Dismiss. The flash repeats every `repeat_seconds` until Dismiss or `repeat_until_minutes` after start. Two events with one start time share one popup.

### CAL-STALE-WARN: Stale feed warning
**Applies to:** skedaddle
**Verification:** manual

**Verification (manual):** Set a feed URL that fails after one good sync (or disconnect the network). After 30 min the tray icon shows the warning badge and the tooltip says "not synced since HH:MM". After a good sync the badge goes away.

### CAL-OFFLINE-CACHE: Offline start
**Applies to:** skedaddle
**Verification:** manual

**Verification (manual):** Run once with network. Quit. Disconnect network and start the app. The view shows events and alerts fire from the cache.

## Shell

### CAL-TRAY: Tray
**Applies to:** skedaddle
**Verification:** manual

**Verification (manual):** Left click toggles the calendar window. Right click shows: Open calendar, Refresh now, Test alert, Open config file, Start with Windows, Quit. The tooltip shows the next event. A second launch opens the window of the first instance. After `taskkill /f /im explorer.exe` and restart of Explorer, the icon returns.

### CAL-VIEW: Calendar window
**Applies to:** skedaddle
**Verification:** manual

**Verification (manual):** Day, Week and Month views show the correct events, all-day strip, overlap columns, now line, and "+N more" in a full month cell. Keys D W M T ← → PgUp PgDn Esc work. Click on an event shows details and Join. The window follows Windows light/dark mode and renders sharp at 100% and 200% DPI on two monitors.

### CAL-AUTOSTART: Start with Windows
**Applies to:** skedaddle
**Verification:** manual

**Verification (manual):** Tick "Start with Windows". `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\skedaddle` holds the exe path. Sign out and in: the app runs. Untick: the value is gone.

### CAL-FOOTPRINT: Resource use
**Applies to:** skedaddle
**Verification:** manual

**Verification (manual):** Release binary ≤ 4 MB. Private working set (Task Manager → Details) ≤ 8 MB with the window never opened, ≤ 40 MB with the window open, ≤ 15 MB after it closes. CPU 0% between timers.
