# Linear-style features and modern calendar for Calm Days

Build: `cargo build -j 2` clean, no warnings. Tests: `cargo test -j 2` gives **86 passed, 0 failed** (was 67; 19 new). All old tests still pass; two old test helpers in `src/approval.rs` and one in `src/view.rs` got one new struct field each (`allday`, `cycle`).

Real `data/calm-days.db` was not touched (still 36864 bytes). Backup: `.scratch/backup/calm-days.before-linear.db`. Browser runs used a fresh database in `.scratch/run`. A separate copy of the real DB was opened in `.scratch/run2`: it migrated in place, an Inbox list appeared at the left, all 10 cards kept. The migration adds columns `cycle`, table `activity`, and an Inbox list once (meta `inbox_id`).

## Features

| # | Feature | Status | Notes |
|---|---|---|---|
| 1 | Ctrl+K / Cmd+K command menu | done | Fuzzy search of cards (by title or CD-id) and 9 actions: new card, go to Board/Calendar/Summary, dark mode, move selected card, plan my day, what now, shortcuts. Arrow keys, Enter, Esc. Typed text can be added to the Inbox. Driven by keys only in the browser. |
| 2 | Shortcuts C, /, G then B/C/S, E, ? | done | Ignored in inputs, with modifiers, or with a dialog open. Checked in browser (C, /, G+C, G+S, G+B, ?). E opens the focused, open or hovered card: **not run in browser**. |
| 3 | Short ids CD-12 | done | On card face, in the panel header, searchable (board search and Ctrl+K). Id = card row id. |
| 4 | Activity log | done | Created, moved list, reopened, completed, due changed/removed, label added, cycle set, length changed. "2 min ago" style. Undo does not remove log lines. |
| 5 | Inbox list | done | Added once at the left. Ctrl+K / C cards land there. The "+ Add a card" boxes, repeat copies and sample board still use To do. |
| 6 | Panel with properties sidebar | done | Right column: list, position, priority, due, length, labels, repeat, until, reminder, cycle, fixed event. Left: title, notes, checklist, activity. |
| 7 | Auto-label suggestions | done | Dashed chip "Suggested label: Money. Add it?" in the panel. Never applied until clicked (browser: chip seen, click added the label, log line written). Labels now have names: green Money, red Health, blue People, purple Work, orange Home, yellow Ideas. |
| 8 | Weekly cycles | done | Select None / This week / Next week (stored as the week's Monday, rolls over by itself). Cycle bar on the board ("0 of 1 done", next week count) and a chip on the card. |
| 9 | Weekly Pulse | done | On Summary: done this week, slipped (overdue), at risk (due in 2 days and still in Inbox or the first open list), cycle line. |

## Modern calendar

| Item | Status |
|---|---|
| Month, Week, Day, Agenda with segmented switch, Today, prev/next, title "November 2026" | done |
| Hour grid (24 h, scrolls to morning/now), red now line, all-day row, rounded blocks coloured by first label, title and time inside | done |
| Overlapping blocks side by side | done (checked: two overlapping cards sat at different x) |
| Drag a block to a new time or day (real mouse) | done. Free move applies with Undo; day with a fixed event asks first (dialog seen) |
| Drag bottom edge to resize (15 min steps, default 30 min) | done. Real mouse: 60 to 120 min, Undo back to 60 |
| Month: 3 pills + "+N more", click a day opens Day view, drag a card to another day (keeps its time, or stays all-day) | done |
| Unscheduled cards in left sidebar, drag onto grid or month day | done (month drop checked with real mouse; grid drop not run) |
| Mini month picker in sidebar, hidden on phones | done |
| Phone 375 px: opens Agenda | done |
| Calm look, dark mode | done |
| Approval gate | kept: 3+ moves, fixed-event day and long moves still ask. Bulk moves are reachable through Plan my day and Move all; there is no multi-select drag. |

## Tests added (19)

`extra.rs`: short ids, label suggestions, relative time, cycles, Pulse numbers, overlap layout. `ops.rs`: activity log entries, duration and resize with Undo, cycle save and undo, Inbox once and quick capture, all-day drop plus approval still asks. `calview.rs`: titles, segmented switch, 3 pills + "+2 more", block position and width, escaping on all four views, agenda. `view.rs`: card id and search text, Pulse on Summary.

## Browser check

Headless Edge over DevTools (`.scratch/linear.mjs`), real mouse and key events, at 1280 and 375 px, light and dark. No page errors. No sideways scroll (scrollWidth equals window width, apart from the 15 px vertical scrollbar).

Shots in `reports/shots/linear-*.png` (52 files), named `linear-<topic>-<width>-<theme>.png`:
- Board: `linear-board-*`, Ctrl+K: `linear-cmdk-*`, shortcut sheet: `linear-help-*`, panel: `linear-panel-*`, Summary with Pulse: `linear-summary-*`
- Calendar: `linear-cal-{default,month,week,day,agenda}-*`, `linear-cal-overlap-*`, `linear-cal-dragging-*`, `linear-cal-after-drag-*` (1280 only), `linear-cal-approval-*`, `linear-cal-resizing-*`, `linear-cal-month-drop-*`

## Not checked / limits

- Touch drag: drag and resize use mouse events only (phones use the Move to dialog). Not tested on a real phone, Firefox or Safari (Edge only).
- Key E, Cmd+K on Mac, the "Move selected card" action and the grid drop from the sidebar were not run in the browser.
- The last CSS tweak (properties sidebar inputs fit their column) was made after the final screenshots.
- Activity lines stay after Undo. "At risk" uses list position (Inbox and first open list) as "not started".
- Sample cards keep their old colours, so label names such as "Money" do not match every sample card.
- Month and week show open cards only. Cards with a repeat show their next date only. No multi-day blocks. Daylight-saving days not tested.
- A drop onto the all-day row is wired in the code but was not run in the browser.
