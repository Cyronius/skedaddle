# Skedaddle

**A tiny Windows tray calendar that makes sure you never miss a meeting.**

Calendar notifications are easy to miss. A toast slides in while you are focused, Focus Assist hides it, or it appears on the wrong monitor. Skedaddle does one job: when a meeting is about to start, it flashes every screen and puts a popup in front of you with a **Join** button. Then it gets out of the way.

It reads your calendar from a standard iCalendar (`.ics`) feed, so it works with Google Calendar, Outlook, iCloud, Fastmail and others. No account, no OAuth, no cloud service.

## Lightweight by design

| | Skedaddle |
|---|---|
| Download | **one 0.95 MB exe**, no installer, no dependencies |
| RAM in the tray | **about 2.5 MB** (private memory, measured) |
| CPU when idle | **0%**: no polling loop; it wakes only for a feed sync or an alert |
| Runtime | none: no Electron, no browser engine, no .NET, no background service |

It is written in Rust against the plain Win32 API and draws its own UI with GDI. It ships no time zone database of its own: it reads zone rules from your calendar feed, or from the one already built into Windows. When you close the calendar view, it frees the view's memory and goes back to its tray-only footprint. For comparison, a typical Electron app uses well over 100 MB of RAM just to sit in the tray.

## Features

- **Alerts you cannot miss.** One minute before a meeting, every monitor flashes once and a topmost popup shows the meeting, its time, its location and a Join button. The flash repeats every minute until you dismiss the popup.
- **Works when Focus Assist is on.** Skedaddle uses its own windows, not Windows notifications, so Do Not Disturb does not hide it.
- **The popup never steals your keyboard.** It appears without focus, so a keystroke in the middle of a sentence cannot dismiss it by accident.
- **Join in one click.** It finds Google Meet, Zoom, Teams and Webex links in the event.
- **Skips what you do not need.** It does not alert for declined meetings or all-day events. Optionally, it skips events marked "free".
- **Catches up after sleep.** If your PC wakes up just after a meeting started, you still get the alert.
- **Read-only calendar view.** Click the tray icon for a day, week or month view, with a detail panel for each event.
- **Works offline.** The last good copy of each feed is cached, so alerts still fire without a network.
- **Tells you when it is out of date.** If a feed has not synced for 30 minutes, the tray icon shows a warning badge.

## Requirements

- Windows 10 (version 1903 or later) or Windows 11
- A calendar that can publish an iCalendar (`.ics`) URL

## Install

1. Download `skedaddle.exe` from the [latest release](https://github.com/Cyronius/skedaddle/releases/latest).
2. Put it anywhere, for example `%LOCALAPPDATA%\Programs\Skedaddle\`.
3. Run it. On first start, Skedaddle creates its config file and opens it in Notepad.
4. Right-click the tray icon and select **Start with Windows**, if you want it to start at sign-in.

The exe is not code-signed yet, so Windows SmartScreen can show "Windows protected your PC" on the first run. Select **More info → Run anyway**. Each release also has a `skedaddle.exe.sha256` file if you want to check the download.

To uninstall: untick **Start with Windows**, select **Quit**, then delete the exe and the `%APPDATA%\skedaddle` and `%LOCALAPPDATA%\skedaddle` folders.

### Build from source

1. Install Rust from [rustup.rs](https://rustup.rs).
2. Clone and build:

   ```
   git clone https://github.com/Cyronius/skedaddle.git
   cd skedaddle
   cargo build --release
   ```

3. Run `target\release\skedaddle.exe`.

## Connect your calendar

Paste your calendar's iCalendar URL into the config file as a `[[feed]]`. You can add more than one feed.

**Google Calendar**
1. Open [Google Calendar](https://calendar.google.com) on the web.
2. Go to **Settings**, then select your calendar under **Settings for my calendars**.
3. In **Integrate calendar**, copy **Secret address in iCal format**.

If you do not see the secret address on a Google Workspace (work) account, your admin has turned it off. An admin can turn it on: Admin console → Apps → Google Workspace → Calendar → Sharing settings → External sharing options for primary calendars → "Share all information".

**Outlook / Microsoft 365**
1. In Outlook on the web, open **Settings → Calendar → Shared calendars**.
2. Under **Publish a calendar**, select the calendar and **Can view all details**, then **Publish**.
3. Copy the **ICS** link.

**iCloud**
1. In the Calendar app, share the calendar as a **Public Calendar**.
2. Copy the link. `webcal://` links work.

> The feed URL is a password. Anyone with it can read your calendar. Do not share it or commit it anywhere.

## Configure

The config file is at `%APPDATA%\skedaddle\config.toml`. Right-click the tray icon and select **Open config file** to edit it. Changes load on the next sync. Use **Refresh now** to load them at once.

```toml
poll_minutes = 5        # how often to fetch the feeds
week_start = "sunday"   # or "monday"
clock = "12h"           # or "24h"

[alert]
lead_minutes = 1             # alert this many minutes before the start
grace_minutes = 5            # still alert this long after the start (for example, after sleep)
repeat_seconds = 60          # flash again until dismissed; 0 = flash one time only
repeat_until_minutes = 5     # stop the repeat this long after the start
flash = true                 # set false to keep only the popup
flash_color = "#2F80ED"
sound = "default"            # "none", "default", or a path to a .wav file
skip_free = false            # true = no alert for events marked "free"

[[feed]]
name = "Work"
url = "https://calendar.google.com/calendar/ical/.../private-.../basic.ics"
color = "#4285F4"
# owner_email = "me@example.com"   # only for non-Google feeds; used to skip meetings you declined
```

## Use

| Action | What it does |
|---|---|
| Left-click the tray icon | Open or close the calendar view |
| Right-click the tray icon | Open calendar, Refresh now, Test alert, Open config file, Start with Windows, Quit |
| Hover the tray icon | Shows your next meeting and when the feeds last synced |

Keys in the calendar view:

| Key | Action |
|---|---|
| `D` / `W` / `M` | Day, week or month view |
| `T` | Go to today |
| `←` `→` or `PgUp` `PgDn` | Previous or next day, week or month |
| `Esc` | Close the detail panel, then close the window |

Use **Test alert** in the tray menu to see what an alert looks like.

## Privacy

Skedaddle talks only to the feed URLs that you give it. It has no telemetry, no accounts and no server. Your config and cached feeds stay in your Windows profile:

- `%APPDATA%\skedaddle\config.toml`: settings and feed URLs
- `%LOCALAPPDATA%\skedaddle\cache\`: last good copy of each feed
- `%LOCALAPPDATA%\skedaddle\alerted.txt`: which meetings already alerted, so a restart does not alert again

## Limitations

- **Read-only.** You cannot create or edit events. Use your normal calendar for that.
- **Windows only.**
- **Feed delay.** Each provider decides how fast its feed shows a new event. A meeting added a minute before it starts can be missed.
- **Uncommon repeat rules.** Rules built on week numbers, days of the year, or several times per day show their first instance only. Google, Outlook and iCloud do not create these from their normal UI.
- **Screen sharing.** The flash and popup show in a shared screen, including the meeting title. Set `flash = false` if that is a problem.

## Build and test

```
cargo test              # core logic tests: parsing, recurrence, time zones, alert rules
cargo build --release   # optimized build in target\release
```

The behavior is specified in [`specs/cal/spec.md`](specs/cal/spec.md). Each requirement there has either an automated test in [`specs/cal/tests/`](specs/cal/tests/) or a manual check procedure.

Code layout:

- `src/` — the platform-neutral core: iCalendar parser, recurrence expansion, alert rules, config
- `src/win/` — the Windows shell: tray icon, sync thread, flash overlay, alert popup, calendar view (plain Win32 + GDI)

## License

[MIT](LICENSE)
