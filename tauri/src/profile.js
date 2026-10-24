import { getDb } from "./db.js";
import { MOCK_USER_ID } from "./api.js";

/// Demo names: instance 1 (yarn dev1) is sultan1, instance 2 (yarn dev2) is James2.
export const DEFAULT_NAME = MOCK_USER_ID.endsWith("1") ? "James2" : "sultan1";

// The single curr_user_profile row (id = 1). token NULL = signed out.
export const PROFILE_FIELDS = ["name", "school", "career", "bio", "age_group", "country", "discord_handle", "languages"];

/// Suggestions for the Languages field (any language can be typed).
export const LANGUAGES = [
  "English", "Arabic", "Spanish", "French", "Mandarin", "Hindi", "Urdu", "Punjabi",
  "Portuguese", "Russian", "Japanese", "Korean", "German", "Italian", "Vietnamese",
  "Tagalog", "Persian", "Turkish", "Bengali", "Cantonese",
];

/// "English, Arabic" (stored text) <-> ["English", "Arabic"].
export function parseLanguages(text) {
  return [...new Set((text ?? "").split(",").map((language) => language.trim()).filter(Boolean))];
}

export const AGE_GROUPS = [
  ["under_18", "Under 18"],
  ["18_24", "18–24"],
  ["25_34", "25–34"],
  ["35_44", "35–44"],
  ["45_plus", "45+"],
];

export async function loadProfile() {
  const db = await getDb();
  // Rename profiles left over from before the demo names existed.
  await db.execute("UPDATE curr_user_profile SET name = $1 WHERE id = 1 AND name LIKE 'New User%'", [DEFAULT_NAME]);
  const rows = await db.select("SELECT * FROM curr_user_profile WHERE id = 1");
  return rows[0] ?? null;
}

/// Mock sign-in: always succeeds. Hands out a fake token and keeps any
/// profile already on this device. Swap for POST /users once the backend exists.
export async function mockLogin() {
  const db = await getDb();
  const token = `mock-${crypto.randomUUID()}`;
  await db.execute(
    `INSERT INTO curr_user_profile (id, token, name) VALUES (1, $1, $2)
     ON CONFLICT (id) DO UPDATE SET token = $1,
       name = CASE WHEN name IS NULL OR name LIKE 'New User%' THEN $2 ELSE name END`,
    [token, DEFAULT_NAME],
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
     SET name = $1, school = $2, career = $3, bio = $4, age_group = $5, country = $6,
         discord_handle = $7, languages = $8, sharing_enabled = $9
     WHERE id = 1`,
    [...values, profile.sharing_enabled ? 1 : 0],
  );
}

export async function saveAvatarUrl(url) {
  const db = await getDb();
  await db.execute("UPDATE curr_user_profile SET avatar_url = $1 WHERE id = 1", [url]);
}

/// Center-crop and resize an image file to a size×size JPEG blob.
export async function resizeImage(file, size = 256) {
  const bitmap = await createImageBitmap(file);
  const side = Math.min(bitmap.width, bitmap.height);
  const canvas = document.createElement("canvas");
  canvas.width = canvas.height = size;
  canvas
    .getContext("2d")
    .drawImage(bitmap, (bitmap.width - side) / 2, (bitmap.height - side) / 2, side, side, 0, 0, size, size);
  bitmap.close();
  return new Promise((resolve, reject) =>
    canvas.toBlob((blob) => (blob ? resolve(blob) : reject(new Error("could not read image"))), "image/jpeg", 0.85),
  );
}
