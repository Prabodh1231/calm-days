# Calm Days

A calm calendar and to-do app. Type a plain sentence like "Remind me to call John tomorrow at 5 PM". The app finds the date, time, priority, reminder and repeat rule, and shows them as editable chips before you save.

Written in Rust. Pages are made by the server. There is no JavaScript framework; `assets/app.js` is a small plain file on purpose.

## Start

1. Install Rust (https://rustup.rs) and, on Windows, the Visual Studio C++ build tools.
2. Double-click `start.bat`, or run `cargo run -j 2` in this folder.
3. Open http://127.0.0.1:3000

The first build takes a few minutes and uses a lot of memory. Do not close it. Data is kept in `data/calm-days.db` (SQLite).

The app listens on 127.0.0.1 only. Other computers on your network cannot reach it. There is no login.

## What it does

Add by sentence with a preview, task list, calendar (day and week), dashboard, daily planner, "What should I do now?", missed-task rescheduling, repeating tasks, reminders, daily and weekly summaries, dark mode, and a layout that works at 375 px wide. The first screen has a one-line purpose, example sentences and a "Load sample day" button.

## "Smart text reading" is rules, not AI

The reader works offline with hand-written rules. It can be wrong, so always check the chips. Rules:

- Numeric dates are day-first: `12/11` is 12 November.
- A time with no am/pm: 1 to 6 means PM, 7 to 11 means AM ("at 5" is 5 PM, "at 9" is 9 AM).
- A bare time that has already passed today moves to tomorrow. "today at 9 AM" stays today and shows as overdue.
- "midnight" is the start of the next day. "noon" is 12:00.
- "Friday" and "this Friday" are the coming Friday (today if it is Friday). "next Friday" is Friday of next week. "next week" is next Monday.
- "31st" in a short month means the last day of that month. "Feb 29" waits for the next leap year.
- Priority: `urgent`, `asap`, `!!`, `high priority` mean high. `low`, `low priority` mean low. Otherwise medium.
- Reminder: `remind me 30 minutes before`, `remind me at 4`. With no reminder given, a timed task reminds 10 minutes before. A date-only task reminds at 9:00 that day.
- Repeat: `every day`, `every weekday`, `every Monday and Wednesday`, `every 2 weeks`, `monthly on the 15th`, `until Dec 20`, `for 5 weeks`.
- A time after "the" ("about the 5 PM meeting") is not read as a time. Text in "quotes" is left alone.
- Input is cut at 500 characters.

## Times

The app uses this computer's local time zone. Times are stored as UTC and shown in local time.

## Reminders: the limit

Reminders pop up while the app is open in a browser tab. The server checks every 30 seconds. Reminders that came due while the page was closed show in a yellow bar when you return. If the server or browser is closed, nothing can pop up. The browser asks for pop-up permission only after you press the button.

## Planner and approvals

The planner only suggests. Nothing is written until you press Approve, and every change can be undone from the Plan page. These need approval (`needs_approval` in `src/approval.rs`):

- moving 3 or more items at once, moving an item more than 7 days, or moving onto a day with a fixed event;
- deleting any event, 2 or more tasks, or a repeating series;
- editing "all future" of a repeating series;
- any "Plan my day" or auto-reschedule, even for one item.

Moving one task by hand to a free slot needs no approval and shows an Undo for 10 seconds.

## Phones and keyboards

Drag and drop uses mouse events and does not work by touch. Use **Move to...** on any task for a date and time picker.

## Known limits

- A repeating task is one item that jumps to its next date when you mark it done. The calendar shows only the next one.
- Working hours for the planner are fixed at 9:00 to 18:00.
- Not tested in a real browser by the builder (see `reports/build.md`).

## Test

`cargo test -j 2`
