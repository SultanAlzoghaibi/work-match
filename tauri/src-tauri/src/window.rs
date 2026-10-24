// Trimmed from TyA_Tauri src-tauri/src/app/window.rs: the spotlight-style
// NSPanel conversion and cursor-monitor centering, without the reminder,
// auto-install and panel-position extras.
use tauri::{Manager, Runtime, WebviewWindow};
use thiserror::Error;

#[cfg(target_os = "macos")]
use tauri_nspanel::{
    tauri_panel, CollectionBehavior, ManagerExt, PanelHandle, PanelLevel, StyleMask,
    WebviewWindowExt as WebviewPanelExt,
};

#[cfg(target_os = "macos")]
tauri_panel! {
    panel!(SpotlightPanel {
        config: {
            can_become_key_window: true,
            is_floating_panel: true,
        }
    })

    panel_event!(SpotlightPanelEventHandler {
        window_did_resign_key(notification: &NSNotification) -> (),
    })
}

/// Open the panel from a tray click. The app runs as an Accessory (no dock
/// icon) and the panel is nonactivating, so activate the app first or the
/// panel never really becomes key and resign-key hides it right away.
#[cfg(target_os = "macos")]
pub fn open_panel<R: Runtime>(app: &tauri::AppHandle<R>) {
    let app_for_main = app.clone();
    let _ = app.run_on_main_thread(move || {
        let Some(main_window) = app_for_main.get_webview_window(crate::MAIN_LABEL) else {
            return;
        };
        if let Ok(panel) = app_for_main
            .get_webview_panel(crate::MAIN_LABEL)
            .or_else(|_| main_window.to_spotlight_panel())
        {
            let Some(main_thread) = tauri_nspanel::objc2::MainThreadMarker::new() else {
                return;
            };
            let ns_app =
                tauri_nspanel::objc2_app_kit::NSApplication::sharedApplication(main_thread);
            #[allow(deprecated)]
            ns_app.activateIgnoringOtherApps(true);

            // Pop out of the tray: start as a small pill right under the
            // icon, then let AppKit animate the frame up to full size.
            let (small, full) = tray_anchored_frames(main_thread);
            let ns_panel = panel.as_panel();
            ns_panel.setFrame_display(small, true);
            panel.show_and_make_key();
            ns_panel.setFrame_display_animate(full, true, true);
            return;
        }
        let _ = main_window.show();
        let _ = main_window.set_focus();
    });
}

/// Full panel size in points; keep in sync with tauri.conf.json width/height.
#[cfg(target_os = "macos")]
const PANEL_SIZE: NSSize = NSSize::new(360.0, 440.0);
/// Size the panel starts at before it grows out of the tray icon.
#[cfg(target_os = "macos")]
const START_SIZE: NSSize = NSSize::new(60.0, 30.0);
/// Gap between the menu bar / screen edge and the panel.
#[cfg(target_os = "macos")]
const EDGE_GAP: f64 = 8.0;

/// (start, end) frames for the open animation, both hanging from the menu
/// bar under the cursor (which is on the tray icon when it was clicked).
/// Cocoa coordinates: origin bottom-left, y grows upward.
#[cfg(target_os = "macos")]
fn tray_anchored_frames(main_thread: tauri_nspanel::objc2::MainThreadMarker) -> (NSRect, NSRect) {
    use tauri_nspanel::objc2_app_kit::{NSEvent, NSScreen};

    let mouse = NSEvent::mouseLocation();
    let screens = NSScreen::screens(main_thread);
    let screen = screens
        .iter()
        .find(|screen| {
            let frame = screen.frame();
            mouse.x >= frame.origin.x
                && mouse.x <= frame.origin.x + frame.size.width
                && mouse.y >= frame.origin.y
                && mouse.y <= frame.origin.y + frame.size.height
        })
        .or_else(|| NSScreen::mainScreen(main_thread));
    let visible = screen.map(|screen| screen.visibleFrame()).unwrap_or(NSRect::new(
        NSPoint::new(0.0, 0.0),
        NSSize::new(1440.0, 900.0),
    ));

    let top = visible.origin.y + visible.size.height - EDGE_GAP;
    let min_x = visible.origin.x + EDGE_GAP;
    let max_x = visible.origin.x + visible.size.width - EDGE_GAP;
    let centered_under_mouse = |width: f64| (mouse.x - width / 2.0).clamp(min_x, max_x - width);

    let small = NSRect::new(
        NSPoint::new(centered_under_mouse(START_SIZE.width), top - START_SIZE.height),
        START_SIZE,
    );
    let full = NSRect::new(
        NSPoint::new(centered_under_mouse(PANEL_SIZE.width), top - PANEL_SIZE.height),
        PANEL_SIZE,
    );
    (small, full)
}

/// Windows (and anything non-macOS): plain show + focus on the cursor's monitor.
#[cfg(not(target_os = "macos"))]
pub fn open_panel<R: Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(main_window) = app.get_webview_window(crate::MAIN_LABEL) {
        let _ = main_window.center_at_cursor_monitor();
        let _ = main_window.show();
        let _ = main_window.set_focus();
    }
}

type TauriError = tauri::Error;

#[derive(Error, Debug)]
enum Error {
    #[cfg(target_os = "macos")]
    #[error("Unable to convert window to panel")]
    Panel,
    #[cfg(not(target_os = "macos"))]
    #[error("Monitor not found")]
    MonitorNotFound,
}

pub trait WebviewWindowExt<R: Runtime> {
    #[cfg(target_os = "macos")]
    fn to_spotlight_panel(&self) -> tauri::Result<PanelHandle<R>>;
    #[cfg(not(target_os = "macos"))]
    fn center_at_cursor_monitor(&self) -> tauri::Result<()>;
}

impl<R: Runtime> WebviewWindowExt<R> for WebviewWindow<R> {
    #[cfg(target_os = "macos")]
    fn to_spotlight_panel(&self) -> tauri::Result<PanelHandle<R>> {
        let panel = self
            .to_panel::<SpotlightPanel<R>>()
            .map_err(|_| TauriError::Anyhow(Error::Panel.into()))?;

        panel.set_level(PanelLevel::Floating.value());
        panel.set_collection_behavior(
            CollectionBehavior::new()
                .can_join_all_spaces()
                .full_screen_auxiliary()
                .value(),
        );
        let _ = panel.set_style_mask(StyleMask::empty().nonactivating_panel().into());
        // Round the NSPanel itself so the glass matches the rounded CSS layout.
        panel.set_corner_radius(26.0);

        // Click away -> hide, like a menu-bar popover.
        let handler = SpotlightPanelEventHandler::new();
        let app_handle = self.app_handle().clone();
        handler.window_did_resign_key(move |_| {
            if let Ok(panel) = app_handle.get_webview_panel(crate::MAIN_LABEL) {
                if panel.is_visible() {
                    panel.hide();
                }
            }
        });
        panel.set_event_handler(Some(handler.as_ref()));

        Ok(panel)
    }

    #[cfg(not(target_os = "macos"))]
    fn center_at_cursor_monitor(&self) -> tauri::Result<()> {
        let monitor = self
            .current_monitor()?
            .ok_or(TauriError::Anyhow(Error::MonitorNotFound.into()))?;
        let monitor_position = monitor.position();
        let monitor_size = monitor.size();
        let window_size = self.outer_size()?;
        let center_x =
            monitor_position.x + (monitor_size.width as i32 - window_size.width as i32) / 2;
        let center_y =
            monitor_position.y + (monitor_size.height as i32 - window_size.height as i32) / 2;
        self.set_position(tauri::PhysicalPosition::new(center_x, center_y))?;
        Ok(())
    }
}
