# Build report: Calm Days

## Start
- Command: `start.bat` (runs `cargo run -j 2`), or `cargo run -j 2`.
- URL: http://127.0.0.1:3000 (port 3000, bound to 127.0.0.1 only; confirmed with `netstat`: `127.0.0.1:3000 LISTENING`).
- Data: `data/calm-days.db` (SQLite, git-ignored).

## Storage check
A hello-world `rusqlite` (bundled) build ran first and printed 42 on this machine (MSVC toolchain, 21 s). So the app uses SQLite. The JSON-file fallback was not needed.

## Crates (point 9's list only)
`axum 0.7` (features: tokio, http1, json, query), `tokio` (rt-multi-thread, macros, net), `rusqlite` (bundled), `serde`, `serde_json`, `chrono` (no default features; clock, serde, std). No `tower-http` (CSS and JS are served with `include_str!`), no regex, no template crate. Scheduler is a plain `std::thread`, so tokio's `time` feature is not used.
Profiles: `[profile.dev] debug = 0` and `[profile.test] debug = 0`. All cargo runs used `-j 2`, one at a time, no watch.

## Test result (real output of `cargo test -j 2`)
`test result: ok. 52 passed; 0 failed; 0 ignored` (src/lib.rs). The binary and doc-tests have 0 tests. `cargo build -j 2` passed with no warnings.

Per area: parser 18 (incl. 3000-string random run and Unicode/junk/500-char cap), repeat 6, approval gate 6, planner 6, ops (apply / approve / undo / reminders / sample) 12, view (escaping, first screen, insights) 4.

## How the plan-review MUST-FIX points were met
| Point | Done |
|---|---|
| 1 "AI" wording | UI and README say "Smart text reading, works offline" and "it is not an AI". A test checks the page does not claim an AI understands you. |
| 2, 14 Reminders | Option (b). Server thread marks due reminders every 30 s. Page polls `/api/due` every 30 s and uses `Notification` while open. Missed ones show in a yellow banner. Permission is asked only after a button click. The limit is written in the UI and README. |
| 3 Time zone | `chrono::Local` for "now"; UTC stored. `to_utc` handles skipped and doubled local times. |
| 8 Build rules | Followed (see above). |
| 9 Crates, C compiler | Followed. Hello-world rusqlite build first. |
| 9 Escaping | `view::esc` on every user string; test renders `<script>alert(1)</script> "q" & 's'` on Today, Calendar (week and day), Summary and Plan pages. |
| 10 Approval gate | `needs_approval(&Change)` in `src/approval.rs`, exactly the listed rules, with tests. Every move, delete and edit goes through it (`ops::dispatch`). Planner output is stored as a proposal and stays unapplied until Approve (test checks the DB row is unchanged). Undo log table; 10-second Undo toast. |
| 13 Parser edge cases | Covered by tests with a fixed "now" (see list below). |
| 15 Browser limits and mobile | "Move to..." button with a date-time picker on every task and calendar card. Browser checks listed below. |
| 11, 12, 16, 17, 19 | Scoring rule written at the top of `src/planner.rs`; do-now ranking; sample day + examples + purpose line; focus outlines, labelled buttons, keyboard path via Move to...; this report. |

Rulings applied: bare past time rolls to tomorrow, "today at 9 AM" stays today and shows overdue; day-first numeric dates with a UI hint; "midnight" = start of next day. Bare time without am/pm: 1-6 = PM, 7-11 = AM (documented, with a note chip).

## What was checked
- Unit tests above.
- A real server run (debug binary, started and stopped by me): `GET /`, `/calendar`, `/calendar?view=day`, `/plan`, `/summary`, `/static/app.js`, `/static/style.css` all returned 200. `POST /api/parse` for "Remind me to call John tomorrow at 5 PM" returned title "Call John", time 17:00. `POST /api/sample` worked once and refused a second time. `POST /api/plan` returned a proposal, not a change. A task titled `<script>alert(1)</script>` saved through the API was escaped in the served home page. A request with a wrong `Host` header got 403. A non-JSON POST got 415.
- `node --check assets/app.js`: no syntax errors.

## What was NOT checked
- **No real browser.** The browser tool here only opens public URLs, so the UI was never seen. Untested: layout at 375 px, dark mode look and contrast, drag and drop, the Move to... dialog, the preview chips flow, toasts, banner, `Notification` pop-ups. A round-2 reviewer should open http://127.0.0.1:3000 in Chrome and check these.
- Touch drag does not work (HTML5 drag events); Move to... is the phone path.
- Reminders with the app or browser closed (not possible by design).
- Time zones other than this PC's, and daylight saving (only the logic is zone-safe; the parser uses local wall-clock values).
- No automated HTTP-level tests (no extra test crate allowed); the routes were smoke-tested by hand as above.

## Rejected options
- **Leptos / WASM**: needs a wasm target and trunk or cargo-leptos, roughly double the build memory, and web-sys glue for drag and drop. Too risky on this low-memory machine.
- `rrule`, `regex`, `chrono-english`, `askama`/`tera`, `sqlx`, `tower-http`, `web-push`, `notify-rust`: not needed or too heavy (point 9).
- Reading the browser's time zone: the server's local zone was chosen instead (ruling).

## Skipped (cut order from point 18; nothing in the never-cut list was cut)
- Planner working hours are a constant (9-18), not a setting with a screen.
- No UI for "edit all future" (the endpoint `POST /api/tasks/:id/edit` and the approval rule exist and are tested).
- A repeating task is a single row that jumps to its next date; the calendar shows only the next occurrence, not a whole month of future ones.
- "every 2 months" is read as "every month". No "N times" repeat limit.
- `notify-rust` desktop toast (option c) not added.
- Day-of-week short names such as "sat" or "sun" are only read as dates after on/this/next/every/by/until.

## Files
`src/{parser,repeat,planner,approval,store,ops,view,web,lib,main}.rs`, `assets/app.js`, `assets/style.css`, `start.bat`, `README.md`, `Cargo.toml`. Git: local repo, one commit, nothing pushed.

Risk: the UI has never been seen in a browser, so layout and JS behaviour bugs are possible even though the server side is tested.
