import os
import time
from uuid import UUID

import httpx

BUCKET = os.environ.get("SUPABASE_BUCKET", "blob")


def _settings() -> tuple[str, str]:
    url = os.environ.get("SUPABASE_URL", "").rstrip("/")
    key = os.environ.get("SUPABASE_SERVICE_ROLE_KEY", "")
    if not url or not key:
        raise RuntimeError("SUPABASE_URL / SUPABASE_SERVICE_ROLE_KEY are not set (see .env.example)")
    return url, key


def upload_avatar(user_id: UUID, data: bytes, content_type: str) -> str:
    """Upload (or replace) {user_id}.jpg in the public avatars bucket.

    Returns the public URL with ?v=<time>, so the app doesn't show a cached old picture.
    """
    url, key = _settings()
    path = f"{user_id}.jpg"
    response = httpx.post(
        f"{url}/storage/v1/object/{BUCKET}/{path}",
        content=data,
        headers={
            "Authorization": f"Bearer {key}",
            "apikey": key,
            "Content-Type": content_type,
            "x-upsert": "true",
        },
        timeout=20,
    )
    response.raise_for_status()
    return f"{url}/storage/v1/object/public/{BUCKET}/{path}?v={int(time.time())}"
