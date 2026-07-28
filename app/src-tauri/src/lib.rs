use std::{
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::Duration,
};

use serde::Serialize;
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    AppHandle, Emitter, Manager, PhysicalPosition, Position, State, WebviewUrl,
    WebviewWindowBuilder, WindowEvent,
};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

#[cfg(target_os = "windows")]
use windows_sys::Win32::{
    Foundation::POINT,
    UI::{
        Shell::{SHAppBarMessage, ABM_GETTASKBARPOS, APPBARDATA},
        WindowsAndMessaging::GetCursorPos,
    },
};

const MIRROR_LABEL: &str = "mirror";
const SETTINGS_LABEL: &str = "settings";
const MIRROR_ASPECT_RATIO: f64 = 4.0;

struct MirrorState {
    shortcut_held: AtomicBool,
    move_enabled: AtomicBool,
}

impl Default for MirrorState {
    fn default() -> Self {
        Self {
            shortcut_held: AtomicBool::new(false),
            move_enabled: AtomicBool::new(true),
        }
    }
}

#[derive(Clone, Serialize)]
struct SavedPosition {
    x: i32,
    y: i32,
}

#[cfg(target_os = "windows")]
fn taskbar_bounds() -> Option<(i32, i32, i32, i32)> {
    let mut taskbar = unsafe { std::mem::zeroed::<APPBARDATA>() };
    taskbar.cbSize = std::mem::size_of::<APPBARDATA>() as u32;

    if unsafe { SHAppBarMessage(ABM_GETTASKBARPOS, &mut taskbar) } != 0 {
        Some((
            taskbar.rc.left,
            taskbar.rc.top,
            taskbar.rc.right,
            taskbar.rc.bottom,
        ))
    } else {
        None
    }
}

#[tauri::command]
fn set_move_enabled(state: State<'_, MirrorState>, enabled: bool) {
    state.move_enabled.store(enabled, Ordering::SeqCst);
}

#[tauri::command]
fn set_mirror_size(app: AppHandle, longest_edge: u32) -> Result<(), String> {
    let width = longest_edge.clamp(128, 1600);
    let height = (width as f64 / MIRROR_ASPECT_RATIO).round() as u32;
    mirror_window(&app)?
        .set_size(tauri::Size::Physical(tauri::PhysicalSize::new(width, height)))
        .map_err(|error| error.to_string())
}

/// 横に引き延ばした4:1ミラーの短辺が、Windows タスクバーの厚みと等しくなる幅を返す。
#[cfg(target_os = "windows")]
#[tauri::command]
fn get_taskbar_mirror_size() -> u32 {
    if let Some((left, top, right, bottom)) = taskbar_bounds() {
        let width = (right - left).unsigned_abs();
        let height = (bottom - top).unsigned_abs();
        let taskbar_thickness = width.min(height);
        return (taskbar_thickness as f64 * MIRROR_ASPECT_RATIO).round() as u32;
    }

    192
}

#[cfg(not(target_os = "windows"))]
#[tauri::command]
fn get_taskbar_mirror_size() -> u32 {
    192
}

/// タスクバーの右端にミラーを収める初期位置を返す。
#[cfg(target_os = "windows")]
#[tauri::command]
fn get_taskbar_mirror_position(width: u32) -> SavedPosition {
    let width = width.clamp(128, 1600) as i32;
    let height = (width as f64 / MIRROR_ASPECT_RATIO).round() as i32;

    if let Some((left, top, right, bottom)) = taskbar_bounds() {
        let taskbar_width = right - left;
        let taskbar_height = bottom - top;
        return if taskbar_width >= taskbar_height {
            SavedPosition {
                x: right - width,
                y: top,
            }
        } else {
            SavedPosition {
                x: left,
                y: bottom - height,
            }
        };
    }

    SavedPosition { x: 0, y: 0 }
}

#[cfg(not(target_os = "windows"))]
#[tauri::command]
fn get_taskbar_mirror_position(_width: u32) -> SavedPosition {
    SavedPosition { x: 0, y: 0 }
}

#[tauri::command]
fn set_mirror_position(app: AppHandle, x: i32, y: i32) -> Result<(), String> {
    mirror_window(&app)?
        .set_position(Position::Physical(PhysicalPosition::new(x, y)))
        .map_err(|error| error.to_string())
}

fn mirror_window(app: &AppHandle) -> Result<tauri::WebviewWindow, String> {
    app.get_webview_window(MIRROR_LABEL)
        .ok_or_else(|| "ミラーウィンドウを取得できませんでした。".to_string())
}

fn show_settings(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(SETTINGS_LABEL) {
        let _ = window.show();
        let _ = window.set_focus();
        let _ = app.emit_to(SETTINGS_LABEL, "settings:opened", ());
    }
}

#[cfg(target_os = "windows")]
fn cursor_position() -> Option<PhysicalPosition<i32>> {
    let mut point = POINT { x: 0, y: 0 };
    if unsafe { GetCursorPos(&mut point) } != 0 {
        Some(PhysicalPosition::new(point.x, point.y))
    } else {
        None
    }
}

#[cfg(not(target_os = "windows"))]
fn cursor_position() -> Option<PhysicalPosition<i32>> {
    None
}

fn start_pointer_tracking(app: AppHandle) {
    thread::spawn(move || {
        let Some(start_cursor) = cursor_position() else {
            return;
        };
        let Ok(window) = mirror_window(&app) else {
            return;
        };
        let Ok(start_window) = window.outer_position() else {
            return;
        };

        let mut moved_past_dead_zone = false;
        while app
            .state::<MirrorState>()
            .shortcut_held
            .load(Ordering::SeqCst)
        {
            if let Some(cursor) = cursor_position() {
                let dx = cursor.x - start_cursor.x;
                let dy = cursor.y - start_cursor.y;
                if moved_past_dead_zone || dx * dx + dy * dy >= 16 {
                    moved_past_dead_zone = true;
                    let _ = window.set_position(Position::Physical(PhysicalPosition::new(
                        start_window.x + dx,
                        start_window.y + dy,
                    )));
                }
            }
            thread::sleep(Duration::from_millis(16));
        }
    });
}

fn show_mirror(app: &AppHandle) {
    let state = app.state::<MirrorState>();
    if state.shortcut_held.swap(true, Ordering::SeqCst) {
        return;
    }

    if let Ok(window) = mirror_window(app) {
        let _ = window.show();
    }
    let _ = app.emit_to(MIRROR_LABEL, "mirror:show", ());

    if state.move_enabled.load(Ordering::SeqCst) {
        start_pointer_tracking(app.clone());
    }
}

fn hide_mirror(app: &AppHandle) {
    app.state::<MirrorState>()
        .shortcut_held
        .store(false, Ordering::SeqCst);

    if let Ok(window) = mirror_window(app) {
        if let Ok(position) = window.outer_position() {
            let _ = app.emit_to(
                MIRROR_LABEL,
                "mirror:position",
                SavedPosition {
                    x: position.x,
                    y: position.y,
                },
            );
        }
    }
    let _ = app.emit_to(MIRROR_LABEL, "mirror:hide", ());
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(MirrorState::default())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    let mirror_shortcut = Shortcut::new(
                        Some(Modifiers::CONTROL | Modifiers::ALT),
                        Code::Space,
                    );
                    if shortcut == &mirror_shortcut {
                        match event.state() {
                            ShortcutState::Pressed => show_mirror(app),
                            ShortcutState::Released => hide_mirror(app),
                        }
                    }
                })
                .build(),
        )
        .plugin(tauri_plugin_store::Builder::default().build())
        .setup(|app| {
            let mirror_shortcut = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::Space);
            app.global_shortcut().register(mirror_shortcut)?;

            let is_first_launch = app
                .path()
                .app_data_dir()
                .map(|path| !path.join("rearview-mirror.json").exists())
                .unwrap_or(true);

            WebviewWindowBuilder::new(app, SETTINGS_LABEL, WebviewUrl::App("index.html".into()))
                .title("Rearview Mirror 設定")
                .inner_size(420.0, 540.0)
                .min_inner_size(360.0, 460.0)
                .resizable(true)
                .visible(is_first_launch)
                .build()?;

            let settings_item = MenuItem::with_id(app, "settings", "設定", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "終了", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&settings_item, &quit_item])?;
            TrayIconBuilder::with_id("rearview-mirror-tray")
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("Rearview Mirror")
                .menu(&menu)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "settings" => show_settings(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            set_move_enabled,
            set_mirror_size,
            get_taskbar_mirror_size,
            get_taskbar_mirror_position,
            set_mirror_position
        ])
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!())
        .expect("Rearview Mirrorの起動に失敗しました。");
}
