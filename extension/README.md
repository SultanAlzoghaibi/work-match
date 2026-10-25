# work-match capture (Chrome extension)

Reads the readable text of the tab you're looking at and posts it to the work-match
desktop app on the same computer (`http://127.0.0.1:47321/capture`). The desktop app
keeps it for about 5 minutes, summarizes it with the local Llama model, then deletes it.
See [docs/activity-capture.md](../docs/activity-capture.md) for the whole flow.

## What it captures

Nothing, unless the desktop app is running with **friend finding** on. Before each
capture it asks the app (`POST http://127.0.0.1:47321/status`, answered
`{"tracking": true|false}`) and reads no page unless the answer is `true`. Turning
friend finding off, or quitting the app, stops it right away.

While friend finding is on: the active tab of the focused Chrome window, when you:

- switch tabs or windows,
- finish loading a page (plus once more 3 s later, for pages that render late),
- stay on a page (every minute, for single-page apps like LeetCode).

Each tab is captured at most once every 1.5 s. Text comes from
[Readability](https://github.com/mozilla/readability) (falling back to the page's
visible text) and is cut to 2000 characters. The URL is sent without its query string
or fragment.

It skips:

- incognito tabs (the manifest sets `"incognito": "not_allowed"`, and the code also
  checks `tab.incognito`),
- anything that isn't `http:`/`https:` (`chrome://`, `file://`, extension pages),
- pages served from this computer (`localhost`, `127.0.0.1`),
- pages Chrome won't let extensions script (the Web Store, PDFs, error pages),
- everything while Chrome isn't the focused app.

The extension stores nothing. If the desktop app isn't running, the status check fails
and no page is read.
It logs only a page's host and text length (`console.debug`).

## Load it

1. Open `chrome://extensions` and turn on **Developer mode**.
2. Click **Load unpacked** and select this `extension/` folder.
3. Check the ID: it must be **`cifmncfppiahnojgipabfaaifahknecg`**.

The desktop app only accepts requests whose `Origin` is
`chrome-extension://cifmncfppiahnojgipabfaaifahknecg`. To use another ID, set
`ALLOWED_EXTENSION_ID` before starting the app.

## Why the ID is stable

An unpacked extension's ID normally depends on its folder path. The `"key"` in
`manifest.json` pins it: the ID is derived from that public key, so it's the same on
every machine. The keypair was generated with OpenSSL:

```sh
openssl genrsa 2048 | openssl pkcs8 -topk8 -nocrypt -out key.pem
# "key" in manifest.json:
openssl rsa -in key.pem -pubout -outform DER | openssl base64 -A
# Extension ID (Git Bash: use sha256sum instead of shasum -a 256):
openssl rsa -in key.pem -pubout -outform DER | shasum -a 256 | head -c32 | tr 0-9a-f a-p
```

The private key (`key.pem`) is **not** in the repo, and `*.pem` is gitignored. Loading
unpacked only needs the public key. You'd need the private key to pack a `.crx`; if it's
lost, generate a new pair and update both `"key"` and `DEFAULT_EXTENSION_ID` in
`tauri/src-tauri/src/capture_server.rs`.

## Files

- `manifest.json`: MV3 manifest (`tabs`, `scripting`, `alarms`; host access to all URLs).
- `background.js`: the service worker: triggers, filtering, the status check, injection, POST.
- `extract.js`: runs in the page and returns the text.
- `Readability.js`: copied unchanged from `@mozilla/readability` 0.6.0 (Apache-2.0;
  license header kept). To update it, run `npm i @mozilla/readability` in a temp folder
  and copy `node_modules/@mozilla/readability/Readability.js` here. The file has only a
  top-level `function Readability`, so injecting it again on every capture is safe.
