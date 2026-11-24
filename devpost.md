# Calm Days: Devpost draft

Paste each section into the Devpost form.

**Tagline:** A calm to-do board and calendar. Type one sentence, and a small AI on your own PC fills in the details.

## Inspiration

To-do apps ask you to fill in many boxes: date, time, priority, reminder. Most people just want to type "call John tomorrow at 5 PM" and move on. We also wanted an AI app that does not send your plans to the cloud. A tiny model on your own PC can do that.

## What it does

Calm Days is a board with lists and cards, plus a calendar with Month, Week, Day and Agenda views. You type a plain sentence. A small AI (Qwen 3.5 0.8B, running in Ollama on your computer) reads it and suggests a title, date, time, priority, reminder, repeat and labels. You see them as chips, change anything, and then press Add card.

The AI never changes cards by itself. Big changes (moving 3 or more cards, archiving a list, deleting events, planning your day) need your approval. If the AI is off or slow, simple rules read the sentence, so the app always works. Ctrl+K opens a command menu. Dark mode and a phone layout are included.

## How we built it

- Rust with Axum for the server, SQLite for storage, plain HTML and CSS from the server, and a small JavaScript file for drag and drop.
- The model answers in a fixed JSON schema through Ollama.
- Rust checks every field the model returns. A bad date, repeat or reminder is dropped, and the rule reader's value is used instead.
- 103 automated tests, none of which need the model.
- The app only talks to 127.0.0.1, so nothing leaves the PC.

## Challenges

- A 0.8B model is weak on dates. In our tests the rules were better on dates, times and repeats, so we let the rules win there and use the model for titles, labels, priority and length.
- The first answer after the model is idle is slow. We cap the wait at about 4 seconds and fall back to the rules.
- Making an AI feature feel safe. The answer is a preview with chips, and nothing is saved without a click.
- Small laptops have little memory. We kept the build light and the model small.

## Accomplishments that we're proud of

- A working local AI app that needs no account, no key and no internet.
- The app is never worse with AI on. On 15 test sentences the AI result was never worse than the rules on date, time, repeat or reminder.
- Approval before big changes, and a 10-second Undo after a drag.
- Two independent reviews with no must-fix findings on the AI reader.

## What we learned

- Small models are useful when a checker sits behind them. Trust the model for words, trust code for dates.
- A fallback is part of the feature, not an extra.
- Showing the AI's guess as editable chips builds trust more than a clever answer.

## What's next

- A faster first answer when the model is cold.
- Better touch support, including drag by finger.
- Notifications when the app is closed.
- Better label choices and tests in more browsers.

## Built with

rust, axum, sqlite, ollama, qwen, javascript, html, css

## Links

- Code: https://github.com/Prabodh1231/calm-days
- Demo video: (add your link here)

---

# 2-minute demo video script

Record at 1280x720 in Edge. Before you start: Ollama running, model pulled, `start.bat` running, sample board loaded.

| Time | Shot | Say |
|---|---|---|
| 0:00-0:10 | Board, light mode | "This is Calm Days. A to-do board and calendar that runs on my own PC." |
| 0:10-0:25 | Click + Add a card, type `Call John about the 5 PM meeting next Friday`, press Read it | "I type one sentence. A small AI on my computer reads it." |
| 0:25-0:45 | Zoom on the chips: AI chip, due date, dashed labels | "It finds the date, reminder and labels. Dashed chips came from the AI. I can change any of them." |
| 0:45-0:55 | Press Add card. Card appears | "Nothing is saved until I press Add." |
| 0:55-1:10 | Open AI reader settings, press Check it works. Show the model name | "It is Qwen 3.5, 0.8 billion parameters, in Ollama. No cloud, no key." |
| 1:10-1:25 | Stop Ollama (or turn the AI off), add another card. Show the `rules` chip | "If the AI is off, simple rules answer. The app still works." |
| 1:25-1:40 | Ctrl+K, type `Gym every weekday at 7am`, show preview | "Ctrl+K does the same from anywhere." |
| 1:40-1:55 | Calendar: Week, then Month. Drag a card onto an hour | "A calendar with four views. Drag a card to plan it." |
| 1:55-2:05 | Try Move all cards on Done. The approval dialog shows | "Big changes ask first. The AI never changes my cards by itself." |
| 2:05-2:15 | Dark mode, then phone width | "Dark mode and a phone layout. All private, all local." |

Trim the last row if you pass 2:00.
