# work-match – Product Spec

How work-match uses AI to match people doing similar work. This builds on
[DESIGN.md](DESIGN.md), which covers the tray app and glass panel already built.

Items marked **(open)** still need a team decision.

## 1. Summary

work-match is a macOS menu-bar app that notices what you are working on and
introduces you to someone doing the most similar work right now. It reads your
screen on-device, summarizes your activity every 5 minutes, and sends only that
summary to the server. There, an AI decides who is a good match and another AI
writes the intro, shown as a Liquid Glass popup.

## 2. Hackathon fit

Track: Meta, "Bringing people closer together with AI".

| Criterion | How work-match meets it |
|---|---|
| Human connection | Live intro → call together → connection → "your connection is working on this now" |
| AI is essential | Matching is by meaning across apps (VS Code `dijkstra.py` ↔ YouTube shortest-path lecture). Without the models there is no match |
| AI well-integrated | No chatbot; the user does nothing extra. AI shows up as one good popup |
| Originality | Matches on what you are doing right now, not a profile or a feed |
| Working demo | Two Macs, cross-app match, both popups appear live |

## 3. Pipeline

```
every 1 min (on the Mac)
  frontmost app + window title + browser URL
  + screenshot of the active window → Vision OCR → text      (if enabled)
  → kept in memory only

every 5 min (on the Mac)
  last 5 snapshots → local model → summary (JSON)
  → snapshots and screenshots deleted
  → summary sent to backend

backend
  summaries of everyone online → Jev: P(good match), P(ok to interrupt)
  → above threshold → Muse Spark writes the intro for each person
  → glass popup on both Macs
```

### 3.1 Capture (every 1 min)

- Frontmost app and window title (`NSWorkspace`), browser tab URL (AppleScript).
- Screenshot of the **active window only**, read with Apple's **Vision
  framework** OCR (`VNRecognizeTextRequest`), on-device. The image stays in
  memory and is never written to disk.
- Use OCR text positions to keep the main content and drop menus and sidebars.
- Skip capture when the front app or site is on the blocklist (§6).
- Vision OCR is called from Tauri through a small Swift/Objective-C bridge.

### 3.2 Summarize (every 5 min)

- A local model turns the last ~5 snapshots into a summary. It focuses on the
  main task and ignores short distractions (a Slack check, Spotify).
- The first summary runs right after launch (after 1–2 snapshots), not after 5 min.
- **Demo mode:** summarize every ~30 s, plus a hidden "summarize now" menu item.
- The summary is **more than one sentence**: 2–4 sentences plus structured fields.

```json
{
  "summary": "Implementing Dijkstra's algorithm in Python for CS170 Homework 4. Started with a priority-queue version, now debugging wrong distances on a test case. Earlier watched a lecture on shortest paths.",
  "topic": "graph algorithms",
  "subtopics": ["dijkstra", "priority queues", "python"],
  "context": "university homework (CS170)",
  "activity_type": "coding / debugging",
  "focus": "high",
  "stuck": true
}
```

- Fallback if the local model fails: build a basic summary from window titles alone.

### 3.3 Decide: Jev

[Jev](https://openrouter.ai/docs/guides/community/jev) (TypeSafe, via
OpenRouter) is a decision model: it answers typed questions with
probabilities, not text.

- `noul` "these two would benefit from working alongside each other right
  now" → P(good match), stored as `matches.score`.
- `noul` "this person is in deep focus and an interruption would be
  unwelcome" → skip people Jev thinks shouldn't be interrupted.
- At demo scale a `choice` over all online candidates can replace embeddings.
- Jev has no built-in learning. To improve over time, include a few past
  matches and their outcomes (accepted? became a connection?) in `state`. The
  schema already stores these outcomes.
- Only summaries, school and bio are sent, never raw screen data.

### 3.4 Write: Muse Spark

Muse Spark (Meta Model API, `https://api.meta.ai/v1`, OpenAI-SDK compatible)
writes the shared topic and a personal intro for each side:

> "Alex is also working on shortest-path algorithms for an algorithms class. Want to work alongside them?"

Jev decides **whether** to match; Muse decides **what to say**. Use standard
pricing (hackathon credits), not the contributor tier, which may use data to
improve Meta's products.

### 3.5 After a match

Match popup expires in 2 min → both accept → Discord call → each answers "keep in
touch?" → both yes = a connection on that topic → the friend's icon lights up whenever
the other pushes a task on that topic. A declined pair isn't matched again for 1 day.
See [DATABASE.md](DATABASE.md) and `backend/schema.sql`.

## 4. Models

| Job | Runs | Model | Fallback |
|---|---|---|---|
| Read screen | Mac | Apple Vision OCR | window titles only |
| Summarize | Mac | Muse Glimmer **(open: check size and Ollama support)** or Llama 3.2 3B via Ollama (~2 GB, free) | Apple Foundation Models (macOS 26) |
| Match score | Cloud | Jev (`typesafe/jev-1.14`) | Muse Spark scoring |
| Intro text | Cloud | Muse Spark | template text |

Show "Built with Llama" if using Llama (license requirement). Download the
local model on both demo laptops before the event.

## 5. UI

- Existing Tauri tray app with a Liquid Glass panel (macOS 26), vibrancy fallback on 11–25.
- **Glass popup** for each match: intro text plus Accept / Not now.
- **"What work-match sees" panel:** the exact summary currently being shared.
- Pause button in the tray menu.

## 6. Privacy

Screenshots are the biggest risk: passwords, messages, other people's data.

1. **Blocklist before capture:** Messages, Mail, password managers, banking, WhatsApp, Gmail, incognito.
2. **Capture the minimum:** active window only, once a minute.
3. **On-device:** OCR and summarizing run on the Mac. Screenshots never leave it.
4. **Delete immediately:** images are never written to disk; snapshots are dropped after each summary.
5. **Share the minimum:** the server gets the summary and topic. A match sees your name, school, bio and current summary in the popup; connections see only the topic.
6. **User control:** screen capture is opt-in (off by default), there's a pause button, and the panel shows exactly what is shared.

Jev and Muse run in the cloud but only ever receive summaries. Check Jev's
data policy on its OpenRouter model page before launch.

## 7. Open questions

- **Screen capture vs. DESIGN.md:** DESIGN.md lists screen recording as a
  non-goal and says to send only a derived topic. This spec adds opt-in
  screenshots with on-device OCR and sends a full summary. The team needs to agree.
- Chrome extension for richer page content: build it, or rely on AppleScript URLs plus OCR?
- Muse Glimmer vs. Llama 3.2 3B for on-device summaries.
- Jev match threshold (start at 0.7) and how much detail goes into the summary.
- Schema still missing: intro text on `matches`, `users.discord_handle`.
