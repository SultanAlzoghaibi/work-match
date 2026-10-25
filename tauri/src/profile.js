import { getDb } from "./db.js";

/// Shown when a profile has no name yet.
export const DEFAULT_NAME = "New User";

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
  const rows = await db.select("SELECT * FROM curr_user_profile WHERE id = 1");
  return rows[0] ?? null;
}

/// After sign-up / log-in: remember the session on this Mac and take the
/// profile from the server, so switching accounts never mixes two people.
export async function saveSession({ user, token }, username) {
  const db = await getDb();
  await db.execute(
    `INSERT OR IGNORE INTO curr_user_profile (id) VALUES (1)`,
  );
  await db.execute(
    `UPDATE curr_user_profile SET token = $1, user_id = $2, username = $3, name = $4, school = $5,
       career = $6, bio = $7, age_group = $8, country = $9, discord_handle = $10, avatar_url = $11,
       languages = $12, sharing_enabled = $13
     WHERE id = 1`,
    [
      token, user.id, username.toLowerCase(), user.name, user.school, user.career, user.bio,
      user.age_group, user.country, user.discord_handle, user.avatar_url,
      (user.languages ?? []).join(", ") || null, 1,
    ],
  );
}

/// Sign out on this Mac. username stays so the login form can say "welcome back".
export async function logout() {
  const db = await getDb();
  await db.execute("UPDATE curr_user_profile SET token = NULL, user_id = NULL WHERE id = 1");
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
    [...values, 1], // sharing is always on
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
