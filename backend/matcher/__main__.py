"""Run the matcher: `python -m matcher` (loop) or `python -m matcher --once`."""

import sys
import time

from .config import Config
from .intro import MuseIntroWriter, TemplateIntroWriter
from .jev import FallbackScorer, JevScorer
from .match import Thresholds, run_once
from .store import SupabaseStore


def main() -> None:
    cfg = Config.from_env()
    if not cfg.supabase_url or not cfg.supabase_service_role_key:
        sys.exit("Set SUPABASE_URL and SUPABASE_SERVICE_ROLE_KEY (see backend/.env.example)")

    store = SupabaseStore(cfg.supabase_url, cfg.supabase_service_role_key)
    scorer = JevScorer(cfg.openrouter_api_key, cfg.jev_model) if cfg.openrouter_api_key else FallbackScorer()
    writer = (
        MuseIntroWriter(cfg.muse_api_key, cfg.muse_base_url, cfg.muse_model)
        if cfg.muse_api_key and cfg.muse_model else TemplateIntroWriter()
    )
    thresholds = Thresholds(cfg.match_threshold, cfg.interrupt_threshold)
    print(f"[matcher] scorer={scorer.model} intro={writer.model} "
          f"thresholds=match {thresholds.match}, interrupt {thresholds.interrupt}")

    if "--once" in sys.argv:
        print(f"[matcher] created {len(run_once(store, scorer, writer, thresholds))} offer(s)")
        return

    while True:
        try:
            run_once(store, scorer, writer, thresholds)
        except Exception as err:  # keep the loop alive; log and retry next tick
            print(f"[matcher] pass failed: {err!r}")
        time.sleep(cfg.match_interval_sec)


if __name__ == "__main__":
    main()
