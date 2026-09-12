use anyhow::{anyhow, Context};
use autoplay::{
    events::{CoreEvent, EventSink},
    runtime::{AutoplayController, RuntimeOptions},
    settings::{read_settings_unchecked, validate_settings, write_settings, Settings},
};
use mortal::model_import::{ensure_exported_model_dir, import_model_file};
use serde::Serialize;
use std::{
    collections::VecDeque,
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::{Emitter, Manager, State};

struct GuiState {
    settings_path: PathBuf,
    controller: AutoplayController,
    runtime_log_path: PathBuf,
    core_events: Arc<CoreEventBuffer>,
}

struct CoreEventBuffer {
    next_seq: AtomicU64,
    events: Mutex<VecDeque<CoreEventRecord>>,
}

#[derive(Debug, Clone, Serialize)]
struct RuntimeSnapshot {
    running: bool,
    status: autoplay::events::RuntimeStatus,
    last_error: Option<String>,
    settings_path: String,
    runtime_log_path: String,
}

#[derive(Debug, Clone, Serialize)]
struct CoreEventRecord {
    seq: u64,
    event: CoreEvent,
}

#[derive(Debug, Clone, Serialize)]
struct CoreEventBatch {
    cursor: u64,
    events: Vec<CoreEventRecord>,
}

#[derive(Debug, Clone, Serialize)]
struct ModelImportResult {
    model_path: String,
    model_name: String,
}

#[derive(Debug, Clone, Serialize)]
struct ModelChoice {
    label: String,
    model_path: String,
    builtin: bool,
}

#[tauri::command]
fn load_settings(state: State<'_, GuiState>) -> Result<Settings, String> {
    read_settings_unchecked(&state.settings_path).map_err(|err| err.to_string())
}

#[tauri::command]
fn save_settings(state: State<'_, GuiState>, settings: Settings) -> Result<(), String> {
    write_settings(&state.settings_path, &settings).map_err(|err| err.to_string())
}

#[tauri::command]
async fn import_model(
    state: State<'_, GuiState>,
    source_path: String,
) -> Result<ModelImportResult, String> {
    let settings_path = state.settings_path.clone();
    tokio::task::spawn_blocking(move || {
        import_model_from_source(Path::new(&source_path), &settings_path)
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(|err| err.to_string())
}

#[tauri::command]
fn list_models(state: State<'_, GuiState>) -> Result<Vec<ModelChoice>, String> {
    Ok(list_models_for_settings(&state.settings_path))
}

#[tauri::command]
async fn start_autoplay(state: State<'_, GuiState>, settings: Settings) -> Result<(), String> {
    validate_settings(&settings).map_err(|err| err.to_string())?;
    state
        .controller
        .start(RuntimeOptions {
            settings,
            settings_path: Some(state.settings_path.clone()),
            device_id: "rust-gui-device".to_string(),
        })
        .await
        .map_err(|err| err.to_string())
}

#[tauri::command]
fn stop_after_current_game(state: State<'_, GuiState>) -> Result<(), String> {
    state.controller.stop_after_current_game();
    Ok(())
}

#[tauri::command]
async fn emergency_stop(state: State<'_, GuiState>) -> Result<(), String> {
    state.controller.emergency_stop().await;
    Ok(())
}

#[tauri::command]
fn get_runtime_snapshot(state: State<'_, GuiState>) -> RuntimeSnapshot {
    let snapshot = state.controller.snapshot();
    RuntimeSnapshot {
        running: snapshot.running,
        status: snapshot.status,
        last_error: snapshot.last_error,
        settings_path: state.settings_path.display().to_string(),
        runtime_log_path: state.runtime_log_path.display().to_string(),
    }
}

#[tauri::command]
fn get_core_event_batch(state: State<'_, GuiState>, after: u64) -> CoreEventBatch {
    state.core_events.batch_after(after)
}

#[tauri::command]
async fn fetch_game_records(
    state: State<'_, GuiState>,
) -> Result<Vec<protocol::record::GameRecordSummary>, String> {
    let settings = read_settings_unchecked(&state.settings_path).map_err(|err| err.to_string())?;
    if settings.autoplay_account.username.trim().is_empty()
        || settings.autoplay_account.password.trim().is_empty()
    {
        return Err("未配置账号或密码".to_string());
    }
    let mut client = protocol::session::ProtocolClient::login(
        &settings.autoplay_account.username,
        &settings.autoplay_account.password,
        "rust-gui-device",
    )
    .await
    .map_err(|err| err.to_string())?;

    client.fetch_game_records(10).await.map_err(|err| err.to_string())
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let app_handle = app.handle().clone();
            let settings_path = resolve_settings_path(app)?;
            let runtime_log_path = runtime_log_path_for_settings(&settings_path);
            let runtime_log = Arc::new(Mutex::new(open_runtime_log(&runtime_log_path)?));
            let core_events = Arc::new(CoreEventBuffer::new());
            let sink_core_events = core_events.clone();
            write_runtime_log_header(&runtime_log, &settings_path, &runtime_log_path);
            let sink: EventSink = Arc::new(move |event: CoreEvent| {
                write_runtime_event(&runtime_log, &event);
                sink_core_events.push(event.clone());
                let _ = app_handle.emit("core-event", event);
            });
            app.manage(GuiState {
                settings_path,
                controller: AutoplayController::new(sink),
                runtime_log_path,
                core_events,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            load_settings,
            save_settings,
            import_model,
            list_models,
            start_autoplay,
            stop_after_current_game,
            emergency_stop,
            get_runtime_snapshot,
            get_core_event_batch,
            fetch_game_records
        ])
        .run(tauri::generate_context!())
        .expect("error while running Tauri app");
}

fn resolve_settings_path(app: &tauri::App) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let current_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let current_exe = std::env::current_exe().ok();

    let config_dir = app.path().app_config_dir()?;
    std::fs::create_dir_all(&config_dir)?;
    Ok(resolve_settings_path_from(
        &current_dir,
        current_exe.as_deref(),
        &config_dir,
    ))
}

fn resolve_settings_path_from(
    current_dir: &Path,
    current_exe: Option<&Path>,
    fallback_config_dir: &Path,
) -> PathBuf {
    let mut bases = Vec::new();
    push_unique_base(&mut bases, current_dir.to_path_buf());
    if let Some(exe_parent) = current_exe.and_then(Path::parent) {
        for ancestor in exe_parent.ancestors() {
            push_unique_base(&mut bases, ancestor.to_path_buf());
        }
    }

    for base in bases {
        let settings = base.join("settings.json");
        if settings.exists() || base.join("settings.example.json").exists() {
            return settings;
        }
    }

    fallback_config_dir.join("settings.json")
}

fn push_unique_base(bases: &mut Vec<PathBuf>, base: PathBuf) {
    if !bases.iter().any(|existing| existing == &base) {
        bases.push(base);
    }
}

fn runtime_log_path_for_settings(settings_path: &Path) -> PathBuf {
    settings_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("logs/runtime/gui_autoplay.log")
}

fn import_model_from_source(
    source: &Path,
    settings_path: &Path,
) -> anyhow::Result<ModelImportResult> {
    if !source.exists() {
        return Err(anyhow!("model source not found: {}", source.display()));
    }
    if source.is_dir() {
        return Err(anyhow!(
            "model directory import is not supported; choose a .safetensors file"
        ));
    }

    let store_root = imported_model_store_dir(settings_path);
    fs::create_dir_all(&store_root)
        .with_context(|| format!("create imported model store {}", store_root.display()))?;

    let model_name = source
        .file_stem()
        .or_else(|| source.file_name())
        .and_then(|name| name.to_str())
        .unwrap_or("model");
    let output_dir = store_root.join(format!(
        "{}-{}",
        mortal::model_import::sanitize_model_name(model_name),
        now_ms()
    ));
    let result = import_model_file(source, &output_dir).map(|result| ModelImportResult {
        model_path: result.model_path.display().to_string(),
        model_name: result.model_name,
    });

    if result.is_err() {
        let _ = fs::remove_dir_all(&output_dir);
    }
    result
}

fn imported_model_store_dir(settings_path: &Path) -> PathBuf {
    settings_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("models/imported")
}

fn list_models_for_settings(settings_path: &Path) -> Vec<ModelChoice> {
    let mut models = vec![ModelChoice {
        label: "mortal".to_string(),
        model_path: "models/mortal".to_string(),
        builtin: true,
    }];

    let store_root = imported_model_store_dir(settings_path);
    let Ok(entries) = fs::read_dir(store_root) else {
        return models;
    };
    let mut imported = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if !path.is_dir() || ensure_exported_model_dir(&path).is_err() {
                return None;
            }
            let label = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("imported-model")
                .to_string();
            Some(ModelChoice {
                label,
                model_path: path.display().to_string(),
                builtin: false,
            })
        })
        .collect::<Vec<_>>();
    imported.sort_by(|a, b| a.label.cmp(&b.label));
    models.extend(imported);
    models
}

fn open_runtime_log(path: &Path) -> Result<File, Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(OpenOptions::new().create(true).append(true).open(path)?)
}

fn write_runtime_log_header(
    runtime_log: &Arc<Mutex<File>>,
    settings_path: &Path,
    runtime_log_path: &Path,
) {
    let ts_ms = now_ms();
    if let Ok(mut file) = runtime_log.lock() {
        let _ = writeln!(
            file,
            "{{\"ts_ms\":{ts_ms},\"event\":{{\"type\":\"gui_session_start\",\"settings_path\":{},\"runtime_log_path\":{}}}}}",
            serde_json::to_string(&settings_path.display().to_string()).unwrap_or_else(|_| "\"\"".to_string()),
            serde_json::to_string(&runtime_log_path.display().to_string()).unwrap_or_else(|_| "\"\"".to_string())
        );
    }
}

fn write_runtime_event(runtime_log: &Arc<Mutex<File>>, event: &CoreEvent) {
    let Ok(payload) = serde_json::to_string(event) else {
        return;
    };
    let ts_ms = now_ms();
    if let Ok(mut file) = runtime_log.lock() {
        let _ = writeln!(file, "{{\"ts_ms\":{ts_ms},\"event\":{payload}}}");
    }
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

impl CoreEventBuffer {
    fn new() -> Self {
        Self {
            next_seq: AtomicU64::new(1),
            events: Mutex::new(VecDeque::with_capacity(600)),
        }
    }

    fn push(&self, event: CoreEvent) {
        let seq = self.next_seq.fetch_add(1, Ordering::Relaxed);
        let mut events = self.events.lock().expect("core event buffer poisoned");
        events.push_back(CoreEventRecord { seq, event });
        while events.len() > 600 {
            events.pop_front();
        }
    }

    fn batch_after(&self, after: u64) -> CoreEventBatch {
        let events = self.events.lock().expect("core event buffer poisoned");
        let batch = events
            .iter()
            .filter(|record| record.seq > after)
            .cloned()
            .collect::<Vec<_>>();
        let cursor = batch
            .last()
            .map(|record| record.seq)
            .unwrap_or_else(|| self.next_seq.load(Ordering::Relaxed).saturating_sub(1));
        CoreEventBatch {
            cursor,
            events: batch,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn temp_dir(name: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("majsoul-autopilot-tauri-{name}-{stamp}"));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn settings_path_prefers_executable_ancestor_over_app_config_dir() {
        let root = temp_dir("ancestor");
        fs::write(root.join("settings.example.json"), "{}").unwrap();
        let exe = root.join("target/release/bundle/macos/Majsoul Autopilot.app/Contents/MacOS/app");
        fs::create_dir_all(exe.parent().unwrap()).unwrap();
        fs::write(&exe, "").unwrap();
        let app_config = temp_dir("config");

        let resolved = resolve_settings_path_from(Path::new("/"), Some(&exe), &app_config);

        assert_eq!(resolved, root.join("settings.json"));
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(app_config);
    }

    #[test]
    fn settings_path_keeps_current_directory_when_project_settings_exist() {
        let root = temp_dir("cwd");
        fs::write(root.join("settings.json"), "{}").unwrap();
        let app_config = temp_dir("config");

        let resolved = resolve_settings_path_from(&root, None, &app_config);

        assert_eq!(resolved, root.join("settings.json"));
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(app_config);
    }

    #[test]
    fn runtime_log_path_lives_next_to_project_settings() {
        let root = temp_dir("runtime-log");
        let path = runtime_log_path_for_settings(&root.join("settings.json"));

        assert_eq!(path, root.join("logs/runtime/gui_autoplay.log"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn imported_model_dir_lives_next_to_settings() {
        let root = temp_dir("model-store");
        let settings = root.join("settings.json");

        assert_eq!(
            imported_model_store_dir(&settings),
            root.join("models/imported")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn model_directory_import_is_rejected() {
        let root = temp_dir("import-dir");
        let source = root.join("source-model");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("model.safetensors"), b"weights").unwrap();
        fs::write(source.join("model_config.json"), b"{}").unwrap();

        let error = import_model_from_source(&source, &root.join("settings.json"))
            .unwrap_err()
            .to_string();

        assert!(error.contains("model directory import is not supported"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn model_list_includes_builtin_and_imported_exports() {
        let root = temp_dir("model-list");
        let store = imported_model_store_dir(&root.join("settings.json"));
        let imported = store.join("custom-model");
        fs::create_dir_all(&imported).unwrap();
        fs::write(imported.join("model.safetensors"), b"weights").unwrap();
        fs::write(imported.join("model_config.json"), b"{}").unwrap();
        fs::create_dir_all(store.join("broken-model")).unwrap();

        let models = list_models_for_settings(&root.join("settings.json"));

        assert_eq!(models.len(), 2);
        assert!(models[0].builtin);
        assert_eq!(models[0].label, "mortal");
        assert_eq!(models[0].model_path, "models/mortal");
        assert_eq!(models[1].label, "custom-model");
        assert!(!models[1].builtin);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn safetensors_file_uses_sibling_config() {
        let root = temp_dir("import-safetensors");
        let source = root.join("mortal.safetensors");
        fs::write(&source, b"weights").unwrap();
        fs::write(root.join("model_config.json"), b"{\"version\":4}").unwrap();

        let result = import_model_from_source(&source, &root.join("settings.json")).unwrap();
        let imported = PathBuf::from(result.model_path);

        assert_eq!(result.model_name, "mortal");
        assert_eq!(
            fs::read(imported.join("model.safetensors")).unwrap(),
            b"weights"
        );
        assert_eq!(
            fs::read(imported.join("model_config.json")).unwrap(),
            b"{\"version\":4}"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn unsupported_model_file_is_rejected() {
        let root = temp_dir("import-bad-extension");
        let source = root.join("mortal.txt");
        fs::write(&source, b"nope").unwrap();

        let error = import_model_from_source(&source, &root.join("settings.json"))
            .unwrap_err()
            .to_string();

        assert!(error.contains("unsupported model file .txt"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn pth_model_file_is_rejected() {
        let root = temp_dir("import-pth");
        let source = root.join("mortal.pth");
        fs::write(&source, b"checkpoint").unwrap();

        let error = import_model_from_source(&source, &root.join("settings.json"))
            .unwrap_err()
            .to_string();

        assert!(error.contains("only .safetensors"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn core_event_buffer_returns_only_events_after_cursor() {
        let buffer = CoreEventBuffer::new();
        buffer.push(CoreEvent::RuntimeStatus {
            status: autoplay::events::RuntimeStatus::LoggingIn,
        });
        buffer.push(CoreEvent::RuntimeStatus {
            status: autoplay::events::RuntimeStatus::Matching,
        });

        let first = buffer.batch_after(0);
        assert_eq!(first.events.len(), 2);
        assert_eq!(first.cursor, 2);

        buffer.push(CoreEvent::RuntimeStatus {
            status: autoplay::events::RuntimeStatus::InGame,
        });
        let second = buffer.batch_after(first.cursor);
        assert_eq!(second.events.len(), 1);
        assert_eq!(second.cursor, 3);
    }
}
