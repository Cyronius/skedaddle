# Plan: settings screen (replace Notepad config)

Status: settings window, gear, first run, trimmed settings and 20 themes are built. The guided provider flow (below) is on hold: the Settings window already shows the Google steps. Archive this plan when that ships.

## Goal

Nobody should have to edit TOML. On first launch, open a Settings window. After that, a gear icon in the calendar view toolbar opens it. The tray menu item "Open config file" goes away.

## Spec impact

### Modified: CAL-TRAY
Menu becomes: Open calendar, Refresh now, Test alert, **Settings**, Start with Windows, Quit. Left-click when no feed is configured opens Settings instead of an empty calendar.

### Modified: CAL-FEED-CONFIG
Add: `Config` serializes to TOML that parses back to an equal `Config` (round trip). `Config::validate` returns one message per bad field.

New acceptance criteria:
- A `Config` with two feeds and non-default alert values survives serialize → parse unchanged.
- `validate` rejects: `poll_minutes` outside 1..=120, a color that is not `#RRGGBB`, a feed with an empty URL or a URL that is not `https://`, `http://` or `webcal://`, a negative `lead_minutes`/`grace_minutes`, a `sound` that is not `none`, `default` or an existing `.wav` path.
- A default `Config` with no feeds is valid (so Save works before the user adds a feed).

### New: CAL-SETTINGS-FIRST-RUN (manual)
With no `config.toml`, starting the app opens the Settings window and does not start Notepad. Closing it without Save writes nothing, so the next launch opens it again. Save writes the file and the first sync starts at once.

### New: CAL-SETTINGS-GEAR (manual)
The calendar toolbar has a gear button at the far right, after Month/Week/Day. It opens Settings. A second click focuses the open window instead of opening another.

### New: CAL-SETTINGS-EDIT (manual)
Settings shows every config field: general (poll interval, week start, clock), alerts (lead, grace, repeat, flash on/off, flash color, sound, skip free), and a feed list with Add, Edit and Remove. Save validates, shows the first error inline and keeps the window open, or writes the file and applies it without waiting for the next sync. Cancel discards.

## Design

**Controls.** Text fields are standard `EDIT` controls (a hand-drawn text box needs caret, selection, clipboard and IME, and Ctrl+V into the URL field matters). Everything else is owner-drawn with the calendar view's palette: toggle buttons for on/off fields, segmented buttons for pairs (Sunday/Monday, 12h/24h), push buttons, and a feed list. No `COMBOBOX`, no native check boxes: they cannot follow a custom theme. Edit colors follow the palette through `WM_CTLCOLOREDIT`. Because every surface reads the same `Pal` as the view, adding themes later means adding palettes, not reworking Settings. No new crates; `windows` feature needed: `Win32_UI_Controls`.

**Visual styles.** Owner-drawn controls need no comctl32 v6 manifest, so none is added.

**Layout.** One page: General and Alerts side by side, Feeds below. Feed Add/Edit is a second small window with Name, URL, Color (hex box plus a row of preset swatches) and owner email.

**New files.** `src/win/settings.rs` (window, controls, load/save). Pure logic in `src/config.rs`: `Serialize` derives, `validate`, `to_toml`. `settings.rs` stays thin so the testable part lives in the core crate.

**Saving.** Write `config.toml` via temp file + rename. The sync thread already reloads on mtime change; add `Cmd::ReloadConfig` so Save applies immediately instead of at the next poll. The alert timers re-read `shared::config()` on each snapshot, so no restart.

**Gear icon.** New `Hit::Settings` in `view.rs`, drawn as a button right of the Day button in `draw_toolbar`. Draw the gear with GDI polygons/circles rather than a font glyph so it does not depend on Segoe UI Symbol being present and stays crisp at any DPI. The existing "Loading…"/status text sits left of the mode buttons, so move its right edge in by one button.

**First run.** In `app.rs::run`, replace the template-write plus Notepad block: if `config.toml` is missing, post `WM_APP_SETTINGS` to the main window after the message loop starts (the window needs the loop running). `TEMPLATE` is deleted; `Config::default()` is the first-run state. The tooltip text changes from "Right-click > Open config file" to "Click to open settings".

## Decisions (settled)

1. Comments in a hand-edited file are lost on Save. Accepted: there is no file editing surface any more.
2. No raw-file access of any kind: no menu item, no link, no "open folder".
3. Theming is a later task, with several themes wanted. Settings is built on the view's `Pal` so it follows whatever palette is active. Until themes exist it uses the same light/dark choice as the view.

## Tests

- `specs/cal/tests/cal.rs`: round trip and `validate` cases above (`Traces: CAL-FEED-CONFIG`).
- Manual procedures for the three new manual requirements go into `spec.md` when this merges.
- README: replace "Open config file" and the Notepad install step with Settings; keep the TOML reference as an "Advanced: config file" section.

## Order of work

1. `config.rs`: `Serialize`, `validate`, round-trip tests (red first).
2. Manifest + `InitCommonControlsEx`.
3. `settings.rs` with General/Alerts fields, then the feed list and feed modal.
4. `Cmd::ReloadConfig`, atomic write.
5. Gear button in `view.rs`, tray menu change, first-run hook.
6. Spec merge, README, archive this plan.

## Phase 1 as built (differs from the plan above)

- Feed editing is inline in the Settings window (list on the left, fields below it), not a second window.
- Save validates, writes `config.toml` via temp file + rename, and sends the existing `Cmd::Refresh`; no new command was needed.
- A blank calendar entry that the user never filled in is dropped on Save, so first-run Save with no calendars works.
- The tooltip and left-click open Settings when there are no calendars or the config file is damaged.
- `Pal`, `palette` and `system_dark` moved to `src/win/theme.rs` so Settings and the calendar share them.

## Phase 2 (proposed, not started)

### Provider setup
No provider hands out a calendar's iCal address from just an email, so "Add calendar" becomes guided instead:
1. Pick Google, Outlook or Microsoft 365, iCloud, or Other.
2. The window shows that provider's steps and a button that opens the exact settings page in the browser.
3. The user pastes the address. Skedaddle fetches it, confirms it is a calendar, fills the name from `X-WR-CALNAME`, derives the owner email, and picks the color.
OAuth (no pasting at all) is possible for Google and Microsoft but is a separate project: sign-in, token storage, an API client, a second event source, Google app verification, and it breaks the "no account" promise in the README.

### Fewer settings, themes
Settings left on the right: alert lead time, Flash screens, Play sound, Skip events marked free, Theme. Hardcoded: poll interval 5 min, grace 5 min, repeat 60 s for 5 min. Week start and 12/24 hour come from the Windows locale. Flash color and every other color come from the theme. Sound file path goes away.

## Phase 2 as built

- Settings left: week start, clock, alert lead (1/2/5/10), play a sound, skip free, theme. Hardcoded: poll 5 min, grace 5 min, repeat 60 s for 5 min, flash always on.
- Themes live in `src/theme.rs` (core, tested). Each has a Windows-installed font for its picker name, with Segoe UI as the fallback when the font is missing.
- The exe grew from 0.92 to 1.02 MiB.
- Not built: the guided provider flow (Google / Outlook / iCloud steps, fetch to fill the name).
