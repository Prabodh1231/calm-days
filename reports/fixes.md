# Fixes for review.md (MUST-FIX 1, 2; NICE 3, 6, 7, 8, 13)

Tests: `cargo build -j 2` clean, `cargo test -j 2`: **67 passed, 0 failed** (was 64; 3 new). Other findings are parked.

Checked in headless Edge over DevTools (`.scratch/fixcheck.mjs`, a copy of the drive.mjs method), at 375 and 1280 px, light and dark, on a scratch database in `.scratch/run/data`. The real `data/calm-days.db` was not touched (still 36864 bytes). Page errors: none.

Shots are in `reports/shots/`, named `fix-<topic>-<width>-<theme>.png`. Each topic below has all four variants.

| # | What changed | How checked | Shots |
|---|---|---|---|
| MUST-FIX 1 | Archive list and "Move all cards to..." now open a dialog on the board: "Please confirm", the change in plain words (`Archive the list "To do" and its 7 cards`), Approve, Cancel, "Nothing has changed yet". New `GET /api/proposals/:id` gives the text. Server approval gate unchanged. Cancel and Esc cancel the proposal. The Planner page now starts with "Back to board" and says plainly that suggestions wait for approval. Singular "1 card". Long lists scroll inside the dialog. | After click, path stays `/`. Lists identical after Cancel. Esc closes with nothing pending. Approve archives, Undo restores. Move-all dialog lists the moves. | `fix-confirm-archive-*`, `fix-confirm-moveall-*`, `fix-plan-*` |
| MUST-FIX 2 | `overflow-wrap:anywhere` on body, dialogs, panel and toast. Buttons and chips may wrap. `min-width:0` on page parts. | A 200-letter title: `scrollWidth` equals `innerWidth` (375 and 1265/1280) on Board, Summary, Calendar week, Calendar day, Planner, with the panel open and with the Move dialog open. | `fix-wrap-summary-*`, `fix-wrap-cal-week-*`, `fix-wrap-cal-day-*`, `fix-wrap-panel-*`, `fix-wrap-dialog-*` |
| NICE 3 | One line: "Type a sentence like 'call John tomorrow at 5 PM' in any + Add a card box. Dates like 12/11 mean 12 November." No "AI" word. | Read from the page. | `fix-intro-*` |
| NICE 6 | After "Card added" the card scrolls into view and gets a 2 s highlight (no animation when reduced motion is set). | New card had the `flash` class and sat inside the list's visible area. | `fix-newcard-flash-*` |
| NICE 7 | "No cards match." with a Clear button when filters hide everything. List counts show the visible count. | Search "zzzz": message shown, counts 0/0/0. Clear restores 7/3/1. Search "dentist": 1/0/1. | `fix-nomatch-*` |
| NICE 8 | Preview chip says when: "Reminder: 10 min before" (timed), "Reminder: 9:00 AM that day" (date only), or the chosen minutes. Text comes from the server (`Parsed::reminder_text`). | Both chips seen in the preview. | `fix-reminder-chip-*` |
| NICE 13 | "1 suggested change is waiting for your approval. Review it" / "2 suggested changes are waiting ... Review them" (link to the Planner). | Seen with 1 and with 2 waiting. | `fix-banner-1-*` |

New tests: `view::pending_banner_is_plural_correct_and_links`, `view::approval_dialog_and_planner_page_are_plain`, `parser::reminder_text_says_when`. The old empty-board test now checks the new intro line.

Files: `src/view.rs`, `src/web.rs`, `src/parser.rs`, `assets/app.js`, `assets/style.css`.

Not checked: the wrap CSS has no unit test (it is checked only by the browser run). Touch use on a real phone, Firefox and Safari. Real mouse drag was not re-tested. A bulk move of many cards makes a long dialog list (it scrolls).
