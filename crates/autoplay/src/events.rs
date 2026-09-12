use crate::{action::BotAction, settings::Settings, table::TableSnapshot};
use mjai::bridge;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CoreEvent {
    RuntimeStatus { status: RuntimeStatus },
    AccountSnapshot { account: AccountSnapshot },
    TableSnapshot { table: TableSnapshot },
    GameEvent { event: bridge::Event },
    ModelDecision { decision: ModelDecision },
    ActionPlanned { action: PlannedAction },
    ActionAck { ack: ActionAck },
    Log { level: LogLevel, message: String },
    GameCompleted { games_done: u32 },
    StopScheduled { after_current_game: bool },
    RuntimeError { message: String },
    GameRecords { records: Vec<protocol::record::GameRecordSummary> },
}

#[derive(Debug, Clone, Serialize)]
pub struct AccountSnapshot {
    pub refreshing: bool,
    pub username: String,
    pub account_id: Option<u32>,
    pub nickname: Option<String>,
    pub level_id: Option<u32>,
    pub level_score: Option<u32>,
    pub rank_tier: Option<u32>,
    pub target_mode: Option<String>,
    pub target_room: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeStatus {
    Idle,
    LoggingIn,
    Matching,
    Reconnecting,
    InGame,
    StoppingAfterGame,
    Stopped,
    Error,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelDecision {
    pub action_index: usize,
    pub action_label: String,
    pub confidence: f32,
    pub candidates: Vec<ModelCandidate>,
    pub is_greedy: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelCandidate {
    pub action_index: usize,
    pub action_label: String,
    pub q_value: f32,
    pub confidence: f32,
    pub legal: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlannedAction {
    pub action: BotAction,
}

#[derive(Debug, Clone, Serialize)]
pub struct ActionAck {
    pub ok: bool,
    pub message: String,
}

pub type EventSink = std::sync::Arc<dyn Fn(CoreEvent) + Send + Sync + 'static>;

pub fn stdout_sink() -> EventSink {
    std::sync::Arc::new(|event| match event {
        CoreEvent::Log { level, message } => {
            println!("{level:?}: {message}");
        }
        CoreEvent::GameEvent { event } => println!("event: {event:?}"),
        CoreEvent::GameCompleted { games_done } => {
            println!("game loop completed: total_games={games_done}");
        }
        _ => {}
    })
}

pub fn action_label(index: usize) -> String {
    match index {
        0..=8 => format!("discard {}m", index + 1),
        9..=17 => format!("discard {}p", index - 8),
        18..=26 => format!("discard {}s", index - 17),
        27 => "discard E".to_string(),
        28 => "discard S".to_string(),
        29 => "discard W".to_string(),
        30 => "discard N".to_string(),
        31 => "discard P".to_string(),
        32 => "discard F".to_string(),
        33 => "discard C".to_string(),
        34 => "discard 5mr".to_string(),
        35 => "discard 5pr".to_string(),
        36 => "discard 5sr".to_string(),
        37 => "reach".to_string(),
        38 => "chi low".to_string(),
        39 => "chi mid".to_string(),
        40 => "chi high".to_string(),
        41 => "pon".to_string(),
        42 => "kan".to_string(),
        43 => "hora".to_string(),
        44 => "ryukyoku".to_string(),
        _ => "none".to_string(),
    }
}

pub fn settings_snapshot(settings: &Settings) -> serde_json::Value {
    serde_json::json!({
        "model_path": settings.model_path,
        "autoplay_account": {
            "username": settings.autoplay_account.username,
            "password": settings.autoplay_account.password,
        },
        "autoplay": settings.autoplay,
    })
}
