mod api;
mod buffer;
mod capture_server;
mod collecting_data;
mod db;
mod friend_finding;
mod llm;
mod match_popup;
mod scrub;
mod sharing;
mod summarizer;
mod window;

use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::Manager;
#[cfg(any(target_os = "macos", windows))]
use tauri::WebviewWindow;
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
        Migration {
            version: 3,
            description: "avatar",
            sql: include_str!("migrations/003_avatar.sql"),
            kind: MigrationKind::Up,
        },
        Migration {
            version: 4,
            description: "career",
            sql: include_str!("migrations/004_career.sql"),
            kind: MigrationKind::Up,
        },
        Migration {
            version: 5,
            description: "app_sessions",
            sql: include_str!("migrations/005_app_sessions.sql"),
            kind: MigrationKind::Up,
        },
        Migration {
            version: 6,
            description: "languages",
            sql: include_str!("migrations/006_languages.sql"),
            kind: MigrationKind::Up,
        },
        Migration {
            version: 7,
            description: "auth",
            sql: include_str!("migrations/007_auth.sql"),
            kind: MigrationKind::Up,
        },
    ];

    let builder = tauri::Builder::default()
        .plugin(
            tauri_plugin_sql::Builder::default()
                .add_migrations(DB_FILE, migrations)
                .build(),
        )
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_opener::init());

    #[cfg(target_os = "macos")]
    let builder = builder.plugin(tauri_nspanel::init());

    builder
        .manage(match_popup::MatchState::default())
        .manage(friend_finding::FriendFinding::default())
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
            let friends_item = tauri::menu::CheckMenuItemBuilder::new("Turn on friend finding")
                .id("friend_finding")
                .checked(false)
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
                .item(&friends_item)
                .separator()
                .item(&open_item)
                .item(&quit_item)
                .build()?;
            app.manage(sharing::SharingMenuItem(sharing_item));
            app.manage(friend_finding::FriendFindingMenuItem(friends_item));

            TrayIconBuilder::with_id(friend_finding::TRAY_ID)
                .icon(friend_finding::base_icon())
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
                    "friend_finding" => {
                        let on = !app.state::<friend_finding::FriendFinding>().is_on();
                        friend_finding::set(app, on);
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
            #[cfg(windows)]
            apply_window_style(&match_window);

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
            match_popup::open_match_profile,
            match_popup::start_match_polling,
            match_popup::stop_match_polling,
            match_popup::current_connections,
            match_popup::join_connection,
            match_popup::keep_match,
            match_popup::open_call,
            friend_finding::get_friend_finding,
            friend_finding::set_friend_finding,
            sharing::get_sharing,
            sharing::set_sharing,
            summarizer::get_last_summary,
            summarizer::summarize_now,
            collecting_data::app_tracker::get_app_sessions,
            collecting_data::dev::dev_status,
            collecting_data::dev::dev_captures,
            collecting_data::dev::dev_last_run,
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
    // Only one app can own the extension's port. The second dev instance
    // (yarn dev2 / prod2) is for demo buttons, so it never claims it.
    if std::env::var("WORKMATCH_NO_CAPTURE").is_ok_and(|value| value == "1") {
        capture_server::disable();
    } else {
        capture_server::start(buffer.clone(), sharing.clone());
    }
    app.manage(collecting_data::dev::DevState::default());
    app.manage(buffer);
    app.manage(sharing);
    app.manage(llm::start(app.handle()));
    summarizer::start(app.handle().clone());
    // Which app / window is in front, while sharing is on (collecting_data/).
    collecting_data::app_tracker::start(app.handle().clone());
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
            // Light tint so the clear glass stays see-through; the CSS adds a
            // text shadow for readability.
            Some((0, 0, 0, 70)),
            Some(22.0),
        )
        .expect("liquid glass unsupported");
    } else {
        apply_vibrancy(window, NSVisualEffectMaterial::HudWindow, None, Some(22.0))
            .expect("vibrancy unsupported");
    }
}

/// Windows 11: dark Mica. Windows 10: dark acrylic. match.css also tints the
/// popup on Windows, so its white text stays readable if neither applies.
#[cfg(windows)]
fn apply_window_style(window: &WebviewWindow) {
    use window_vibrancy::{apply_acrylic, apply_mica};
    use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND};

    if apply_mica(window, Some(true)).is_err() && apply_acrylic(window, Some((18, 18, 24, 160))).is_err() {
        eprintln!("[window] no Mica or acrylic on this Windows version; the popup uses its CSS background");
    }
    // Undecorated windows have square corners; Windows 11 can round them like
    // the macOS panel (Windows 10 ignores this).
    if let Ok(hwnd) = window.hwnd() {
        let _ = unsafe {
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &DWMWCP_ROUND as *const _ as *const _,
                std::mem::size_of_val(&DWMWCP_ROUND) as u32,
            )
        };
    }
}
