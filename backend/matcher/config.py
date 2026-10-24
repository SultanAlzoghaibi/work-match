import os
from dataclasses import dataclass
from pathlib import Path


def load_dotenv(path: Path = Path(__file__).resolve().parent.parent / ".env") -> None:
    """Minimal .env loader: KEY=value lines; real environment variables win."""
    if not path.exists():
        return
    for line in path.read_text().splitlines():
        line = line.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        key, value = line.split("=", 1)
        os.environ.setdefault(key.strip(), value.strip())


@dataclass(frozen=True)
class Config:
    supabase_url: str
    supabase_service_role_key: str
    openrouter_api_key: str
    jev_model: str
    muse_api_key: str
    muse_base_url: str
    muse_model: str
    match_threshold: float
    interrupt_threshold: float
    match_interval_sec: float

    @classmethod
    def from_env(cls) -> "Config":
        load_dotenv()
        env = os.environ.get
        return cls(
            supabase_url=env("SUPABASE_URL", ""),
            supabase_service_role_key=env("SUPABASE_SERVICE_ROLE_KEY", ""),
            openrouter_api_key=env("OPENROUTER_API_KEY", ""),
            jev_model=env("JEV_MODEL", "typesafe/jev-1.14"),
            muse_api_key=env("MUSE_API_KEY", ""),
            muse_base_url=env("MUSE_BASE_URL", "https://api.meta.ai/v1"),
            muse_model=env("MUSE_MODEL", ""),
            match_threshold=float(env("MATCH_THRESHOLD", "0.7")),
            interrupt_threshold=float(env("INTERRUPT_THRESHOLD", "0.5")),
            match_interval_sec=float(env("MATCH_INTERVAL_SEC", "20")),
        )
