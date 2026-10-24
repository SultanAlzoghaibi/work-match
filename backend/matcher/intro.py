"""Intro text with Muse Spark (Meta Model API, OpenAI-compatible).

Each side gets its own line naming the other person and the shared topic.
Only the topic is revealed, never the other person's summary. Without a key,
or if the model's reply can't be used, a template is used instead.
"""

import json
from dataclasses import dataclass

import httpx

from .people import Person

PROMPT = """Two people working alone right now are about to get a friendly intro so they can work alongside each other.

Person A ({a_name}): {a_summary}
Person B ({b_name}): {b_summary}

Write JSON with exactly these keys:
- "topic": the shared topic in 2-5 lowercase words, e.g. "shortest-path algorithms"
- "intro_a": shown to {a_name}. Names {b_name}, says they are also working on the topic, ends with an invitation to work alongside them. Max 20 words.
- "intro_b": the same for {b_name}, naming {a_name}.

Mention only the shared topic. Do not reveal anything else about either person's work, school, or situation. Use "them" for pronouns. Reply with the JSON only."""


@dataclass(frozen=True)
class Intro:
    topic: str
    intro_a: str   # for `me`
    intro_b: str   # for `other`
    model: str


def template_intro(me: Person, other: Person) -> Intro:
    topic = me.topic if me.topic.lower() == other.topic.lower() else other.topic
    return Intro(
        topic=topic,
        intro_a=f"{other.name} is also working on {topic} right now. Want to work alongside them?",
        intro_b=f"{me.name} is also working on {topic} right now. Want to work alongside them?",
        model="fallback:template",
    )


def _parse(text: str) -> dict:
    text = text.strip()
    if text.startswith("```"):
        text = text.strip("`").removeprefix("json").strip()
    data = json.loads(text)
    if not all(isinstance(data.get(k), str) and data[k].strip() for k in ("topic", "intro_a", "intro_b")):
        raise ValueError("missing keys")
    return data


class MuseIntroWriter:
    def __init__(self, api_key: str, base_url: str, model: str, timeout: float = 20.0):
        self.model = model
        self._url = base_url.rstrip("/") + "/chat/completions"
        self._client = httpx.Client(timeout=timeout, headers={"Authorization": f"Bearer {api_key}"})

    def write(self, me: Person, other: Person) -> Intro:
        prompt = PROMPT.format(a_name=me.name, a_summary=me.summary, b_name=other.name, b_summary=other.summary)
        try:
            resp = self._client.post(self._url, json={
                "model": self.model,
                "messages": [{"role": "user", "content": prompt}],
                "temperature": 0.7,
            })
            resp.raise_for_status()
            data = _parse(resp.json()["choices"][0]["message"]["content"])
        except (httpx.HTTPError, KeyError, IndexError, ValueError, TypeError) as err:
            print(f"[intro] Muse failed, using template: {err}")
            return template_intro(me, other)
        return Intro(data["topic"].strip(), data["intro_a"].strip(), data["intro_b"].strip(), self.model)


class TemplateIntroWriter:
    model = "fallback:template"

    def write(self, me: Person, other: Person) -> Intro:
        return template_intro(me, other)
