# Demo video script: Privacy

**Length:** ~20 seconds, right after the demo and before the AI section.
**Who:** Person 3 (the narrator), on camera.
**Shot:** Person 3 at the hackathon table, facing the camera, with their laptop open beside them showing the "Your summary" card.

## Lines

> **Person 3:**
> "So you're probably wondering: *it sees what I'm working on?*
>
> Here's the thing. **Everything stays on your laptop.** The AI that reads your tabs and apps runs **right on your computer**, not in the cloud.
>
> It turns all of that into **one short summary**, like this *(points at the summary card)*. That summary is the **only thing that ever leaves your laptop**, and it's what someone you match with sees.
>
> Your actual pages and apps never leave your computer, and they're deleted within minutes. Email, messages, banking and health sites are never read at all.
>
> And **you decide what it can see.** Turn any website on or off *(shows the blocked-sites list, toggles one off)*, and it's never read.
>
> Want a break? **One switch turns it all off.**"

> ⚠️ **The "turn any website on or off" line needs that feature built before filming.** Right now the blocked sites are a fixed list in the code, with no setting in the app. If it isn't built in time, use this line instead:
> "Sensitive sites like email, banking and health are **blocked automatically**, before the AI ever sees them."

## On-screen graphic (optional)

```
ON YOUR LAPTOP 🔒                 SHARED
tabs, apps, page text   →  AI  →  "Building a website with React"
(deleted in minutes)
```

## Why each line is true (if judges ask)

- **"Everything stays on your laptop":** page text from Chrome is kept in a local file and swept after about 6 minutes. Only the summary, topic and focus are sent to the server.
- **"The AI runs on your computer":** Llama 3.2 3B runs locally through llama.cpp (`llama-server` on `127.0.0.1`).
- **"Email, messages, banking and health are never read":** private sites and apps are filtered out before the AI sees anything. Jitsi calls are filtered too.
- **"Turn any website on or off":** not in the app yet (see the note above).
- **"One switch turns it all off":** turning friend finding off stops all tracking (app tracker, Chrome extension, summaries), clears what was buffered, and deletes your task from the server. It's also off at every launch and turns off when you close the window or quit.
