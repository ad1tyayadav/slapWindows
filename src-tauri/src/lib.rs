mod audio;
mod detector;

use std::{
    env, fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use audio::{AudioController, PlaybackMode};
use detector::{spawn_detector, DetectorConfig};
use cpal::traits::{DeviceTrait, HostTrait};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    AppHandle, Emitter, Manager, RunEvent, Wry,
};
use tauri_plugin_autostart::ManagerExt as AutostartExt;

const APP_EVENT_STATE: &str = "slap-state";
const MENU_SETTINGS_ID: &str = "settings";
const MENU_QUIT_ID: &str = "quit";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub amplitude_threshold: f32,
    pub freq_ratio_threshold: f32,
    pub cooldown_secs: f32,
    pub active_pack: String,
    pub playback_mode: String,
    pub selected_sound: Option<String>,
    pub slap_count: u64,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            amplitude_threshold: 0.08,
            freq_ratio_threshold: 1.15,
            cooldown_secs: 0.55,
            active_pack: "classic".into(),
            playback_mode: "cycle".into(),
            selected_sound: None,
            slap_count: 0,
        }
    }
}

impl AppConfig {
    fn detector_config(&self) -> DetectorConfig {
        DetectorConfig {
            amplitude_threshold: self.amplitude_threshold,
            freq_ratio_threshold: self.freq_ratio_threshold,
            cooldown_secs: self.cooldown_secs,
        }
    }

    fn apply_settings(&mut self, settings: SettingsPayload) {
        self.amplitude_threshold = settings.amplitude_threshold;
        self.freq_ratio_threshold = settings.freq_ratio_threshold;
        self.cooldown_secs = settings.cooldown_secs;
        self.active_pack = settings.active_pack;
        self.playback_mode = settings.playback_mode;
        self.selected_sound = settings.selected_sound;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingsPayload {
    pub amplitude_threshold: f32,
    pub freq_ratio_threshold: f32,
    pub cooldown_secs: f32,
    pub active_pack: String,
    pub playback_mode: String,
    pub selected_sound: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AppSnapshot {
    pub settings: SettingsPayload,
    pub slap_count: u64,
    pub sound_packs: Vec<String>,
    pub custom_packs: Vec<String>,
    pub active_pack_sounds: Vec<String>,
    pub active_pack_is_custom: bool,
    pub mic_error: Option<String>,
    pub custom_pack_root: String,
    pub meme_pack_hint: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConfigResponse {
    pub settings: SettingsPayload,
    pub slap_count: u64,
    pub sound_packs: Vec<String>,
    pub custom_packs: Vec<String>,
    pub active_pack_sounds: Vec<String>,
    pub active_pack_is_custom: bool,
    pub mic_error: Option<String>,
    pub autolaunch_enabled: bool,
}

impl From<&AppConfig> for SettingsPayload {
    fn from(config: &AppConfig) -> Self {
        Self {
            amplitude_threshold: config.amplitude_threshold,
            freq_ratio_threshold: config.freq_ratio_threshold,
            cooldown_secs: config.cooldown_secs,
            active_pack: config.active_pack.clone(),
            playback_mode: config.playback_mode.clone(),
            selected_sound: config.selected_sound.clone(),
        }
    }
}

struct AppState {
    config: Mutex<AppConfig>,
    detector_config: Arc<Mutex<DetectorConfig>>,
    audio: AudioController,
    config_path: PathBuf,
    custom_sounds_path: PathBuf,
    tray: Mutex<Option<tauri::tray::TrayIcon<Wry>>>,
    mic_error: Mutex<Option<String>>,
}

impl AppState {
    fn snapshot(&self) -> AppSnapshot {
        let config = self.config.lock().clone();
        AppSnapshot {
            settings: SettingsPayload::from(&config),
            slap_count: config.slap_count,
            sound_packs: self.audio.sound_packs().unwrap_or_default(),
            custom_packs: self.audio.custom_pack_names().unwrap_or_default(),
            active_pack_sounds: self.audio.sound_files(&config.active_pack).unwrap_or_default(),
            active_pack_is_custom: self.audio.is_custom_pack(&config.active_pack),
            mic_error: self.mic_error.lock().clone(),
            custom_pack_root: self.custom_sounds_path.display().to_string(),
            meme_pack_hint: self.custom_sounds_path.join("memes").display().to_string(),
        }
    }

    fn config_response(&self, app: &AppHandle<Wry>) -> ConfigResponse {
        let config = self.config.lock().clone();
        let autolaunch_enabled = app
            .autolaunch()
            .is_enabled()
            .unwrap_or(false);

        ConfigResponse {
            settings: SettingsPayload::from(&config),
            slap_count: config.slap_count,
            sound_packs: self.audio.sound_packs().unwrap_or_default(),
            custom_packs: self.audio.custom_pack_names().unwrap_or_default(),
            active_pack_sounds: self.audio.sound_files(&config.active_pack).unwrap_or_default(),
            active_pack_is_custom: self.audio.is_custom_pack(&config.active_pack),
            mic_error: self.mic_error.lock().clone(),
            autolaunch_enabled,
        }
    }
}

#[derive(Debug, Deserialize)]
struct ImportSoundPayload {
    file_name: String,
    bytes: Vec<u8>,
}

#[tauri::command]
fn get_snapshot(state: tauri::State<'_, AppState>) -> Result<AppSnapshot, String> {
    Ok(state.inner().snapshot())
}

#[tauri::command]
fn get_config(app: AppHandle<Wry>, state: tauri::State<'_, AppState>) -> Result<ConfigResponse, String> {
    Ok(state.inner().config_response(&app))
}

#[tauri::command]
fn set_settings(
    app: AppHandle<Wry>,
    state: tauri::State<'_, AppState>,
    settings: SettingsPayload,
) -> Result<AppSnapshot, String> {
    let state = state.inner();
    let sound_packs = state.audio.sound_packs()?;
    if !sound_packs.iter().any(|pack| pack == &settings.active_pack) {
        return Err(format!("unknown sound pack '{}'", settings.active_pack));
    }

    validate_playback_mode(&settings.playback_mode)?;
    let pack_files = state.audio.sound_files(&settings.active_pack)?;
    if pack_files.is_empty() {
        return Err(format!(
            "sound pack '{}' has no playable files",
            settings.active_pack
        ));
    }
    validate_selected_sound(
        &settings.playback_mode,
        settings.selected_sound.as_deref(),
        &pack_files,
    )?;

    {
        let mut config = state.config.lock();
        config.apply_settings(settings);
        normalize_sound_selection(&mut config, &state.audio)?;
        *state.detector_config.lock() = config.detector_config();
        persist_config(&state.config_path, &config)?;
    }

    let snapshot = state.snapshot();
    emit_snapshot(&app, &snapshot);
    Ok(snapshot)
}

#[tauri::command]
fn update_config(
    app: AppHandle<Wry>,
    state: tauri::State<'_, AppState>,
    key: String,
    value: Value,
) -> Result<ConfigResponse, String> {
    {
        let state = state.inner();
        let mut config = state.config.lock();

        match key.as_str() {
            "amplitude_threshold" => {
                config.amplitude_threshold = serde_json::from_value::<f32>(value)
                    .map_err(|error| format!("invalid amplitude_threshold value: {error}"))?;
            }
            "freq_ratio_threshold" => {
                config.freq_ratio_threshold = serde_json::from_value::<f32>(value)
                    .map_err(|error| format!("invalid freq_ratio_threshold value: {error}"))?;
            }
            "cooldown_secs" => {
                config.cooldown_secs = serde_json::from_value::<f32>(value)
                    .map_err(|error| format!("invalid cooldown_secs value: {error}"))?;
            }
            other => return Err(format!("unknown config key '{other}'")),
        }

        *state.detector_config.lock() = config.detector_config();
        persist_config(&state.config_path, &config)?;
    }

    let snapshot = state.inner().snapshot();
    emit_snapshot(&app, &snapshot);
    Ok(state.inner().config_response(&app))
}

#[tauri::command]
fn set_sound_pack(
    app: AppHandle<Wry>,
    state: tauri::State<'_, AppState>,
    pack: String,
) -> Result<ConfigResponse, String> {
    let state = state.inner();
    let sound_packs = state.audio.sound_packs()?;
    if !sound_packs.iter().any(|candidate| candidate == &pack) {
        return Err(format!("unknown sound pack '{pack}'"));
    }

    {
        let mut config = state.config.lock();
        config.active_pack = pack;
        normalize_sound_selection(&mut config, &state.audio)?;
        persist_config(&state.config_path, &config)?;
    }

    let snapshot = state.snapshot();
    emit_snapshot(&app, &snapshot);
    Ok(state.config_response(&app))
}

#[tauri::command]
fn play_test_sound(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let state = state.inner();
    let config = state.config.lock().clone();
    state.audio.play_sound(
        &config.active_pack,
        1.0,
        playback_mode_from_str(&config.playback_mode)?,
        config.selected_sound,
    )
}

#[tauri::command]
fn test_sound(state: tauri::State<'_, AppState>) -> Result<(), String> {
    play_test_sound(state)
}

#[tauri::command]
fn reset_slap_count(app: AppHandle<Wry>, state: tauri::State<'_, AppState>) -> Result<ConfigResponse, String> {
    {
        let state = state.inner();
        let mut config = state.config.lock();
        config.slap_count = 0;
        persist_config(&state.config_path, &config)?;
    }

    refresh_tray_tooltip(&app);
    let snapshot = state.inner().snapshot();
    emit_snapshot(&app, &snapshot);
    Ok(state.inner().config_response(&app))
}

#[tauri::command]
fn toggle_autolaunch(app: AppHandle<Wry>) -> Result<bool, String> {
    let manager = app.autolaunch();
    let enabled = manager
        .is_enabled()
        .map_err(|error| format!("failed to read auto-launch state: {error}"))?;

    if enabled {
        manager
            .disable()
            .map_err(|error| format!("failed to disable auto-launch: {error}"))?;
        Ok(false)
    } else {
        manager
            .enable()
            .map_err(|error| format!("failed to enable auto-launch: {error}"))?;
        Ok(true)
    }
}

#[tauri::command]
fn get_mic_name() -> Result<String, String> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or("no default microphone input device found")?;
    device
        .name()
        .map_err(|error| format!("failed to read microphone device name: {error}"))
}

#[tauri::command]
fn import_custom_sounds(
    app: AppHandle<Wry>,
    state: tauri::State<'_, AppState>,
    pack_name: Option<String>,
    files: Vec<ImportSoundPayload>,
) -> Result<AppSnapshot, String> {
    let state = state.inner();
    if files.is_empty() {
        return Err("no files selected".into());
    }

    let target_pack = pack_name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or("custom");

    let mut resolved_pack = None;
    for file in &files {
        resolved_pack = Some(state.audio.import_sound_to_pack(target_pack, &file.file_name, &file.bytes)?);
    }

    {
        let mut config = state.config.lock();
        config.active_pack = resolved_pack.unwrap_or_else(|| "custom".into());
        normalize_sound_selection(&mut config, &state.audio)?;
        persist_config(&state.config_path, &config)?;
    }

    let snapshot = state.snapshot();
    emit_snapshot(&app, &snapshot);
    Ok(snapshot)
}

#[tauri::command]
fn remove_custom_sound(
    app: AppHandle<Wry>,
    state: tauri::State<'_, AppState>,
    pack_name: String,
    file_name: String,
) -> Result<AppSnapshot, String> {
    let state = state.inner();
    state.audio.remove_custom_sound(&pack_name, &file_name)?;

    {
        let mut config = state.config.lock();
        normalize_active_pack(&mut config, &state.audio.sound_packs()?);
        normalize_sound_selection(&mut config, &state.audio)?;
        persist_config(&state.config_path, &config)?;
    }

    let snapshot = state.snapshot();
    emit_snapshot(&app, &snapshot);
    Ok(snapshot)
}

fn emit_snapshot(app: &AppHandle<Wry>, snapshot: &AppSnapshot) {
    let _ = app.emit(APP_EVENT_STATE, snapshot);
}

fn show_settings_window(app: &AppHandle<Wry>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn refresh_tray_tooltip(app: &AppHandle<Wry>) {
    let state = app.state::<AppState>();
    let state = state.inner();
    let tooltip = if let Some(error) = state.mic_error.lock().clone() {
        format!("Slap Windows - mic error: {error}")
    } else {
        let slap_count = state.config.lock().slap_count;
        format!("Slap Windows - {slap_count} slaps")
    };

    if let Some(tray) = state.tray.lock().as_ref() {
        let _ = tray.set_tooltip(Some(tooltip));
    }
}

fn persist_config(path: &Path, config: &AppConfig) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "failed to create config directory '{}': {error}",
                parent.display()
            )
        })?;
    }

    let json = serde_json::to_string_pretty(config)
        .map_err(|error| format!("failed to serialize config: {error}"))?;

    fs::write(path, json)
        .map_err(|error| format!("failed to write config file '{}': {error}", path.display()))
}

fn load_config(path: &Path) -> AppConfig {
    let Ok(contents) = fs::read_to_string(path) else {
        return AppConfig::default();
    };

    serde_json::from_str(&contents).unwrap_or_default()
}

fn config_path() -> PathBuf {
    if let Some(appdata) = env::var_os("APPDATA") {
        return PathBuf::from(appdata)
            .join("slapwindows")
            .join("config.json");
    }

    PathBuf::from("slapwindows-config.json")
}

fn custom_sounds_path() -> PathBuf {
    if let Some(appdata) = env::var_os("APPDATA") {
        return PathBuf::from(appdata)
            .join("slapwindows")
            .join("sounds");
    }

    PathBuf::from("slapwindows-sounds")
}

fn resolve_sounds_root(app: &AppHandle<Wry>) -> PathBuf {
    let mut candidates = Vec::new();

    if let Ok(resource_dir) = app.path().resource_dir() {
        candidates.push(resource_dir.join("sounds"));
        candidates.push(resource_dir.join("resources").join("sounds"));
    }

    candidates.push(PathBuf::from("resources").join("sounds"));
    candidates.push(PathBuf::from("src-tauri").join("resources").join("sounds"));

    candidates
        .into_iter()
        .find(|path| path.exists())
        .unwrap_or_else(|| PathBuf::from("src-tauri").join("resources").join("sounds"))
}

fn normalize_active_pack(config: &mut AppConfig, packs: &[String]) {
    if packs.is_empty() {
        return;
    }

    if !packs.iter().any(|pack| pack == &config.active_pack) {
        config.active_pack = packs[0].clone();
    }
}

fn validate_playback_mode(playback_mode: &str) -> Result<(), String> {
    match playback_mode {
        "cycle" | "random" | "single" => Ok(()),
        _ => Err(format!("unknown playback mode '{playback_mode}'")),
    }
}

fn playback_mode_from_str(playback_mode: &str) -> Result<PlaybackMode, String> {
    match playback_mode {
        "cycle" => Ok(PlaybackMode::Cycle),
        "random" => Ok(PlaybackMode::Random),
        "single" => Ok(PlaybackMode::Single),
        _ => Err(format!("unknown playback mode '{playback_mode}'")),
    }
}

fn validate_selected_sound(
    playback_mode: &str,
    selected_sound: Option<&str>,
    pack_files: &[String],
) -> Result<(), String> {
    if playback_mode != "single" {
        return Ok(());
    }

    let selected_sound = selected_sound.ok_or("single mode requires a selected sound")?;
    if pack_files.iter().any(|file| file == selected_sound) {
        Ok(())
    } else {
        Err(format!(
            "selected sound '{}' was not found in the active pack",
            selected_sound
        ))
    }
}

fn normalize_sound_selection(config: &mut AppConfig, audio: &AudioController) -> Result<(), String> {
    validate_playback_mode(&config.playback_mode)?;

    let pack_files = audio.sound_files(&config.active_pack)?;
    if pack_files.is_empty() {
        config.selected_sound = None;
        config.playback_mode = "cycle".into();
        return Ok(());
    }

    match config.playback_mode.as_str() {
        "single" => {
            if config
                .selected_sound
                .as_ref()
                .is_none_or(|selected| !pack_files.iter().any(|file| file == selected))
            {
                config.selected_sound = Some(pack_files[0].clone());
            }
        }
        "cycle" | "random" => {
            if config
                .selected_sound
                .as_ref()
                .is_some_and(|selected| !pack_files.iter().any(|file| file == selected))
            {
                config.selected_sound = None;
            }
        }
        _ => {}
    }

    Ok(())
}

fn handle_slap(app: &AppHandle<Wry>, force: f32) {
    let state = app.state::<AppState>();
    let state = state.inner();

    let (pack, playback_mode, selected_sound, config_to_persist) = {
        let mut config = state.config.lock();
        config.slap_count += 1;
        let pack = config.active_pack.clone();
        let playback_mode = config.playback_mode.clone();
        let selected_sound = config.selected_sound.clone();
        let config_to_persist = config.clone();
        (pack, playback_mode, selected_sound, config_to_persist)
    };

    let snapshot = state.snapshot();

    let _ = state.audio.play_sound(
        &pack,
        force,
        playback_mode_from_str(&playback_mode).unwrap_or(PlaybackMode::Cycle),
        selected_sound,
    );

    if let Err(error) = persist_config(&state.config_path, &config_to_persist) {
        eprintln!("config persist failed after slap: {error}");
    }

    refresh_tray_tooltip(app);
    emit_snapshot(app, &snapshot);
}

fn handle_mic_error(app: &AppHandle<Wry>, error: String) {
    {
        let state = app.state::<AppState>();
        let state = state.inner();
        *state.mic_error.lock() = Some(error.clone());
    }

    eprintln!("{error}");
    refresh_tray_tooltip(app);
    let snapshot = app.state::<AppState>().inner().snapshot();
    emit_snapshot(app, &snapshot);
}

fn build_tray(app: &AppHandle<Wry>) -> Result<tauri::tray::TrayIcon<Wry>, tauri::Error> {
    let settings = MenuItem::with_id(app, MENU_SETTINGS_ID, "Settings", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, MENU_QUIT_ID, "Quit", true, None::<&str>)?;
    let menu = Menu::new(app)?;
    menu.append(&settings)?;
    menu.append(&quit)?;

    let mut builder = TrayIconBuilder::with_id("main")
        .menu(&menu)
        .tooltip("Slap Windows - 0 slaps")
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            MENU_SETTINGS_ID => show_settings_window(app),
            MENU_QUIT_ID => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                tauri::tray::TrayIconEvent::DoubleClick { .. }
                    | tauri::tray::TrayIconEvent::Click {
                        button: tauri::tray::MouseButton::Left,
                        ..
                    }
            ) {
                show_settings_window(tray.app_handle());
            }
        });

    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }

    builder.build(app)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .app_name("Slap Windows")
                .build(),
        )
        .setup(|app| {
            let config_path = config_path();
            let custom_sounds_path = custom_sounds_path();
            let mut config = load_config(&config_path);
            let sounds_root = resolve_sounds_root(&app.handle());
            let audio = AudioController::new(sounds_root, custom_sounds_path.clone())
                .map_err(std::io::Error::other)?;
            let sound_packs = audio.sound_packs().map_err(std::io::Error::other)?;

            normalize_active_pack(&mut config, &sound_packs);
            normalize_sound_selection(&mut config, &audio).map_err(std::io::Error::other)?;
            persist_config(&config_path, &config).map_err(std::io::Error::other)?;

            let detector_config = Arc::new(Mutex::new(config.detector_config()));
            app.manage(AppState {
                config: Mutex::new(config),
                detector_config: detector_config.clone(),
                audio,
                config_path,
                custom_sounds_path,
                tray: Mutex::new(None),
                mic_error: Mutex::new(None),
            });

            let tray = build_tray(&app.handle())?;
            *app.state::<AppState>().inner().tray.lock() = Some(tray);
            refresh_tray_tooltip(&app.handle());

            let app_handle = app.handle().clone();
            let slap_handle = app_handle.clone();
            let error_handle = app_handle.clone();
            spawn_detector(
                detector_config,
                Arc::new(move |force| handle_slap(&slap_handle, force)),
                Arc::new(move |error| handle_mic_error(&error_handle, error)),
            );

            #[cfg(debug_assertions)]
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_skip_taskbar(false);
            }

            #[cfg(debug_assertions)]
            show_settings_window(&app_handle);

            emit_snapshot(&app_handle, &app.state::<AppState>().inner().snapshot());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            get_config,
            set_settings,
            update_config,
            set_sound_pack,
            play_test_sound,
            test_sound,
            reset_slap_count,
            toggle_autolaunch,
            get_mic_name,
            import_custom_sounds,
            remove_custom_sound
        ])
        .build(tauri::generate_context!())
        .expect("error while running tauri application")
        .run(|app, event| {
            if let RunEvent::WindowEvent { label, event, .. } = event {
                if label == "main" {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.hide();
                        }
                    }
                }
            }
        });
}
