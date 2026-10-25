// Whether activity is being tracked: the app tracker, Chrome capture and the
// summarizer. It's on exactly while friend finding is (friend_finding::set is
// the only switch), so it's off at every launch and after every quit. While it
// is off nothing is captured, summarized or pushed, and turning it off forgets
// what was buffered.
use crate::buffer::Buffer;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Manager, Runtime, State};

#[derive(Clone)]
pub struct Sharing(Arc<Flags>);

struct Flags {
    on: AtomicBool,
    /// Bumped every time sharing turns off, so a summary that started before a
    /// pause can tell its input is stale.
    epoch: AtomicU64,
}

impl Sharing {
    pub fn new(on: bool) -> Self {
        Self(Arc::new(Flags {
            on: AtomicBool::new(on),
            epoch: AtomicU64::new(0),
        }))
    }

    pub fn is_on(&self) -> bool {
        self.0.on.load(Ordering::SeqCst)
    }

    pub fn epoch(&self) -> u64 {
        self.0.epoch.load(Ordering::SeqCst)
    }

    fn store(&self, on: bool) {
        if !on {
            self.0.epoch.fetch_add(1, Ordering::SeqCst);
        }
        self.0.on.store(on, Ordering::SeqCst);
    }
}

/// The one way tracking changes. Only friend_finding::set calls it, so
/// tracking can't run without friend finding.
pub fn set<R: Runtime>(app: &AppHandle<R>, on: bool) {
    app.state::<Sharing>().store(on);
    if !on {
        // Tracking is already off either way; anything left behind is swept within 6 minutes.
        if let Err(err) = app.state::<Buffer>().clear_all() {
            eprintln!("[sharing] clearing the buffer failed: {err}");
        }
    }
    crate::db::save_sharing_enabled(app, on);
    println!("[sharing] tracking {}", if on { "on" } else { "off" });
}

#[tauri::command]
pub fn get_sharing(sharing: State<'_, Sharing>) -> bool {
    sharing.is_on()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turning_off_bumps_the_epoch() {
        let sharing = Sharing::new(true);
        let before = sharing.epoch();
        sharing.store(true);
        assert_eq!(sharing.epoch(), before);
        sharing.store(false);
        assert!(!sharing.is_on());
        assert_eq!(sharing.epoch(), before + 1);
    }
}
