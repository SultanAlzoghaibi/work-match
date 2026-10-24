// work-match FastAPI backend (backend/fastapi-sultan/app/main.py).
// Override with VITE_API_URL, e.g. VITE_API_URL=https://api.example.com yarn tauri dev
// A trailing "/" is dropped: "…app/" + "/users" would hit "//users", which
// Vercel redirects (308) and breaks POST / PATCH from the app.
export const API_URL = (import.meta.env.VITE_API_URL ?? "http://127.0.0.1:8000").replace(/\/+$/, "");

/// The signed-in user's backend id (users.id). Set from the saved session on
/// launch, or by sign-up / log-in; null while signed out.
let currentUserId = null;
export const userId = () => currentUserId;
export function setUserId(id) {
  currentUserId = id;
}

async function request(method, path, body) {
  const response = await fetch(`${API_URL}${path}`, {
    method,
    headers: body ? { "Content-Type": "application/json" } : undefined,
    body: body ? JSON.stringify(body) : undefined,
  });
  if (!response.ok) {
    const detail = await response.json().then((data) => data.detail, () => null);
    throw new Error(`${response.status} ${typeof detail === "string" ? detail : response.statusText}`);
  }
  return response.status === 204 ? null : response.json();
}

/// PATCH /users/{id}: send the whole editable profile; empty strings clear a field.
export function updateUser(userId, profile) {
  const blankToNull = (value) => {
    const trimmed = (value ?? "").trim();
    return trimmed === "" ? null : trimmed;
  };
  return request("PATCH", `/users/${userId}`, {
    name: blankToNull(profile.name),
    school: blankToNull(profile.school),
    career: blankToNull(profile.career),
    bio: blankToNull(profile.bio),
    discord_handle: blankToNull(profile.discord_handle),
    age_group: blankToNull(profile.age_group),
    country: blankToNull(profile.country),
    languages: (profile.languages ?? "")
      .split(",")
      .map((language) => language.trim())
      .filter(Boolean),
    sharing_enabled: Boolean(profile.sharing_enabled),
  });
}

/// Match filters (Filters tab). Matching is mutual: both people's filters
/// must allow the pair. Empty lists mean "anyone".
export function getFilters(userId) {
  return request("GET", `/users/${userId}/filters`);
}

export function saveFilters(userId, filters) {
  return request("PUT", `/users/${userId}/filters`, filters);
}

/// POST /users/{id}/avatar: upload a profile picture (a JPEG blob). Returns the
/// updated user, whose avatar_url points at Supabase Storage.
export async function uploadAvatar(userId, blob) {
  const form = new FormData();
  form.append("file", blob, "avatar.jpg");
  const response = await fetch(`${API_URL}/users/${userId}/avatar`, { method: "POST", body: form });
  if (!response.ok) {
    const detail = await response.json().then((data) => data.detail, () => null);
    throw new Error(`${response.status} ${typeof detail === "string" ? detail : response.statusText}`);
  }
  return response.json();
}

/// POST /users/{id}/push: overwrite what this user is working on (curr_task),
/// then the backend runs the Jev match search. Returns { ..., match }.
export function pushTask(userId, taskSummary, topic, filters = {}) {
  return request("POST", `/users/${userId}/push`, { task_summary: taskSummary, topic, filters });
}

/// DELETE /users/{id}/task: stop being matchable (curr_task removed).
export function clearTask(userId) {
  return request("DELETE", `/users/${userId}/task`);
}

/// GET /users/{id}/task-history: past pushes, newest first (Dev tab).
export function getTaskHistory(userId, limit = 20) {
  return request("GET", `/users/${userId}/task-history?limit=${limit}`);
}

/// Remove a work friend: deletes the connection for both people.
export function deleteConnection(userId, connectionId) {
  return request("DELETE", `/users/${userId}/connections/${connectionId}`);
}

/// POST /auth/signup: new account. Returns { user, token }; 409 = name taken.
export function signup(username, password) {
  return request("POST", "/auth/signup", { username, password });
}

/// POST /auth/login. Returns { user, token }; 401 = wrong username or password.
export function login(username, password) {
  return request("POST", "/auth/login", { username, password });
}
