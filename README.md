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

A Trello-style board. Lists (To do, Doing, Done to start) hold cards. Drag cards between lists and reorder them, or use the Move to... button on a card (list and position, works on phones and with a keyboard). Lists can be added, renamed, moved left or right and archived from the ... menu. Each card has a title, notes, due date and time, priority, colour labels, checklist, reminder and repeat rule. "+ Add a card" reads a plain sentence and shows chips before saving. Moving a card to a Done list completes it; a repeating card then adds its next copy to To do. Also: search and filter, "What should I do now?", a calendar (drag cards onto an hour), a daily planner that only proposes, daily and weekly summaries, reminders, dark mode and a 375 px phone layout. A fresh install opens with a sample board. An old Calm Days database is migrated in place (cards keep their data; finished ones go to Done).

Asks first: moving 3 or more cards at once, archiving a list that has cards, deleting events or series, and every planner proposal. One drag gets a 10-second Undo.

## Sentence reader: small local AI, with simple rules as the backup

Calm Days reads your sentence with a small AI on your computer (Ollama). Falls back to simple rules. Nothing leaves your computer: the app talks only to 127.0.0.1 or localhost.

The AI only reads the sentence and suggests title, date, time, priority, reminder, repeat and labels. You see them as chips (a dashed chip came from the AI), you can change them, and nothing is saved until you press Add. It never moves, deletes or changes cards you already have. Every field it returns is checked by Rust. A bad or missing field uses the rules' value for that field. If Ollama is off, the model is missing or it does not answer in about 4 seconds, the rules answer and the preview shows a small "rules" chip. A small "AI" chip means the model answered.

Install the model once (free, about 1 GB):

1. Install Ollama from https://ollama.com
2. In a terminal run: `ollama pull qwen3.5:0.8b`
3. In Calm Days open "AI reader" in the top bar, press "Check it works".

Settings (top bar, "AI reader"): on/off, model name (default `qwen3.5:0.8b`), Ollama address (default `http://127.0.0.1:11434`, only this computer is allowed). They are stored in the database. To free memory run `ollama stop qwen3.5:0.8b`; Ollama also unloads the model by itself after about 5 minutes.

The small model is weaker than the rules on dates, times and repeats, so the app lets the rules win there and uses the model for labels, a cleaner title, priority words, duration and sentences the rules cannot read. See `reports/ai-build.md` for measured results.

## The rule reader

The rules work offline. They can be wrong, so always check the chips.
Rules:

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
