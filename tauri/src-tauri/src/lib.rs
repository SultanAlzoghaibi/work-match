mod api;
mod buffer;
mod capture_server;
mod db;
mod llm;
mod match_popup;
mod scrub;
mod sharing;
mod summarizer;
mod window;

use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Manager, WebviewWindow};
use tauri_plugin_sql::{Migration, MigrationKind};

pub const MAIN_LABEL: &str = "main";
pub const MATCH_LABEL: &str = "match";
/// Local SQLite file, created in the app data dir. Keep in sync with
/// plugins.sql.preload in tauri.conf.json and src/db.js.
pub const DB_FILE: &str = "sqlite:work_match.db";

/// Quit the whole app (tray icon included) from the window's Quit button / Cmd+Q.
#[tauri::command]
fn quit_app(app: tauri::AppHandle) {
    app.exit(0);
}

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

    let builder = tauri::Builder::default()
        .plugin(
            tauri_plugin_sql::Builder::default()
                .add_migrations(DB_FILE, migrations)
                .build(),
        )
        .plugin(tauri_plugin_shell::init());

    #[cfg(target_os = "macos")]
    let builder = builder.plugin(tauri_nspanel::init());

    builder
        .manage(match_popup::MatchState::default())
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
            let match_window = app.get_webview_window(MATCH_LABEL).unwrap();
            match_window.hide().ok();

            start_activity_capture(app)?;

            let header_item = tauri::menu::MenuItemBuilder::new("work-match")
                .id("header")
                .enabled(false)
                .build(app)?;
            let sharing_item = tauri::menu::CheckMenuItemBuilder::new("Share my activity")
                .id("sharing")
                .checked(app.state::<sharing::Sharing>().is_on())
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
                .item(&sharing_item)
                .separator()
                .item(&open_item)
                .item(&quit_item)
                .build()?;
            app.manage(sharing::SharingMenuItem(sharing_item));

            let tray_icon = tauri::image::Image::from_bytes(include_bytes!("../icons/tray.png"))
                .expect("tray icon load failed");
            TrayIconBuilder::new()
                .icon(tray_icon)
                .tooltip("work-match")
                .menu(&menu)
                // Left click opens the main window; right click shows the menu.
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    // Driven by our flag, not the item's own toggle, so the checkmark can't drift.
                    "sharing" => {
                        let on = !app.state::<sharing::Sharing>().is_on();
                        sharing::set(app, on);
                    }
                    "open" => window::open_main_window(app),
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
                        window::toggle_main_window(tray.app_handle());
                    }
                })
                .build(app)?;

            // Main window is a normal solid dark window; only the match
            // popup is a floating panel with liquid glass.
            #[cfg(target_os = "macos")]
            {
                window::to_match_panel(&match_window)?;
                apply_window_style(&match_window);
            }

            // Red close button hides instead of quitting (Quit is in the tray).
            let app_for_close = app.handle().clone();
            main_window.on_window_event(move |event| {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    window::hide_main_window(&app_for_close);
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            os_name,
            quit_app,
            match_popup::trigger_mock_match,
            match_popup::current_match,
            match_popup::respond_match,
            match_popup::dismiss_match,
            match_popup::start_match_polling,
            sharing::get_sharing,
            sharing::set_sharing,
            summarizer::get_last_summary,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            if let tauri::RunEvent::Exit = event {
                // TODO: tell the backend to delete curr_task (QUIT / PAUSE in backend/schema.sql).
                if let Some(llm) = app_handle.try_state::<llm::Llm>() {
                    llm.shutdown();
                }
            }
        });
}

/// Chrome tab capture → local SQLite buffer → local Llama summary.
/// See docs/activity-capture.md.
fn start_activity_capture(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = app.path().app_data_dir()?;
    std::fs::create_dir_all(&data_dir)?;
    let buffer = buffer::Buffer::open(&data_dir.join(buffer::FILE_NAME))?;
    // Captured text never survives a restart.
    buffer.clear_captures()?;
    let sharing = sharing::Sharing::new(db::load_sharing_enabled(app.handle()).unwrap_or(false));

    buffer::start_sweeper(buffer.clone());
    capture_server::start(buffer.clone(), sharing.clone());
    app.manage(buffer);
    app.manage(sharing);
    app.manage(llm::start(app.handle()));
    summarizer::start(app.handle().clone());
    Ok(())
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
            Some((0, 0, 0, 200)),
            Some(22.0),
        )
        .expect("liquid glass unsupported");
    } else {
        apply_vibrancy(window, NSVisualEffectMaterial::HudWindow, None, Some(22.0))
            .expect("vibrancy unsupported");
    }
}
