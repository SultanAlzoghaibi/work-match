"""Minute-by-minute pipeline: map each minute to an activity, propose 3
labels from the timeline, then a judge picks one. Compare with results.md."""
import json, sys, time, urllib.request
from cases import CASES

URL = "http://127.0.0.1:8095/v1/chat/completions"
LABELS = "LeetCode, Job hunting, Apartment hunting, Coursework, Studying, Figma design, Writing, Hackathon, or a course name like \"CS 61B\""
PRIVATE_HOSTS = ["mail.google.com", "chase.com", "mychart", "webmd.com", "slack.com"]
PRIVATE_APPS = ["Messages", "Slack", "Mail"]

MINUTE_SYSTEM = """You see one minute of a person's computer use. Say in a few words what they were doing,
describing their goal rather than the page and naming the specific subject, e.g. "comparing apartment listings in Brooklyn" or
"fixing a pandas warning while cleaning sales data". Never copy a page title. Never name a
website, brand, app or file name except LeetCode or Figma. If it is music, social media or idle, say so
("listening to music", "scrolling social media", "idle"). Output ONLY the JSON object."""
MINUTE_SCHEMA = {"type": "object", "properties": {"activity": {"type": "string", "maxLength": 80}},
                 "required": ["activity"], "additionalProperties": False}
# The minute prompt asks for these exact phrases; such minutes are dropped from the timeline.
LEISURE = ("listening to music", "scrolling social media", "idle", "watching videos for fun")

PROPOSE_SYSTEM = f"""You get a timeline of what a person worked on, with minutes per activity. Write 3 different
descriptions of their main work, for matching them with a stranger doing similar work:
"most_time": the single activity with the most minutes;
"combined": all the activities together as one piece of work;
"goal": the bigger goal or project the activities serve.
Only describe activities in the timeline; never invent any.
Each one has "topic" (1 to 3 words, one of: {LABELS} when one fits) and
"task_summary" (one sentence, at most 120 characters, saying what they are making, learning or
looking for). If a "Setting" line is given (e.g. at a hackathon), use it only when the timeline is building
their project (coding, designing, setting up a backend): then mention it and use it as the topic.
Job hunting, studying and other activities keep their own topic and don't mention the setting.
Never name a website, brand, app or file name except LeetCode or Figma. Output ONLY the JSON object."""
CAND = {"type": "object", "properties": {"topic": {"type": "string", "maxLength": 40}, "task_summary": {"type": "string", "maxLength": 120}},
        "required": ["topic", "task_summary"], "additionalProperties": False}
ANGLES = ["most_time", "combined", "goal"]
PROPOSE_SCHEMA = {"type": "object", "properties": {angle: CAND for angle in ANGLES},
                  "required": ANGLES, "additionalProperties": False}

PICK_SYSTEM = """You get a person's minute-by-minute timeline and 3 candidate descriptions of their main work.
Pick the one a stranger doing the same work would recognize best: it must fit the activities
that took most of the minutes, be specific about the subject, and add nothing that is not in the timeline.
Reject candidates that name an app, website or file (LeetCode and Figma are fine).
Writing beats reading for the same subject. Output ONLY the JSON object."""
PICK_SCHEMA = {"type": "object", "properties": {"best": {"type": "integer", "enum": [1, 2, 3]}},
               "required": ["best"], "additionalProperties": False}

def ask(system, user, schema):
    body = {"messages": [{"role": "system", "content": system}, {"role": "user", "content": user}],
            "temperature": 0, "response_format": {"type": "json_object", "schema": schema}}
    request = urllib.request.Request(URL, json.dumps(body).encode(), {"Content-Type": "application/json"})
    return json.loads(json.loads(urllib.request.urlopen(request).read())["choices"][0]["message"]["content"])

def minutes(apps, pages):
    """One slot per minute: the app in front, plus the pages whose title matches its window."""
    slots = []
    for app, title, count in apps:
        if app in PRIVATE_APPS or any(word in title.lower() for word in ["gmail", "mychart", "chase online"]):
            continue
        match = [p for p in pages if p[2].split(" |")[0].split(" - ")[0].lower() in title.lower()
                 and not any(h in p[0] for h in PRIVATE_HOSTS)]
        for i in range(count):
            slots.append((app, title, match if i == 0 else []))
    return slots

def run(apps, pages, ctx):
    slots = minutes(apps, pages)
    if not slots:
        return "— not shared (all private)", 0, []
    cache, labels, calls = {}, [], 0
    for app, title, slot_pages in slots:
        key = (app, title)
        if key not in cache or slot_pages:
            text = f"App in front: {app}: {title} (60 s)\n" + "".join(
                f"Page: {h}{p.rstrip('/')} | {t}\n{body[:500]}\n" for h, p, t, body in slot_pages)
            cache[key] = ask(MINUTE_SYSTEM, text, MINUTE_SCHEMA)
            calls += 1
        labels.append(cache[key])
    # Merge runs of the same activity: "minutes 1-6 (6 min): ...". Breaks and private minutes are dropped.
    timeline, start = [], 0
    for i in range(1, len(labels) + 1):
        if i == len(labels) or labels[i]["activity"] != labels[start]["activity"]:
            if not labels[start]["activity"].lower().startswith(LEISURE):
                span = f"minute {start + 1}" if i - start == 1 else f"minutes {start + 1}-{i}"
                timeline.append(f"{span} ({i - start} min): {labels[start]['activity']}")
            start = i
    if not timeline:
        return "— not shared (no work minutes)", calls, [l["activity"] for l in labels]
    user = (f"Setting: {ctx}\n\n" if ctx else "") + "\n".join(timeline)
    proposal = ask(PROPOSE_SYSTEM, user, PROPOSE_SCHEMA); calls += 1
    cands = [proposal[angle] for angle in ANGLES]
    listing = "\n".join(f"{i + 1}. {c['topic']}: {c['task_summary']}" for i, c in enumerate(cands))
    best = ask(PICK_SYSTEM, user + "\n\nCandidates:\n" + listing, PICK_SCHEMA)["best"]; calls += 1
    c = cands[best - 1]
    others = " / ".join(f"{x['topic']}: {x['task_summary']}" for i, x in enumerate(cands) if i != best - 1)
    return f"**{c['topic']}**: {c['task_summary']}<br><sub>rejected: {others}</sub>", calls, timeline

rows = ["| Case | Expected | Minute timeline | Picked (3 proposals → judge) | Calls · time |", "|---|---|---|---|---|"]
for name, apps, pages, ctx, expected in CASES:
    start = time.time()
    result, calls, timeline = run(apps, pages, ctx)
    rows.append(f"| {name} | {expected} | {'<br>'.join(timeline)} | {result} | {calls} · {time.time() - start:.1f}s |")
    print(rows[-1], flush=True)
open(sys.argv[1] if len(sys.argv) > 1 else "pipeline_results.md", "w").write("\n".join(rows) + "\n")
