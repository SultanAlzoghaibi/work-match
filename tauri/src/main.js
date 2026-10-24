import { invoke } from "@tauri-apps/api/core";
import { MOCK_USER_ID, updateUser } from "./api.js";
import { AGE_GROUPS, loadProfile, logout, mockLogin, saveProfile } from "./profile.js";

// Same idea as TyA's getOs(): ask Rust for the target OS instead of sniffing
// the user agent. macOS gets native liquid glass, so the page stays clear;
// everything else paints a CSS semi-transparent background.
const os = await invoke("os_name").catch(() => "unknown");
document.documentElement.classList.add(os === "macos" ? "os-macos" : "os-other");

const root = document.getElementById("root");

/// Result of the last Save, shown once on the profile view.
let syncStatus = null;

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
  root.replaceChildren(...nodes);
}

function renderError(err) {
  show(el("p", { class: "error" }, `Something went wrong: ${err}`));
}

// --- Login (mock) -----------------------------------------------------------

function renderLogin() {
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
      el("div", { class: "logo" }, "🌍"),
      el("h1", {}, "work-match"),
      el("p", { class: "muted" }, "Find someone working on the same thing, right now."),
      continueButton,
      el("p", { class: "fine" }, "Mock sign-in for the demo: no account needed."),
    ),
  );
}

// --- Profile view -----------------------------------------------------------

function ageLabel(value) {
  return AGE_GROUPS.find(([key]) => key === value)?.[1] ?? null;
}

function detailRow(label, value) {
  return el(
    "div",
    { class: "row" },
    el("span", { class: "label" }, label),
    el("span", { class: value ? "value" : "value empty" }, value || "Not set"),
  );
}

function renderProfile(profile) {
  const initial = (profile.name || "?").trim().charAt(0).toUpperCase();
  show(
    el(
      "section",
      { class: "profile" },
      el(
        "header",
        {},
        el("div", { class: "avatar" }, initial),
        el(
          "div",
          { class: "who" },
          el("h2", {}, profile.name || "New User"),
          el("p", { class: "muted" }, profile.bio || "No bio yet"),
        ),
      ),
      syncStatus ? el("p", { class: `sync ${syncStatus.ok ? "ok" : "fail"}` }, syncStatus.text) : null,
      el(
        "div",
        { class: "details" },
        detailRow("School", profile.school),
        detailRow("Age group", ageLabel(profile.age_group)),
        detailRow("Country", profile.country),
        detailRow("Discord", profile.discord_handle),
        detailRow("Sharing", profile.sharing_enabled ? "On" : "Off"),
      ),
      // Demo only: raise the liquid glass match popup with mock data.
      el(
        "button",
        { class: "secondary trigger", onclick: () => invoke("trigger_mock_match") },
        "Trigger match found",
      ),
      el(
        "footer",
        {},
        el("button", { class: "link", onclick: () => logout().then(render, renderError) }, "Log out"),
        el("button", { class: "primary", onclick: () => renderEdit(profile) }, "Edit"),
      ),
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
  const ageSelect = el(
    "select",
    { name: "age_group" },
    el("option", { value: "" }, "Prefer not to say"),
    ...AGE_GROUPS.map(([value, label]) => {
      const option = el("option", { value }, label);
      option.selected = profile.age_group === value;
      return option;
    }),
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
        try {
          // Local first so the edit is never lost, then the backend.
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
      },
    },
    el("h2", {}, "Edit profile"),
    textField("Name", "name", profile.name, { required: "", maxlength: "60" }),
    textField("School", "school", profile.school, { maxlength: "80" }),
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
