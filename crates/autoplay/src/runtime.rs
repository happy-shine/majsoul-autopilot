use crate::{
    action::{
        ActionState, BotAction, Operation, OperationContext, PendingAction, RpcPlan, OP_DISCARD,
        OP_LIQI,
    },
    events::{
        action_label, AccountSnapshot, ActionAck, CoreEvent, EventSink, LogLevel, ModelCandidate,
        ModelDecision, PlannedAction, RuntimeStatus,
    },
    settings::{manual_target, validate_settings, ActionInterval, Settings},
    table::{TableSnapshot, TableTracker},
};
use anyhow::{anyhow, Context, Result};
use liqi::pb;
use mjai::bridge;
use mortal::{
    candle_engine::CandleMortalEngine,
    native::{EngineDecision, NativeBot},
};
use protocol::{
    config::{Mode, Room},
    session::{self, LoginSummary, StartMatchResult},
};
use serde_json::{json, Value};
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::{sync::Mutex, task::JoinHandle};

pub struct AutoplayController {
    task: Mutex<Option<JoinHandle<Result<u32>>>>,
    running: Arc<AtomicBool>,
    stop_after_current_game: Arc<AtomicBool>,
    state: Arc<std::sync::Mutex<ControllerState>>,
    sink: EventSink,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControllerSnapshot {
    pub running: bool,
    pub status: RuntimeStatus,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone)]
struct ControllerState {
    status: RuntimeStatus,
    last_error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RuntimeOptions {
    pub settings: Settings,
    pub settings_path: Option<PathBuf>,
    pub device_id: String,
}

impl AutoplayController {
    pub fn new(sink: EventSink) -> Self {
        Self {
            task: Mutex::new(None),
            running: Arc::new(AtomicBool::new(false)),
            stop_after_current_game: Arc::new(AtomicBool::new(false)),
            state: Arc::new(std::sync::Mutex::new(ControllerState {
                status: RuntimeStatus::Idle,
                last_error: None,
            })),
            sink,
        }
    }

    pub async fn start(&self, options: RuntimeOptions) -> Result<()> {
        let mut task = self.task.lock().await;
        if task.as_ref().is_some_and(|handle| !handle.is_finished()) {
            return Err(anyhow!("autoplay is already running"));
        }
        self.stop_after_current_game.store(false, Ordering::Relaxed);
        self.running.store(true, Ordering::Relaxed);
        {
            let mut state = self.state.lock().expect("controller state mutex poisoned");
            state.status = RuntimeStatus::LoggingIn;
            state.last_error = None;
        }
        let user_sink = self.sink.clone();
        let running = self.running.clone();
        let stop_after_current_game = self.stop_after_current_game.clone();
        let state = self.state.clone();
        let handle = tokio::spawn(async move {
            let sink: EventSink = Arc::new({
                let user_sink = user_sink.clone();
                let state = state.clone();
                move |event| {
                    {
                        let mut state = state.lock().expect("controller state mutex poisoned");
                        match &event {
                            CoreEvent::RuntimeStatus { status } => state.status = *status,
                            CoreEvent::RuntimeError { message } => {
                                state.status = RuntimeStatus::Error;
                                state.last_error = Some(message.clone());
                            }
                            _ => {}
                        }
                    }
                    user_sink(event);
                }
            });
            let result = run_autoplay(options, sink.clone(), stop_after_current_game).await;
            running.store(false, Ordering::Relaxed);
            if let Err(err) = &result {
                {
                    let mut state = state.lock().expect("controller state mutex poisoned");
                    state.status = RuntimeStatus::Error;
                    state.last_error = Some(err.to_string());
                }
                sink(CoreEvent::RuntimeError {
                    message: err.to_string(),
                });
                sink(CoreEvent::RuntimeStatus {
                    status: RuntimeStatus::Error,
                });
            }
            result
        });
        *task = Some(handle);
        Ok(())
    }

    pub fn stop_after_current_game(&self) {
        self.stop_after_current_game.store(true, Ordering::Relaxed);
        (self.sink)(CoreEvent::StopScheduled {
            after_current_game: true,
        });
        (self.sink)(CoreEvent::RuntimeStatus {
            status: RuntimeStatus::StoppingAfterGame,
        });
    }

    pub async fn emergency_stop(&self) {
        if let Some(handle) = self.task.lock().await.take() {
            handle.abort();
        }
        self.running.store(false, Ordering::Relaxed);
        self.stop_after_current_game.store(false, Ordering::Relaxed);
        {
            let mut state = self.state.lock().expect("controller state mutex poisoned");
            state.status = RuntimeStatus::Stopped;
            state.last_error = None;
        }
        (self.sink)(CoreEvent::RuntimeStatus {
            status: RuntimeStatus::Stopped,
        });
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    pub fn snapshot(&self) -> ControllerSnapshot {
        let state = self.state.lock().expect("controller state mutex poisoned");
        ControllerSnapshot {
            running: self.is_running(),
            status: state.status,
            last_error: state.last_error.clone(),
        }
    }
}

pub async fn run_autoplay(
    options: RuntimeOptions,
    sink: EventSink,
    stop_after_current_game: Arc<AtomicBool>,
) -> Result<u32> {
    let RuntimeOptions {
        mut settings,
        settings_path,
        device_id,
    } = options;
    let resolved_model_path = resolve_model_path(&settings.model_path, settings_path.as_deref())?;
    if resolved_model_path.as_path() != Path::new(&settings.model_path) {
        emit_log(
            &sink,
            LogLevel::Info,
            format!(
                "resolved model path: {} -> {}",
                settings.model_path,
                resolved_model_path.display()
            ),
        );
    }
    settings.model_path = resolved_model_path.display().to_string();
    validate_settings(&settings)?;
    let mut games_done = 0u32;
    let max_games = settings.autoplay.max_games;
    loop {
        if max_games.is_some_and(|limit| games_done >= limit)
            || stop_after_current_game.load(Ordering::Relaxed)
        {
            emit_log(
                &sink,
                LogLevel::Info,
                format!("max/stop reached: {games_done}"),
            );
            sink(CoreEvent::RuntimeStatus {
                status: RuntimeStatus::Stopped,
            });
            return Ok(games_done);
        }

        sink(CoreEvent::RuntimeStatus {
            status: RuntimeStatus::LoggingIn,
        });
        sink(CoreEvent::AccountSnapshot {
            account: AccountSnapshot {
                refreshing: true,
                username: settings.autoplay_account.username.clone(),
                account_id: None,
                nickname: None,
                level_id: None,
                level_score: None,
                rank_tier: None,
                target_mode: None,
                target_room: None,
            },
        });
        emit_log(
            &sink,
            LogLevel::Info,
            format!(
                "login begin: username={}",
                settings.autoplay_account.username
            ),
        );
        let mut client = tokio::time::timeout(
            Duration::from_secs(20),
            session::ProtocolClient::login(
                &settings.autoplay_account.username,
                &settings.autoplay_account.password,
                &device_id,
            ),
        )
        .await
        .map_err(|_| anyhow!("login timed out after 20s"))??;
        if let Some((mode, room)) = manual_target(&settings.autoplay) {
            client.set_match_target(mode, room);
        }
        sink(CoreEvent::AccountSnapshot {
            account: account_snapshot_from_login(
                false,
                &settings.autoplay_account.username,
                &client.summary,
            ),
        });
        emit_log(
            &sink,
            LogLevel::Info,
            format!(
                "login ok: {} account_id={} level_id={} target={}/{}",
                client.summary.nickname,
                client.summary.account_id,
                client.summary.level_id,
                mode_key(&client.summary.target_mode),
                room_key(&client.summary.target_room)
            ),
        );

        let target_mode = client.summary.target_mode.clone();
        let (game, initial_events) = connect_or_match_game(
            &mut client,
            &sink,
            max_games,
            stop_after_current_game.clone(),
        )
        .await?;
        let completed = match run_game_loop(
            game,
            initial_events,
            &target_mode,
            &settings,
            &sink,
            stop_after_current_game.clone(),
        )
        .await
        {
            Ok(completed) => completed,
            Err(err) if is_recoverable_game_connection_error(&err) => {
                sink(CoreEvent::RuntimeStatus {
                    status: RuntimeStatus::Reconnecting,
                });
                emit_log(
                    &sink,
                    LogLevel::Warn,
                    format!(
                        "对局连接已断开，正在重连: {}",
                        recoverable_game_connection_summary(&err)
                    ),
                );
                tokio::time::sleep(Duration::from_millis(800)).await;
                continue;
            }
            Err(err) => return Err(err),
        };
        games_done += completed;
        sink(CoreEvent::GameCompleted { games_done });
        if let Err(err) = refresh_account_snapshot_after_game(&mut client, &settings, &sink).await {
            emit_log(
                &sink,
                LogLevel::Warn,
                format!("account refresh after game failed: {err}"),
            );
        }
    }
}

async fn refresh_account_snapshot_after_game(
    client: &mut session::ProtocolClient,
    settings: &Settings,
    sink: &EventSink,
) -> Result<()> {
    client.refresh_account_summary().await?;
    if let Some((mode, room)) = manual_target(&settings.autoplay) {
        client.set_match_target(mode, room);
    }
    sink(CoreEvent::AccountSnapshot {
        account: account_snapshot_from_login(
            false,
            &settings.autoplay_account.username,
            &client.summary,
        ),
    });
    emit_log(
        sink,
        LogLevel::Info,
        format!(
            "account refreshed after game: {} level_id={} score={} target={}/{}",
            client.summary.nickname,
            client.summary.level_id,
            client.summary.level_score,
            mode_key(&client.summary.target_mode),
            room_key(&client.summary.target_room)
        ),
    );
    Ok(())
}

fn is_recoverable_game_connection_error(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        let message = cause.to_string();
        message.contains("peer closed connection without sending TLS close_notify")
            || message.contains("websocket closed:")
            || message.contains("websocket ended before next frame")
            || message.contains("Connection reset by peer")
            || message.contains("Broken pipe")
    })
}

fn recoverable_game_connection_summary(err: &anyhow::Error) -> &'static str {
    if err.chain().any(|cause| {
        cause
            .to_string()
            .contains("peer closed connection without sending TLS close_notify")
    }) {
        return "服务器关闭了 TLS 连接";
    }
    if err
        .chain()
        .any(|cause| cause.to_string().contains("websocket closed:"))
    {
        return "服务器关闭了对局连接";
    }
    "网络连接中断"
}

async fn connect_or_match_game(
    client: &mut session::ProtocolClient,
    sink: &EventSink,
    max_games: Option<u32>,
    stop_after_current_game: Arc<AtomicBool>,
) -> Result<(session::GameSession, Vec<bridge::Event>)> {
    let (mode, room) = (
        client.summary.target_mode.clone(),
        client.summary.target_room.clone(),
    );
    if let Some(existing) = client.fetch_existing_game().await? {
        sink(CoreEvent::RuntimeStatus {
            status: RuntimeStatus::Reconnecting,
        });
        emit_log(
            sink,
            LogLevel::Info,
            format!(
                "existing game found: account_id={} target={mode:?}/{room:?} game_uuid={} location={} max_games={max_games:?}",
                client.summary.account_id, existing.game_uuid, existing.location
            ),
        );
        return client.connect_existing_game(&existing).await;
    }

    if stop_after_current_game.load(Ordering::Relaxed) {
        return Err(anyhow!("stop requested before entering match"));
    }
    sink(CoreEvent::RuntimeStatus {
        status: RuntimeStatus::Matching,
    });
    match client.start_match().await? {
        StartMatchResult::Queued(match_sid) => {
            emit_log(
                sink,
                LogLevel::Info,
                format!(
                    "match queued: account_id={} target={mode:?}/{room:?} match_sid={match_sid} max_games={max_games:?}",
                    client.summary.account_id
                ),
            );
            let start = client.wait_for_match_start().await?;
            emit_log(
                sink,
                LogLevel::Info,
                format!(
                    "match found: mode_id={} game_uuid={} location={} url={}",
                    start.match_mode_id, start.game_uuid, start.location, start.game_url
                ),
            );
            client.connect_game(&start).await
        }
        StartMatchResult::Busy => {
            if let Some(existing) = client.fetch_existing_game().await? {
                sink(CoreEvent::RuntimeStatus {
                    status: RuntimeStatus::Reconnecting,
                });
                emit_log(
                    sink,
                    LogLevel::Info,
                    format!(
                        "account busy; reconnecting existing game: account_id={} game_uuid={} location={}",
                        client.summary.account_id, existing.game_uuid, existing.location
                    ),
                );
                client.connect_existing_game(&existing).await
            } else {
                Err(anyhow!(
                    "account busy but fetchGamingInfo returned no active game"
                ))
            }
        }
    }
}

async fn run_game_loop(
    mut game: session::GameSession,
    initial_events: Vec<bridge::Event>,
    mode: &Mode,
    settings: &Settings,
    sink: &EventSink,
    stop_after_current_game: Arc<AtomicBool>,
) -> Result<u32> {
    sink(CoreEvent::RuntimeStatus {
        status: RuntimeStatus::InGame,
    });
    emit_log(
        sink,
        LogLevel::Info,
        format!(
            "game connected: initial_events={} operation_window={}",
            initial_events.len(),
            game.bridge.last_operation_list().len()
        ),
    );

    let engine = CandleMortalEngine::load(&settings.model_path)
        .with_context(|| format!("load exported Mortal model from {}", settings.model_path))?;
    let mut bot = NativeBot::new(game.bridge.seat() as u8, engine);
    let mut table = TableTracker::new(game.bridge.seat());
    let mut queue = VecDeque::new();
    let mut waiting_for_new_round_since = None;
    if !initial_events.is_empty() {
        let mut last_action_json = None;
        for event in &initial_events {
            emit_game_event(sink, &mut table, event);
            if matches!(event, bridge::Event::EndGame) {
                return Ok(1);
            }
            last_action_json = bot.react(event)?;
        }
        if initial_events.last().is_some_and(|event| {
            should_confirm_new_round_after_event(mode, event, &VecDeque::new(), &table.snapshot())
        }) {
            let new_events = confirm_new_round(&mut game, sink).await?;
            let is_ending = new_events.iter().any(|e| matches!(e, bridge::Event::EndGame));
            queue.extend(new_events);
            if !is_ending {
                waiting_for_new_round_since = Some(Instant::now());
            }
        } else if initial_events.last().is_some_and(|event| {
            is_terminal_all_last_after_end_kyoku(mode, event, &table.snapshot())
        }) {
            emit_log(
                sink,
                LogLevel::Info,
                "terminal hand ended; waiting for game end result",
            );
        } else if let Some(action_json) = last_action_json {
            emit_model_decision(sink, bot.last_decision());
            let ack_events =
                handle_bot_action_json(&mut game, &mut bot, action_json, settings, sink).await?;
            queue.extend(ack_events);
        }
    }

    loop {
        let event = if let Some(event) = queue.pop_front() {
            event
        } else {
            let events = game.next_events().await?;
            queue.extend(events);
            continue;
        };

        if let Some(confirmed_at) = waiting_for_new_round_since.take() {
            emit_log(
                sink,
                LogLevel::Info,
                format!(
                    "next round event arrived after {:.1}s: {}",
                    confirmed_at.elapsed().as_secs_f64(),
                    event_kind_label(&event)
                ),
            );
        }
        emit_game_event(sink, &mut table, &event);
        if matches!(event, bridge::Event::EndGame) {
            if stop_after_current_game.load(Ordering::Relaxed) {
                sink(CoreEvent::RuntimeStatus {
                    status: RuntimeStatus::Stopped,
                });
            }
            return Ok(1);
        }

        let table_snapshot = table.snapshot();
        if should_confirm_new_round_after_event(mode, &event, &queue, &table_snapshot) {
            let _ = bot.react(&event)?;
            let new_events = confirm_new_round(&mut game, sink).await?;
            let is_ending = new_events.iter().any(|e| matches!(e, bridge::Event::EndGame));
            queue.extend(new_events);
            if !is_ending {
                waiting_for_new_round_since = Some(Instant::now());
            }
            continue;
        }
        if is_terminal_all_last_after_end_kyoku(mode, &event, &table_snapshot) {
            emit_log(
                sink,
                LogLevel::Info,
                "terminal hand ended; waiting for game end result",
            );
        }

        let ack_events = handle_bot_event(&mut game, &mut bot, &event, settings, sink).await?;
        queue.extend(ack_events);
    }
}

async fn handle_bot_event(
    game: &mut session::GameSession,
    bot: &mut NativeBot<CandleMortalEngine>,
    event: &bridge::Event,
    settings: &Settings,
    sink: &EventSink,
) -> Result<Vec<bridge::Event>> {
    let Some(action_json) = bot.react(event)? else {
        return Ok(Vec::new());
    };
    emit_model_decision(sink, bot.last_decision());
    handle_bot_action_json(game, bot, action_json, settings, sink).await
}

async fn handle_bot_action_json(
    game: &mut session::GameSession,
    bot: &mut NativeBot<CandleMortalEngine>,
    action_json: String,
    settings: &Settings,
    sink: &EventSink,
) -> Result<Vec<bridge::Event>> {
    let action_json = resolve_riichi_action(game, bot, action_json, sink)?;
    let Some(pending) = pending_action_from_json(&action_json, &game.bridge)? else {
        return Ok(Vec::new());
    };
    sink(CoreEvent::ActionPlanned {
        action: PlannedAction {
            action: pending.action.clone(),
        },
    });
    execute_pending_action(game, pending, settings, sink).await
}

fn resolve_riichi_action(
    game: &session::GameSession,
    bot: &mut NativeBot<CandleMortalEngine>,
    action_json: String,
    sink: &EventSink,
) -> Result<String> {
    let mut action: Value = serde_json::from_str(&action_json)?;
    if action.get("type").and_then(Value::as_str) != Some("reach") || action.get("pai").is_some() {
        return Ok(action_json);
    }

    let actor = action
        .get("actor")
        .and_then(Value::as_u64)
        .unwrap_or(game.bridge.seat() as u64) as u32;
    let reach_event = bridge::Event::Reach { actor };
    if let Some(dahai_json) = bot.react(&reach_event)? {
        emit_model_decision(sink, bot.last_decision());
        let dahai: Value = serde_json::from_str(&dahai_json)?;
        if dahai.get("type").and_then(Value::as_str) == Some("dahai") {
            action["pai"] = dahai
                .get("pai")
                .cloned()
                .unwrap_or(Value::String(String::new()));
            action["tsumogiri"] = dahai
                .get("tsumogiri")
                .cloned()
                .unwrap_or(Value::Bool(false));
            emit_log(
                sink,
                LogLevel::Info,
                format!(
                    "riichi: model chose {} tsumogiri={}",
                    action.get("pai").and_then(Value::as_str).unwrap_or(""),
                    action
                        .get("tsumogiri")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                ),
            );
            return Ok(serde_json::to_string(&action)?);
        }
    }

    if let Some(tile) = game.bridge.my_tsumohai() {
        action["pai"] = json!(tile);
        action["tsumogiri"] = json!(true);
        emit_log(
            sink,
            LogLevel::Warn,
            format!("riichi: model did not return dahai, using last tsumo {tile}"),
        );
    } else {
        emit_log(
            sink,
            LogLevel::Warn,
            "riichi: requested but no tile available",
        );
    }
    Ok(serde_json::to_string(&action)?)
}

fn pending_action_from_json(
    action_json: &str,
    bridge: &bridge::Bridge,
) -> Result<Option<PendingAction>> {
    let action: Value = serde_json::from_str(action_json)?;
    let Some(action_type) = action.get("type").and_then(Value::as_str) else {
        return Ok(Some(PendingAction {
            action: BotAction::None,
            context: operation_context(bridge),
        }));
    };

    let bot_action = match action_type {
        "none" => BotAction::None,
        "dahai" => BotAction::Dahai {
            tile: string_field(&action, "pai"),
            tsumogiri: bool_field(&action, "tsumogiri"),
        },
        "reach" => BotAction::Reach {
            tile: string_field(&action, "pai"),
            tsumogiri: bool_field(&action, "tsumogiri"),
        },
        "chi" => BotAction::Chi {
            consumed: string_array_field(&action, "consumed"),
        },
        "pon" => BotAction::Pon {
            consumed: string_array_field(&action, "consumed"),
        },
        "daiminkan" => BotAction::Daiminkan {
            consumed: string_array_field(&action, "consumed"),
        },
        "ankan" => BotAction::Ankan {
            consumed: string_array_field(&action, "consumed"),
        },
        "kakan" => BotAction::Kakan {
            tile: string_field(&action, "pai"),
        },
        "hora" => {
            let actor = action.get("actor").and_then(Value::as_u64);
            let target = action.get("target").and_then(Value::as_u64).or(actor);
            BotAction::Hora {
                tsumo: actor == target,
            }
        }
        "ryukyoku" => BotAction::Ryukyoku,
        other => return Err(anyhow!("unknown MJAI action type: {other}")),
    };

    Ok(Some(PendingAction {
        action: bot_action,
        context: operation_context(bridge),
    }))
}

async fn execute_pending_action(
    game: &mut session::GameSession,
    pending: PendingAction,
    settings: &Settings,
    sink: &EventSink,
) -> Result<Vec<bridge::Event>> {
    tokio::time::sleep(sample_action_delay(settings.autoplay.action_interval_ms)).await;
    let state = action_state(&game.bridge);
    let plan = crate::action::plan_action(&pending, &state);
    match plan {
        RpcPlan::IgnoreStale => {
            emit_log(
                sink,
                LogLevel::Warn,
                format!("stale action ignored: {:?}", pending.action),
            );
            Ok(Vec::new())
        }
        RpcPlan::RefuseNoDiscardWindow => {
            if should_wait_for_riichi_auto_discard(
                &pending,
                &state,
                game.bridge.self_riichi_accepted(),
            ) {
                return Ok(Vec::new());
            }
            emit_log(
                sink,
                LogLevel::Warn,
                "discard refused without discard operation window",
            );
            Ok(Vec::new())
        }
        RpcPlan::Skip => {
            check_common_error(game.skip().await?, "inputChiPengGang skip")?;
            sink(CoreEvent::ActionAck {
                ack: ActionAck {
                    ok: true,
                    message: "skip accepted".to_string(),
                },
            });
            Ok(Vec::new())
        }
        RpcPlan::ChiPengGang {
            r#type,
            index,
            timeuse,
        } => {
            check_common_error(
                game.input_chi_peng_gang(pb::ReqChiPengGang {
                    r#type,
                    index,
                    timeuse,
                    ..Default::default()
                })
                .await?,
                "inputChiPengGang",
            )?;
            sink(CoreEvent::ActionAck {
                ack: ActionAck {
                    ok: true,
                    message: format!(
                        "inputChiPengGang accepted type={type_} index={index}",
                        type_ = r#type
                    ),
                },
            });
            Ok(game.next_events().await?)
        }
        RpcPlan::InputOperation {
            r#type,
            tile,
            mut moqie,
            timeuse,
        } => {
            let before_discard = game.bridge.discard_counter();
            let before_round = game.bridge.round_end_counter();
            let expected_discard = tile.as_deref().map(ms_tile_to_mjai);
            if r#type == OP_DISCARD {
                if let Some(op_context) = game.bridge.last_operation_context().cloned() {
                    if op_context.source == "ActionNewRound" {
                        if moqie {
                            emit_log(
                                sink,
                                LogLevel::Info,
                                "opening-round discard uses hand-discard mode instead of moqie",
                            );
                            moqie = false;
                        }
                        wait_opening_round_discard_window(&op_context, sink).await;
                        let still_same_window =
                            game.bridge.last_operation_context().is_some_and(|current| {
                                current.source == op_context.source
                                    && current.seat == op_context.seat
                                    && current.received_key == op_context.received_key
                            }) && game
                                .bridge
                                .last_operation_list()
                                .iter()
                                .any(|op| op.r#type == OP_DISCARD);
                        if !still_same_window {
                            emit_log(
                                sink,
                                LogLevel::Warn,
                                "opening-round discard window changed before submit",
                            );
                            return Ok(Vec::new());
                        }
                    }
                }
            }
            check_common_error(
                game.input_operation(pb::ReqSelfOperation {
                    r#type,
                    tile: tile.clone().unwrap_or_default(),
                    moqie,
                    timeuse,
                    ..Default::default()
                })
                .await?,
                "inputOperation",
            )?;
            if matches!(r#type, OP_DISCARD | OP_LIQI) {
                wait_for_discard_ack(game, before_discard, before_round, expected_discard, sink)
                    .await
            } else {
                sink(CoreEvent::ActionAck {
                    ack: ActionAck {
                        ok: true,
                        message: format!("inputOperation accepted type={}", r#type),
                    },
                });
                Ok(game.next_events().await?)
            }
        }
    }
}

async fn confirm_new_round(
    game: &mut session::GameSession,
    sink: &EventSink,
) -> Result<Vec<bridge::Event>> {
    let started_at = Instant::now();
    emit_log(sink, LogLevel::Info, "round ended; confirming new round");
    let res = game.confirm_new_round().await?;
    if let Some(ref error) = res.error {
        if error.code == 1204 {
            emit_log(
                sink,
                LogLevel::Info,
                "game already ended on server (confirmNewRound code=1204); ending game",
            );
            return Ok(vec![bridge::Event::EndGame]);
        }
    }
    check_common_error(res, "confirmNewRound")?;
    emit_log(
        sink,
        LogLevel::Info,
        format!(
            "confirmNewRound accepted in {}ms; waiting for next round event",
            started_at.elapsed().as_millis()
        ),
    );
    sink(CoreEvent::ActionAck {
        ack: ActionAck {
            ok: true,
            message: "confirmNewRound accepted".to_string(),
        },
    });
    Ok(Vec::new())
}

async fn wait_opening_round_discard_window(
    op_context: &bridge::OperationContext,
    sink: &EventSink,
) {
    const OPENING_ROUND_DISCARD_MIN_DELAY: f64 = 12.0;
    if op_context.source != "ActionNewRound" {
        return;
    }
    let passed_waiting = op_context.passed_waiting_time as f64;
    let elapsed = op_context.received_at.elapsed().as_secs_f64();
    let wait_time = OPENING_ROUND_DISCARD_MIN_DELAY - passed_waiting.max(elapsed).max(0.0);
    if wait_time <= 0.0 {
        return;
    }
    emit_log(
        sink,
        LogLevel::Info,
        format!("opening-round discard waits {wait_time:.1}s before submit"),
    );
    tokio::time::sleep(Duration::from_secs_f64(wait_time)).await;
}

async fn wait_for_discard_ack(
    game: &mut session::GameSession,
    before_discard: u64,
    before_round: u64,
    expected_pai: Option<String>,
    sink: &EventSink,
) -> Result<Vec<bridge::Event>> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(anyhow!(
                "discard RPC accepted but no matching broadcast ACK arrived"
            ));
        }
        let events = tokio::time::timeout(remaining, game.next_events()).await??;
        if game.bridge.round_end_counter() > before_round {
            sink(CoreEvent::ActionAck {
                ack: ActionAck {
                    ok: true,
                    message: "round ended after action".to_string(),
                },
            });
            return Ok(events);
        }
        if game.bridge.discard_counter() <= before_discard {
            continue;
        }

        let self_discard = events.iter().find_map(|event| match event {
            bridge::Event::Dahai { actor, pai, .. } if *actor == game.bridge.seat() => {
                Some(pai.as_str())
            }
            _ => None,
        });
        match (expected_pai.as_deref(), self_discard) {
            (None, Some(_)) => {
                sink(CoreEvent::ActionAck {
                    ack: ActionAck {
                        ok: true,
                        message: "discard broadcast ack matched".to_string(),
                    },
                });
                return Ok(events);
            }
            (Some(expected), Some(actual)) if expected == actual => {
                sink(CoreEvent::ActionAck {
                    ack: ActionAck {
                        ok: true,
                        message: format!("discard broadcast ack matched {actual}"),
                    },
                });
                return Ok(events);
            }
            (Some(expected), Some(actual)) => {
                return Err(anyhow!(
                    "no matching broadcast ACK: expected self discard {expected}, got {actual}"
                ));
            }
            (_, None) => continue,
        }
    }
}

fn emit_game_event(sink: &EventSink, table: &mut TableTracker, event: &bridge::Event) {
    sink(CoreEvent::GameEvent {
        event: event.clone(),
    });
    let snapshot = table.apply(event);
    sink(CoreEvent::TableSnapshot { table: snapshot });
}

fn emit_model_decision(sink: &EventSink, decision: Option<&EngineDecision>) {
    let Some(decision) = decision else {
        return;
    };
    let legal = decision
        .q_values
        .iter()
        .zip(&decision.mask)
        .filter_map(|(q, legal)| legal.then_some(*q))
        .collect::<Vec<_>>();
    let max_q = legal
        .iter()
        .copied()
        .max_by(|left, right| left.total_cmp(right))
        .unwrap_or(0.0);
    let denom = legal
        .iter()
        .map(|q| (*q - max_q).exp())
        .sum::<f32>()
        .max(f32::EPSILON);
    let confidence_for = |q: f32| (q - max_q).exp() / denom;
    let mut candidates = decision
        .q_values
        .iter()
        .enumerate()
        .filter(|(idx, q)| decision.mask.get(*idx).copied().unwrap_or(false) && q.is_finite())
        .map(|(idx, q)| ModelCandidate {
            action_index: idx,
            action_label: action_label(idx),
            q_value: *q,
            confidence: confidence_for(*q),
            legal: true,
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| right.q_value.total_cmp(&left.q_value));
    candidates.truncate(5);
    let top_confidence = candidates
        .first()
        .map(|item| item.confidence)
        .unwrap_or(0.0);
    sink(CoreEvent::ModelDecision {
        decision: ModelDecision {
            action_index: decision.action,
            action_label: action_label(decision.action),
            confidence: top_confidence,
            candidates,
            is_greedy: decision.is_greedy,
        },
    });
}

fn check_common_error(response: pb::ResCommon, label: &str) -> Result<()> {
    if let Some(error) = response.error {
        if error.code != 0 {
            return Err(anyhow!("{label} failed: code={}", error.code));
        }
    }
    Ok(())
}

fn action_state(bridge: &bridge::Bridge) -> ActionState {
    ActionState {
        current_context: operation_context(bridge),
        operations: bridge
            .last_operation_list()
            .iter()
            .map(|op| Operation {
                r#type: op.r#type,
                combination: op.combination.clone(),
            })
            .collect(),
    }
}

fn operation_context(bridge: &bridge::Bridge) -> Option<OperationContext> {
    bridge.last_operation_context().map(|ctx| OperationContext {
        source: ctx.source.clone(),
        seat: ctx.seat,
        received_key: ctx.received_key,
    })
}

fn should_wait_for_riichi_auto_discard(
    pending: &PendingAction,
    state: &ActionState,
    self_riichi_accepted: bool,
) -> bool {
    self_riichi_accepted
        && state.current_context.is_none()
        && state.operations.is_empty()
        && matches!(pending.action, BotAction::Dahai { .. })
}

fn should_confirm_new_round_after_event(
    mode: &Mode,
    event: &bridge::Event,
    queued_events: &VecDeque<bridge::Event>,
    table: &TableSnapshot,
) -> bool {
    matches!(event, bridge::Event::EndKyoku)
        && !matches!(
            queued_events.front(),
            Some(bridge::Event::StartKyoku { .. } | bridge::Event::EndGame)
        )
        && !is_terminal_all_last_after_end_kyoku(mode, event, table)
}

fn is_terminal_all_last_after_end_kyoku(
    mode: &Mode,
    event: &bridge::Event,
    table: &TableSnapshot,
) -> bool {
    if !matches!(event, bridge::Event::EndKyoku) {
        return false;
    }
    match &table.last_event {
        Some(bridge::Event::Hule {
            actor,
            target,
            point_sum,
            ..
        }) => {
            if let Some(target_seat) = target {
                let target_score = table.scores.get(*target_seat as usize).copied().unwrap_or(0);
                if target_score < *point_sum as i32 {
                    return true;
                }
            }
            if !is_all_last(mode, table) {
                return false;
            }
            if *actor != table.oya {
                true
            } else {
                let oya_idx = table.oya as usize;
                let oya_score = table.scores.get(oya_idx).copied().unwrap_or(0);
                let est_oya_score = oya_score + *point_sum as i32;
                let is_top = table
                    .scores
                    .iter()
                    .enumerate()
                    .all(|(idx, &score)| idx == oya_idx || est_oya_score > score);
                est_oya_score >= 30_000 && is_top
            }
        }
        _ => false,
    }
}

fn is_all_last(mode: &Mode, table: &TableSnapshot) -> bool {
    match mode {
        Mode::FourPlayerEast => table.bakaze == "E" && table.kyoku >= 4,
        Mode::FourPlayerSouth => table.bakaze == "S" && table.kyoku >= 4,
    }
}

fn event_kind_label(event: &bridge::Event) -> &'static str {
    match event {
        bridge::Event::StartGame { .. } => "start_game",
        bridge::Event::StartKyoku { .. } => "start_kyoku",
        bridge::Event::Tsumo { .. } => "tsumo",
        bridge::Event::Reach { .. } => "reach",
        bridge::Event::ReachAccepted { .. } => "reach_accepted",
        bridge::Event::Dahai { .. } => "dahai",
        bridge::Event::Chi { .. } => "chi",
        bridge::Event::Pon { .. } => "pon",
        bridge::Event::Daiminkan { .. } => "daiminkan",
        bridge::Event::Ankan { .. } => "ankan",
        bridge::Event::Kakan { .. } => "kakan",
        bridge::Event::Dora { .. } => "dora",
        bridge::Event::Hule { .. } => "hule",
        bridge::Event::NoTile { .. } => "no_tile",
        bridge::Event::LiuJu { .. } => "liu_ju",
        bridge::Event::EndKyoku => "end_kyoku",
        bridge::Event::EndGame => "end_game",
    }
}

fn string_field(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn bool_field(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn string_array_field(value: &Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn ms_tile_to_mjai(tile: &str) -> String {
    match tile {
        "0m" => "5mr",
        "0p" => "5pr",
        "0s" => "5sr",
        "1z" => "E",
        "2z" => "S",
        "3z" => "W",
        "4z" => "N",
        "5z" => "P",
        "6z" => "F",
        "7z" => "C",
        other => other,
    }
    .to_string()
}

fn emit_log(sink: &EventSink, level: LogLevel, message: impl Into<String>) {
    sink(CoreEvent::Log {
        level,
        message: message.into(),
    });
}

pub fn resolve_model_path(raw: &str, settings_path: Option<&Path>) -> Result<PathBuf> {
    let current_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let current_exe = std::env::current_exe().ok();
    resolve_model_path_from(raw, settings_path, &current_dir, current_exe.as_deref())
}

fn resolve_model_path_from(
    raw: &str,
    settings_path: Option<&Path>,
    current_dir: &Path,
    current_exe: Option<&Path>,
) -> Result<PathBuf> {
    let raw_path = PathBuf::from(raw);
    if raw_path.is_absolute() {
        return model_path_exists(&raw_path)
            .then_some(raw_path.clone())
            .ok_or_else(|| anyhow!("model path not found: {}", raw_path.display()));
    }

    let mut candidates = Vec::new();
    push_unique_candidate(&mut candidates, current_dir.join(raw));
    if let Some(parent) = settings_path.and_then(Path::parent) {
        push_unique_candidate(&mut candidates, parent.join(raw));
    }
    if let Some(exe_parent) = current_exe.and_then(Path::parent) {
        for ancestor in exe_parent.ancestors() {
            push_unique_candidate(&mut candidates, ancestor.join(raw));
            if ancestor.file_name().is_some_and(|name| name == "bin") {
                if let Some(prefix) = ancestor.parent() {
                    push_unique_candidate(
                        &mut candidates,
                        prefix.join("lib/Majsoul Autopilot").join(raw),
                    );
                }
            }
            if ancestor.file_name().is_some_and(|name| name == "Contents") {
                push_unique_candidate(&mut candidates, ancestor.join("Resources").join(raw));
            }
            if ancestor
                .extension()
                .is_some_and(|extension| extension == "app")
            {
                push_unique_candidate(
                    &mut candidates,
                    ancestor.join("Contents/Resources").join(raw),
                );
            }
        }
    }

    for candidate in &candidates {
        if model_path_exists(candidate) {
            return Ok(candidate.clone());
        }
    }

    let tried = candidates
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    Err(anyhow!("model path not found: {raw}; tried: {tried}"))
}

fn push_unique_candidate(candidates: &mut Vec<PathBuf>, candidate: PathBuf) {
    if !candidates.iter().any(|existing| existing == &candidate) {
        candidates.push(candidate);
    }
}

fn model_path_exists(path: &Path) -> bool {
    path.join("model_config.json").is_file() && path.join("model.safetensors").is_file()
}

fn account_snapshot_from_login(
    refreshing: bool,
    username: &str,
    summary: &LoginSummary,
) -> AccountSnapshot {
    AccountSnapshot {
        refreshing,
        username: username.to_string(),
        account_id: Some(summary.account_id),
        nickname: Some(summary.nickname.clone()),
        level_id: Some(summary.level_id),
        level_score: Some(summary.level_score),
        rank_tier: Some(summary.rank_tier),
        target_mode: Some(mode_key(&summary.target_mode).to_string()),
        target_room: Some(room_key(&summary.target_room).to_string()),
    }
}

fn mode_key(mode: &Mode) -> &'static str {
    match mode {
        Mode::FourPlayerEast => "four_player_east",
        Mode::FourPlayerSouth => "four_player_south",
    }
}

fn room_key(room: &Room) -> &'static str {
    match room {
        Room::Bronze => "bronze",
        Room::Silver => "silver",
        Room::Gold => "gold",
        Room::Jade => "jade",
        Room::Throne => "throne",
    }
}

fn sample_action_delay(interval: ActionInterval) -> Duration {
    let min = interval.min;
    let max = interval.max.max(min);
    let span = max - min;
    if span == 0 {
        return Duration::from_millis(min);
    }
    let entropy = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.subsec_nanos() as u64)
        .unwrap_or(0);
    Duration::from_millis(min + entropy % (span + 1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir(name: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("majsoul-autopilot-runtime-{name}-{stamp}"));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn write_fake_model(path: &Path) {
        fs::create_dir_all(path).unwrap();
        fs::write(path.join("model_config.json"), "{}").unwrap();
        fs::write(path.join("model.safetensors"), "").unwrap();
    }

    #[test]
    fn sampled_action_delay_stays_inside_configured_range() {
        let interval = ActionInterval { min: 123, max: 456 };
        for _ in 0..100 {
            let delay = sample_action_delay(interval).as_millis() as u64;
            assert!((123..=456).contains(&delay));
        }
    }

    #[test]
    fn sampled_action_delay_tolerates_reversed_range() {
        let delay = sample_action_delay(ActionInterval { min: 800, max: 200 });
        assert_eq!(delay, Duration::from_millis(800));
    }

    #[test]
    fn rustls_close_notify_eof_is_recoverable_game_connection_error() {
        let err = anyhow!(
            "IO error: peer closed connection without sending TLS close_notify: https://docs.rs/rustls/latest/rustls/manual/_03_howto/index.html#unexpected-eof"
        );
        assert!(is_recoverable_game_connection_error(&err));
        assert_eq!(
            recoverable_game_connection_summary(&err),
            "服务器关闭了 TLS 连接"
        );
    }

    #[test]
    fn model_or_logic_errors_are_not_treated_as_recoverable_connections() {
        let err = anyhow!("no matching broadcast ACK: expected self discard E, got S");
        assert!(!is_recoverable_game_connection_error(&err));
    }

    #[test]
    fn relative_model_path_resolves_from_executable_ancestor() {
        let root = temp_dir("model-ancestor");
        let model = root.join("models/mortal");
        write_fake_model(&model);
        let exe = root.join("target/release/bundle/macos/Majsoul Autopilot.app/Contents/MacOS/app");
        fs::create_dir_all(exe.parent().unwrap()).unwrap();
        fs::write(&exe, "").unwrap();

        let resolved =
            resolve_model_path_from("models/mortal", None, Path::new("/"), Some(&exe)).unwrap();

        assert_eq!(resolved, model);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn relative_model_path_resolves_from_macos_app_resources() {
        let root = temp_dir("model-app-resources");
        let model = root.join("Majsoul Autopilot.app/Contents/Resources/models/mortal");
        write_fake_model(&model);
        let exe = root.join("Majsoul Autopilot.app/Contents/MacOS/app");
        fs::create_dir_all(exe.parent().unwrap()).unwrap();
        fs::write(&exe, "").unwrap();

        let resolved =
            resolve_model_path_from("models/mortal", None, Path::new("/"), Some(&exe)).unwrap();

        assert_eq!(resolved, model);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn relative_model_path_resolves_from_linux_deb_resources() {
        let root = temp_dir("model-linux-deb-resources");
        let model = root.join("usr/lib/Majsoul Autopilot/models/mortal");
        write_fake_model(&model);
        let exe = root.join("usr/bin/majsoul-autopilot-desktop");
        fs::create_dir_all(exe.parent().unwrap()).unwrap();
        fs::write(&exe, "").unwrap();

        let resolved =
            resolve_model_path_from("models/mortal", None, Path::new("/"), Some(&exe)).unwrap();

        assert_eq!(resolved, model);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn relative_model_path_resolves_from_settings_parent() {
        let root = temp_dir("model-settings");
        let model = root.join("models/mortal");
        write_fake_model(&model);
        let settings_path = root.join("settings.json");

        let resolved =
            resolve_model_path_from("models/mortal", Some(&settings_path), Path::new("/"), None)
                .unwrap();

        assert_eq!(resolved, model);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn self_riichi_auto_discard_waits_for_server_broadcast_without_window() {
        let pending = PendingAction {
            action: BotAction::Dahai {
                tile: "3m".to_string(),
                tsumogiri: true,
            },
            context: None,
        };
        let state = ActionState {
            current_context: None,
            operations: vec![],
        };
        assert!(should_wait_for_riichi_auto_discard(&pending, &state, true));
    }

    #[test]
    fn non_riichi_discard_without_window_remains_an_error_condition() {
        let pending = PendingAction {
            action: BotAction::Dahai {
                tile: "3m".to_string(),
                tsumogiri: true,
            },
            context: None,
        };
        let state = ActionState {
            current_context: None,
            operations: vec![],
        };
        assert!(!should_wait_for_riichi_auto_discard(
            &pending, &state, false
        ));
    }

    #[test]
    fn end_kyoku_with_no_following_round_state_needs_confirm_new_round() {
        let queue = VecDeque::new();

        assert!(should_confirm_new_round_after_event(
            &Mode::FourPlayerSouth,
            &bridge::Event::EndKyoku,
            &queue,
            &TableTracker::new(0).snapshot()
        ));
    }

    #[test]
    fn restored_end_kyoku_before_start_kyoku_does_not_confirm_again() {
        let mut queue = VecDeque::new();
        queue.push_back(bridge::Event::StartKyoku {
            bakaze: "E".to_string(),
            dora_marker: "1m".to_string(),
            honba: 0,
            kyoku: 1,
            kyotaku: 0,
            oya: 0,
            scores: vec![25000; 4],
            tehais: vec![vec![]; 4],
        });

        assert!(!should_confirm_new_round_after_event(
            &Mode::FourPlayerSouth,
            &bridge::Event::EndKyoku,
            &queue,
            &TableTracker::new(0).snapshot()
        ));
    }

    #[test]
    fn restored_end_kyoku_before_end_game_does_not_confirm_again() {
        let mut queue = VecDeque::new();
        queue.push_back(bridge::Event::EndGame);

        assert!(!should_confirm_new_round_after_event(
            &Mode::FourPlayerSouth,
            &bridge::Event::EndKyoku,
            &queue,
            &TableTracker::new(0).snapshot()
        ));
    }

    #[test]
    fn south_four_non_dealer_hule_does_not_confirm_new_round() {
        let mut table = TableTracker::new(1);
        table.apply(&bridge::Event::StartKyoku {
            bakaze: "S".to_string(),
            dora_marker: "5m".to_string(),
            honba: 0,
            kyoku: 4,
            kyotaku: 0,
            oya: 3,
            scores: vec![21_300, 15_400, 31_000, 32_300],
            tehais: vec![vec![]; 4],
        });
        table.apply(&bridge::Event::Hule {
            actor: 2,
            target: Some(1),
            pai: "2m".to_string(),
            zimo: false,
            title: String::new(),
            count: 2,
            fu: 30,
            fans: Vec::new(),
            point_sum: 26_300,
            hand: vec![],
            ming: vec![],
        });

        assert!(!should_confirm_new_round_after_event(
            &Mode::FourPlayerSouth,
            &bridge::Event::EndKyoku,
            &VecDeque::new(),
            &table.snapshot()
        ));
    }

    #[test]
    fn south_four_dealer_top_hule_does_not_confirm_new_round() {
        let mut table = TableTracker::new(3);
        table.apply(&bridge::Event::StartKyoku {
            bakaze: "S".to_string(),
            dora_marker: "5m".to_string(),
            honba: 0,
            kyoku: 4,
            kyotaku: 0,
            oya: 3,
            scores: vec![21_300, 15_400, 31_000, 32_300],
            tehais: vec![vec![]; 4],
        });
        table.apply(&bridge::Event::Hule {
            actor: 3,
            target: Some(0),
            pai: "7s".to_string(),
            zimo: false,
            title: String::new(),
            count: 4,
            fu: 40,
            fans: Vec::new(),
            point_sum: 22_600,
            hand: vec![],
            ming: vec![],
        });

        assert!(!should_confirm_new_round_after_event(
            &Mode::FourPlayerSouth,
            &bridge::Event::EndKyoku,
            &VecDeque::new(),
            &table.snapshot()
        ));
    }

    #[test]
    fn south_four_dealer_behind_hule_confirms_new_round() {
        let mut table = TableTracker::new(3);
        table.apply(&bridge::Event::StartKyoku {
            bakaze: "S".to_string(),
            dora_marker: "5m".to_string(),
            honba: 0,
            kyoku: 4,
            kyotaku: 0,
            oya: 3,
            scores: vec![45_000, 15_000, 15_000, 10_000],
            tehais: vec![vec![]; 4],
        });
        table.apply(&bridge::Event::Hule {
            actor: 3,
            target: Some(1),
            pai: "2m".to_string(),
            zimo: false,
            title: String::new(),
            count: 1,
            fu: 30,
            fans: Vec::new(),
            point_sum: 4_000,
            hand: vec![],
            ming: vec![],
        });

        assert!(should_confirm_new_round_after_event(
            &Mode::FourPlayerSouth,
            &bridge::Event::EndKyoku,
            &VecDeque::new(),
            &table.snapshot()
        ));
    }

    #[test]
    fn bankrupt_hule_does_not_confirm_new_round() {
        let mut table = TableTracker::new(0);
        table.apply(&bridge::Event::StartKyoku {
            bakaze: "E".to_string(),
            dora_marker: "1m".to_string(),
            honba: 0,
            kyoku: 1,
            kyotaku: 0,
            oya: 0,
            scores: vec![25_000, 5_000, 25_000, 25_000],
            tehais: vec![vec![]; 4],
        });
        table.apply(&bridge::Event::Hule {
            actor: 0,
            target: Some(1),
            pai: "1m".to_string(),
            zimo: false,
            title: String::new(),
            count: 3,
            fu: 40,
            fans: Vec::new(),
            point_sum: 8_000,
            hand: vec![],
            ming: vec![],
        });

        assert!(!should_confirm_new_round_after_event(
            &Mode::FourPlayerSouth,
            &bridge::Event::EndKyoku,
            &VecDeque::new(),
            &table.snapshot()
        ));
    }

    #[test]
    fn event_kind_label_names_round_transition_events() {
        assert_eq!(
            event_kind_label(&bridge::Event::StartKyoku {
                bakaze: "E".to_string(),
                dora_marker: "1m".to_string(),
                honba: 0,
                kyoku: 1,
                kyotaku: 0,
                oya: 0,
                scores: vec![25000; 4],
                tehais: vec![vec![]; 4],
            }),
            "start_kyoku"
        );
        assert_eq!(event_kind_label(&bridge::Event::EndGame), "end_game");
    }
}
