// Whether the user is sharing their activity. While it is off nothing is
// captured, summarized or pushed, and turning it off forgets what was buffered.
use crate::buffer::Buffer;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};

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

/// The one way sharing changes, from the tray or from the UI.
pub fn set(app: &AppHandle, on: bool) {
    app.state::<Sharing>().store(on);
    if !on {
        // Sharing is already off either way; anything left behind is swept within 6 minutes.
        if let Err(err) = app.state::<Buffer>().clear_all() {
            eprintln!("[sharing] clearing the buffer failed: {err}");
        }
        // TODO: tell the backend to delete curr_task (QUIT / PAUSE in backend/schema.sql).
    }
    crate::db::save_sharing_enabled(app, on);
    println!("[sharing] {}", if on { "on" } else { "off" });
}

#[tauri::command]
pub fn get_sharing(sharing: State<'_, Sharing>) -> bool {
    sharing.is_on()
}

/// Sharing is always on now (friend finding is the switch), so a request to
/// turn it off is ignored.
#[tauri::command]
pub fn set_sharing(app: AppHandle, enabled: bool) {
    if !enabled {
        println!("[sharing] stays on: turn friend finding off instead");
    }
    set(&app, true);
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
