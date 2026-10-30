use anyhow::{anyhow, Context, Result};
use autoplay::{
    events::{CoreEvent, EventSink, LogLevel},
    runtime::{resolve_model_path, run_autoplay, RuntimeOptions},
    settings::{
        default_device_store_path, read_settings_unchecked, resolve_account_device_id,
        resolve_account_device_id_in_store, validate_settings, Account, AutoplaySettings, Settings,
    },
};
use clap::{Parser, Subcommand};
use mortal::{
    candle_engine::CandleMortalEngine,
    model_import::ensure_exported_model_dir,
    native::{NativeEngine, Observation},
};
use protocol::session;
use serde::Deserialize;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{atomic::AtomicBool, Arc},
};
use tokio::sync::Semaphore;

#[derive(Parser)]
#[command(name = "majsoul-autopilot-rs")]
#[command(about = "Pure Liqi protocol Mahjong Soul autopilot in Rust")]
struct Cli {
    #[arg(long, default_value = "settings.json")]
    settings: PathBuf,

    #[arg(
        long,
        global = true,
        value_name = "MODEL_DIR",
        help = "Override model directory containing model.safetensors and model_config.json"
    )]
    model: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    #[command(about = "Check Mahjong Soul email/password login and target room")]
    CheckLogin,
    #[command(about = "Check the model configured in settings.json")]
    CheckModel,
    #[command(about = "Validate a replay fixture path")]
    ReplayFixture { fixture: PathBuf },
    #[command(about = "Run one account from settings.json")]
    Run {
        #[arg(long)]
        max_games: Option<u32>,
    },
    #[command(about = "Run several accounts concurrently from an accounts JSON file")]
    RunMany {
        #[arg(long, help = "Path to accounts.json")]
        accounts: PathBuf,

        #[arg(long, help = "Override max_games for every account")]
        max_games: Option<u32>,

        #[arg(
            long,
            default_value_t = 0,
            help = "Maximum parallel accounts; 0 means all"
        )]
        concurrency: usize,
    },
}

#[derive(Debug, Deserialize)]
struct MultiAccountFile {
    #[serde(default)]
    model_path: Option<String>,
    #[serde(default)]
    autoplay: Option<AutoplaySettings>,
    accounts: Vec<MultiAccountEntry>,
}

#[derive(Debug, Deserialize)]
struct MultiAccountEntry {
    username: String,
    password: String,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    device_id: Option<String>,
    #[serde(default)]
    model_path: Option<String>,
    #[serde(default)]
    autoplay: Option<AutoplaySettings>,
}

#[derive(Debug, Clone)]
struct AccountRun {
    label: String,
    device_id: String,
    settings: Settings,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let mut settings = read_settings_unchecked(&cli.settings)?;
    apply_model_override(&mut settings, cli.model.as_deref());
    if command_requires_valid_settings(&cli.command) {
        validate_settings(&settings)?;
    }

    match cli.command {
        Command::CheckLogin => {
            let device_id = resolve_account_device_id(
                &settings.autoplay_account.username,
                settings.autoplay_account.device_id.as_deref(),
                Some(&cli.settings),
            )?;
            let summary = session::check_login(
                &settings.autoplay_account.username,
                &settings.autoplay_account.password,
                &device_id,
            )
            .await?;
            println!(
                "login ok: account_id={} nickname={} level={} tier={} target={:?}/{:?} device_id={}",
                summary.account_id,
                summary.nickname,
                summary.level_id,
                summary.rank_tier,
                summary.target_mode,
                summary.target_room,
                device_id
            );
        }
        Command::CheckModel => {
            check_model(&settings.model_path, Some(&cli.settings))?;
        }
        Command::ReplayFixture { fixture } => {
            if !fixture.exists() {
                return Err(anyhow!("fixture not found: {}", fixture.display()));
            }
            println!("fixture path ok: {}", fixture.display());
        }
        Command::Run { max_games } => {
            if max_games.is_some() {
                settings.autoplay.max_games = max_games;
            }
            let device_id = resolve_account_device_id(
                &settings.autoplay_account.username,
                settings.autoplay_account.device_id.as_deref(),
                Some(&cli.settings),
            )?;
            run_autoplay(
                RuntimeOptions {
                    settings,
                    settings_path: Some(cli.settings),
                    device_id,
                },
                prefixed_stdout_sink("main".to_string()),
                Arc::new(AtomicBool::new(false)),
            )
            .await?;
        }
        Command::RunMany {
            accounts,
            max_games,
            concurrency,
        } => {
            let store_path = default_device_store_path(Some(&cli.settings));
            let runs = load_multi_account_runs(&settings, &accounts, max_games, &store_path)
                .with_context(|| format!("load accounts from {}", accounts.display()))?;
            run_many(runs, cli.settings, concurrency).await?;
        }
    }

    Ok(())
}

fn command_requires_valid_settings(command: &Command) -> bool {
    matches!(command, Command::CheckLogin | Command::Run { .. })
}

fn apply_model_override(settings: &mut Settings, model: Option<&Path>) {
    if let Some(model) = model {
        settings.model_path = model.display().to_string();
    }
}

fn check_model(raw_path: &str, settings_path: Option<&Path>) -> Result<()> {
    let path = resolve_model_path(raw_path, settings_path)?;
    if !path.exists() {
        return Err(anyhow!("model path not found: {}", path.display()));
    }
    ensure_exported_model_dir(&path)?;
    let mut engine = CandleMortalEngine::load(&path)
        .with_context(|| format!("load exported Mortal model from {}", path.display()))?;
    let mut mask = vec![false; 46];
    mask[0] = true;
    let decisions = engine.react_batch(&[Observation {
        values: vec![0.0; 1012 * 34],
        mask,
        channels: 1012,
        width: 34,
    }])?;
    println!(
        "model ok: {} action={} version={}",
        path.display(),
        decisions[0].action,
        engine.version()
    );
    Ok(())
}

fn load_multi_account_runs(
    base: &Settings,
    accounts_path: &Path,
    max_games: Option<u32>,
    store_path: &Path,
) -> Result<Vec<AccountRun>> {
    let raw = fs::read_to_string(accounts_path)
        .with_context(|| format!("read accounts from {}", accounts_path.display()))?;
    let file: MultiAccountFile = serde_json::from_str(&raw)
        .with_context(|| format!("parse accounts from {}", accounts_path.display()))?;
    if file.accounts.is_empty() {
        return Err(anyhow!("accounts file must contain at least one account"));
    }

    let mut runs = Vec::with_capacity(file.accounts.len());
    for (index, account) in file.accounts.into_iter().enumerate() {
        let mut settings = base.clone();
        if let Some(model_path) = file.model_path.clone() {
            settings.model_path = model_path;
        }
        if let Some(autoplay) = file.autoplay.clone() {
            settings.autoplay = autoplay;
        }
        if let Some(model_path) = account.model_path {
            settings.model_path = model_path;
        }
        if let Some(autoplay) = account.autoplay {
            settings.autoplay = autoplay;
        }
        if max_games.is_some() {
            settings.autoplay.max_games = max_games;
        }
        let device_id = resolve_account_device_id_in_store(
            &account.username,
            account.device_id.as_deref(),
            store_path,
        )?;
        settings.autoplay_account = Account {
            username: account.username,
            password: account.password,
            device_id: Some(device_id.clone()),
        };
        let label = account
            .label
            .unwrap_or_else(|| format!("{}:{}", index + 1, settings.autoplay_account.username));
        runs.push(AccountRun {
            label,
            device_id,
            settings,
        });
    }
    Ok(runs)
}

async fn run_many(runs: Vec<AccountRun>, settings_path: PathBuf, concurrency: usize) -> Result<()> {
    let limit = if concurrency == 0 {
        runs.len()
    } else {
        concurrency.min(runs.len())
    };
    let semaphore = Arc::new(Semaphore::new(limit));
    let stop_flags = runs
        .iter()
        .map(|_| Arc::new(AtomicBool::new(false)))
        .collect::<Vec<_>>();
    let mut handles = Vec::with_capacity(runs.len());

    for (run, stop_flag) in runs.into_iter().zip(stop_flags) {
        let permit = semaphore.clone().acquire_owned().await?;
        let settings_path = settings_path.clone();
        handles.push(tokio::spawn(async move {
            let _permit = permit;
            let label = run.label.clone();
            let result = run_autoplay(
                RuntimeOptions {
                    settings: run.settings,
                    settings_path: Some(settings_path),
                    device_id: run.device_id,
                },
                prefixed_stdout_sink(label.clone()),
                stop_flag,
            )
            .await;
            (label, result)
        }));
    }

    let mut failures = Vec::new();
    for handle in handles {
        let (label, result) = handle.await?;
        match result {
            Ok(games) => println!("[{label}] finished: games={games}"),
            Err(err) => {
                eprintln!("[{label}] failed: {err}");
                failures.push(format!("{label}: {err}"));
            }
        }
    }

    if failures.is_empty() {
        Ok(())
    } else {
        Err(anyhow!(
            "{} account(s) failed: {}",
            failures.len(),
            failures.join("; ")
        ))
    }
}

fn prefixed_stdout_sink(label: String) -> EventSink {
    Arc::new(move |event| match event {
        CoreEvent::Log { level, message } => {
            let level = match level {
                LogLevel::Info => "INFO",
                LogLevel::Warn => "WARN",
                LogLevel::Error => "ERROR",
            };
            println!("[{label}] {level}: {message}");
        }
        CoreEvent::GameEvent { event } => println!("[{label}] event: {event:?}"),
        CoreEvent::GameCompleted { games_done } => {
            println!("[{label}] game completed: total_games={games_done}");
        }
        CoreEvent::RuntimeError { message } => eprintln!("[{label}] ERROR: {message}"),
        _ => {}
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_file(name: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("majsoul-cli-{name}-{stamp}.json"))
    }

    #[test]
    fn check_model_does_not_require_login_credentials() {
        assert!(!command_requires_valid_settings(&Command::CheckModel));
        assert!(!command_requires_valid_settings(&Command::RunMany {
            accounts: PathBuf::from("accounts.json"),
            max_games: None,
            concurrency: 0,
        }));
    }

    #[test]
    fn single_account_run_commands_require_login_credentials() {
        assert!(command_requires_valid_settings(&Command::CheckLogin));
        assert!(command_requires_valid_settings(&Command::Run {
            max_games: None
        }));
    }

    #[test]
    fn model_override_replaces_settings_model_path() {
        let mut settings = Settings {
            model_path: "models/mortal".to_string(),
            ..Settings::default()
        };

        apply_model_override(&mut settings, Some(Path::new("/tmp/custom-model")));

        assert_eq!(settings.model_path, "/tmp/custom-model");
    }

    #[test]
    fn global_model_argument_is_available_to_subcommands() {
        let cli = Cli::try_parse_from([
            "majsoul-autopilot-rs",
            "--settings",
            "settings.json",
            "check-model",
            "--model",
            "/tmp/custom-model",
        ])
        .unwrap();

        assert_eq!(cli.model.as_deref(), Some(Path::new("/tmp/custom-model")));
    }

    #[test]
    fn multi_account_file_merges_shared_defaults_and_overrides() {
        let path = temp_file("accounts");
        let store = temp_file("device-store");
        fs::write(
            &path,
            r#"{
  "model_path": "models/shared",
  "autoplay": {
    "room_policy": {"type": "manual"},
    "manual_room": "silver",
    "manual_mode": "four_player_south",
    "action_interval_ms": {"min": 1000, "max": 1200},
    "max_games": 9
  },
  "accounts": [
    {"username": "a@example.com", "password": "pa", "label": "alpha"},
    {
      "username": "b@example.com",
      "password": "pb",
      "model_path": "models/b",
      "autoplay": {
        "room_policy": {"type": "manual"},
        "manual_room": "gold",
        "manual_mode": "four_player_east",
        "action_interval_ms": {"min": 800, "max": 900},
        "max_games": 2
      }
    }
  ]
}"#,
        )
        .unwrap();
        let base = Settings::default();

        let runs = load_multi_account_runs(&base, &path, Some(1), &store).unwrap();
        let runs_again = load_multi_account_runs(&base, &path, Some(1), &store).unwrap();

        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].label, "alpha");
        assert_eq!(runs[0].settings.model_path, "models/shared");
        assert_eq!(runs[0].settings.autoplay.max_games, Some(1));
        assert_eq!(runs[1].settings.model_path, "models/b");
        assert_eq!(runs[1].settings.autoplay.max_games, Some(1));
        assert!(autoplay::settings::is_valid_browser_uuid_v4(&runs[0].device_id));
        assert!(autoplay::settings::is_valid_browser_uuid_v4(&runs[1].device_id));
        assert_ne!(runs[0].device_id, runs[1].device_id);
        assert_eq!(runs[0].device_id, runs_again[0].device_id);
        assert_eq!(runs[1].device_id, runs_again[1].device_id);
        let _ = fs::remove_file(path);
        let _ = fs::remove_file(store);
    }

    #[test]
    fn multi_account_file_requires_accounts() {
        let path = temp_file("empty-accounts");
        let store = temp_file("empty-store");
        fs::write(&path, r#"{"accounts":[]}"#).unwrap();

        let error = load_multi_account_runs(&Settings::default(), &path, None, &store)
            .unwrap_err()
            .to_string();

        assert!(error.contains("at least one account"));
        let _ = fs::remove_file(path);
        let _ = fs::remove_file(store);
    }
}
