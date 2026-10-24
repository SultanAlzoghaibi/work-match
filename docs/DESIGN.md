# work-match – Design Doc

## 1. Problem

Working alone is isolating. Someone in your field is probably working right now too, but finding them takes effort (Discord posts, class chats, meetups), so most people grind alone. When people do find someone, it's by luck and rarely while they are actually working.

## 2. Idea

A background app that notices what you're working on (apps, sites, calendar) and matches you live with someone in your field doing the same thing. You get one low-pressure intro:

> "PERSON A is also doing LeetCode right now. Want to work alongside her?"

Accept → add as a connection and jump into a call (a Discord call link for the hackathon, so we don't rebuild calling). After that you get a ping whenever that connection is working on the same topic.

## 3. Goals / non-goals (hackathon scope)

**Goals**
- Zero-effort: runs in the menu bar, no setup beyond sign-in.
- Live matching on a shared "topic" (e.g. LeetCode, Figma, a course).
- One intro at a time, easy to ignore.
- Connections + "your connection is working on X now" notifications.

**Non-goals**
- Our own video/voice calling (use Discord links).
- Full productivity tracking or screen recording.
- Linux support.

## 4. Platforms

| Platform | Status | Window style |
|---|---|---|
| macOS 26+ | Main target | Native Liquid Glass NSPanel |
| macOS 11–25 | Works | NSPanel + HUD vibrancy blur |
| Windows | Builds/runs, not the focus | Plain transparent window + CSS semi-transparent card |
| Linux | Out of scope | – |

### Why a desktop app and not just a Chrome extension?

For the hackathon, the signals are **calendar + which app you're in**. A Chrome extension can't see that well:

| Signal | Desktop (tray) app | Chrome extension |
|---|---|---|
| Frontmost native app (VS Code, Xcode, Figma, Word) | ✅ `NSWorkspace` | ❌ only sees Chrome tabs |
| Running when Chrome is closed | ✅ | ❌ |
| Calendar | ✅ EventKit (all accounts) | ⚠️ Google Calendar API only |
| Website / tab | ⚠️ AppleScript, needs Automation permission | ✅ easy |
| Always-visible entry point | ✅ menu-bar globe | ⚠️ toolbar icon |

Decision: the desktop app is the product. A Chrome extension can come later as an optional "sensor" that sends tab topics to the desktop app or the backend. Most people grinding LeetCode, coding or designing spend a lot of time outside the browser, and an extension-only version would miss them.

## 5. Architecture

```
┌────────────────────── tauri/ (desktop) ──────────────────────┐
│ Tray icon (smiling globe) ─click─▶ Glass panel (Vite + JS)    │
│ Activity detector (future): frontmost app, browser URL,       │
│   calendar → "topic" string                                   │
│ WS client (future) ─────────────────────────────┐             │
└──────────────────────────────────────────────────┼────────────┘
                                                   ▼
┌────────────────────── backend/ (future) ─────────────────────┐
│ Presence: who is online + current topic                       │
│ Matcher: pair users on same topic/field, rate-limit intros    │
│ Connections: accepted pairs, "connection is live" fan-out     │
└───────────────────────────────────────────────────────────────┘
```

Flow: detect topic → send presence → backend finds match → desktop shows intro in the panel / notification → accept → connection saved + Discord call link opened.

## 6. Current milestone: tray + glass hello world ✅

`tauri/src-tauri/src/`
- `lib.rs` – builder, tray icon + menu (Open / Quit), Accessory activation policy (no dock icon), liquid glass styling, `os_name` command.
- `window.rs` – converts the main window to a spotlight-style `NSPanel` (floating, all spaces, nonactivating, 26px corners), centers on the cursor's monitor, hides on click-away (resign key).

Behaviour:
- App starts hidden, lives in the menu bar.
- Left-click tray → panel shows "Hello World" on Liquid Glass.
- Click away → hides. Right-click → menu → Quit.
- Windows: same, but hides on window blur and uses a CSS translucent card.

### Local SQLite

- The file is `work_match.db` in `~/Library/Application Support/com.sultanalzoghaibi.workmatch/`. It is opened by `tauri-plugin-sql`, which is preloaded at startup.
- `src-tauri/src/migrations/001_init_local.sql` holds the local-only tables:
  - `curr_user_profile`: a single row for the signed-in user and token
  - `activity_log`: app and calendar stretches, plus a derived `topic`
  - `settings`
- Raw activity stays on the device. Only the topic goes to the backend, whose schema is in `backend/schema.sql`.
- Demo data lives in `src-tauri/src/seed/defaults.sql` and is applied by `db::seeder::seed_defaults`. It is **not run by default**. To use it, uncomment the call in `lib.rs` setup.
- The frontend opens the same DB via `src/db.js` (`getDb()`).

### Borrowed from TyA_Tauri
Copied and trimmed from the TypeAnywhere app: `tauri-nspanel` panel conversion, `monitor` cursor-monitor centering, `window-vibrancy` 0.7 `apply_liquid_glass`, `sw_vers` macOS-26 check, Accessory policy and activate-before-show pattern.
Left out: global shortcut, SQLite, updater, autostart, websocket, telemetry, reminders.

## 7. Next steps

1. **Activity detection (macOS):** frontmost app via `NSWorkspace`, browser tab URL via AppleScript (needs Automation permission), optional calendar via EventKit → map to a topic.
2. **Backend (`backend/`):** auth, presence over WebSocket, simple matcher (same topic, both opted in, not recently intro'd).
3. **Intro UI:** match card in the panel + native notification; Accept / Not now.
4. **Connections:** store accepted pairs; notify when a connection goes live on a shared topic.
5. **Call:** open a Discord call/invite link on accept.
6. **Privacy:** only send the derived topic, never raw URLs/window titles; pause toggle in the tray menu.
