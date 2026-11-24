# Fixes for the Linear review (MUST-FIX 1-2, NICE 3, 4, 6, 8, 9)

Base commit 332bf6b. `cargo build -j 2` clean. `cargo test -j 2`: **88 passed, 0 failed** (was 86; 2 new tests, 1 parser test extended). `data\calm-days.db` not touched; browser runs used a fresh DB in `.scratch\fx`. Nothing pushed.

Checked in headless Edge (`.scratch\lfix.mjs`, real clicks and DOM reads) at 1280 and 375 px, light and dark. No page errors, no sideways scroll. Shots: `reports\shots\lfix-*.png` (banner, week, day, agenda, panel, discard; each x4).

| # | Finding | What changed | How checked |
|---|---|---|---|
| MUST 1 | Short blocks unreadable | `src\calview.rs`, `assets\style.css`. Block height is `max(22px, ...)`, start position unchanged, short blocks (45 min or less) overlay what is below. Short blocks show one line "Title, 5:00 PM". Every block has a title tooltip "Name, time range": week/day blocks, month pills, all-day pills, agenda rows. | Browser: all five sample blocks at least 22 px, tooltips present in week, month and agenda. Test `short_blocks_keep_their_start...`. Limit: in a narrow day column the title is cut with "..." (full text in the tooltip). |
| MUST 2 | Banner after Cancel | `src\ops.rs`, `src\web.rs`. I could not make the banner appear with a plain Cancel click: cancel already marked the proposal cancelled. The banner showed a question that was left behind (dialog closed another way). Now: the banner and /plan list count only suggested plans (Plan my day); a new move/archive/delete question drops older unanswered ones. | Browser: Move all then Cancel, Archive then Cancel, and reload with the dialog open: no banner each time. Test `cancelled_or_forgotten_questions_leave_no_waiting_banner`. Not run in browser: calendar drop on a fixed-event day and Plan my day cancel (same code path, covered by the unit test). |
| NICE 3 | Save/Archive/Delete off screen | Sticky footer in the card panel. | Footer bottom equals viewport bottom at 1280x900 and 375x812. |
| NICE 4 | No unsaved warning | `assets\app.js`. Close with edits shows "Discard changes? Keep editing / Discard". A clean Close closes at once. | Browser: edit title, Close, bar appears; Keep editing keeps the text; Discard closes. |
| NICE 6 | "next friday" | `src\parser.rs`. "Friday" and "this Friday" = coming Friday. "next Friday" = the coming Friday if it is 3 or more days away, else the one after. Tue 24 Nov: 27 Nov. Wed 25 Nov and Fri 27 Nov: 4 Dec. Doc comment updated. Old test said Monday gives 4 Dec; changed to 27 Nov. | Test `weekdays_this_next_and_today_is_that_day` extended; `/api/parse` "report next friday" gives 2026-11-27. |
| NICE 8 | Agenda time wraps at 375 px | `assets\style.css`. Time never wraps, smaller at phone width. | Browser: agenda time boxes are one line (22 px) at 375 px. |
| NICE 9 | Sample lengths | `src\ops.rs`. Call John, Water the plants, Pay electricity bill, Book the dentist are now 30 min. Others were 30-90 min. | Seen in week view; tests pass. |

Not done (parked): all other findings. Not checked: a real phone, other browsers, real mouse drag after these CSS changes (drag code untouched).
