import "@fontsource/bowlby-one/400.css";
import "@fontsource/inter/500.css";
import "@fontsource/inter/700.css";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { API_URL, MOCK_USER_ID, clearTask, pushTask, updateUser, uploadAvatar } from "./api.js";
import { MOCK_TASKS, randomSummary } from "./mockTasks.js";
import { SCHOOLS } from "./schools.js";
import { AGE_GROUPS, DEFAULT_NAME, loadProfile, logout, mockLogin, resizeImage, saveAvatarUrl, saveProfile } from "./profile.js";

// Same idea as TyA's getOs(): ask Rust for the target OS instead of sniffing
// the user agent. macOS gets native liquid glass, so the page stays clear;
// everything else paints a CSS semi-transparent background.
const os = await invoke("os_name").catch(() => "unknown");
document.documentElement.classList.add(os === "macos" ? "os-macos" : "os-other");

// Rust polls GET /users/{id}/poll every 3 s and shows the match popup.
invoke("start_match_polling", { userId: MOCK_USER_ID, apiUrl: API_URL }).catch(console.error);

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
    await clearTask(MOCK_USER_ID).catch(() => {});
  }
  invoke("quit_app");
}

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
  paintTitlebarStatus();
  for (const button of friendButtons) {
    if (button.isConnected) paintFriendButton(button);
    else friendButtons.delete(button);
  }
});

// --- Demo task push ---------------------------------------------------------

/// Buttons that push a random mock summary to curr_task, so matching can be
/// tested before the on-device summarizer exists.
function matchLine(match) {
  if (match.match_id != null) return `Matched with ${match.other_name} (${match.scores[0].score.toFixed(2)}). Check the popup.`;
  if (match.skipped_reason) return `No match: ${match.skipped_reason}.`;
  const best = match.scores[0];
  return best ? `No match: best was ${best.name} at ${best.score.toFixed(2)} (needs 0.80).` : "No match.";
}

/// A task search: clicking a task button shares it and keeps searching,
/// re-pushing every PUSH_EVERY_MS (like the real app's 5-minute summaries)
/// until the user presses Stop or a match is found.
const PUSH_EVERY_MS = 5 * 60 * 1000;

let session = null; // { kind, summary, topic, filters, nextPushAt, pushing }
let lastPush = null; // { summary, topic, match } from the latest push, or { error } / { ended }
let sessionTimer = null;
const taskUi = { status: null, matchStatus: null, countdown: null, stopButton: null };

function startSession(kind, filters) {
  const task = MOCK_TASKS[kind];
  session = {
    kind,
    summary: randomSummary(kind),
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

/// Stop searching. Stop / Quit also remove the task from the backend; a match
/// keeps it, so the other person still sees what you're working on.
async function endSession(message, { keepTask = false } = {}) {
  clearInterval(sessionTimer);
  session = null;
  lastPush = { ended: message };
  if (!keepTask) {
    try {
      await clearTask(MOCK_USER_ID);
    } catch (err) {
      lastPush = { error: `Couldn't remove the task: ${err.message}` };
    }
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
    try {
      const saved = await pushTask(MOCK_USER_ID, current.summary, current.topic, current.filters);
      if (session === current) {
        lastPush = { summary: saved.task_summary, topic: saved.topic, match: saved.match };
        if (saved.match.match_id != null) {
          await endSession(`Match found with ${saved.match.other_name}: answer the popup. Searching stopped.`, {
            keepTask: true,
          });
          return;
        }
      }
    } catch (err) {
      if (session === current) lastPush = { error: `Couldn't push: ${err.message}` };
    } finally {
      current.pushing = false;
    }
  }
  updateTaskUi();
}

// The other person's push can create the match too: stop searching as soon
// as a new, unanswered match reaches this app (Rust's 3 s poll).
listen("match-state", (event) => {
  const view = event.payload;
  if (session && view.my_status === "pending" && !view.expired) {
    endSession(`Match found with ${view.other.name}: answer the popup. Searching stopped.`, { keepTask: true });
  }
});

// Automatic 5-minute summaries from the Chrome capture (api::on_summary in
// Rust) push on their own; show the latest one like a demo push.
listen("activity-pushed", (event) => {
  const saved = event.payload;
  if (session) return; // a manual demo search is running; its own status wins
  lastPush = { summary: saved.task_summary, topic: `${saved.topic} (from your activity)`, match: saved.match };
  updateTaskUi();
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
  const sameCountry = el("input", { type: "checkbox" });
  const sameAgeGroup = el("input", { type: "checkbox" });
  const buttons = Object.entries(MOCK_TASKS).map(([kind, task]) =>
    el(
      "button",
      {
        class: "secondary",
        onclick: () =>
          startSession(kind, {
            same_country: sameCountry.checked,
            same_age_group: sameAgeGroup.checked,
          }),
      },
      task.label,
    ),
  );
  taskUi.stopButton = el(
    "button",
    { class: "link", onclick: () => endSession("Stopped searching: your task was removed.") },
    "Stop",
  );
  taskUi.countdown = el("p", { class: "task-status muted" });
  taskUi.status = el("p", { class: "task-status muted" });
  taskUi.matchStatus = el("p", { class: "task-status muted" });
  const section = el(
    "div",
    { class: "task-demo" },
    el("h3", { class: "section-title" }, "Find someone now"),
    el("span", { class: "label" }, "Share a task and search until matched (demo)"),
    el("div", { class: "task-buttons" }, ...buttons, taskUi.stopButton),
    el(
      "div",
      { class: "task-filters" },
      el("label", { class: "toggle" }, sameCountry, el("span", {}, "Only same country")),
      el("label", { class: "toggle" }, sameAgeGroup, el("span", {}, "Only same age group")),
    ),
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

function friendRow(connection) {
  const { friend, topic, active } = connection;
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

function renderLogin() {
  paintTitlebarUser(null);
  const continueButton = el(
    "button",
    {
      class: "primary",
      onclick: async () => {
        continueButton.disabled = true;
        await mockLogin().catch(renderError);
        await render();
      },
    },
    "Continue",
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
      el("p", { class: "tagline" }, "Find someone working on the same thing, right now."),
      continueButton,
      el("p", { class: "fine" }, "Mock sign-in for the demo: no account needed."),
    ),
  );
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
      await updateUser(MOCK_USER_ID, data);
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

/// Which main-window tab is showing; kept across re-renders (e.g. after Save).
let activeTab = "friends";

function tabBar(profile) {
  const tab = (key, label) =>
    el(
      "button",
      {
        class: activeTab === key ? "tab primary" : "tab secondary",
        onclick: () => {
          activeTab = key;
          renderProfile(profile);
        },
      },
      label,
    );
  return el("nav", { class: "tabs" }, tab("friends", "Work friends"), tab("profile", "Your profile"));
}

function renderProfile(profile) {
  paintTitlebarUser(profile);
  show(
    el(
      "section",
      { class: "app" },
      tabBar(profile),
      activeTab === "friends" ? renderFriendsTab() : renderProfileTab(profile),
    ),
  );
}

/// Work friends: find someone new, and see / join existing friends.
function renderFriendsTab() {
  return el(
    "div",
    { class: "tab-body" },
    friendFindingButton(),
    renderTaskDemo(),
    renderFriends(),
    // Demo only: raise the liquid glass match popup with mock data.
    el(
      "button",
      { class: "secondary trigger", onclick: () => invoke("trigger_mock_match") },
      "Trigger match found",
    ),
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
        detailRow("Sharing", profile.sharing_enabled ? "On" : "Off", () =>
          saveAndSync({ ...profile, sharing_enabled: !profile.sharing_enabled }),
        ),
        schoolOptions(),
      ),
      el(
        "footer",
        {},
        el(
          "div",
          {},
          el("button", { class: "link", onclick: () => logout().then(render, renderError) }, "Log out"),
          el("button", { class: "link", title: "Quit work-match (⌘Q)", onclick: quitApp }, "Quit"),
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
      const user = await uploadAvatar(MOCK_USER_ID, await resizeImage(file));
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

async function render() {
  try {
    const profile = await loadProfile();
    if (profile?.token) renderProfile(profile);
    else renderLogin();
  } catch (err) {
    renderError(err);
  }
}

render();
