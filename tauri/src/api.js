// work-match FastAPI backend (backend/fastapi-sultan/app/main.py).
// Override with VITE_API_URL, e.g. VITE_API_URL=https://api.example.com yarn tauri dev
export const API_URL = import.meta.env.VITE_API_URL ?? "http://127.0.0.1:8000";

/// Hard-coded user until real sign-up exists. Seeded by backend/seed.sql.
/// Instance 2 (yarn dev2) sets VITE_USER_ID to ...0001 so the two apps are
/// two different people.
export const MOCK_USER_ID = import.meta.env.VITE_USER_ID ?? "00000000-0000-0000-0000-000000000000";

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
    sharing_enabled: Boolean(profile.sharing_enabled),
  });
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
