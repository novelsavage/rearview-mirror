use std::{
    sync::{
        atomic::{AtomicBool, AtomicIsize, Ordering},
        Mutex,
    },
    thread,
    time::Duration,
};

use serde::{Deserialize, Serialize};
use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem},
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
    moved_while_shortcut_held: AtomicBool,
    toggle_press_started_visible: AtomicBool,
    move_enabled: AtomicBool,
    toggle_mode: AtomicBool,
    mirrored: AtomicBool,
    grayscale: AtomicBool,
    display_mode: Mutex<DisplayMode>,
}

#[cfg(target_os = "windows")]
struct TaskbarOverlayState {
    hwnd: AtomicIsize,
    enabled: AtomicBool,
    refresh_running: AtomicBool,
}

#[cfg(target_os = "windows")]
impl Default for TaskbarOverlayState {
    fn default() -> Self {
        Self {
            hwnd: AtomicIsize::new(0),
            enabled: AtomicBool::new(false),
            refresh_running: AtomicBool::new(false),
        }
    }
}

struct TrayControls {
    mirrored: CheckMenuItem<tauri::Wry>,
    grayscale: CheckMenuItem<tauri::Wry>,
    move_enabled: CheckMenuItem<tauri::Wry>,
    toggle_mode: CheckMenuItem<tauri::Wry>,
}

#[derive(PartialEq)]
enum DisplayMode {
    Hidden,
    Held,
    Toggled,
}

impl Default for MirrorState {
    fn default() -> Self {
        Self {
            shortcut_held: AtomicBool::new(false),
            moved_while_shortcut_held: AtomicBool::new(false),
            toggle_press_started_visible: AtomicBool::new(false),
            move_enabled: AtomicBool::new(true),
            toggle_mode: AtomicBool::new(true),
            mirrored: AtomicBool::new(true),
            grayscale: AtomicBool::new(false),
            display_mode: Mutex::new(DisplayMode::Hidden),
        }
    }
}

#[derive(Clone, Serialize)]
struct SavedPosition {
    x: i32,
    y: i32,
}

#[derive(Clone, Serialize, Deserialize)]
struct DisplayOptions {
    mirrored: bool,
    grayscale: bool,
}

#[derive(Clone, Serialize)]
struct TrayOptions {
    mirrored: bool,
    grayscale: bool,
    move_enabled: bool,
    toggle_mode: bool,
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
fn set_move_enabled(app: AppHandle, state: State<'_, MirrorState>, enabled: bool) {
    state.move_enabled.store(enabled, Ordering::SeqCst);
    sync_tray_controls(&app, &state);
}

#[tauri::command]
fn set_toggle_mode(app: AppHandle, state: State<'_, MirrorState>, enabled: bool) {
    state.toggle_mode.store(enabled, Ordering::SeqCst);
    sync_tray_controls(&app, &state);
}

#[tauri::command]
fn set_display_options(app: AppHandle, state: State<'_, MirrorState>, options: DisplayOptions) {
    state.mirrored.store(options.mirrored, Ordering::SeqCst);
    state.grayscale.store(options.grayscale, Ordering::SeqCst);
    sync_tray_controls(&app, &state);
}

fn sync_tray_controls(app: &AppHandle, state: &MirrorState) {
    if let Some(controls) = app.try_state::<TrayControls>() {
        let _ = controls
            .mirrored
            .set_checked(state.mirrored.load(Ordering::SeqCst));
        let _ = controls
            .grayscale
            .set_checked(state.grayscale.load(Ordering::SeqCst));
        let _ = controls
            .move_enabled
            .set_checked(state.move_enabled.load(Ordering::SeqCst));
        let _ = controls
            .toggle_mode
            .set_checked(state.toggle_mode.load(Ordering::SeqCst));
    }
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

/// Task Bar Hero風オーバーレイとして、タスクバー右端の中に収める位置を返す。
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

#[cfg(target_os = "windows")]
fn move_cursor_to_mirror_center(app: &AppHandle) {
    if let Ok(window) = mirror_window(app) {
        if let (Ok(position), Ok(size)) = (window.outer_position(), window.outer_size()) {
            let x = position.x + size.width as i32 / 2;
            let y = position.y + size.height as i32 / 2;
            let _ = unsafe { windows_sys::Win32::UI::WindowsAndMessaging::SetCursorPos(x, y) };
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn move_cursor_to_mirror_center(_app: &AppHandle) {}

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
                    app.state::<MirrorState>()
                        .moved_while_shortcut_held
                        .store(true, Ordering::SeqCst);
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

#[cfg(target_os = "windows")]
fn apply_taskbar_overlay(hwnd: isize, refresh_frame: bool) {
    use windows_sys::Win32::{
        Foundation::HWND,
        UI::WindowsAndMessaging::{
            GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE, HWND_TOPMOST,
            SWP_ASYNCWINDOWPOS, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
            WS_EX_LAYERED, WS_EX_NOACTIVATE,
        },
    };

    if hwnd == 0 {
        return;
    }
    let hwnd = hwnd as HWND;
    let style = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) };
    let overlay_style = style | WS_EX_LAYERED as isize | WS_EX_NOACTIVATE as isize;
    if overlay_style != style {
        unsafe { SetWindowLongPtrW(hwnd, GWL_EXSTYLE, overlay_style) };
    }
    let mut flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS;
    if refresh_frame {
        flags |= SWP_FRAMECHANGED;
    }
    unsafe {
        SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            flags,
        )
    };
}

#[cfg(target_os = "windows")]
fn start_taskbar_overlay_refresh(app: AppHandle) {
    let Some(overlay) = app.try_state::<TaskbarOverlayState>() else {
        return;
    };
    overlay.enabled.store(true, Ordering::SeqCst);
    // ウィンドウスタイルを変えた直後だけ、非クライアント領域を再計算する。
    apply_taskbar_overlay(overlay.hwnd.load(Ordering::SeqCst), true);

    if overlay.refresh_running.swap(true, Ordering::SeqCst) {
        return;
    }
    thread::spawn(move || loop {
        let Some(overlay) = app.try_state::<TaskbarOverlayState>() else {
            return;
        };
        if !overlay.enabled.load(Ordering::SeqCst) {
            overlay.refresh_running.store(false, Ordering::SeqCst);
            return;
        }
        // 定期処理はz-orderだけを静かに再適用する。映像や枠の再描画は発生させない。
        apply_taskbar_overlay(overlay.hwnd.load(Ordering::SeqCst), false);
        thread::sleep(Duration::from_secs(1));
    });
}

#[cfg(target_os = "windows")]
fn stop_taskbar_overlay_refresh(app: &AppHandle) {
    if let Some(overlay) = app.try_state::<TaskbarOverlayState>() {
        overlay.enabled.store(false, Ordering::SeqCst);
    }
}

#[cfg(not(target_os = "windows"))]
fn start_taskbar_overlay_refresh(_app: AppHandle) {}

#[cfg(not(target_os = "windows"))]
fn stop_taskbar_overlay_refresh(_app: &AppHandle) {}

fn show_mirror_window(app: &AppHandle) {
    if let Ok(window) = mirror_window(app) {
        let _ = window.show();
        let _ = window.set_ignore_cursor_events(true);
    }
    start_taskbar_overlay_refresh(app.clone());
    let _ = app.emit_to(MIRROR_LABEL, "mirror:show", ());
}

fn hide_mirror_window(app: &AppHandle) {
    stop_taskbar_overlay_refresh(app);
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

fn show_held_mirror(app: &AppHandle) {
    let state = app.state::<MirrorState>();
    let should_show = {
        let mut mode = state.display_mode.lock().expect("表示状態のロックに失敗しました");
        if *mode == DisplayMode::Hidden {
            *mode = DisplayMode::Held;
            true
        } else {
            false
        }
    };

    state.shortcut_held.store(true, Ordering::SeqCst);
    if should_show {
        show_mirror_window(app);
    }
    move_cursor_to_mirror_center(app);

    if state.move_enabled.load(Ordering::SeqCst) {
        start_pointer_tracking(app.clone());
    }
}

fn release_held_mirror(app: &AppHandle) {
    let state = app.state::<MirrorState>();
    state.shortcut_held.store(false, Ordering::SeqCst);
    let should_hide = {
        let mut mode = state.display_mode.lock().expect("表示状態のロックに失敗しました");
        if *mode == DisplayMode::Held {
            *mode = DisplayMode::Hidden;
            true
        } else {
            false
        }
    };
    if should_hide {
        hide_mirror_window(app);
    }
}

fn toggle_mirror(app: &AppHandle) {
    let state = app.state::<MirrorState>();
    state.shortcut_held.store(false, Ordering::SeqCst);
    let should_show = {
        let mut mode = state.display_mode.lock().expect("表示状態のロックに失敗しました");
        match *mode {
            DisplayMode::Hidden | DisplayMode::Held => {
                *mode = DisplayMode::Toggled;
                true
            }
            DisplayMode::Toggled => {
                *mode = DisplayMode::Hidden;
                false
            }
        }
    };
    if should_show {
        show_mirror_window(app);
    } else {
        hide_mirror_window(app);
    }
}

fn press_toggle_shortcut(app: &AppHandle) {
    let state = app.state::<MirrorState>();
    let (should_show, started_visible) = {
        let mut mode = state.display_mode.lock().expect("表示状態のロックに失敗しました");
        match *mode {
            DisplayMode::Hidden => {
                *mode = DisplayMode::Toggled;
                (true, false)
            }
            DisplayMode::Toggled => (false, true),
            DisplayMode::Held => {
                *mode = DisplayMode::Toggled;
                (false, true)
            }
        }
    };

    state.shortcut_held.store(true, Ordering::SeqCst);
    state
        .moved_while_shortcut_held
        .store(false, Ordering::SeqCst);
    state
        .toggle_press_started_visible
        .store(started_visible, Ordering::SeqCst);

    if should_show {
        show_mirror_window(app);
    }
    move_cursor_to_mirror_center(app);
    if state.move_enabled.load(Ordering::SeqCst) {
        start_pointer_tracking(app.clone());
    }
}

fn release_toggle_shortcut(app: &AppHandle) {
    let state = app.state::<MirrorState>();
    state.shortcut_held.store(false, Ordering::SeqCst);

    let should_hide = state
        .toggle_press_started_visible
        .load(Ordering::SeqCst)
        && !state.moved_while_shortcut_held.load(Ordering::SeqCst);

    if should_hide {
        let mut mode = state.display_mode.lock().expect("表示状態のロックに失敗しました");
        if *mode == DisplayMode::Toggled {
            *mode = DisplayMode::Hidden;
            drop(mode);
            hide_mirror_window(app);
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(MirrorState::default())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    let hold_shortcut = Shortcut::new(
                        Some(Modifiers::CONTROL | Modifiers::ALT),
                        Code::Space,
                    );
                    if shortcut == &hold_shortcut {
                        if app.state::<MirrorState>().toggle_mode.load(Ordering::SeqCst) {
                            match event.state() {
                                ShortcutState::Pressed => press_toggle_shortcut(app),
                                ShortcutState::Released => release_toggle_shortcut(app),
                            }
                        } else {
                            match event.state() {
                                ShortcutState::Pressed => show_held_mirror(app),
                                ShortcutState::Released => release_held_mirror(app),
                            }
                        }
                    }
                })
                .build(),
        )
        .plugin(tauri_plugin_store::Builder::default().build())
        .setup(|app| {
            #[cfg(target_os = "windows")]
            {
                let overlay = TaskbarOverlayState::default();
                if let Some(window) = app.get_webview_window(MIRROR_LABEL) {
                    if let Ok(hwnd) = window.hwnd() {
                        overlay.hwnd.store(hwnd.0 as isize, Ordering::SeqCst);
                    }
                }
                app.manage(overlay);
            }
            let mirror_shortcut = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::Space);
            app.global_shortcut().register(mirror_shortcut)?;

            let is_first_launch = app
                .path()
                .app_data_dir()
                .map(|path| !path.join("rearview-mirror.json").exists())
                .unwrap_or(true);

            WebviewWindowBuilder::new(app, SETTINGS_LABEL, WebviewUrl::App("index.html".into()))
                .title("Rearview Mirror 設定")
                .inner_size(360.0, 420.0)
                .min_inner_size(320.0, 360.0)
                .resizable(true)
                .visible(is_first_launch)
                .build()?;

            let toggle_item = MenuItem::with_id(app, "toggle", "表示を切り替え", true, None::<&str>)?;
            let mirrored_item = CheckMenuItem::with_id(app, "mirrored", "左右を反転", true, true, None::<&str>)?;
            let grayscale_item = CheckMenuItem::with_id(app, "grayscale", "白黒で表示", true, false, None::<&str>)?;
            let move_item = CheckMenuItem::with_id(app, "move", "マウス移動を有効にする", true, true, None::<&str>)?;
            let toggle_mode_item = CheckMenuItem::with_id(app, "toggle-mode", "ショートカットを切替表示にする", true, true, None::<&str>)?;
            let settings_item = MenuItem::with_id(app, "settings", "カメラ・サイズ設定…", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "終了", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&toggle_item, &mirrored_item, &grayscale_item, &move_item, &toggle_mode_item, &settings_item, &quit_item])?;
            let mirrored_item_for_event = mirrored_item.clone();
            let grayscale_item_for_event = grayscale_item.clone();
            let move_item_for_event = move_item.clone();
            let toggle_mode_item_for_event = toggle_mode_item.clone();
            app.manage(TrayControls {
                mirrored: mirrored_item.clone(),
                grayscale: grayscale_item.clone(),
                move_enabled: move_item.clone(),
                toggle_mode: toggle_mode_item.clone(),
            });
            TrayIconBuilder::with_id("rearview-mirror-tray")
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("Rearview Mirror")
                .menu(&menu)
                .on_menu_event(move |app, event| match event.id.as_ref() {
                    "toggle" => toggle_mirror(app),
                    "mirrored" | "grayscale" | "move" | "toggle-mode" => {
                        let state = app.state::<MirrorState>();
                        if event.id.as_ref() == "mirrored" {
                            let checked = !state.mirrored.fetch_xor(true, Ordering::SeqCst);
                            let _ = mirrored_item_for_event.set_checked(checked);
                        } else if event.id.as_ref() == "grayscale" {
                            let checked = !state.grayscale.fetch_xor(true, Ordering::SeqCst);
                            let _ = grayscale_item_for_event.set_checked(checked);
                        } else if event.id.as_ref() == "move" {
                            let checked = !state.move_enabled.fetch_xor(true, Ordering::SeqCst);
                            let _ = move_item_for_event.set_checked(checked);
                        } else {
                            let checked = !state.toggle_mode.fetch_xor(true, Ordering::SeqCst);
                            let _ = toggle_mode_item_for_event.set_checked(checked);
                        }
                        let options = TrayOptions {
                            mirrored: state.mirrored.load(Ordering::SeqCst),
                            grayscale: state.grayscale.load(Ordering::SeqCst),
                            move_enabled: state.move_enabled.load(Ordering::SeqCst),
                            toggle_mode: state.toggle_mode.load(Ordering::SeqCst),
                        };
                        let _ = app.emit_to(MIRROR_LABEL, "mirror:tray-options", options.clone());
                        let _ = app.emit_to(SETTINGS_LABEL, "settings:tray-options", options);
                    }
                    "settings" => show_settings(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            set_move_enabled,
            set_toggle_mode,
            set_display_options,
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
