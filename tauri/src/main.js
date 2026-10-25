import "@fontsource/bowlby-one/400.css";
import "@fontsource/inter/500.css";
import "@fontsource/inter/700.css";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { API_URL, clearTask, login, setUserId, signup, userId, deleteConnection, getFilters, pushTask, saveFilters, updateUser, uploadAvatar } from "./api.js";
import { MOCK_TASKS, randomSummary } from "./mockTasks.js";
import { renderDevTab } from "./devTab.js";
import { SCHOOLS } from "./schools.js";
import { AGE_GROUPS, LANGUAGES, parseLanguages, DEFAULT_NAME, loadProfile, logout, resizeImage, saveSession, saveAvatarUrl, saveProfile } from "./profile.js";

// Same idea as TyA's getOs(): ask Rust for the target OS instead of sniffing
// the user agent. macOS gets native liquid glass, so the page stays clear;
// everything else paints a CSS semi-transparent background.
const os = await invoke("os_name").catch(() => "unknown");
document.documentElement.classList.add(`os-${os}`); // os-macos, os-windows, os-linux

// Rust polls GET /users/{id}/poll every 3 s and shows the match popup. Starts
// once someone is signed in (see render).

/// Per-device memory for "continue where you left off": last tab and whether
/// friend finding was on. localStorage can be unavailable, so never rely on it.
const remember = {
  get(key) {
    try {
      return localStorage.getItem(`wm-${key}`);
    } catch {
      return null;
    }
  },
  set(key, value) {
    try {
      localStorage.setItem(`wm-${key}`, value);
    } catch {}
  },
};

const root = document.getElementById("root");

// Cmd+Q (Ctrl+Q on Windows) quits the app, tray icon included. The red close
// button only hides the window.
document.addEventListener("keydown", (event) => {
  if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "q") {
    event.preventDefault();
    quitApp();
  }
});

/// Quit, removing a shared demo task first so nobody gets matched with an app
/// that's gone.
async function quitApp() {
  if (session) {
    session = null;
    await clearTask(userId()).catch(() => {});
  }
  invoke("quit_app");
}

// --- Light / dark mode ------------------------------------------------------

/// "auto" follows the Mac's appearance; "light" / "dark" pin it. The toggle in
/// the title bar cycles auto -> light -> dark.
const THEMES = { auto: "◐", light: "☀", dark: "☾" };
const themeToggle = document.getElementById("theme-toggle");

function applyTheme(mode) {
  if (mode === "auto") delete document.documentElement.dataset.theme;
  else document.documentElement.dataset.theme = mode;
  themeToggle.textContent = THEMES[mode];
  themeToggle.title = `Appearance: ${mode === "auto" ? "match the Mac" : mode} (click to change)`;
}

let themeMode = THEMES[remember.get("theme")] ? remember.get("theme") : "auto";
applyTheme(themeMode);
themeToggle.addEventListener("click", () => {
  const order = Object.keys(THEMES);
  themeMode = order[(order.indexOf(themeMode) + 1) % order.length];
  remember.set("theme", themeMode);
  applyTheme(themeMode);
});

/// Result of the last Save, shown once on the profile view.
let syncStatus = null;

// --- Friend finding ---------------------------------------------------------

/// Same flag as the tray's "Turn on friend finding" item; Rust emits
/// "friend-finding" whenever either side flips it.
let friendFinding = await invoke("get_friend_finding").catch(() => false);
const friendButtons = new Set();

const titlebarStatus = document.getElementById("titlebar-status");

/// Title bar pill mirrors friend finding so it's visible from any view.
function paintTitlebarStatus() {
  titlebarStatus.textContent = friendFinding ? "● Finding friends" : "Friend finding off";
  titlebarStatus.classList.toggle("on", friendFinding);
}
paintTitlebarStatus();

function paintFriendButton(button) {
  button.textContent = friendFinding ? "Turn off friend finding" : "Turn on friend finding";
  button.className = friendFinding ? "secondary friend-finding on" : "primary friend-finding";
}

function friendFindingButton() {
  const button = el("button", {
    onclick: () => invoke("set_friend_finding", { enabled: !friendFinding }).catch(console.error),
  });
  paintFriendButton(button);
  friendButtons.add(button);
  return button;
}

listen("friend-finding", (event) => {
  friendFinding = event.payload;
  remember.set("friend-finding", friendFinding ? "1" : "0");
  paintTitlebarStatus();
  for (const button of friendButtons) {
    if (button.isConnected) paintFriendButton(button);
    else friendButtons.delete(button);
  }
  // Turning friend finding off (tray or button) also stops a demo task search.
  if (!friendFinding && session) endSession("Friend finding turned off: stopped searching.");
});

// --- Demo task push ---------------------------------------------------------

/// Buttons that push a random mock summary to curr_task, so matching can be
/// tested before the on-device summarizer exists.
function matchLine(match) {
  if (match.match_id != null) return `Matched with ${match.other_name} (${match.scores[0].score.toFixed(2)}). Check the popup.`;
  if (match.skipped_reason) return `No match: ${match.skipped_reason}.`;
  const best = match.scores[0];
  // The cutoff lives on the backend (MATCH_THRESHOLD, 0.45 by default).
  return best ? `No match: best was ${best.name} at ${best.score.toFixed(2)}, below the match cutoff.` : "No match.";
}

/// A pretend activity: clicking a task button behaves like the real 5-minute
/// summaries (summarizer.rs), pushing a new random example of that task every
/// PUSH_EVERY_MS until Stop. Like the real pipeline it keeps going after a
/// match; the backend's match cooldown keeps it from re-matching the same person.
const PUSH_EVERY_MS = 5 * 60 * 1000;

let session = null; // { kind, summary, topic, filters, nextPushAt, pushing }
let lastPush = null; // { summary, topic, match } from the latest push, or { error } / { ended }
let sessionTimer = null;
const taskUi = { status: null, matchStatus: null, countdown: null, stopButton: null, pushNote: null };

/// The latest push, demo or real, for the "Your summary" card:
/// { topic, summary, source, at, match } or { skipped, at } for a real
/// summary that wasn't shareable.
let latestSummary = null;
const summaryUi = { card: null };

function setLatestSummary(next) {
  latestSummary = { ...next, at: Date.now() };
  updateSummaryCard();
}

function updateSummaryCard() {
  const { card } = summaryUi;
  if (!card?.isConnected) return;
  if (!latestSummary) {
    card.replaceChildren(
      el("h3", { class: "section-title" }, "Your summary"),
      el("p", { class: "muted fine-print" }, "Nothing pushed yet. Turn on sharing and press ⚡ Push now in the Dev tab, or pick a demo task there."),
    );
    return;
  }
  const minutes = Math.floor((Date.now() - latestSummary.at) / 60_000);
  const when = minutes < 1 ? "just now" : `${minutes} min ago`;
  if (latestSummary.skipped) {
    card.replaceChildren(
      el("h3", { class: "section-title" }, "Your summary"),
      el("p", {}, latestSummary.skipped),
      el("p", { class: "muted fine-print" }, when),
    );
    return;
  }
  card.replaceChildren(
    el("h3", { class: "section-title" }, "Your summary"),
    el("div", { class: "summary-head" }, el("span", { class: "topic-pill" }, latestSummary.topic), el("span", { class: "muted fine-print" }, `${latestSummary.source} · ${when}`)),
    el("p", { class: "summary-text" }, latestSummary.summary),
    latestSummary.match ? el("p", { class: "fine-print" }, matchLine(latestSummary.match)) : null,
  );
}

// Refresh "x min ago" while the card is visible.
setInterval(updateSummaryCard, 30_000);

/// "Push now": don't wait for the 5-minute timer. A running demo task pushes
/// its next example (and restarts the timer); otherwise the real summarizer
/// summarizes the last 5 minutes of activity and pushes that.
async function pushNow() {
  const note = (text) => {
    if (taskUi.pushNote) taskUi.pushNote.textContent = text;
  };
  await ensureSharing().catch((err) => console.warn("couldn't turn sharing on", err));
  if (session) {
    if (session.pushing) return;
    session.nextPushAt = 0;
    note("Pushing the next example now…");
    await tickSession();
    note("");
    return;
  }
  note("Summarizing your last 5 minutes…");
  try {
    const summary = await invoke("summarize_now");
    note(
      summary.shareable
        ? `Pushed: ${summary.topic}: ${summary.task_summary}`
        : summary.source === "idle"
          ? "Nothing was captured in the last 5 minutes."
          : "Nothing shareable in the last 5 minutes, so nothing was pushed.",
    );
  } catch (err) {
    note(`Couldn't summarize: ${err}`);
  }
}

/// Searching means you want to be matched: turn Sharing on (locally and on
/// the server) if it's off, so the backend doesn't skip you as "not sharing".
async function ensureSharing() {
  const profile = await loadProfile();
  const trackingOn = await invoke("get_sharing").catch(() => false);
  if (profile?.sharing_enabled && trackingOn) return;
  await setSharing(true);
}

/// The one sharing switch: Thang's set_sharing turns real tracking on/off
/// (tray "Share my activity", app tracker, Chrome capture, summarizer) and
/// saves curr_user_profile.sharing_enabled; then the server is told too.
async function setSharing(on) {
  await invoke("set_sharing", { enabled: on }).catch((err) => console.warn("set_sharing failed", err));
  const profile = await loadProfile();
  if (profile) {
    await updateUser(userId(), { ...profile, sharing_enabled: on }).catch((err) => console.warn("sharing sync failed", err));
  }
}

async function startSession(kind, filters) {
  await ensureSharing().catch((err) => console.warn("couldn't turn sharing on", err));
  // A demo search is friend finding: turn it on so the tray planet animates.
  if (!friendFinding) invoke("set_friend_finding", { enabled: true }).catch(console.error);
  const task = MOCK_TASKS[kind];
  session = {
    kind,
    summary: null, // picked fresh on every push
    topic: task.topic,
    filters,
    nextPushAt: Date.now(),
    pushing: false,
  };
  lastPush = null;
  clearInterval(sessionTimer);
  sessionTimer = setInterval(tickSession, 1000);
  tickSession();
}

/// Stop searching. Stop / Quit also remove the task from the backend.
/// Dev tab "Clear": stop a demo search (friend finding stays as it is), then
/// Rust removes the backend task and the local build-up. Returns what was cleared.
async function clearEverything() {
  if (session) {
    clearInterval(sessionTimer);
    session = null;
    lastPush = { ended: "Cleared from the Dev tab." };
    updateTaskUi();
  }
  return invoke("dev_clear");
}

async function endSession(message) {
  clearInterval(sessionTimer);
  session = null;
  if (friendFinding) invoke("set_friend_finding", { enabled: false }).catch(console.error);
  lastPush = { ended: message };
  try {
    await clearTask(userId());
  } catch (err) {
    lastPush = { error: `Couldn't remove the task: ${err.message}` };
  }
  updateTaskUi();
}

async function tickSession() {
  if (!session) return;
  const now = Date.now();
  if (now >= session.nextPushAt && !session.pushing) {
    const current = session;
    current.pushing = true;
    current.nextPushAt = now + PUSH_EVERY_MS;
    current.summary = randomSummary(current.kind, current.summary);
    try {
      const saved = await pushTask(userId(), current.summary, current.topic, current.filters);
      if (session === current) {
        lastPush = { summary: saved.task_summary, topic: saved.topic, match: saved.match };
        setLatestSummary({
          topic: saved.topic,
          summary: saved.task_summary,
          source: `demo: ${MOCK_TASKS[current.kind].label}`,
          match: saved.match,
        });
      }
    } catch (err) {
      if (session === current) lastPush = { error: `Couldn't push: ${err.message}` };
    } finally {
      current.pushing = false;
    }
  }
  updateTaskUi();
}

// The other person's push can create the match too: show it in the status
// line as soon as it reaches this app (Rust's 3 s poll). Pushing continues.
listen("match-state", (event) => {
  const view = event.payload;
  if (session && lastPush?.match && view.my_status === "pending" && !view.expired) {
    lastPush = {
      ...lastPush,
      match: { match_id: view.match_id, other_name: view.other.name, scores: [{ score: view.score ?? 0 }] },
    };
    updateTaskUi();
  }
});

// Automatic 5-minute summaries from the Chrome capture (api::on_summary in
// Rust) push on their own; show the latest one like a demo push.
listen("activity-pushed", (event) => {
  const saved = event.payload;
  setLatestSummary({ topic: saved.topic, summary: saved.task_summary, source: "from your activity", match: saved.match });
  if (session) return; // a manual demo search is running; its own status line wins
  lastPush = { summary: saved.task_summary, topic: `${saved.topic} (from your activity)`, match: saved.match };
  updateTaskUi();
});

// A real 5-minute summary that wasn't shareable isn't pushed; say so.
listen("activity-summary", (event) => {
  const summary = event.payload;
  if (summary.shareable) return; // activity-pushed covers shared ones
  setLatestSummary({
    skipped:
      summary.source === "idle"
        ? "Your last 5 minutes had no captured activity, so nothing was pushed."
        : "Your last 5 minutes weren't shared (e.g. email or banking, or a detail like a price in the summary).",
  });
});

function formatWait(ms) {
  const seconds = Math.max(0, Math.ceil(ms / 1000));
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;
}

/// Refresh the task section in place (the profile view may have re-rendered).
function updateTaskUi() {
  const { status, matchStatus, countdown, stopButton } = taskUi;
  if (!status) return;
  if (session) {
    countdown.textContent = session.pushing
      ? "Searching… pushing now"
      : `Searching… next push in ${formatWait(session.nextPushAt - Date.now())}`;
    stopButton.hidden = false;
  } else {
    countdown.textContent = "";
    stopButton.hidden = true;
  }
  if (lastPush?.error) {
    status.className = "task-status error";
    status.textContent = lastPush.error;
    matchStatus.textContent = "";
  } else if (lastPush?.ended) {
    status.className = "task-status muted";
    status.textContent = lastPush.ended;
    matchStatus.textContent = "";
  } else if (lastPush) {
    status.className = "task-status";
    status.replaceChildren(el("strong", {}, `${lastPush.topic}: `), lastPush.summary);
    matchStatus.textContent = matchLine(lastPush.match);
  } else if (session) {
    status.className = "task-status muted";
    status.textContent = "Pushing…";
    matchStatus.textContent = "Looking for a match…";
  } else {
    status.className = "task-status muted";
    status.textContent = "Not sharing a task yet.";
    matchStatus.textContent = "";
  }
}

function renderTaskDemo() {
  // Who you can be matched with comes from the Filters tab (saved on the server).
  const buttons = Object.entries(MOCK_TASKS).map(([kind, task]) =>
    el("button", { class: "secondary", onclick: () => startSession(kind, {}) }, task.label),
  );
  taskUi.stopButton = el(
    "button",
    { class: "link", onclick: () => endSession("Stopped searching: your task was removed.") },
    "Stop",
  );
  taskUi.pushNote = el("p", { class: "task-status" });
  taskUi.countdown = el("p", { class: "task-status muted" });
  taskUi.status = el("p", { class: "task-status muted" });
  taskUi.matchStatus = el("p", { class: "task-status muted" });
  const section = el(
    "div",
    { class: "task-demo" },
    el("h3", { class: "section-title" }, "Find someone now"),
    el("span", { class: "label" }, "Pretend to work on a task: a new example every 5 min until Stop (demo)"),
    el(
      "div",
      { class: "task-buttons" },
      ...buttons,
      taskUi.stopButton,
    ),
    taskUi.pushNote,
    taskUi.countdown,
    taskUi.status,
    taskUi.matchStatus,
  );
  updateTaskUi();
  return section;
}

// --- Work friends (connections) --------------------------------------------

/// Latest GET /users/{id}/connections from Rust's 3 s poll; active ones first.
let connections = [];
const friendsUi = { list: null, status: null };

listen("connections", (event) => {
  connections = event.payload ?? [];
  updateFriendsList();
});
invoke("current_connections")
  .then((list) => {
    connections = list ?? [];
    updateFriendsList();
  })
  .catch(() => {});

async function joinFriend(connection) {
  friendsUi.status.textContent = "";
  try {
    await invoke("join_connection", { connectionId: connection.connection_id });
    friendsUi.status.textContent = `Asked ${connection.friend.name} to work together: see the popup.`;
  } catch (err) {
    friendsUi.status.textContent = `Couldn't start: ${err}`;
  }
}

/// Connection id whose Remove was clicked once; a second click removes it.
let confirmingRemove = null;

async function removeFriend(connection) {
  if (confirmingRemove !== connection.connection_id) {
    confirmingRemove = connection.connection_id;
    updateFriendsList();
    return;
  }
  confirmingRemove = null;
  try {
    await deleteConnection(userId(), connection.connection_id);
    connections = connections.filter((other) => other.connection_id !== connection.connection_id);
    friendsUi.status.textContent = `Removed ${connection.friend.name} (for both of you).`;
  } catch (err) {
    friendsUi.status.textContent = `Couldn't remove: ${err.message ?? err}`;
  }
  updateFriendsList();
}

function friendRow(connection) {
  const { friend, topic, active } = connection;
  const confirming = confirmingRemove === connection.connection_id;
  return el(
    "div",
    { class: active ? "friend online" : "friend offline" },
    avatar(friend),
    el(
      "div",
      { class: "friend-info" },
      el("div", { class: "friend-name" }, friend.name),
      el("div", { class: "friend-topic" }, active ? `🟢 On ${topic} now` : `${topic} · offline`),
      active && friend.task_summary ? el("p", { class: "friend-task" }, friend.task_summary) : null,
    ),
    active
      ? el("button", { class: "primary", onclick: () => joinFriend(connection) }, "Work together")
      : null,
    el(
      "button",
      {
        class: confirming ? "secondary danger" : "link",
        title: "Remove this work friend for both of you",
        onclick: () => removeFriend(connection),
      },
      confirming ? "Remove for both?" : "Remove",
    ),
  );
}

/// Refresh the friends list in place on every poll.
function updateFriendsList() {
  const { list } = friendsUi;
  if (!list) return;
  if (!connections.length) {
    list.replaceChildren(
      el(
        "p",
        { class: "muted friends-empty" },
        "No work friends yet. After a match, you both answer “Keep in touch” to add each other.",
      ),
    );
    return;
  }
  const online = connections.filter((connection) => connection.active);
  const offline = connections.filter((connection) => !connection.active);
  list.replaceChildren(
    ...[
      el("span", { class: "label" }, `Online now · ${online.length}`),
      ...(online.length
        ? online.map(friendRow)
        : [el("p", { class: "muted friends-empty" }, "Nobody is on your topics right now.")]),
      offline.length ? el("span", { class: "label" }, `Offline · ${offline.length}`) : null,
      ...offline.map(friendRow),
    ].filter((node) => node != null),
  );
}

function renderFriends() {
  friendsUi.list = el("div", { class: "friends-list" });
  friendsUi.status = el("p", { class: "task-status" });
  updateFriendsList();
  return el(
    "div",
    { class: "friends" },
    el("h3", { class: "section-title" }, "Your work friends"),
    friendsUi.status,
    friendsUi.list,
  );
}

/// Tiny DOM helper. Text always goes through textContent, never innerHTML.
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

function show(...nodes) {
  root.replaceChildren(...nodes.filter((node) => node != null));
}

function renderError(err) {
  show(el("p", { class: "error" }, `Something went wrong: ${err}`));
}

// --- Login (mock) -----------------------------------------------------------

/// Username + password. A returning user (username kept after logout) lands on
/// "Log in" with the name filled in; everyone else starts on "Sign up".
function renderLogin(lastUsername) {
  paintTitlebarUser(null);
  let mode = lastUsername ? "login" : "signup";
  const username = el("input", {
    name: "username",
    autocomplete: "username",
    placeholder: "Username",
    required: "",
    minlength: "3",
    maxlength: "30",
    pattern: "[A-Za-z0-9_.\\-]+",
    title: "Letters, numbers, _ . -",
    autocapitalize: "off",
    spellcheck: "false",
  });
  username.value = lastUsername ?? "";
  const password = el("input", {
    name: "password",
    type: "password",
    placeholder: "Password (6+ characters)",
    required: "",
    minlength: "6",
  });
  const error = el("p", { class: "error" });
  const submit = el("button", { type: "submit", class: "primary" });
  const swap = el("button", { type: "button", class: "link" });
  const heading = el("p", { class: "tagline" });

  const paint = () => {
    password.autocomplete = mode === "login" ? "current-password" : "new-password";
    submit.textContent = mode === "login" ? "Log in" : "Create account";
    swap.textContent = mode === "login" ? "New here? Sign up" : "Have an account? Log in";
    heading.textContent =
      mode === "login" && lastUsername
        ? `Welcome back, ${lastUsername}. Pick up where you left off.`
        : "Find someone working on the same thing, right now.";
  };
  swap.addEventListener("click", () => {
    mode = mode === "login" ? "signup" : "login";
    error.textContent = "";
    paint();
  });
  paint();

  const form = el(
    "form",
    {
      class: "auth",
      onsubmit: async (event) => {
        event.preventDefault();
        submit.disabled = true;
        error.textContent = "";
        const name = username.value.trim();
        try {
          const session = mode === "login" ? await login(name, password.value) : await signup(name, password.value);
          await saveSession(session, name);
          await render();
        } catch (err) {
          error.textContent = err.message.replace(/^\d+ /, "");
          submit.disabled = false;
        }
      },
    },
    username,
    password,
    error,
    submit,
    swap,
  );
  // Skip: a throwaway guest account, so judges can try it in one click. The
  // session is saved like any other, so relaunching continues as the guest.
  const skip = el(
    "button",
    {
      type: "button",
      class: "secondary skip",
      onclick: async () => {
        skip.disabled = true;
        error.textContent = "";
        const guest = `guest-${crypto.randomUUID().slice(0, 8)}`;
        try {
          await saveSession(await signup(guest, crypto.randomUUID()), guest);
          await render();
        } catch (err) {
          error.textContent = err.message.replace(/^\d+ /, "");
          skip.disabled = false;
        }
      },
    },
    "Skip, try it as a guest",
  );
  show(
    el(
      "section",
      { class: "login" },
      el("div", { class: "sticker blue" }, "🌍"),
      el("div", { class: "sticker sun" }, "⚡"),
      el("div", { class: "sticker mint" }, "✓"),
      el("div", { class: "sticker ember" }, "★"),
      el("h1", { class: "display" }, "Work", el("br"), "Match"),
      heading,
      form,
      skip,
    ),
  );
  (lastUsername ? password : username).focus();
}

/// Log out: stop being matchable, stop polling, turn friend finding off.
async function signOut() {
  if (session) session = null;
  await clearTask(userId()).catch(() => {});
  await invoke("stop_match_polling").catch(() => {});
  if (friendFinding) await invoke("set_friend_finding", { enabled: false }).catch(() => {});
  setUserId(null);
  profileSynced = false;
  await logout();
  await render();
}

// --- Profile view -----------------------------------------------------------

function ageLabel(value) {
  return AGE_GROUPS.find(([key]) => key === value)?.[1] ?? null;
}

/// Save the whole profile: on this Mac first so the edit is never lost, then the
/// backend. Sets syncStatus and re-renders.
async function saveAndSync(data) {
  try {
    await saveProfile(data);
    try {
      await updateUser(userId(), data);
      syncStatus = { ok: true, text: "Saved to the server." };
    } catch (err) {
      syncStatus = { ok: false, text: `Saved on this Mac only. Server: ${err.message}` };
    }
    await render();
    syncStatus = null;
  } catch (err) {
    renderError(err);
  }
}

/// <datalist> of school suggestions; inputs use it with list="school-options".
function schoolOptions() {
  return el("datalist", { id: "school-options" }, ...SCHOOLS.map((name) => el("option", { value: name })));
}

function ageSelectFor(value) {
  const select = el("select", { name: "age_group" }, el("option", { value: "" }, "Prefer not to say"));
  for (const [key, label] of AGE_GROUPS) {
    const option = el("option", { value: key }, label);
    option.selected = value === key;
    select.append(option);
  }
  return select;
}

/// Click to edit one field in place. Enter or click away saves, Escape cancels.
function inlineEditor(profile, field, makeInput) {
  return (event) => {
    const target = event.currentTarget;
    if (target.querySelector("input, select")) return;
    const input = makeInput();
    input.classList.add("inline-input");
    let done = false;
    const finish = (save) => {
      if (done) return;
      done = true;
      let value = input.value.trim();
      if (field === "country") value = value.toUpperCase();
      if (!save || value === (profile[field] ?? "")) return renderProfile(profile);
      if (field === "name" && !value) return renderProfile(profile);
      saveAndSync({ ...profile, [field]: value });
    };
    input.addEventListener("keydown", (e) => {
      if (e.key === "Enter") finish(true);
      if (e.key === "Escape") finish(false);
    });
    input.addEventListener("blur", () => finish(true));
    if (input.tagName === "SELECT") input.addEventListener("change", () => finish(true));
    target.replaceChildren(input);
    input.focus();
  };
}

function textInput(profile, field, attrs = {}) {
  return () => el("input", { value: profile[field] ?? "", ...attrs });
}

function detailRow(label, value, onEdit) {
  return el(
    "div",
    { class: onEdit ? "row editable" : "row", title: onEdit ? "Click to edit" : "" },
    el("span", { class: "label" }, label),
    el("span", { class: value ? "value" : "value empty", onclick: onEdit ?? (() => {}) }, value || "Not set"),
  );
}

/// Profile picture if set, otherwise the name's initial.
function avatar(profile) {
  const initial = (profile.name || "?").trim().charAt(0).toUpperCase();
  return el("div", { class: "avatar" }, profile.avatar_url ? el("img", { src: profile.avatar_url, alt: "" }) : initial);
}

/// Top-left of the title bar: who's signed in, so two instances can't be mixed up.
function paintTitlebarUser(profile) {
  const slot = document.getElementById("titlebar-user");
  if (!profile) return slot.replaceChildren();
  const mini = avatar(profile);
  mini.classList.add("mini");
  mini.setAttribute("data-tauri-drag-region", "");
  slot.replaceChildren(mini, el("span", { "data-tauri-drag-region": "" }, profile.name || DEFAULT_NAME));
}

// --- Filters tab ------------------------------------------------------------

/// Who you're willing to be matched with, saved on the server (user_filters).
/// Matching is mutual: a pair only matches if both people's filters allow it.
function renderFiltersTab(profile) {
  const body = el("div", { class: "tab-body filters" }, el("p", { class: "muted" }, "Loading your filters…"));
  getFilters(userId())
    .then((filters) => body.replaceChildren(filtersForm(profile, filters)))
    .catch((err) => body.replaceChildren(el("p", { class: "error" }, `Couldn't load filters: ${err.message}`)));
  return body;
}

function filtersForm(profile, filters) {
  const status = el("p", { class: "task-status" });

  // Age: allowed groups; none checked = anyone.
  const ageBoxes = AGE_GROUPS.map(([key, label]) => {
    const box = el("input", { type: "checkbox", value: key });
    box.checked = filters.age_groups.includes(key);
    return el("label", { class: "chip" }, box, el("span", {}, label));
  });

  // Location: my country quick toggle plus any other ISO codes.
  const myCountry = (profile.country ?? "").toUpperCase();
  const onlyMine = el("input", { type: "checkbox" });
  onlyMine.checked = Boolean(myCountry) && filters.countries.includes(myCountry);
  onlyMine.disabled = !myCountry;
  const otherCountries = el("input", {
    value: filters.countries.filter((country) => country !== myCountry).join(", "),
    placeholder: "Other countries, e.g. US, GB",
    maxlength: "80",
  });

  // Language: only people who share one of my languages.
  const myLanguages = parseLanguages(profile.languages);
  const sharedLanguage = el("input", { type: "checkbox" });
  sharedLanguage.checked = filters.shared_language;

  const save = el(
    "button",
    {
      class: "primary",
      onclick: async () => {
        const countries = otherCountries.value
          .split(",")
          .map((country) => country.trim().toUpperCase())
          .filter(Boolean);
        if (onlyMine.checked && myCountry) countries.unshift(myCountry);
        status.className = "task-status";
        status.textContent = "Saving…";
        try {
          await saveFilters(userId(), {
            age_groups: ageBoxes.map((chip) => chip.querySelector("input")).filter((box) => box.checked).map((box) => box.value),
            countries,
            shared_language: sharedLanguage.checked,
          });
          status.textContent = "Saved. New searches use these filters.";
        } catch (err) {
          status.className = "task-status error";
          status.textContent = `Couldn't save: ${err.message}`;
        }
      },
    },
    "Save filters",
  );

  return el(
    "div",
    { class: "filters-card" },
    el("h3", { class: "section-title" }, "Who you match with"),
    el("p", { class: "muted fine-print" }, "Both people's filters have to allow a match. Leave a section empty for anyone."),
    el("span", { class: "label" }, "Age"),
    el("div", { class: "chips" }, ...ageBoxes),
    el("span", { class: "label" }, "Location"),
    el(
      "label",
      { class: "toggle" },
      onlyMine,
      el("span", {}, myCountry ? `People in my country (${myCountry})` : "People in my country (set your country in Your profile)"),
    ),
    otherCountries,
    el("span", { class: "label" }, "Language"),
    el(
      "label",
      { class: "toggle" },
      sharedLanguage,
      el("span", {}, "Only people who share a language with me"),
    ),
    el(
      "p",
      { class: "muted fine-print" },
      myLanguages.length
        ? `Your languages: ${myLanguages.join(", ")} (edit in Your profile)`
        : "Add your languages in Your profile for this to match anyone.",
    ),
    el("footer", {}, status, save),
  );
}

/// Which main-window tab is showing; kept across re-renders (e.g. after Save).
let activeTab = remember.get("tab") ?? "friends";

function tabBar(profile) {
  const tab = (key, label) =>
    el(
      "button",
      {
        class: activeTab === key ? "tab primary" : "tab secondary",
        onclick: () => {
          activeTab = key;
          remember.set("tab", key);
          renderProfile(profile);
        },
      },
      label,
    );
  return el(
    "nav",
    { class: "tabs" },
    tab("friends", "Work friends"),
    tab("filters", "Filters"),
    tab("profile", "Your profile"),
    import.meta.env.DEV ? tab("dev", "Dev") : null,
  );
}

function renderProfile(profile) {
  paintTitlebarUser(profile);
  show(
    el(
      "section",
      { class: "app" },
      tabBar(profile),
      activeTab === "friends"
        ? renderFriendsTab()
        : activeTab === "filters"
          ? renderFiltersTab(profile)
          : activeTab === "dev"
            ? renderDevTab({ onPushNow: pushNow, onClear: clearEverything, top: renderTaskDemo() })
            : renderProfileTab(profile),
    ),
  );
}

/// Work friends: find someone new, and see / join existing friends.
function renderFriendsTab() {
  queueMicrotask(updateSummaryCard);
  return el(
    "div",
    { class: "tab-body" },
    friendFindingButton(),
    (summaryUi.card = el("div", { class: "summary-card" })),
    renderFriends(),
  );
}

/// Your profile: everything about you, and editing it.
function renderProfileTab(profile) {
  return el(
      "div",
      { class: "tab-body profile" },
      el(
        "header",
        {},
        avatar(profile),
        el("div", { class: "sticker violet" }, "🌍"),
        el(
          "div",
          { class: "who" },
          el(
            "h2",
            { class: "editable", title: "Click to edit", onclick: inlineEditor(profile, "name", textInput(profile, "name", { maxlength: "60" })) },
            profile.name || DEFAULT_NAME,
          ),
          el(
            "p",
            { class: "muted editable", title: "Click to edit", onclick: inlineEditor(profile, "bio", textInput(profile, "bio", { maxlength: "120", placeholder: "One line about you" })) },
            profile.bio || "No bio yet",
          ),
        ),
      ),
      syncStatus ? el("p", { class: `sync ${syncStatus.ok ? "ok" : "fail"}` }, syncStatus.text) : null,
      el(
        "div",
        { class: "details" },
        detailRow("School", profile.school, inlineEditor(profile, "school", textInput(profile, "school", { maxlength: "80", list: "school-options", placeholder: "Search schools" }))),
        detailRow("Career", profile.career, inlineEditor(profile, "career", textInput(profile, "career", { maxlength: "80", placeholder: "e.g. CS student" }))),
        detailRow("Age group", ageLabel(profile.age_group), inlineEditor(profile, "age_group", () => ageSelectFor(profile.age_group))),
        detailRow("Country", profile.country, inlineEditor(profile, "country", textInput(profile, "country", { maxlength: "2", placeholder: "CA" }))),
        detailRow("Discord", profile.discord_handle, inlineEditor(profile, "discord_handle", textInput(profile, "discord_handle", { maxlength: "40" }))),
        detailRow("Languages", parseLanguages(profile.languages).join(", "), inlineEditor(profile, "languages", textInput(profile, "languages", { maxlength: "200", placeholder: "English, Arabic" }))),
        detailRow("Sharing", profile.sharing_enabled ? "On" : "Off", () =>
          setSharing(!profile.sharing_enabled).then(render, renderError),
        ),
        schoolOptions(),
      ),
      el(
        "footer",
        {},
        el(
          "div",
          {},
          el("button", { class: "link", onclick: () => signOut().catch(renderError) }, "Log out"),
          el("button", { class: "link", title: `Quit work-match (${os === "macos" ? "⌘Q" : "Ctrl+Q"})`, onclick: quitApp }, "Quit"),
        ),
        el("button", { class: "primary", onclick: () => renderEdit(profile) }, "Edit"),
      ),
  );
}

// --- Profile edit -----------------------------------------------------------

function textField(label, name, value, attrs = {}) {
  return el(
    "label",
    { class: "field" },
    el("span", {}, label),
    el("input", { name, value: value ?? "", ...attrs }),
  );
}

function renderEdit(profile) {
  const ageSelect = ageSelectFor(profile.age_group);
  const photoInput = el("input", { type: "file", accept: "image/jpeg,image/png,image/webp", hidden: "" });
  photoInput.addEventListener("change", async () => {
    const file = photoInput.files[0];
    if (!file) return;
    try {
      const user = await uploadAvatar(userId(), await resizeImage(file));
      await saveAvatarUrl(user.avatar_url);
      syncStatus = { ok: true, text: "Photo updated." };
    } catch (err) {
      syncStatus = { ok: false, text: `Photo not saved: ${err.message}` };
    }
    await render();
    syncStatus = null;
  });
  const photo = el(
    "div",
    { class: "photo" },
    avatar(profile),
    photoInput,
    el("button", { type: "button", class: "secondary", onclick: () => photoInput.click() }, "Change photo"),
  );

  const sharing = el("input", { type: "checkbox", name: "sharing_enabled" });
  sharing.checked = Boolean(profile.sharing_enabled);

  const form = el(
    "form",
    {
      class: "edit",
      onsubmit: async (event) => {
        event.preventDefault();
        const data = Object.fromEntries(new FormData(form));
        data.sharing_enabled = sharing.checked;
        data.country = (data.country ?? "").toUpperCase();
        await invoke("set_sharing", { enabled: sharing.checked }).catch(console.warn);
        await saveAndSync(data);
      },
    },
    el("h2", {}, "Edit profile"),
    photo,
    textField("Name", "name", profile.name, { required: "", maxlength: "60" }),
    textField("School", "school", profile.school, { maxlength: "80", list: "school-options", placeholder: "Search schools" }),
    schoolOptions(),
    textField("Career", "career", profile.career, { maxlength: "80", placeholder: "e.g. CS student" }),
    textField("Bio", "bio", profile.bio, { maxlength: "120", placeholder: "One line about you" }),
    el("label", { class: "field" }, el("span", {}, "Age group"), ageSelect),
    textField("Country", "country", profile.country, { maxlength: "2", placeholder: "CA" }),
    textField("Discord", "discord_handle", profile.discord_handle, { maxlength: "40" }),
    textField("Languages", "languages", profile.languages, { maxlength: "200", placeholder: "English, Arabic", list: "language-options" }),
    el("datalist", { id: "language-options" }, ...LANGUAGES.map((name) => el("option", { value: name }))),
    el("label", { class: "toggle" }, sharing, el("span", {}, "Share what I'm working on")),
    el(
      "footer",
      {},
      el("button", { type: "button", class: "link", onclick: () => renderProfile(profile) }, "Cancel"),
      el("button", { type: "submit", class: "primary" }, "Save"),
    ),
  );
  show(form);
  form.querySelector("input[name=name]").focus();
}

// --- Entry ------------------------------------------------------------------

/// The local profile is the source of truth: copy it to the server once per
/// app start, so the backend's sharing flag, name and filters inputs (age,
/// country, languages) can't drift from what the app shows.
let profileSynced = false;

// --- Match profile ("Learn more" in the match popup) --------------------------

const PERSON_AGE = Object.fromEntries(AGE_GROUPS);

/// Full profile of the person you just matched with. Read-only; Back returns
/// to your own views.
function renderPerson({ other, topic, score }) {
  if (!other) return;
  show(
    el(
      "section",
      { class: "app" },
      el("div", { class: "tabs" }, el("button", { class: "tab secondary", onclick: render }, "← Back")),
      el(
        "div",
        { class: "tab-body profile person" },
        el(
          "header",
          {},
          avatar(other),
          el("div", { class: "sticker mint" }, "🤝"),
          el(
            "div",
            { class: "who" },
            el("h2", {}, other.name || "Someone"),
            el("p", { class: "muted" }, [other.career, other.school].filter(Boolean).join(" · ") || "No details yet"),
          ),
        ),
        topic
          ? el(
              "div",
              { class: "person-match" },
              el("span", { class: "label" }, "You match on"),
              el("strong", {}, topic),
              score != null ? el("span", { class: "muted" }, `${Math.round(score * 100)}% match`) : null,
            )
          : null,
        other.bio ? el("div", { class: "person-block" }, el("span", { class: "label" }, "Bio"), el("p", {}, other.bio)) : null,
        other.task_summary
          ? el("div", { class: "person-block" }, el("span", { class: "label" }, "Working on right now"), el("p", {}, other.task_summary))
          : null,
        el(
          "div",
          { class: "details" },
          detailRow("School", other.school),
          detailRow("Career", other.career),
          detailRow("Age group", PERSON_AGE[other.age_group]),
          detailRow("Country", other.country),
          detailRow("Discord", other.discord_handle),
        ),
      ),
    ),
  );
}

listen("show-person", (event) => renderPerson(event.payload));

/// Once per sign-in (or app launch while signed in): point everything at this
/// user and pick up where they left off.
function resumeSession(profile) {
  setUserId(profile.user_id);
  invoke("start_match_polling", { userId: profile.user_id, apiUrl: API_URL }).catch(console.error);
  updateUser(profile.user_id, profile).catch((err) => console.warn("profile sync failed", err));
  if (remember.get("friend-finding") === "1" && !friendFinding) {
    invoke("set_friend_finding", { enabled: true }).catch(console.error);
  }
}

async function render() {
  try {
    const profile = await loadProfile();
    // Needs both: sessions from the old mock sign-in have a token but no user_id.
    if (profile?.token && profile.user_id) {
      if (!profileSynced) {
        profileSynced = true;
        resumeSession(profile);
      }
      renderProfile(profile);
    } else {
      renderLogin(profile?.username);
    }
  } catch (err) {
    renderError(err);
  }
}

render();
