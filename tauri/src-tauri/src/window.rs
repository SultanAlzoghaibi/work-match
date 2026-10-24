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

    // Match popup: floats in the corner over whatever you're doing. Clicks
    // still reach its buttons, but it never takes keyboard focus.
    panel!(MatchPanel {
        config: {
            can_become_key_window: false,
            is_floating_panel: true,
        }
    })
}

/// Tray left-click: open the panel, or close it if it's already showing.
/// Clicking elsewhere does NOT hide it; it stays until toggled or quit.
pub fn toggle_panel<R: Runtime>(app: &tauri::AppHandle<R>) {
    let Some(main_window) = app.get_webview_window(crate::MAIN_LABEL) else {
        return;
    };
    if main_window.is_visible().unwrap_or(false) {
        let _ = main_window.hide();
    } else {
        open_panel(app);
    }
}

/// Open the panel. The app runs as an Accessory (no dock icon) and the panel
/// is nonactivating, so activate the app first or the panel never really
/// becomes key and typing into it goes to the previous app.
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

            // Open like any menu-bar app: full size, right under the icon.
            panel
                .as_panel()
                .setFrame_display(tray_anchored_frame(main_thread), true);
            panel.show_and_make_key();
            return;
        }
        let _ = main_window.show();
        let _ = main_window.set_focus();
    });
}

/// Full panel size in points; keep in sync with tauri.conf.json width/height.
#[cfg(target_os = "macos")]
const PANEL_SIZE: NSSize = NSSize::new(360.0, 440.0);
/// Gap between the menu bar / screen edge and the panel.
#[cfg(target_os = "macos")]
const EDGE_GAP: f64 = 8.0;

/// Panel frame hanging from the menu bar under the cursor (which is on the
/// tray icon when it was clicked), kept on-screen.
/// Cocoa coordinates: origin bottom-left, y grows upward.
#[cfg(target_os = "macos")]
fn tray_anchored_frame(main_thread: tauri_nspanel::objc2::MainThreadMarker) -> NSRect {
    let mouse = tauri_nspanel::objc2_app_kit::NSEvent::mouseLocation();
    let visible = cursor_screen_visible_frame(main_thread);

    let top = visible.origin.y + visible.size.height - EDGE_GAP;
    let min_x = visible.origin.x + EDGE_GAP;
    let max_x = visible.origin.x + visible.size.width - EDGE_GAP;
    let x = (mouse.x - PANEL_SIZE.width / 2.0).clamp(min_x, max_x - PANEL_SIZE.width);

    NSRect::new(NSPoint::new(x, top - PANEL_SIZE.height), PANEL_SIZE)
}

/// Visible frame (minus menu bar and Dock) of the screen under the cursor.
#[cfg(target_os = "macos")]
fn cursor_screen_visible_frame(main_thread: tauri_nspanel::objc2::MainThreadMarker) -> NSRect {
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
    screen.map(|screen| screen.visibleFrame()).unwrap_or(NSRect::new(
        NSPoint::new(0.0, 0.0),
        NSSize::new(1440.0, 900.0),
    ))
}

/// Match popup size in points; keep in sync with the "match" window in tauri.conf.json.
#[cfg(target_os = "macos")]
const MATCH_SIZE: NSSize = NSSize::new(340.0, 200.0);

/// Show the match popup in the top-right corner of the screen with the
/// cursor, without activating the app or stealing focus from the user's work.
#[cfg(target_os = "macos")]
pub fn show_match_popup<R: Runtime>(app: &tauri::AppHandle<R>) {
    let app_for_main = app.clone();
    let _ = app.run_on_main_thread(move || {
        let Ok(panel) = app_for_main.get_webview_panel(crate::MATCH_LABEL) else {
            return;
        };
        let Some(main_thread) = tauri_nspanel::objc2::MainThreadMarker::new() else {
            return;
        };
        let visible = cursor_screen_visible_frame(main_thread);
        let origin = NSPoint::new(
            visible.origin.x + visible.size.width - MATCH_SIZE.width - EDGE_GAP,
            visible.origin.y + visible.size.height - MATCH_SIZE.height - EDGE_GAP,
        );
        panel
            .as_panel()
            .setFrame_display(NSRect::new(origin, MATCH_SIZE), true);
        panel.show();
    });
}

#[cfg(not(target_os = "macos"))]
pub fn show_match_popup<R: Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(window) = app.get_webview_window(crate::MATCH_LABEL) {
        let _ = window.show();
    }
}

pub fn hide_match_popup<R: Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(window) = app.get_webview_window(crate::MATCH_LABEL) {
        let _ = window.hide();
    }
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
    #[cfg(target_os = "macos")]
    fn to_match_panel(&self) -> tauri::Result<PanelHandle<R>>;
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

        Ok(panel)
    }

    #[cfg(target_os = "macos")]
    fn to_match_panel(&self) -> tauri::Result<PanelHandle<R>> {
        let panel = self
            .to_panel::<MatchPanel<R>>()
            .map_err(|_| TauriError::Anyhow(Error::Panel.into()))?;

        panel.set_level(PanelLevel::Floating.value());
        panel.set_collection_behavior(
            CollectionBehavior::new()
                .can_join_all_spaces()
                .full_screen_auxiliary()
                .value(),
        );
        let _ = panel.set_style_mask(StyleMask::empty().nonactivating_panel().into());
        panel.set_corner_radius(22.0);

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
