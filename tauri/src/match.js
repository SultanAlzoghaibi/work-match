import "@fontsource/bowlby-one/400.css";
import "@fontsource/inter/500.css";
import "@fontsource/inter/700.css";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

// Liquid glass popup for a match. Rust polls the backend every 3 s and emits
// "match-state" whenever the match changes; we also ask for it on load in case
// the event fired first. The server decides expiry (MatchView.expired).

const AGE_LABELS = {
  under_18: "Under 18",
  "18_24": "18–24",
  "25_34": "25–34",
  "35_44": "35–44",
  "45_plus": "45+",
};

const popup = document.getElementById("popup");

// match.css tints the popup on Windows, where there's no Liquid Glass.
const os = await invoke("os_name").catch(() => "unknown");
document.documentElement.classList.add(`os-${os}`);

// Drag the popup by anything that isn't a button.
popup.addEventListener("mousedown", (event) => {
  if (event.button !== 0 || event.target.closest("button")) return;
  getCurrentWindow().startDragging();
});

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

/// The other person's attributes, shown once the match is decided.
function personCard(other) {
  const rows = [
    ["School", other.school],
    ["Career", other.career],
    ["Bio", other.bio],
    ["Age", AGE_LABELS[other.age_group]],
    ["Country", other.country],
    ["Discord", other.discord_handle],
  ].filter(([, value]) => value);
  return el(
    "div",
    { class: "card" },
    el(
      "div",
      { class: "card-name" },
      other.avatar_url ? el("img", { class: "card-avatar", src: other.avatar_url, alt: "" }) : null,
      other.name,
    ),
    ...rows.map(([label, value]) =>
      el("div", { class: "card-row" }, el("span", {}, label), el("span", {}, value)),
    ),
  );
}

/// "Join call" once both accepted (matches.room_id). The app already opened
/// the call in the browser; this reopens it if the tab was closed.
function callButton(view) {
  if (!view.room_id) return null;
  return el(
    "button",
    { class: "call", onclick: () => invoke("open_call", { matchId: view.match_id }).catch(console.error) },
    "📞 Join call",
  );
}

function closeButton(view) {
  return el(
    "div",
    { class: "actions" },
    callButton(view),
    el("button", { class: "primary", onclick: () => invoke("dismiss_match", { matchId: view.match_id }) }, "Close"),
  );
}

/// Like replaceChildren, but skips null (optional lines), which the DOM would
/// otherwise render as the text "null".
function show(...nodes) {
  popup.replaceChildren(...nodes.filter((node) => node != null));
}

/// Both accepted. A match with a stranger then asks "keep in touch?"; both
/// yes makes you work friends. A match started from a friend is already
/// connected, so it skips the question.
function renderAccepted(view) {
  const { other } = view;
  const name = other.name;
  const topic = el("strong", {}, view.topic);

  if (view.connected && view.my_keep && view.their_keep) {
    show(
      el("div", { class: "eyebrow" }, "🤝 Work friends"),
      el("p", { class: "intro" }, "You and ", el("strong", {}, name), " are connected on ", topic, "."),
      el("p", { class: "meta" }, "You'll see them in Work friends whenever they're on it."),
      personCard(other),
      closeButton(view),
    );
  } else if (view.connected) {
    show(
      el("div", { class: "eyebrow" }, "🎉 Let's go"),
      el("p", { class: "intro" }, "You and ", el("strong", {}, name), " are working on ", topic, " together."),
      personCard(other),
      closeButton(view),
    );
  } else if (view.my_keep === false || view.their_keep === false) {
    show(
      el("div", { class: "eyebrow" }, "Thanks for working together"),
      el(
        "p",
        { class: "intro" },
        view.my_keep === false ? "No connection this time." : `${name} chose not to connect this time.`,
      ),
      closeButton(view),
    );
  } else if (view.my_keep) {
    show(
      el("div", { class: "eyebrow" }, "🎉 It's a match"),
      el("p", { class: "intro" }, "Waiting for ", el("strong", {}, name), " to say if they want to keep in touch…"),
      personCard(other),
      view.room_id ? el("div", { class: "actions" }, callButton(view)) : null,
    );
  } else {
    const error = el("p", { class: "error" });
    const keep = async (answer) => {
      error.textContent = "";
      try {
        await invoke("keep_match", { matchId: view.match_id, keep: answer });
      } catch (err) {
        error.textContent = String(err);
      }
    };
    show(
      el("div", { class: "eyebrow" }, "🎉 It's a match"),
      el("p", { class: "intro" }, "You both accepted! Keep in touch with ", el("strong", {}, name), " on ", topic, "?"),
      el("p", { class: "meta" }, "If you both say yes, you'll see when they're working on it."),
      view.room_id
        ? el("div", { class: "call-row" }, callButton(view), el("span", { class: "meta" }, "Opened in your browser."))
        : null,
      error,
      el(
        "div",
        { class: "actions" },
        el("button", { class: "secondary", onclick: () => keep(false) }, "Not now"),
        el("button", { class: "primary", onclick: () => keep(true) }, "Keep in touch"),
      ),
    );
  }
}

function render(view) {
  if (!view) {
    show();
    return;
  }
  const { other } = view;
  const name = other.name;

  if (view.my_status === "accepted" && view.their_status === "accepted") {
    renderAccepted(view);
  } else if (view.their_status === "declined") {
    show(
      el("div", { class: "eyebrow" }, "Match declined"),
      el("p", { class: "intro" }, el("strong", {}, name), " declined this time."),
      personCard(other),
      closeButton(view),
    );
  } else if (view.my_status === "declined") {
    show(
      el("div", { class: "eyebrow" }, "Match declined"),
      el("p", { class: "intro" }, "You passed on ", el("strong", {}, name), "."),
      personCard(other),
      closeButton(view),
    );
  } else if (view.expired) {
    show(
      el("div", { class: "eyebrow" }, "Match expired"),
      el("p", { class: "intro" }, "This match with ", el("strong", {}, name), " expired."),
      closeButton(view),
    );
  } else if (view.my_status === "accepted") {
    show(
      el("div", { class: "eyebrow" }, "🌍 Match found"),
      el("p", { class: "intro" }, "Waiting for ", el("strong", {}, name), " to answer…"),
      el("p", { class: "task" }, other.task_summary ?? ""),
    );
  } else {
    const error = el("p", { class: "error" });
    const respond = async (accept) => {
      error.textContent = "";
      try {
        await invoke("respond_match", { matchId: view.match_id, accept });
      } catch (err) {
        error.textContent = String(err);
      }
    };
    show(
      el("div", { class: "eyebrow big" }, "You guys match!"),
      el("p", { class: "intro" }, "Both working on ", el("strong", {}, view.topic), " right now."),
      view.their_status === "accepted" ? el("p", { class: "meta" }, `${name} already said yes.`) : null,
      error,
      el(
        "div",
        { class: "choice" },
        el("button", { class: "round no", title: "Not now", onclick: () => respond(false) }, "✕"),
        el("button", { class: "round yes", title: "Work together", onclick: () => respond(true) }, "✓"),
      ),
      profilePreview(other),
    );
  }
}

/// Compact profile under the ✕/✓ choice; "Learn more" opens the full one in
/// the main window.
function profilePreview(other) {
  const initial = (other.name || "?").trim().charAt(0).toUpperCase();
  const sub = [other.career, other.school].filter(Boolean).join(" · ");
  return el(
    "div",
    { class: "card preview" },
    el(
      "div",
      { class: "preview-head" },
      other.avatar_url
        ? el("img", { class: "card-avatar", src: other.avatar_url, alt: "" })
        : el("div", { class: "card-avatar initial" }, initial),
      el("div", {}, el("div", { class: "card-name" }, other.name), sub ? el("div", { class: "meta" }, sub) : null),
    ),
    other.bio ? el("p", { class: "bio" }, other.bio) : null,
    el(
      "button",
      { class: "link", onclick: () => invoke("open_match_profile").catch(console.error) },
      "Learn more →",
    ),
  );
}

await listen("match-state", (event) => render(event.payload));
render(await invoke("current_match"));
