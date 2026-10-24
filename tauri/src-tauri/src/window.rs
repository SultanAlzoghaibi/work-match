// Main window: a normal macOS window (title bar, movable, resizable) opened
// from the tray. Match popup: a floating NSPanel, trimmed from TyA_Tauri's
// src-tauri/src/app/window.rs.
use tauri::{Manager, Runtime};

#[cfg(target_os = "macos")]
use tauri::WebviewWindow;
#[cfg(target_os = "macos")]
use tauri_nspanel::{
    tauri_panel, CollectionBehavior, ManagerExt, PanelHandle, PanelLevel, StyleMask,
    WebviewWindowExt as WebviewPanelExt,
};

#[cfg(target_os = "macos")]
tauri_panel! {
    // Match popup: floats in the corner over whatever you're doing. Clicks
    // still reach its buttons, but it never takes keyboard focus.
    panel!(MatchPanel {
        config: {
            can_become_key_window: false,
            is_floating_panel: true,
        }
    })
}

/// Tray left-click: bring the main window to the front like any other app,
/// or hide it if it is already the focused window.
pub fn toggle_main_window<R: Runtime>(app: &tauri::AppHandle<R>) {
    let Some(main_window) = app.get_webview_window(crate::MAIN_LABEL) else {
        return;
    };
    let visible = main_window.is_visible().unwrap_or(false);
    let focused = main_window.is_focused().unwrap_or(false);
    if visible && focused {
        hide_main_window(app);
    } else {
        open_main_window(app);
    }
}

/// Show the main window and give the app a Dock icon / Cmd-Tab entry while
/// it's open, the way menu-bar apps with a real window usually behave.
pub fn open_main_window<R: Runtime>(app: &tauri::AppHandle<R>) {
    let Some(main_window) = app.get_webview_window(crate::MAIN_LABEL) else {
        return;
    };
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
    let _ = main_window.unminimize();
    let _ = main_window.show();
    let _ = main_window.set_focus();
}

/// Hide the main window and drop back to a menu-bar-only app. The close
/// button routes here too, so closing never quits; use the tray's Quit.
pub fn hide_main_window<R: Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(main_window) = app.get_webview_window(crate::MAIN_LABEL) {
        let _ = main_window.hide();
    }
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
}

/// Match popup size in points; keep in sync with the "match" window in tauri.conf.json.
#[cfg(target_os = "macos")]
const MATCH_SIZE: NSSize = NSSize::new(340.0, 200.0);
/// Gap between the menu bar / screen edge and the popup.
#[cfg(target_os = "macos")]
const EDGE_GAP: f64 = 8.0;

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

/// Visible frame (minus menu bar and Dock) of the screen under the cursor.
/// Cocoa coordinates: origin bottom-left, y grows upward.
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

#[cfg(target_os = "macos")]
#[derive(thiserror::Error, Debug)]
#[error("Unable to convert window to panel")]
struct PanelError;

/// Convert the match window into a floating, nonactivating NSPanel.
#[cfg(target_os = "macos")]
pub fn to_match_panel<R: Runtime>(window: &WebviewWindow<R>) -> tauri::Result<PanelHandle<R>> {
    let panel = window
        .to_panel::<MatchPanel<R>>()
        .map_err(|_| tauri::Error::Anyhow(PanelError.into()))?;

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
