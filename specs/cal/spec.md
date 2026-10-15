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

The engine supports `FREQ` DAILY, WEEKLY, MONTHLY and YEARLY with `INTERVAL`, `COUNT`, `UNTIL` (date, local date-time or UTC), `BYDAY` (with ordinals), `BYMONTHDAY` (also negative), `BYMONTH`, `BYSETPOS` and `WKST`. A rule with any other part (`BYWEEKNO`, `BYYEARDAY`, `BYHOUR`, `BYMINUTE`, `BYSECOND`, or a shorter `FREQ`) shows its first instance only. `DTSTART` is always the first instance. Invalid dates (the 31st in a 30-day month) are skipped. `COUNT` is applied before `EXDATE`.

**Acceptance criteria:**
- Weekly series with one EXDATE, one moved instance and one cancelled instance yields the expected instance list.
- An expansion window returns only events that overlap it.
- Hand-checked lists for: 2nd Tuesday, last Friday, last day of month, last weekday (`BYSETPOS=-1`), monthly from the 31st, every other week on two days with `WKST=SU`, the US DST start rule, `UNTIL` as a date and as UTC (inclusive), and `COUNT=5` with one `EXDATE` (four instances).
- Unsupported rules parse as unsupported. An impossible rule (`BYMONTH=2;BYMONTHDAY=30`) ends with `DTSTART` only.
- Differential: 28 common rules give the same instances as the `rrule` crate (dev-dependency only) through 2034.

### CAL-TZ: Time zones
**Applies to:** skedaddle
**Verification:** test

Events show in the display zone. A recurring event keeps its wall-clock time across DST in its own zone. A `TZID` resolves in this order: the feed's own `VTIMEZONE` with that `TZID`, then the IANA database in Windows ICU (`icu.dll`, Windows 10 1903+), then a floating time in the display zone. A local time in a spring-forward gap moves forward by the gap; a time in a fall-back overlap takes the earlier instant.

**Acceptance criteria:**
- Weekly 09:30 New York event is 14:30Z before US DST and 13:30Z after.
- `Europe/London` 15:00 on 2026-03-10 (no `VTIMEZONE` in the feed) is 11:00 New York.
- All-day event runs from local midnight to the next local midnight.
- A zone defined only by a `VTIMEZONE` ("Test Zone", +05:30) resolves to its offset.
- `TZID=Eastern Standard Time` with its Outlook `VTIMEZONE`: 09:30 is 14:30Z in January and 13:30Z in July.
- 02:30 on the US spring-forward day is 07:30Z (03:30 EDT). 01:30 on the fall-back day is 05:30Z.

## Alerts

### CAL-ALERT-SELECT: Which events alert
**Applies to:** skedaddle
**Verification:** test

An event alerts from `lead` (a setting, 1 to 10 minutes) before start until 5 minutes (fixed) after start. The event is timed, not cancelled, and not declined by the feed owner. With `skip_free`, events marked TRANSPARENT do not alert. Each (feed, uid, start) alerts one time.

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

The config is TOML. An empty file parses to the defaults. A Google feed URL gives the owner email. `owner_email` overrides it. Bad TOML is an error. Keys from older versions (`poll_minutes`, `alert.flash`, `alert.flash_color`, `alert.sound` and others) are ignored. The app writes the file itself from the Settings window; nobody edits it by hand. `Config::validate` returns one message per bad field.

**Acceptance criteria:**
- An empty file parses with no feeds and `lead_minutes = 1`.
- `.../ical/me%40example.com/private-x/basic.ics` gives `me@example.com`.
- `poll_minutes = [` is an error.
- A `Config` with two feeds and non-default settings survives serialize, then parse, unchanged.
- A file written by an older version still loads.
- `validate` accepts the default config (no feeds) and rejects: a week start other than sunday/monday, a clock other than 12h/24h, an unknown theme, `lead_minutes` outside 0..=60, a day view shorter than 4 hours or outside 12 AM to 12 AM, an unknown `text_size`, a feed address that is not `https://`, `http://` or `webcal://`, and a feed color that is not `#RRGGBB`.

### CAL-THEME: Themes
**Applies to:** skedaddle
**Verification:** test

A theme sets every color and the font in the app: calendar, Settings, alert popup, and the screen flash (the theme's accent). `auto` follows the Windows light/dark setting by picking the `light` or `dark` theme. The theme list is Auto, 6 light, 6 midtone and 6 dark themes. Meeting titles never include the meeting time; the time appears on its own line when the block is tall enough. Every button, segment, toggle, theme and calendar row in the calendar view and Settings shows hover feedback; the gear button has no background until hovered. Each theme names a Windows-installed font that all text in the app uses; if the font is missing, Segoe UI is used. A theme id that no longer exists (for example `porcelain`) renders as Auto.

**Acceptance criteria:**
- Theme ids are unique. `auto` is known; `neon` is not.
- `auto` resolves to `light` on a light system and `dark` on a dark system. An unknown id such as `porcelain` resolves the same way.
- For every theme: text on background and on panel has contrast of at least 4.5, dim text on background at least 3, the accent differs from the background (contrast at least 1.5), and the text color chosen for accent and for "ok" buttons has contrast of at least 3.

### CAL-FLASH: Screen flash
**Applies to:** skedaddle
**Verification:** manual

**Verification (manual):** Create a calendar event starting within the alert lead time, with two or more monitors connected. Every monitor shows one pulse in the theme's accent color for about 1.2 s. The flash cannot be turned off. Click and type through the pulse; focus and input do not change. With Focus Assist on "Alarms only", the flash and modal still show.

### CAL-MODAL: Alert popup
**Applies to:** skedaddle
**Verification:** manual

**Verification (manual):** An event reaching its alert lead time opens a topmost popup on the monitor with the cursor. It does not take focus: type in another window while it shows and no key reaches it. It has Join (when a link exists), Details and Dismiss. The flash repeats every 60 s until Dismiss or 5 minutes after start. The popup uses the theme's colors. Two events with one start time share one popup.

### CAL-STALE-WARN: Stale feed warning
**Applies to:** skedaddle
**Verification:** manual

**Verification (manual):** Set a feed URL that fails after one good sync (or disconnect the network). After 30 min the tray icon shows the warning badge. When no meeting is upcoming, the tooltip also says "Not synced since HH:MM". After a good sync the badge goes away.

### CAL-OFFLINE-CACHE: Offline start
**Applies to:** skedaddle
**Verification:** manual

**Verification (manual):** Run once with network. Quit. Disconnect network and start the app. The view shows events and alerts fire from the cache.

## Shell

### CAL-TRAY: Tray
**Applies to:** skedaddle
**Verification:** manual

**Verification (manual):** Left click toggles the calendar window. Clicking a meeting opens its details in a sidebar that narrows the calendar; it does not cover it. Right click shows: Open calendar, Refresh now, Test alert, Settings, Start with Windows, Install Skedaddle (only while running from outside the install folder), "Like this? Tip my local NPR station" (opens https://www.kuaf.com/donate), Quit. Left click opens Settings instead when no calendar is configured or the config file cannot be read. The tooltip shows only the next upcoming event; when there is none it shows the current event or "No upcoming events", plus "Not synced since HH:MM" if the feeds are stale. A second launch opens the window of the first instance. After `taskkill /f /im explorer.exe` and restart of Explorer, the icon returns.

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

## Settings

### CAL-SETTINGS-FIRST-RUN: First run
**Applies to:** skedaddle
**Verification:** manual

**Verification (manual):** Delete `%APPDATA%\skedaddle\config.toml` and start the app. The Settings window opens with one empty calendar and the address field focused; Notepad does not open. Close it with Cancel: no file is written and the next start opens it again. Paste a calendar address, press Save: `config.toml` exists and the calendar syncs without a restart.

### CAL-SETTINGS-GEAR: Gear button
**Applies to:** skedaddle
**Verification:** manual

**Verification (manual):** The calendar toolbar has a gear button at the far right, after Month. It opens Settings. A second click, or "Settings" in the tray menu, focuses the open window instead of opening another.

### CAL-SETTINGS-EDIT: Settings window
**Applies to:** skedaddle
**Verification:** manual

**Verification (manual):** Settings shows: the calendar list with Add and Remove and, for the selected calendar, name, address and "your email" (the calendar color is assigned automatically); Week starts on; Clock; Day view (start and end hour, at least 4 hours apart, default 7 AM to 6 PM; the day and week views stretch that range to fill the window and the rest of the day is a scroll away); Text size (Small, Medium, Large; applies to the calendar, the alert popup and Settings, and Settings resizes its text as soon as a size is clicked); Minutes before start (1, 2, 5, 10); Play a sound; Skip free events; and the theme picker. The picker is a four-way switch, Auto / Light / Midtone / Dark. Auto shows no list; the other three show only that group's themes, each with a color swatch and its name in its own font. Switching group picks the group's first theme. Choosing a theme recolors the window and changes its font at once; Cancel discards it. Save with a bad address (`ftp://x`) keeps the window open and shows the first error in red. Save with valid values writes the file, closes the window, and the calendar view, popup and flash use the new theme and its font. Enter saves, Esc cancels, Tab moves between the text fields.

## Install

### CAL-INSTALL: Self-install
**Applies to:** skedaddle
**Verification:** manual

The exe can install itself for the current user, with no admin rights: it copies itself to `%LOCALAPPDATA%\Programs\Skedaddle\skedaddle.exe`, creates a Start Menu shortcut, registers an uninstall entry (Settings > Apps) whose command is `skedaddle.exe --uninstall`, repoints an existing Start with Windows entry, and starts the installed copy. On launch, from a copy that is not the installed one, it asks once. Answering No is remembered in `%LOCALAPPDATA%\skedaddle\install-declined.txt`, so it never asks again. It also does not ask when an installed copy already exists. The tray menu shows "Install Skedaddle..." whenever the running copy is not the installed one.

**Verification (manual):** With `APPDATA` and `LOCALAPPDATA` pointing at empty folders, run a copy of the exe from another folder. (1) Answer Yes: the installed exe, the shortcut and the uninstall registry key exist, the installed copy runs, the first copy exits. (2) Run `skedaddle.exe --uninstall` from the install folder and answer Yes, then No to deleting settings: the shortcut and registry key are gone, the running copy closes, and the install folder is deleted a few seconds later. (3) Delete the install folder, run the copy again and answer No: `install-declined.txt` exists and the app keeps running; run it a third time: no prompt appears. (4) The tray menu still lists "Install Skedaddle..." and choosing it installs without asking.

