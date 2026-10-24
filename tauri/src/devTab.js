// Dev tab (dev builds only): is real tracking running, what was captured,
// what the summarizer read and returned, and what was pushed. Read-only;
// refreshes every 3 s while it's open.
import { invoke } from "@tauri-apps/api/core";
import { API_URL, MOCK_USER_ID, getTaskHistory } from "./api.js";

const REFRESH_MS = 3000;
const WINDOW_MINUTES = 15;

function el(tag, attrs = {}, ...children) {
  const node = document.createElement(tag);
  for (const [key, value] of Object.entries(attrs)) {
    if (key.startsWith("on")) node.addEventListener(key.slice(2), value);
    else if (key === "class") node.className = value;
    else node.setAttribute(key, value);
  }
  for (const child of children) {
    if (child == null) continue;
    node.append(typeof child === "string" ? document.createTextNode(child) : child);
  }
  return node;
}

const clock = (ms) => new Date(ms).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });
const shortClock = (ms) => new Date(ms).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });

function duration(ms) {
  const seconds = Math.max(0, Math.round(ms / 1000));
  if (seconds < 60) return `${seconds} s`;
  const minutes = Math.round(seconds / 60);
  return minutes < 60 ? `${minutes} min` : `${Math.floor(minutes / 60)} h ${minutes % 60} min`;
}

function ago(ms, now = Date.now()) {
  return `${duration(now - ms)} ago`;
}

function cut(text, max) {
  return text && text.length > max ? `${text.slice(0, max - 1)}…` : text ?? "";
}

function section(title, ...children) {
  return el("div", { class: "dev-section" }, el("h3", { class: "section-title" }, title), ...children);
}

function check(ok, label, detail) {
  return el(
    "div",
    { class: `dev-check ${ok === true ? "ok" : ok === false ? "bad" : "warn"}` },
    el("span", { class: "dot" }),
    el("span", { class: "dev-check-label" }, label),
    el("span", { class: "dev-check-detail" }, detail),
  );
}

// --- Sections ------------------------------------------------------------------

function statusSection(status, backendOk, onRefresh, onPushNow) {
  const now = status.now;
  const toggle = el(
    "button",
    {
      class: "secondary",
      onclick: async () => {
        await invoke("set_sharing", { enabled: !status.sharing_on });
        onRefresh();
      },
    },
    status.sharing_on ? "Turn off" : "Turn on",
  );
  const capture = {
    listening: [true, "listening on 127.0.0.1:47321"],
    failed: [false, "port 47321 is taken: another instance is capturing; restart this one first"],
    starting: [null, "starting…"],
  }[status.capture_server];
  return section(
    "Status",
    el(
      "div",
      { class: "dev-check-row" },
      check(status.sharing_on, "Share my activity", status.sharing_on ? "on: tracking apps + Chrome" : "off: nothing is tracked"),
      toggle,
    ),
    check(capture[0], "Chrome capture server", capture[1]),
    check(
      status.last_capture_at ? true : false,
      "Chrome extension",
      status.last_capture_at
        ? `last page ${ago(status.last_capture_at, now)} · ${status.captures_in_buffer} in buffer`
        : "no pages yet: load extension/ in chrome://extensions and browse in Chrome",
    ),
    check(status.accessibility, "Accessibility (window titles)", status.accessibility ? "granted" : "not granted: System Settings → Privacy & Security → Accessibility"),
    check(status.llm === "ready" ? true : status.llm === "starting" ? null : false, "Llama", status.llm === "down" ? "down: no model / llama-server, summaries fall back (not shared)" : status.llm),
    check(
      status.next_summary_at ? null : false,
      "Next 5-min summary",
      status.next_summary_at ? `in ${duration(status.next_summary_at - now)} (${shortClock(status.next_summary_at)})` : "not scheduled",
    ),
    check(backendOk, "Backend", `${API_URL} · user ${MOCK_USER_ID}`),
    el(
      "div",
      { class: "dev-actions" },
      el("button", { class: "primary", onclick: onPushNow }, "⚡ Push now"),
      el("button", { class: "link", onclick: onRefresh }, "Refresh"),
    ),
  );
}

function appsSection(sessions, now) {
  const since = now - WINDOW_MINUTES * 60_000;
  const recent = sessions.filter((session) => session.ended_at >= since);
  if (!recent.length) {
    return section(`Apps & windows · last ${WINDOW_MINUTES} min`, el("p", { class: "muted fine-print" }, "Nothing recorded. Is Share my activity on?"));
  }
  const totals = new Map();
  for (const session of recent) {
    totals.set(session.app_name, (totals.get(session.app_name) ?? 0) + (session.ended_at - session.started_at));
  }
  const totalLine = [...totals.entries()]
    .sort((a, b) => b[1] - a[1])
    .map(([app, ms]) => `${app} ${duration(ms)}`)
    .join(" · ");
  return section(
    `Apps & windows · last ${WINDOW_MINUTES} min`,
    el("p", { class: "dev-totals" }, totalLine),
    el(
      "div",
      { class: "dev-rows" },
      ...recent.map((session) =>
        el(
          "div",
          { class: "dev-row" },
          el("span", { class: "dev-time" }, `${shortClock(session.started_at)}–${shortClock(session.ended_at)}`),
          el("span", { class: "dev-dur" }, duration(session.ended_at - session.started_at)),
          el("span", { class: "dev-app" }, session.app_name),
          el(
            "span",
            { class: "dev-title" },
            cut(session.window_title ?? "(no title)", 90),
            session.document ? el("span", { class: "muted" }, ` [${session.document}]`) : null,
          ),
        ),
      ),
    ),
  );
}

function capturesSection(captures) {
  if (!captures.length) {
    return section("Chrome pages in the buffer", el("p", { class: "muted fine-print" }, "Empty. Pages are cleared after each summary, so this fills up again as you browse."));
  }
  return section(
    "Chrome pages in the buffer",
    el(
      "div",
      { class: "dev-rows" },
      ...captures.map((capture) =>
        el(
          "details",
          { class: "dev-capture" },
          el(
            "summary",
            {},
            el("span", { class: "dev-time" }, clock(capture.captured_at)),
            el("span", { class: "dev-app" }, `${capture.host}${capture.path}`),
            el("span", { class: "dev-title" }, `${cut(capture.title, 70)} · ${capture.text_chars} chars`),
          ),
          el("pre", { class: "dev-pre" }, capture.preview),
        ),
      ),
    ),
  );
}

function lastRunSection(lastRun) {
  const { run, push } = lastRun;
  if (!run) return section("Last summary run", el("p", { class: "muted fine-print" }, "No summary yet this session. Press ⚡ Push now."));
  const summary = run.summary ?? {};
  const result = summary.shareable
    ? `${summary.topic}: ${summary.task_summary}`
    : summary.source === "idle"
      ? "Nothing captured, so nothing to summarize."
      : "Not shareable, so nothing was pushed.";
  const match = push?.match;
  return section(
    `Last summary run · ${clock(run.at)}`,
    el("p", {}, el("strong", {}, `${summary.source ?? "?"} · ${summary.shareable ? "shared" : "not shared"}: `), result),
    push
      ? el(
          "p",
          { class: "fine-print" },
          `Pushed as "${push.topic}". `,
          match?.match_id != null
            ? `Matched with ${match.other_name}.`
            : match?.skipped_reason
              ? `No match: ${match.skipped_reason}.`
              : match?.scores?.length
                ? `No match: best ${match.scores[0].name} at ${match.scores[0].score.toFixed(2)}.`
                : "No match.",
        )
      : null,
    run.prompt
      ? el("details", {}, el("summary", {}, "What Llama read"), el("pre", { class: "dev-pre" }, run.prompt))
      : null,
    run.replies.length
      ? el("details", {}, el("summary", {}, `Llama replied (${run.replies.length} attempt${run.replies.length > 1 ? "s" : ""})`), el("pre", { class: "dev-pre" }, run.replies.join("\n\n")))
      : null,
  );
}

function historySection(history) {
  if (history === undefined) return section("Push history (server)", el("p", { class: "muted fine-print" }, "Loading…"));
  if (!history) return section("Push history (server)", el("p", { class: "muted fine-print" }, "Couldn't reach the backend."));
  return section(
    "Push history (server)",
    el(
      "div",
      { class: "dev-rows" },
      ...history.map((item) =>
        el(
          "div",
          { class: "dev-row history" },
          el("span", { class: "dev-time" }, shortClock(Date.parse(item.pushed_at))),
          el("span", { class: "dev-app" }, item.topic ?? "?"),
          el("span", { class: "dev-title" }, cut(item.task_summary, 110)),
        ),
      ),
    ),
  );
}

// --- Tab --------------------------------------------------------------------------

/// Builds the Dev tab and keeps it fresh while it's in the document.
export function renderDevTab({ onPushNow }) {
  const body = el("div", { class: "tab-body dev" }, el("p", { class: "muted" }, "Loading…"));
  let timer = null;
  // Server data (~1 s per call) is fetched in the background and shown on the
  // next render, so the local sections never wait for it.
  let history = undefined;
  let backendOk = null;
  let serverBusy = false;

  function refreshServer() {
    if (serverBusy) return;
    serverBusy = true;
    Promise.all([
      getTaskHistory(MOCK_USER_ID, 15).catch(() => null),
      fetch(`${API_URL}/health`).then((response) => response.ok, () => false),
    ]).then(([nextHistory, nextOk]) => {
      history = nextHistory;
      backendOk = nextOk;
      serverBusy = false;
      refresh().catch(() => {});
    });
  }

  async function refresh() {
    if (!body.isConnected && timer) {
      clearInterval(timer);
      return;
    }
    const [status, sessions, captures, lastRun] = await Promise.all([
      invoke("dev_status"),
      invoke("get_app_sessions", { limit: 200 }).catch(() => []),
      invoke("dev_captures").catch(() => []),
      invoke("dev_last_run").catch(() => ({ run: null, push: null })),
    ]);
    const open = new Set([...body.querySelectorAll("details[open] > summary")].map((node) => node.textContent));
    body.replaceChildren(
      statusSection(status, backendOk, refresh, async () => {
        await onPushNow();
        refresh();
      }),
      appsSection(sessions, status.now),
      capturesSection(captures),
      lastRunSection(lastRun),
      historySection(history),
    );
    // Keep expanded sections open across refreshes.
    for (const summary of body.querySelectorAll("details > summary")) {
      if (open.has(summary.textContent)) summary.parentElement.open = true;
    }
  }

  refresh().catch((err) => body.replaceChildren(el("p", { class: "error" }, `Dev tab failed: ${err}`)));
  refreshServer();
  timer = setInterval(() => {
    refresh().catch(() => {});
    refreshServer();
  }, REFRESH_MS);
  return body;
}
