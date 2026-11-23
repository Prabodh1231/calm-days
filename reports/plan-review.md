# Plan review: Rust calendar + to-do app

Read-only review. Nothing was built or run. Crate and memory claims come from experience, not from tests on this box.

## Wrong or weak premises

1. **MUST-FIX. "AI" is a rule parser.** Say so in the UI and README ("smart text reading, works offline"). Do not use words like "AI understands you". A first-time user will test odd sentences and judge it.
2. **MUST-FIX. Reminders with the tab closed cannot work with only a tab and the Notification API.** See point 14. The plan must pick a real answer.
3. **MUST-FIX. Time zones.** Single user, local, but "tomorrow at 5 PM" needs the user's zone. The server clock and the browser can differ (server on this PC, so usually equal). Decision: store UTC timestamps plus the zone name in settings. Take "now" from the browser (send the offset with each request) or the server's local zone. Pick one and test it.
4. **NICE. "Best time slot" is a scoring job, not AI.** Define it as a plain scoring rule (see 11). Otherwise the builder invents it.
5. **NICE. "No login, local" means bind to 127.0.0.1 only.** Binding to 0.0.0.0 for phone testing exposes an open app on the network. Make LAN access an explicit opt-in flag.

## Stack fit

6. **Axum + rusqlite + server-rendered HTML fits "use Rust" and every feature.** The hard rule is "no React", and this has no JS framework. Drag-drop, dark mode and notifications need only about 150 lines of plain JS. Keep. (NICE: say in the README that the JS file is small on purpose.)
7. **Compare with Leptos/WASM (CSR or SSR):**

| | Axum + HTML | Leptos (WASM) |
|---|---|---|
| Peak build RAM | about 1-2 GB | about 2-4 GB (wasm target plus trunk or cargo-leptos, two builds) |
| Extra tools | none | trunk or cargo-leptos, wasm32 target |
| Drag-drop | small JS | needs web-sys glue |
| Risk on 1 GB free | low | high |

   Verdict: **keep Axum.** Leptos doubles the build and adds tooling the builder may fail to install. Mention this in build.md as a rejected option.
8. **MUST-FIX. Build rules.** Use `cargo build -j 2` and `cargo test -j 2`. Set `[profile.dev] debug = 0` (or `"line-tables-only"`) and `[profile.test] debug = 0` to cut RAM and disk. Do not add `opt-level` on dev. Never run `cargo watch`. Run `cargo check -j 2` while writing. First build will take minutes. Warn the builder not to kill it.

## Crates and weight (Windows, low memory)

9. **MUST-FIX. Keep the list short:** `axum`, `tokio` (features `rt-multi-thread`, `macros`, `net`, `signal` only, not `full`), `rusqlite` (`bundled`), `serde`, `serde_json`, `chrono` (default-features off, with `clock`, `serde`), `tower-http` only if serving static files (otherwise use `include_str!` for CSS/JS and skip it).
   - `rusqlite` bundled compiles C SQLite with `cc`. It needs a C compiler on Windows. **Check first that MSVC build tools (or a gnu toolchain) exist.** If `cc` fails, the whole plan stops. This is the top build risk. Fallback: a JSON file store, or `sqlx` is worse (heavier). Tell the builder to test a hello-world rusqlite build before writing anything else.
   - **Skip:** `askama`/`tera`/`maud` templates (maud is light and fine; plain `format!` with an `escape()` helper also works), `sqlx`, `diesel`, `reqwest`, `regex` (heavy; hand-written tokenizing is enough), `chrono-english`/`dateparser` (weak, wrong rules), `rrule` crate (use a small own enum), `tokio full`, `tracing-subscriber`.
   - **MUST-FIX. HTML escaping.** Every user string in HTML must be escaped. Add a test with `<script>` in a task title.
   - Use `chrono-tz` only if real zone names are needed; it is large. Prefer `chrono::Local`.

## "Major calendar change" (approval gate)

10. **MUST-FIX. Define it in code as one function `needs_approval(&Change) -> bool`, with tests.** A change needs approval when ANY of:
    - it moves or reschedules 3 or more items at once;
    - it deletes any event, or deletes 2 or more tasks, or deletes any recurring series (all occurrences);
    - it edits "all future" of a recurring series;
    - any auto-reschedule or "plan my day" action, even for one item (the planner only proposes);
    - it moves an item more than 7 days, or onto a day with a fixed event.
    Single manual drag by the user of one task to a free slot: no prompt, but offer Undo for 10 seconds. The flow is propose, then show a diff list ("Move A 3 PM to 5 PM"), then Approve or Cancel. Nothing is written until Approve. Test: planner output stays unapplied in the DB until approval. Keep an undo log table.

## Planner and "do now"

11. **NICE. Write the scoring rule down.** Slot score = deadline pressure + priority weight + fit in free time - lateness. Working hours are a setting (default 9:00-18:00). Never place on top of a fixed event. Output must be deterministic (same input, same result) so tests can assert. Tie-break by earlier slot, then id.
12. **NICE. "What should I do now?"** Rank: overdue high priority, then due within 2 hours, then task whose slot is now, then quick wins. If nothing is left, show a calm "You're clear" message, not a blank card.

## Parser edge cases to test (MUST-FIX: at least these)

13. Each needs a unit test with a fixed "now":
    - "tomorrow at 5 PM", "5pm", "5 p.m.", "17:00", "at 5" (ambiguous: choose the next occurrence, document it), "noon", "midnight" (which day?).
    - Past times today: "today at 9 AM" said at 3 PM (roll to tomorrow, or keep it as overdue? decide and test).
    - "next Friday" vs "Friday" vs "this Friday" when today is Friday; "in 2 hours", "in 3 days", "next week", "end of month", "Jan 5", "5 Jan", "5th", "12/11" (day-first or month-first: use a setting, default from locale, and say so).
    - Month ends and leap day: "31st" in a 30-day month, "Feb 29" in a non-leap year.
    - DST: India has none, but test with a fixed offset so the code is zone-safe.
    - Priority words: "urgent", "high priority", "!!", "low"; no priority means medium.
    - Reminder: "remind me 30 minutes before", "remind me at 4"; the default reminder when none is given.
    - Repeat: "every day", "every weekday", "every Monday and Wednesday", "every 2 weeks", "monthly on the 15th", "until Dec 20", "for 5 weeks". Each needs next-occurrence tests, including month-end for "monthly on the 31st".
    - Title cleanup: "call John" must stay when the date is removed. "Call John about the 5 PM meeting" must not take 5 PM as the time. Quoted text stays as is.
    - Empty or junk input, very long input (cap at 500 chars), Unicode and emoji, and mixed case. The parser must never panic. Add a fuzz-style test over random strings.
    - When it is unsure, show the parsed result as editable chips before saving ("Tomorrow, 5:00 PM, Medium"), so a wrong guess is easy to fix. This is also the main first-time-user trust feature.

## Reminders with the tab closed

14. **MUST-FIX. Pick one and state its limit in the UI.**
    - (a) **Service Worker + Web Push** needs VAPID keys, a push service and HTTPS (localhost counts as secure for the SW, but push delivery needs the internet). Works with a closed tab if the browser runs. Heavy: needs `web-push` crate (OpenSSL or ring, large compile). **Not recommended here.**
    - (b) **Recommended: server-side scheduler thread plus native Windows toast** is outside the browser stack and adds crates. Cheaper default: the Axum server checks due reminders every 30 s, stores them as "due", and the page shows them on next open plus a catch-up banner ("2 reminders you missed"). With the tab open, JS polls `/api/due` every 30 s and calls `Notification`.
    - (c) **Optional NICE:** a `notify-rust` toast from the server thread. Small, but Windows-only and adds weight; make it behind a cargo feature.
    - Say plainly: "Reminders pop up while the app is open in a browser tab. Missed ones show when you return." Ask for notification permission only after a click, never on page load.
    - The server itself must be running. Document the start command and consider a `start.bat`.

## Hand-offs and extra items

15. **MUST-FIX. Drag-and-drop is untestable by cargo tests.** Round 1 must say what was not tested in a browser. Round 2 reviewer should use a real browser (Chrome) for drag, dark mode, mobile width (375 px) and notifications. Touch drag does not work with HTML5 drag events on phones. Provide a "Move to..." button with a time picker as the mobile path (MUST-FIX for "mobile friendly").
16. **NICE. Seed/demo data.** Empty start screens confuse first-time users. Add example sentences under the input, one "Load sample day" button, and a one-line purpose sentence at the top.
17. **NICE. Accessibility.** Contrast in dark mode, labels on buttons, keyboard alternative to drag.
18. **NICE. The builder is one worker for a whole app.** That is large for 20-30 minutes of agent time. Cut order if short: summaries and insights first, then weekly view, then recurring "until" rules, then notifications. Never cut: parser, planner, approval gate, tests, escaping.
19. **NICE. Report must list** what was run (`cargo test` output count), what was not (browser), the port, and the start command.
20. **NICE. A second item is needed for fixes only if the reviewer finds MUST-FIX.** The builder should not leave known bugs for it.
