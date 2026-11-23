# Board build report: Calm Days as a Trello-style board

## Start
- `start.bat` (runs `cargo run -j 2`), or `cargo run -j 2`. Open http://127.0.0.1:3000 (127.0.0.1 only).
- Data: `data/calm-days.db` (git-ignored). **Migrated, not reset:** an old database gets the new columns and three lists on first start; open cards go to "To do", finished ones to "Done". A brand-new database opens with a sample board. (For the screenshots I moved the old test database aside to `.scratch/` so the sample board showed; `.scratch/calm-days-before-board.db` is a copy of the pre-board file. The migration is covered by a unit test.)

## Tests (real output)
`cargo build -j 2`: no warnings. `cargo test -j 2`: `test result: ok. 64 passed; 0 failed` (was 52).
New tests: lists (3 default lists, add/rename/reorder with undo), card order (within a list, between lists, past-the-end), one-card drag with undo, move-to-Done completes and reopens, repeat spawn in To do (fresh checklist, no double spawn, undo removes the copy, `until` ends the series), bulk move of 3 needs Approve and writes nothing before it, archive list approval (empty = free, with cards = asks, undo restores, archived cards leave the calendar, last/Done list protected), card panel save (trimming, palette-only labels, length limits), old-database migration, card face chips, escaping on every page including a hostile list name and label. Parser, repeat, planner and approval-gate tests are unchanged and pass.

## Screenshots (`reports/shots/`)
- Board: `board-desktop-light.png`, `board-desktop-dark.png`, `board-mobile-light.png`, `board-mobile-dark.png` (375 px, drawn in a 375 px iframe because headless Edge will not go narrower than about 500 px).
- Flow, desktop: `flow-1-add-preview.png`, `flow-2-card-panel.png`, `flow-3-after-drag.png`, `flow-4-move-to-dialog.png`, `flow-5-filter.png`, `flow-6-archive-needs-approval.png`, `flow-7-what-now.png`, `flow-8-panel-dark.png`.
- Pages: `page-calendar-light.png`, `page-summary-light.png`, `page-plan-light.png`, `page-summary-dark.png`.
- Phone (375 px device emulation): `phone-1-board-add.png`, `phone-2-move-dialog.png`, `phone-3-panel.png`, `phone-4-calendar-dark.png`.
(Other files in that folder are from the earlier review chat.)

## Checked in a real browser (Edge headless, driven over the DevTools protocol, `.scratch/drive.mjs`)
- Add a card from "Remind me to email Sara tomorrow at 5 PM urgent": chips (title, due, High, reminder) appear, Add card saves, toast shows Undo.
- Card panel: edit notes, toggle a colour label, add a checklist item, save; the card face then shows the label, "0/1" and the bell.
- Drag a card onto another list (drop line appears, card lands, "Moved. Undo"); Undo puts it back.
- Move to... dialog to Done: card goes to Done, the repeating "Water the plants" copy appears in To do.
- Search "dentist" shows "2 of 12 cards shown"; add a list; archive a list with cards goes to the approval page with nothing changed; "What should I do now?" dialog; calendar, summary and planner pages; light, dark and 375 px layouts. No JavaScript errors were reported.
- Fixed after looking: calendar events were squeezed in week view; native scrollbar was white in dark mode; late sample cards raised a reminder banner on first start.

## Not checked
- Real mouse and touch drag. The drag was driven with synthetic DragEvents (dragstart, dragover, drop), which exercises my handlers but not the browser's own drag. HTML5 drag does not work by touch, so phones use Move to....
- Notification pop-ups, reminders firing while the tab sits open, Safari/Firefox, screen readers, keyboard-only walkthrough (cards open with Enter and have a Move button, but I did not tab through everything).
- Dragging calendar items onto an hour was not re-tested after the rewrite (code path unchanged except delegation); the older Move to... date dialog was not clicked.
- Long text, very many cards or lists (performance), time zones.

## Skipped or changed on purpose
- Lists are reordered with "Move list left / right" in the ... menu, not by dragging.
- No view of archived cards or lists; the only way back is Undo (the Planner page lists recent changes with Undo, and the toast shows it for 10 seconds).
- Deleting a single card is two clicks in the panel ("Delete card", then "Click again to delete") and has Undo; the gate itself still asks first for events, series and 2 or more.
- Editing a repeating card in the panel saves at once (the old "edit all future" approval is still in the API but not in this UI).
- Card order is inside lists only; calendar shows cards that have a due date and are not done.
- The old Today page is gone (the board replaces it); "Plan my day" and proposals live on the Planner page, linked from the top bar.
- Position numbers in "Move to..." are 1-based labels over 0-based indexes; no keyboard drag.

## Files
`src/{store,ops,approval,view,web,main}.rs`, `assets/{app.js,style.css}`, `README.md`. Parser, repeat and planner are untouched. Local commit only, nothing pushed.

Risk: drag was tested with synthetic events only; a real-mouse drag could still behave differently.
