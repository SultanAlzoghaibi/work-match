# work-match

## The problem today

Working alone is isolating. Somewhere, someone in your field is working right now too, but finding them takes effort: posting in a Discord, asking in a class chat, going to meetups. Most people don't, so they grind alone. Even when they do find someone, it's by luck, and it rarely happens while they're actually working.

## What work-match fixes

You do nothing extra. As you work, a background app notices what you're on (apps, sites and calendar) and finds someone in your field who's working right now, live. You get one low-pressure intro:

> "PERSON A is also doing LeetCode right now. Want to work alongside her?"

If it clicks, add her as a connection and join a call with her (or a Discord call, to save time and not reinvent the wheel). From then on, the app tells you whenever she's working on that topic, so you can join her.

## Repo layout

- `backend/` – matching server
- `tauri/` – macOS menu-bar (tray) desktop app

## Platforms

- **macOS (main target):** menu-bar only app (no dock icon). Clicking the tray globe opens a native **Liquid Glass** panel on macOS 26+, with a vibrancy blur fallback on older macOS.
- **Windows (supported, not the focus):** same tray app. The window uses a CSS semi-transparent background instead of native glass.

## Run the desktop app

```bash
cd tauri
yarn
yarn tauri dev
```

Left-click the smiling globe in the menu bar to open the panel, click away to hide it, and right-click → Quit to exit.

See [docs/DESIGN.md](docs/DESIGN.md) for the design doc.
