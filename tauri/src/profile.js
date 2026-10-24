import { getDb } from "./db.js";

// The single curr_user_profile row (id = 1). token NULL = signed out.
export const PROFILE_FIELDS = ["name", "school", "bio", "age_group", "country", "discord_handle"];

export const AGE_GROUPS = [
  ["under_18", "Under 18"],
  ["18_24", "18–24"],
  ["25_34", "25–34"],
  ["35_44", "35–44"],
  ["45_plus", "45+"],
];

export async function loadProfile() {
  const db = await getDb();
  const rows = await db.select("SELECT * FROM curr_user_profile WHERE id = 1");
  return rows[0] ?? null;
}

/// Mock sign-in: always succeeds. Hands out a fake token and keeps any
/// profile already on this device. Swap for POST /users once the backend exists.
export async function mockLogin() {
  const db = await getDb();
  const token = `mock-${crypto.randomUUID()}`;
  await db.execute(
    `INSERT INTO curr_user_profile (id, token, name) VALUES (1, $1, 'New User')
     ON CONFLICT (id) DO UPDATE SET token = $1, name = COALESCE(name, 'New User')`,
    [token],
  );
}

export async function logout() {
  const db = await getDb();
  await db.execute("UPDATE curr_user_profile SET token = NULL WHERE id = 1");
}

export async function saveProfile(profile) {
  const db = await getDb();
  const values = PROFILE_FIELDS.map((field) => {
    const value = (profile[field] ?? "").trim();
    return value === "" ? null : value;
  });
  await db.execute(
    `UPDATE curr_user_profile
     SET name = $1, school = $2, bio = $3, age_group = $4, country = $5,
         discord_handle = $6, sharing_enabled = $7
     WHERE id = 1`,
    [...values, profile.sharing_enabled ? 1 : 0],
  );
}
