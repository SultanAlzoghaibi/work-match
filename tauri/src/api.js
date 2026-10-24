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
  return response.json();
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
    bio: blankToNull(profile.bio),
    discord_handle: blankToNull(profile.discord_handle),
    age_group: blankToNull(profile.age_group),
    country: blankToNull(profile.country),
    sharing_enabled: Boolean(profile.sharing_enabled),
  });
}
