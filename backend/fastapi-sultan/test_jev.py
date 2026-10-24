"""Check that the Jev key in .env works.

Run from backend/fastapi-sultan:  .venv/bin/python test_jev.py

Asks Jev one yes/no ("noul") question for a related and an unrelated pair of
task summaries and prints the scores. A working key gives a high score for the
first pair and a low one for the second.
"""

import json
import os
import urllib.error
import urllib.request
from pathlib import Path

from dotenv import load_dotenv

load_dotenv(Path(__file__).resolve().parent / ".env")

JEV_URL = "https://api.typesafe.ai/v1/systemone"
JEV_MODEL = "jev-latest"

PAIRS = {
    "related (expect high)": (
        "Grinding LeetCode medium problems on dynamic programming.",
        "Doing the NeetCode 150 list in Python to prep for internship interviews.",
    ),
    "unrelated (expect low)": (
        "Grinding LeetCode medium problems on dynamic programming.",
        "Searching StreetEasy for a one-bedroom apartment in Brooklyn.",
    ),
}


def ask_jev(key: str, task_a: str, task_b: str) -> float:
    body = {
        "model": JEV_MODEL,
        "state": [f"Person A is working on: {task_a}", f"Person B is working on: {task_b}"],
        "questions": {
            "good_match": {
                "type": "noul",
                "instructions": "Would A and B benefit from working alongside each other right now?",
                "criteria": {
                    "true": "They are working on the same or closely related things.",
                    "false": "Their work is unrelated.",
                },
            }
        },
    }
    request = urllib.request.Request(
        JEV_URL,
        data=json.dumps(body).encode(),
        headers={"Authorization": f"Bearer {key}", "Content-Type": "application/json"},
    )
    with urllib.request.urlopen(request, timeout=30) as response:
        return json.load(response)["answers"]["good_match"]["noul"]


def main() -> None:
    key = os.environ.get("JEV_API")
    if not key:
        raise SystemExit("JEV_API is not set in backend/fastapi-sultan/.env")
    for label, (task_a, task_b) in PAIRS.items():
        try:
            print(f"{label}: {ask_jev(key, task_a, task_b):.2f}")
        except urllib.error.HTTPError as err:
            raise SystemExit(f"Jev request failed: {err.code} {err.read().decode()}") from None


if __name__ == "__main__":
    main()
