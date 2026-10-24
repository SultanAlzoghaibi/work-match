// work-match capture: reads the active tab's text and posts it to the
// work-match desktop app on this computer. Nothing is stored in the extension;
// if the app isn't running, the capture is dropped.

const ENDPOINT = "http://127.0.0.1:47321/capture";
const MAX_TEXT = 2000;
const DEBOUNCE_MS = 1500;
const LATE_RENDER_MS = 3000;
const ALARM = "recapture";

// tabId -> time of its last capture. In memory only: it resets when the
// service worker sleeps, which at most allows one extra capture.
const lastCaptureAt = new Map();

chrome.tabs.onActivated.addListener(({ tabId }) => captureTab(tabId));

chrome.tabs.onUpdated.addListener((tabId, changeInfo, tab) => {
  if (changeInfo.status !== "complete" || !tab.active) return;
  captureTab(tabId);
  // Single-page apps often render their content after "complete".
  setTimeout(() => captureTab(tabId), LATE_RENDER_MS);
});

chrome.tabs.onRemoved.addListener((tabId) => lastCaptureAt.delete(tabId));

chrome.windows.onFocusChanged.addListener(async (windowId) => {
  if (windowId === chrome.windows.WINDOW_ID_NONE) return;
  const [tab] = await chrome.tabs.query({ active: true, windowId });
  if (tab) captureTab(tab.id);
});

// Sites like LeetCode change their content without a page load.
chrome.alarms.get(ALARM).then((alarm) => {
  if (!alarm) chrome.alarms.create(ALARM, { periodInMinutes: 1 });
});

chrome.alarms.onAlarm.addListener(async (alarm) => {
  if (alarm.name !== ALARM) return;
  const [tab] = await chrome.tabs.query({ active: true, lastFocusedWindow: true });
  if (tab) captureTab(tab.id);
});

async function captureTab(tabId) {
  const now = Date.now();
  if (now - (lastCaptureAt.get(tabId) ?? 0) < DEBOUNCE_MS) return;
  lastCaptureAt.set(tabId, now);
  try {
    const tab = await chrome.tabs.get(tabId);
    const url = capturableUrl(tab);
    if (!url || !(await windowHasFocus(tab.windowId))) return;

    await chrome.scripting.executeScript({ target: { tabId }, files: ["Readability.js"] });
    const [injection] = await chrome.scripting.executeScript({ target: { tabId }, files: ["extract.js"] });
    const text = String(injection?.result ?? "").slice(0, MAX_TEXT);

    await fetch(ENDPOINT, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ url: url.href, title: tab.title ?? "", text, captured_at: now }),
    });
    console.debug("[work-match]", url.hostname, text.length);
  } catch {
    // Expected: the tab closed, the page can't be scripted (Web Store, PDFs,
    // error pages), or the desktop app isn't running. Drop the capture.
  }
}

// Normal web pages only: no incognito, nothing served from this computer.
// The query string and fragment never leave the browser.
function capturableUrl(tab) {
  if (!tab.active || tab.incognito || !tab.url) return null;
  let url;
  try {
    url = new URL(tab.url);
  } catch {
    return null;
  }
  if (url.protocol !== "http:" && url.protocol !== "https:") return null;
  if (["localhost", "127.0.0.1", "[::1]"].includes(url.hostname)) return null;
  url.search = "";
  url.hash = "";
  return url;
}

// Without this check the minute alarm would keep reporting a background tab
// while the user works in another app.
async function windowHasFocus(windowId) {
  const win = await chrome.windows.get(windowId);
  return win.focused;
}
