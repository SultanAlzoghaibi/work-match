"""Builds cases.json for the topic eval (src-tauri/src/topic_eval.rs).

Takes Sultan's realistic cases from ../summarizer-eval/cases.py (read only) and
adds tool-noise cases: work where a package manager, editor, terminal or git
is on screen, which once made the topic "Yarn". Each case gets a pass rule:

  shareable    what the summary's shareable flag should be
  good_topics  topics that count as right (case-insensitive, after the app's own
               topic cleanup); empty = any topic is fine if bad_words pass
  bad_words    words the topic must not contain (tool, app, site names)
  unsure       a judgment call: reported, not counted

    python3 make_cases.py
"""

import json
import os
os.environ.setdefault("PYTHONDONTWRITEBYTECODE", "1")
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.dont_write_bytecode = True  # don't leave __pycache__ in Sultan's folder
sys.path.insert(0, str(HERE.parent / "summarizer-eval"))
from cases import CASES  # noqa: E402  (Sultan's file, imported, never edited)

# Tool and app names that describe what's on screen, not what someone is doing.
TOOLS = ["yarn", "npm", "pnpm", "pip", "git", "github", "terminal", "iterm", "vs code", "vscode",
         "rustrover", "intellij", "xcode", "docker", "chrome", "claude", "jupyter", "notion"]

CODING = ["Coding", "Software development", "Programming"]

# Expectations for Sultan's cases, from his "what a human would say" column.
SULTAN = {
    "design-at-hackathon": dict(good=["Hackathon", "Figma design", "UI design", "Design", "App design"], bad=["refero"]),
    "design-no-context": dict(good=["UI design", "Design", "Figma design", "App design", "Web design"], bad=["refero", "job"]),
    "jobs-with-gmail": dict(good=["Job hunting"], bad=["gmail", "hackathon"]),
    "leetcode-plus-youtube": dict(good=["LeetCode"], bad=["youtube"]),
    "apartments-with-bank": dict(good=["Apartment hunting"], bad=["chase", "bank"]),
    "essay-research": dict(good=["Writing", "Coursework", "History essay", "Essay writing"], bad=["google docs", "jstor"]),
    "python-debugging": dict(good=["Data analysis", "Data cleaning", "Data science", "Python"] + CODING, bad=["stack overflow"]),
    "figma-plus-dribbble": dict(good=["Figma design", "UI design", "Design", "App design"], bad=["dribbble"]),
    "only-messaging": dict(shareable=False),
    "cs61b-course": dict(good=["CS 61B"], bad=[]),
    "thesis-with-twitter": dict(good=["Writing", "Research", "Thesis", "Thesis writing"], bad=["overleaf", "twitter"]),
    "health-portal": dict(shareable=False),
    "laptop-shopping": dict(good=["Laptop shopping", "Shopping"], bad=["amazon"], unsure=True),
    "ai-chat-react": dict(good=["Web development", "React", "Frontend development"] + CODING, bad=["job"]),
    "music-plus-notes": dict(good=["Studying", "Coursework", "Biology", "Bio 101"], bad=["spotify", "music", "61b"]),
    "hackathon-backend": dict(good=["Hackathon", "Backend development", "Web development"] + CODING, bad=["supabase"]),
}

# New: real work with tool noise on screen. (apps [(app, title, minutes)], pages [(host, path, title, text)])
NOISE = [
    ("yarn-tauri-app",
     [("Code", "main.js — work-match", 5), ("Terminal", "yarn dev1 — work-match — zsh", 3)],
     [("v2.tauri.app", "/develop/sidecar/", "Embedding External Binaries | Tauri",
       "You may need to embed external binaries to add additional functionality to your application. Configure externalBin in tauri.conf.json.")],
     dict(good=["App development", "Desktop app", "Desktop app development", "Hackathon"] + CODING)),
    ("npm-react-portfolio",
     [("Code", "App.jsx — portfolio", 6), ("Terminal", "npm run dev — portfolio — zsh", 2)],
     [("react.dev", "/reference/react/useState", "useState – React",
       "useState is a React Hook that lets you add a state variable to your component. const [state, setState] = useState(initialState)")],
     dict(good=["Web development", "React", "Frontend development", "Portfolio site"] + CODING)),
    ("git-merge-conflict",
     [("Terminal", "git rebase origin/main — work-match — zsh", 3), ("Code", "summarizer.rs — work-match", 5)],
     [("docs.github.com", "/en/get-started/using-git/resolving-merge-conflicts-after-a-git-rebase",
       "Resolving merge conflicts after a Git rebase - GitHub Docs",
       "When you perform a git rebase operation, you're typically moving commits around. Because of this, you might get into a situation where a merge conflict is introduced.")],
     dict(good=["Software development", "Hackathon", "Version control"] + CODING)),
    ("pip-pandas-notebook",
     [("Terminal", "pip install pandas matplotlib — zsh", 1), ("Code", "sales_analysis.ipynb — reports", 7)],
     [("pandas.pydata.org", "/docs/user_guide/groupby.html", "Group by: split-apply-combine — pandas documentation",
       "By group by we are referring to a process involving splitting the data into groups, applying a function to each group independently, and combining the results.")],
     dict(good=["Data analysis", "Data science", "Python"] + CODING)),
    ("docker-fastapi-backend",
     [("Terminal", "docker compose up — api — zsh", 2), ("Code", "main.py — todo-api", 6)],
     [("fastapi.tiangolo.com", "/tutorial/sql-databases/", "SQL (Relational) Databases - FastAPI",
       "FastAPI doesn't require you to use a SQL database. Here we'll see an example using SQLModel. Create the engine, the tables, a session dependency.")],
     dict(good=["Backend development", "Web development", "API development"] + CODING)),
    ("xcode-swiftui-app",
     [("Xcode", "WeatherApp — ContentView.swift", 8)],
     [("developer.apple.com", "/documentation/swiftui/list", "List | Apple Developer Documentation",
       "A container that presents rows of data arranged in a single column, optionally providing the ability to select one or more members.")],
     dict(good=["iOS development", "App development", "iOS app", "SwiftUI"] + CODING)),
    ("jupyter-cs189-homework",
     [("Google Chrome", "hw3.ipynb - JupyterLab", 7)],
     [("datahub.berkeley.edu", "/user/lab/tree/hw3.ipynb", "hw3.ipynb - JupyterLab",
       "CS 189 Homework 3: Logistic regression. Q2: derive the gradient of the cross-entropy loss and implement batch gradient descent.")],
     dict(good=["CS 189", "Coursework", "Machine learning"])),
    ("slack-then-coding",
     [("Slack", "#dev - Acme", 2), ("Code", "parser.rs — compiler", 6), ("Terminal", "cargo test — compiler — zsh", 2)],
     [("doc.rust-lang.org", "/book/ch09-02-recoverable-errors-with-result.html", "Recoverable Errors with Result - The Rust Programming Language",
       "Most errors aren't serious enough to require the program to stop entirely. The Result enum is defined as having two variants, Ok and Err.")],
     dict(good=["Rust", "Compiler development", "Programming languages"] + CODING, bad=["slack", "cargo"])),
    ("claude-code-hackathon",
     [("Code", "screen_capture.rs — work-match", 6), ("Terminal", "claude — work-match — zsh", 3)],
     [("doc.rust-lang.org", "/std/process/struct.Command.html", "Command in std::process - Rust",
       "A process builder, providing fine-grained control over how a new process should be spawned.")],
     dict(good=["Hackathon", "App development", "Rust"] + CODING)),
    ("terminal-only-build",
     [("Terminal", "yarn build — work-match — zsh", 4)],
     [],
     dict(good=["Hackathon", "App development"] + CODING, unsure=True)),
]

# Held out: written before any tuning and only run once at the end (holdout.json).
HOLDOUT = [
    ("spanish-practice", [("Google Chrome", "Duolingo - Spanish", 6), ("Anki", "Spanish verbs", 3)],
     [("www.duolingo.com", "/learn", "Duolingo - Spanish", "Lesson 4: preterite tense. Translate: Ayer comí en un restaurante.")],
     dict(good=["Studying", "Language learning", "Spanish"], bad=["duolingo", "anki"])),
    ("unity-game", [("Unity", "Platformer - SampleScene", 7), ("Code", "PlayerController.cs — Platformer", 4)],
     [("docs.unity3d.com", "/ScriptReference/Rigidbody2D.AddForce.html", "Rigidbody2D.AddForce - Unity Scripting API",
       "Apply a force to the rigidbody. The force is specified as two separate components in the X and Y directions.")],
     dict(good=["Game development", "App development"] + CODING, bad=["unity"])),
    ("physics-lab-report", [("Google Chrome", "Physics 7B Lab 3 - Overleaf", 8)],
     [("www.overleaf.com", "/project/xyz", "Physics 7B Lab 3 - Overleaf",
       "\\section{Results} The measured period of the pendulum was 2.01 s, consistent with T = 2 pi sqrt(L/g).")],
     dict(good=["Writing", "Coursework", "Physics 7B", "Physics"], bad=["overleaf"])),
    ("club-poster", [("Google Chrome", "Hackathon poster - Canva", 6)],
     [("www.canva.com", "/design/abc/edit", "Hackathon poster - Canva", "Join us for Code Night! Free pizza. Thursday 7pm. Add text. Elements. Uploads.")],
     dict(good=["Design", "Graphic design", "Poster design"], bad=["canva"])),
    ("resume-and-linkedin", [("Google Chrome", "Resume - Google Docs", 5), ("Google Chrome", "Data Analyst jobs | LinkedIn", 3)],
     [("docs.google.com", "/document/d/9/edit", "Resume - Google Docs", "Experience: Data Analyst Intern. Built dashboards in Tableau. Skills: SQL, Python, Excel."),
      ("www.linkedin.com", "/jobs/search", "Data Analyst jobs | LinkedIn", "Data Analyst - Entry level. Easy Apply. 120 applicants.")],
     dict(good=["Job hunting"], bad=["linkedin", "google docs"])),
    ("kaggle-sklearn", [("Google Chrome", "Titanic notebook | Kaggle", 6), ("Google Chrome", "RandomForestClassifier — scikit-learn", 2)],
     [("www.kaggle.com", "/code/me/titanic", "Titanic notebook | Kaggle", "Feature engineering: fill missing Age with median, one-hot encode Embarked. Accuracy 0.81."),
      ("scikit-learn.org", "/stable/modules/generated/sklearn.ensemble.RandomForestClassifier.html", "RandomForestClassifier — scikit-learn", "A random forest classifier.")],
     dict(good=["Data science", "Machine learning"], bad=["kaggle", "scikit"])),
    ("nextjs-with-npm", [("Terminal", "npm run build — shop — zsh", 2), ("Code", "page.tsx — shop", 6)],
     [("nextjs.org", "/docs/app/building-your-application/routing", "Routing: Fundamentals | Next.js",
       "The App Router works in a new directory named app. Each folder represents a route segment.")],
     dict(good=["Web development", "Frontend development"] + CODING, bad=["next.js", "nextjs"])),
    ("austin-apartments", [("Google Chrome", "Austin TX Apartments for Rent | Zillow", 7)],
     [("www.zillow.com", "/austin-tx/rentals/", "Austin TX Apartments for Rent | Zillow", "2 bd 1 ba, East Austin, pets allowed, in-unit laundry.")],
     dict(good=["Apartment hunting"], bad=["zillow"])),
    ("cs70-homework", [("Preview", "CS70 HW5.pdf", 6), ("Google Chrome", "Note 12: Probability | CS 70", 3)],
     [("www.eecs70.org", "/assets/pdf/notes/n12.pdf", "Note 12: Probability | CS 70", "Probability spaces, events, conditional probability, Bayes rule.")],
     dict(good=["CS 70", "Coursework"], bad=["preview"])),
    ("only-chat-apps", [("WhatsApp", "Family", 4), ("Messages", "Alex", 2)], [], dict(shareable=False)),
]


def case(name, apps, pages, rule):
    shareable = rule.get("shareable", True)
    return {
        "name": name,
        "apps": [{"app": a, "title": t, "minutes": m} for a, t, m in apps],
        "pages": [{"host": h, "path": p, "title": ti, "text": tx} for h, p, ti, tx in pages],
        "shareable": shareable,
        "good_topics": rule.get("good", []) if shareable else [],
        "bad_words": sorted(set(TOOLS + rule.get("bad", []))) if shareable else [],
        "unsure": rule.get("unsure", False),
    }


cases = []
for name, apps, pages, _context, _human in CASES:
    cases.append(case(name, apps, pages, SULTAN[name]) | {"from": "summarizer-eval/cases.py"})
for name, apps, pages, rule in NOISE:
    cases.append(case(name, apps, pages, rule) | {"from": "tool noise"})

(HERE / "cases.json").write_text(json.dumps({"cases": cases}, indent=1, ensure_ascii=False) + "\n")
held = [case(name, apps, pages, rule) | {"from": "holdout"} for name, apps, pages, rule in HOLDOUT]
(HERE / "holdout.json").write_text(json.dumps({"cases": held}, indent=1, ensure_ascii=False) + "\n")
print(f"wrote {len(cases)} cases ({sum(c['unsure'] for c in cases)} unsure) and {len(held)} held-out cases")
