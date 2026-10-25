# Demo video skit: "Apartment hunting in SF"

**Length:** ~2:30 (skit 0:35, then voiceover)
**Cast:** Sultan, Sweksha
**Camera:** Thang (phone, landscape)
**Setting:** the hackathon floor. Sultan and Sweksha sit at different tables, a few seats apart, both on laptops.

---

## Shot 1: Sultan, frustrated (0:00–0:08)

Close-up on Sultan's laptop: a Zillow or Apartments.com search for San Francisco.
A listing: *"1BR, Mission District, $3,900/mo"*

Sultan scrolls, clicks another listing, sighs, and leans back.

> **SULTAN:** Why is finding an apartment in SF so hard?

---

## Shot 3: The notification (0:08–0:13)

A soft chime. The work-match popup slides in on Sultan's screen:

> *"Sweksha is also apartment hunting. Want to work together?"*

Sultan stops scrolling and reads it.

> **SULTAN:** …Wait. Her too?

---

## Shot 4: The pan (0:13–0:18)

The camera pans a few seats over to **Sweksha**, also scrolling SF apartment listings. Her laptop chimes, and she sees the same popup with Sultan's name.

> **SWEKSHA** *(looks up, surprised)*: Oh. Him too?

They look up and spot each other across the table.

---

## Shot 5: The payoff, banter fades out (0:18–0:35)

They both click **Work together**. Sweksha walks over with her laptop and sits down next to Sultan. They've never met. They each keep searching for their own place, and **the audio slowly fades** under the music while they talk:

> **SWEKSHA:** Hi, I'm Sweksha. So you're losing to SF too?
>
> **SULTAN** *(laughs)*: Badly. Every place under three grand has no windows.
>
> **SWEKSHA:** Have you tried the Sunset? It's a little cheaper.
>
> **SULTAN:** Wait, really? Show me what you've found.
>
> *(the audio is gone by here; they keep comparing listings on their own screens and talking)*

The voiceover starts over this shot.

---

# Voiceover (0:35–2:30)

Recorded separately, calm and clear. The shot list says what's on screen under each part.

## What work-match is (0:35–0:50)

**On screen:** the end of the banter, then the app's animated Earth in the menu bar.

> **VOICEOVER:**
> "Sultan and Sweksha were looking for the same thing, a few seats apart, and would never have known. **work-match** is a desktop app that notices what you're working on and connects you with someone doing the same thing, right now. You don't post, search or sign up for anything. You just work."

## How it works: the demo (0:50–1:35)

**On screen:** screen recordings from both laptops, in this order.

| On screen | Voiceover |
|---|---|
| The menu bar Earth, then turning on **Friend finding** | "It lives in your menu bar. Turn on friend finding once, and that's it." |
| Chrome with SF apartment listings | "As you work, it reads your tabs and apps…" |
| The Dev tab summary card: topic **Apartment hunting** | "…and every minute, an AI on your own laptop writes one short summary of what you're doing." |
| The popup on both screens | "When someone's working on something similar, you both get a match." |
| Both click **Work together**, and a Jitsi call opens | "Both say yes, and a call opens instantly." |
| **Work friends** with the saved connection | "If it clicks, you keep each other, and next time they're working on the same thing, you'll know." |

## Privacy (1:35–1:55)

**On screen:** the dark data-flow diagram (`docs/privacy-diagram.svg`). Point at the **ONLY THIS LEAVES** arrow on the second line.

> **VOICEOVER:**
> "Privacy comes first. Everything you have open stays on your laptop. The AI that reads it, Meta's **Llama 3.2**, runs on your device, not in the cloud. **The only thing that ever leaves is that one short summary.** Email, messages, banking and health sites are never read at all, and your pages are deleted within minutes. One switch turns it all off."

## The AI (1:55–2:10)

**On screen:** the same diagram, highlighting the Llama box, then the Jev box.

> **VOICEOVER:**
> "There are two AIs. **Llama** turns messy tabs into a clean, private summary. Then **Jev**, a decision model, compares summaries from everyone working right now and picks who you should meet. We built test sets to measure it, and matching went from **58% to 96%** accurate."

## How we built it (2:10–2:22)

**On screen:** quick logos or text cards.

> **VOICEOVER:**
> "It's a **Tauri** app in Rust and JavaScript, with a Chrome extension. Llama runs locally through **llama.cpp**. The server is **FastAPI on Vercel**, with **Supabase** for storage, **Jev** for matching and **Jitsi** for calls. We used **Claude Code** to help us build and test it."

## Close (2:22–2:30)

**On screen:** back to Sultan and Sweksha, now working side by side.

> **VOICEOVER:**
> "work-match. **The right person is closer than you think.**"

---

## Filming notes

- **Both laptops:** real SF apartment listings in Chrome, with the extension loaded and **friend finding on**, signed in to accounts on the deployed server (`yarn prod`).
- **Test the real match first.** Both should show **"Apartment hunting"** as the topic in the Dev tab. If the match is slow while filming, use **"Trigger match found"** in the Dev tab for Shots 3–4.
- **Shoot Shot 4 as one continuous pan,** from Sultan's screen to Sweksha's face, so it's clear they're in the same room.
- **Do 2–3 takes of each line.** Keep "Her too?" and "Him too?" short and surprised.
- **Profile pictures:** uploads are broken on the server right now (`SUPABASE_URL` is set to the database URL on Vercel). Fix that first, or the popup shows initials instead of photos.
