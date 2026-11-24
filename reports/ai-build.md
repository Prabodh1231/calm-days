# Local AI sentence reader (Ollama, Qwen 3.5 0.8B)

Short verdict: the 0.8B model is **worse than the rules** on dates, times, repeats and reminders. In the raw run it answered "today" or the clock time for most sentences, invented repeats ("every day") and reminders (1 minute), and gave 3 to 6 labels for everything. So Rust checks every field and, for those fields, the rules win. The model adds labels, priority words, a length, a shorter title, and a title for sentences the rules cannot read. The fallback to rules stays.

## What was built

- **Model tag:** `qwen3.5:0.8b` (exact tag exists; pulled once, 1.3 GB on disk, Q8_0, 752M parameters, `ollama show` lists thinking support). Nothing else was pulled.
- **HTTP choice:** plain `std::net::TcpStream` in `src/ai.rs`. No new crate (Cargo.toml unchanged). Reason: only 127.0.0.1 or localhost is allowed, so no TLS is needed, and one deadline covers connect, write and read. HTTP/1.1, `Connection: close`, plain and chunked answers both read. Limit noted in the code: no redirects or keep-alive.
- **Call:** `POST /api/chat` with `stream:false`, `think:false` (retried once without it if the model rejects the flag), `format` = JSON schema (title, date, time, all_day, priority, reminder_min, repeat, labels as an enum of the 6 label names, duration_min), `temperature 0`, `num_ctx 2048`, `num_predict 300`, `keep_alive 5m`, a short system prompt with 5 examples. The user message carries today's date and weekday and the label names. The clock time is left out on purpose: the model copied it into cards.
- **Time limit:** 4 seconds in total, then the rules answer. Ollama off, model missing, junk answer: rules answer too. The preview shows a small `rules` chip (hover says why) or an `AI` chip (hover shows the seconds). Dashed chips came from the model.
- **Checks (`ai::merge`):** title not empty, at most 200 letters, and not longer than the rules' title; date valid, not in the past, within 10 years, with a date word in the sentence; time `HH:MM` with a time cue and not the clock's own time; priority only when the sentence has a priority word; reminder 0 to 10080 only when the sentence has a reminder word; repeat only when the sentence has a repeat word and `parse_repeat_phrase` can read it; duration 5 to 1440; labels only from the 6 known names, at most 3 (more than 3 means the model is guessing, so none). The model fills date, time, repeat and reminder only when the rules found nothing. Any bad field keeps the rules' value for that field.
- **Safety:** the model only reads one sentence. It cannot move, delete or change cards. Nothing saves without the user pressing Add (board adder) or Enter on "Add to the Inbox" (Ctrl+K and C). Approval gate untouched. Text is set with `textContent` in the page; labels sent back are filtered against the 6 known keys on the server.
- **Where it runs:** "+ Add a card" (title is an editable box in the preview), Ctrl+K / C capture (now shows a preview first), Inbox capture (same path). `/api/parse` runs the model on a blocking thread and holds no database lock.
- **Settings:** top bar "AI reader" dialog: on/off, model name, Ollama address (only `http://127.0.0.1:PORT` or `http://localhost:PORT`; anything else is refused by the server), "Check it works" button. Stored in the `meta` table (`ai_on`, `ai_model`, `ai_url`). Loading the model starts when you open an adder (warm call).
- **Wording:** "Reads your sentence with a small AI on your computer (Ollama). Falls back to simple rules." in the adder and the dialog. README section rewritten with install steps (`ollama pull qwen3.5:0.8b`).
- Skipped: the optional one-line reason on "What should I do now?" (a second model call per click is not worth the delay on this PC).

## Tests

`cargo test -j 2`: **98 passed, 0 failed** (88 old + 10 new in `src/ai.rs`). No test needs Ollama. The model sits behind the `Reader` trait with a fake. Covered: valid JSON, bad date (field-only fallback), unknown label, junk JSON (5 shapes), timeout / down / model error to rules with a reason, field-by-field checks, repeat checked by `repeat.rs`, HTML text kept as text, local-only address check, and the real TCP client against a tiny local server (plain, chunked, timeout, nothing listening). `cargo build -j 2` is clean.

## Real runs (Ollama up, model warm)

Today in the test was Tue 24 Nov 2026. "Rules" = rule parser alone. "Final" = what the app shows with the AI on (after the Rust checks). Latency = wall time of `/api/parse` with the model warm (first call after load took about 3.5 s).

| # | Sentence | Rules | Final card (AI on) | Model used for | ms |
|---|---|---|---|---|---|
| 1 | Remind me to call John tomorrow at 5 PM | Call John, 25 Nov 17:00 | same + label People | labels | 1065 |
| 2 | Pay rent every month on the 1st, high priority | Pay rent, 1 Dec, high, monthly on 1st | same + labels Money, Home | priority, labels | 819 |
| 3 | Gym every weekday at 7am | Gym, 25 Nov 07:00, every weekday | same + 60 min + label Health | duration, labels | 876 |
| 4 | Call John about the 5 PM meeting next Friday | Call John about the 5 PM meeting, Fri 27 Nov, no time | + time 17:00, labels People, Work | time, labels | 1017 |
| 5 | submit report by Friday urgent | Submit report, 27 Nov, high | same + label Work | priority, labels | 881 |
| 6 | ping Sara after lunch on payday | no date | no date, rules' title | nothing (answer had nothing usable, `rules` chip) | 842 |
| 7 | dentist 12/11 at 10 | Dentist, 12 Nov 2027 10:00 | same + 60 min + labels Health, Money | labels | 862 |
| 8 | asdf qwerty | "Asdf qwerty", no date | "asdf qwerty" (model's casing) | title | 934 |
| 9 | Buy groceries this weekend and book a doctor visit | whole sentence as title, no date | same title shortened ("Buy groceries and book a doctor visit"), no date | title | 858 |
| 10 | Team standup every Monday and Thursday at 9:30 for 15 minutes | 26 Nov 09:30, every week on mon and thu | same + 15 min + labels Work, Health | duration, labels | 881 |
| 11 | Water the plants in 3 days, low priority | 27 Nov, low | same | priority | 914 |
| 12 | Pay electricity bill by the 15th and remind me 1 day before | 15 Dec, reminder 1440 | same + labels Money, Home | labels | 801 |

Rows 9 to 12 and "asdf" are my own extra cases (9 to 12 are mine; 8 was given). Latency is 0.8 to 1.1 s warm and about 3.5 s cold, so the 4 s limit is close for a cold first sentence (the warm call at page open is there to avoid that).

### What the raw model said (first run, before the checks were tightened)

The model's own date was wrong or missing on 10 of 12 sentences: it answered today (24 Nov) for "next Friday", "Friday urgent", "dentist 12/11" and others, and copied the clock time (16:51) into a time field on 6 sentences. It returned `repeat: "every day"` and `reminder_min: 1` for the electricity bill, `every weekday` for a Monday and Thursday standup, and 3 to 6 labels for nearly every sentence. Correct on its own: priority words (high, low), durations (gym 60), and a few titles. Using its raw answers, it was worse than the rules on 9 of 12 sentences and equal on 3. That is why the checks above only let it fill a date, time, repeat or reminder when the rules found nothing and the sentence contains a matching word.

Final result: with the checks, the card is never worse than the rules on these 12. The model's real gain is labels and a few small fields; it did not read "payday", "after lunch" or "this weekend" any better than the rules.

## Memory

- `ollama ps` with the model loaded: **566 MB** (100% GPU, context 2048). With the default 4096 context it showed 562 to 573 MB. The model files are 1.3 GB on disk.
- Ollama processes: `ollama` about 14 to 54 MB, `ollama app` about 19 to 29 MB working set while idle.
- The model was stopped (`ollama stop qwen3.5:0.8b`; `ollama ps` is empty), the app server and Edge were stopped, and the real `data/calm-days.db` was never opened (40960 bytes, untouched). Browser and real runs used `.scratch/run`.

## Shots (`reports/shots/`, 16 files, `ai-<what>-<width>-<theme>.png`)

- `ai-addcard-ai-*`: preview with the AI chip (dashed chips from the model)
- `ai-addcard-rules-*`: preview with the rules chip (the app pointed at a port where nothing listens, which is what Ollama stopped looks like)
- `ai-cmdk-*`: Ctrl+K capture preview with chips and "Add to the Inbox"
- `ai-settings-*`: settings dialog after "Check it works" (Ready)
- Each in 1280 and 375 px, light and dark. No sideways scroll at 375 (scrollWidth 375). The one page error in the run is my script clicking Cancel on a closed form, not an app error.
- A cold-start bug was found in the browser run and fixed: the warm call loaded the model with a 4096 context, the chat used 2048, so Ollama reloaded it and the first sentence timed out. The warm call now sends `num_ctx 2048` (verified: AI chip with 1.2 s).

## Not checked

- "Rules chip" shots used a wrong port, not a truly stopped Ollama (the real "Ollama not running" error text was seen the same way: connection refused).
- The comparison table is from the final code; the raw-model paragraph is from the run before the checks, and the model answers were not saved to a file after that (the script keeps only the final cards).
- A second run of the whole table after the last warm-call fix was not done (low memory, asked to wrap up).
- Inbox "C" key path and the Back button in the Ctrl+K preview were not driven in the browser; Ctrl+K "Add to the Inbox" was only previewed, not saved, in the final run (earlier script runs left test cards "Pay water bill ..." in the scratch DB only).
- Model "missing" (not pulled) path: covered by a fake in tests, not tried against real Ollama.
- A settings value saved with a bad address is refused by the server (tested by code path only in unit checks of `local_host_port`, not through the dialog).
- Firefox, Safari, touch devices, and models other than qwen3.5:0.8b.
- Risk: the 4 second limit can be too short on a cold or busy PC; then the rules answer (the user sees the rules chip, no error).
