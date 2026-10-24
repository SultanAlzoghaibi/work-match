mod db;
mod window;

use crate::window::WebviewWindowExt;
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Manager, WebviewWindow};
use tauri_plugin_sql::{Migration, MigrationKind};

pub const MAIN_LABEL: &str = "main";
/// Local SQLite file, created in the app data dir. Keep in sync with
/// plugins.sql.preload in tauri.conf.json and src/db.js.
pub const DB_FILE: &str = "sqlite:work_match.db";

/// Expose the compile-time target OS to the frontend (src/main.js).
/// Returns "macos" | "linux" | "windows".
#[tauri::command]
fn os_name() -> &'static str {
    std::env::consts::OS
}

pub fn run() {
    // Migrations run once each, in order, when the DB is first loaded.
    let migrations = vec![
        Migration {
            version: 1,
            description: "init_local",
            sql: include_str!("migrations/001_init_local.sql"),
            kind: MigrationKind::Up,
        },
        Migration {
            version: 2,
            description: "profile_fields",
            sql: include_str!("migrations/002_profile_fields.sql"),
            kind: MigrationKind::Up,
        },
    ];

    let builder = tauri::Builder::default().plugin(
        tauri_plugin_sql::Builder::default()
            .add_migrations(DB_FILE, migrations)
            .build(),
    );

    #[cfg(target_os = "macos")]
    let builder = builder.plugin(tauri_nspanel::init());

    builder
        .setup(|app| {
            // Accessory: no dock icon, lives only in the menu bar.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            // Demo data: uncomment to seed src/seed/defaults.sql. The DB only
            // exists after the sql plugin has run migration 001, so this needs
            // the app launched once (or the frontend's Database.load) first.
            // db::seeder::seed_defaults(app.handle());

            let main_window = app.get_webview_window(MAIN_LABEL).unwrap();
            main_window.hide().ok();

            let header_item = tauri::menu::MenuItemBuilder::new("work-match")
                .id("header")
                .enabled(false)
                .build(app)?;
            let open_item = tauri::menu::MenuItemBuilder::new("Open")
                .id("open")
                .build(app)?;
            let quit_item = tauri::menu::MenuItemBuilder::new("Quit")
                .id("quit")
                .build(app)?;
            let menu = tauri::menu::MenuBuilder::new(app)
                .item(&header_item)
                .separator()
                .item(&open_item)
                .item(&quit_item)
                .build()?;

            let tray_icon = tauri::image::Image::from_bytes(include_bytes!("../icons/tray.png"))
                .expect("tray icon load failed");
            TrayIconBuilder::new()
                .icon(tray_icon)
                .tooltip("work-match")
                .menu(&menu)
                // Left click opens the panel; right click shows the menu.
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "open" => window::open_panel(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        window::open_panel(tray.app_handle());
                    }
                })
                .build(app)?;

            #[cfg(target_os = "macos")]
            {
                main_window.to_spotlight_panel()?;
                apply_window_style(&main_window);
            }

            // Non-macOS: no NSPanel resign-key, so hide on blur instead.
            #[cfg(not(target_os = "macos"))]
            {
                let window_for_blur = main_window.clone();
                main_window.on_window_event(move |event| {
                    if let tauri::WindowEvent::Focused(false) = event {
                        let _ = window_for_blur.hide();
                    }
                });
                let _ = main_window.center_at_cursor_monitor();
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![os_name])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app_handle, _event| {});
}

#[cfg(target_os = "macos")]
fn supports_liquid_glass() -> bool {
    let Ok(output) = std::process::Command::new("sw_vers")
        .arg("-productVersion")
        .output()
    else {
        return false;
    };
    String::from_utf8_lossy(&output.stdout)
        .split('.')
        .next()
        .and_then(|major| major.trim().parse::<u32>().ok())
        .is_some_and(|major| major >= 26)
}

/// macOS 26+: native Liquid Glass. Older macOS: HUD vibrancy blur.
#[cfg(target_os = "macos")]
fn apply_window_style(window: &WebviewWindow) {
    use window_vibrancy::*;
    if supports_liquid_glass() {
        apply_liquid_glass(
            window,
            NSGlassEffectViewStyle::Clear,
            Some((0, 0, 0, 120)),
            Some(26.0),
        )
        .expect("liquid glass unsupported");
    } else {
        apply_vibrancy(window, NSVisualEffectMaterial::HudWindow, None, Some(26.0))
            .expect("vibrancy unsupported");
    }
}
