import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

// Liquid glass popup for an incoming match. Rust stores the offer and emits
// "match-offer"; we also ask for it on load in case the event fired first.

/// Matches expire after 2 min (see docs/DATABASE.md).
const EXPIRES_MS = 2 * 60 * 1000;

const popup = document.getElementById("popup");
let expiryTimer = null;

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

function respond(offer, accept) {
  clearTimeout(expiryTimer);
  invoke("respond_match", { matchId: offer.match_id, accept });
}

function render(offer) {
  clearTimeout(expiryTimer);
  if (!offer) {
    popup.replaceChildren();
    return;
  }
  const meta = [offer.school, offer.bio].filter(Boolean).join(" · ");
  popup.replaceChildren(
    el("div", { class: "eyebrow" }, "🌍 Match found"),
    el(
      "p",
      { class: "intro" },
      el("strong", {}, offer.other_name),
      ` is also working on ${offer.topic} right now. Want to work alongside them?`,
    ),
    meta ? el("p", { class: "meta" }, meta) : null,
    el("p", { class: "task" }, offer.task_summary),
    el(
      "div",
      { class: "actions" },
      el("button", { class: "secondary", onclick: () => respond(offer, false) }, "Not now"),
      el("button", { class: "primary", onclick: () => respond(offer, true) }, "Work together"),
    ),
  );
  // Nobody answered: close quietly, same as "Not now" on the backend side.
  expiryTimer = setTimeout(() => respond(offer, false), EXPIRES_MS);
}

await listen("match-offer", (event) => render(event.payload));
render(await invoke("current_match"));
