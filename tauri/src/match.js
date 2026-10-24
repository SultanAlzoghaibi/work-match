import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

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
    ["Bio", other.bio],
    ["Age", AGE_LABELS[other.age_group]],
    ["Country", other.country],
    ["Discord", other.discord_handle],
  ].filter(([, value]) => value);
  return el(
    "div",
    { class: "card" },
    el("div", { class: "card-name" }, other.name),
    ...rows.map(([label, value]) =>
      el("div", { class: "card-row" }, el("span", {}, label), el("span", {}, value)),
    ),
  );
}

function closeButton(view) {
  return el(
    "div",
    { class: "actions" },
    el("button", { class: "primary", onclick: () => invoke("dismiss_match", { matchId: view.match_id }) }, "Close"),
  );
}

function render(view) {
  if (!view) {
    popup.replaceChildren();
    return;
  }
  const { other } = view;
  const name = other.name;

  if (view.my_status === "accepted" && view.their_status === "accepted") {
    popup.replaceChildren(
      el("div", { class: "eyebrow" }, "🎉 It's a match"),
      el("p", { class: "intro" }, "You both accepted! Time to work together on ", el("strong", {}, view.topic), "."),
      personCard(other),
      closeButton(view),
    );
  } else if (view.their_status === "declined") {
    popup.replaceChildren(
      el("div", { class: "eyebrow" }, "Match declined"),
      el("p", { class: "intro" }, el("strong", {}, name), " declined this time."),
      personCard(other),
      closeButton(view),
    );
  } else if (view.my_status === "declined") {
    popup.replaceChildren(
      el("div", { class: "eyebrow" }, "Match declined"),
      el("p", { class: "intro" }, "You passed on ", el("strong", {}, name), "."),
      personCard(other),
      closeButton(view),
    );
  } else if (view.expired) {
    popup.replaceChildren(
      el("div", { class: "eyebrow" }, "Match expired"),
      el("p", { class: "intro" }, "This match with ", el("strong", {}, name), " expired."),
      closeButton(view),
    );
  } else if (view.my_status === "accepted") {
    popup.replaceChildren(
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
    const meta = [other.school, other.bio].filter(Boolean).join(" · ");
    popup.replaceChildren(
      el("div", { class: "eyebrow" }, "🌍 Match found"),
      el(
        "p",
        { class: "intro" },
        el("strong", {}, name),
        ` is also working on ${view.topic} right now. Want to work alongside them?`,
      ),
      meta ? el("p", { class: "meta" }, meta) : null,
      el("p", { class: "task" }, other.task_summary ?? ""),
      view.their_status === "accepted" ? el("p", { class: "meta" }, `${name} already accepted.`) : null,
      error,
      el(
        "div",
        { class: "actions" },
        el("button", { class: "secondary", onclick: () => respond(false) }, "Not now"),
        el("button", { class: "primary", onclick: () => respond(true) }, "Work together"),
      ),
    );
  }
}

await listen("match-state", (event) => render(event.payload));
render(await invoke("current_match"));
