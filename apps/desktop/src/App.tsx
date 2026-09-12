import { useEffect, useRef, useState } from "react";
import type { ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import {
  AlertTriangle,
  Bot,
  Check,
  CircleHelp,
  CircleDot,
  Copy,
  Gauge,
  KeyRound,
  Languages,
  Play,
  RadioTower,
  RefreshCw,
  Save,
  ScrollText,
  Shield,
  Square,
  TimerReset,
  Upload,
} from "lucide-react";
import { GameTable } from "./GameTable";
import { copy, languageNames } from "./i18n";
import { useAppStore } from "./store";
import type {
  CoreEventBatch,
  GameRecordSummary,
  Language,
  ModeChoice,
  ModelChoice,
  ModelImportResult,
  RoomChoice,
  RuntimeSnapshot,
  Settings,
} from "./types";
import { defaultSettings } from "./types";

function isTauriRuntime() {
  return "__TAURI_INTERNALS__" in window;
}

const BASE_WIDTH = 1440;
const BASE_HEIGHT = 900;

function useViewportScale() {
  const [scale, setScale] = useState(() => {
    if (typeof window === "undefined") return 1;
    return measureScale();
  });

  useEffect(() => {
    const update = () => setScale(measureScale());
    update();
    window.addEventListener("resize", update);
    return () => window.removeEventListener("resize", update);
  }, []);

  return scale;
}

function measureScale() {
  const widthScale = window.innerWidth / BASE_WIDTH;
  const heightScale = window.innerHeight / BASE_HEIGHT;
  return Math.max(0.55, Math.min(widthScale, heightScale));
}

function normalizeSettings(settings: Settings): Settings {
  return {
    ...settings,
    ui_language: settings.ui_language ?? "zh",
    autoplay: {
      ...settings.autoplay,
      max_games: settings.autoplay.max_games ?? null,
    },
  };
}

export function App() {
  const {
    settings,
    setSettings,
    status,
    table,
    modelDecision,
    logs,
    events,
    account,
    gameRecords,
    setGameRecords,
    stopScheduled,
    ingest,
  } = useAppStore();

  const language = settings.ui_language;
  const t = copy[language];
  const viewportScale = useViewportScale();
  const [settingsReady, setSettingsReady] = useState(() => !isTauriRuntime());
  const [launching, setLaunching] = useState(false);
  const [modelImporting, setModelImporting] = useState(false);
  const [modelChoices, setModelChoices] = useState<ModelChoice[]>(() => [
    { label: "mortal", model_path: defaultSettings.model_path, builtin: true },
  ]);
  const [runtimeRunning, setRuntimeRunning] = useState(false);
  const [emergencyConfirmOpen, setEmergencyConfirmOpen] = useState(false);
  const [recordsLoading, setRecordsLoading] = useState(false);
  const [copiedUuid, setCopiedUuid] = useState<string | null>(null);
  const lastSavedSettings = useRef(JSON.stringify(normalizeSettings(settings)));
  const lastSnapshotError = useRef<string | null>(null);
  const coreEventCursor = useRef(0);

  const refreshGameRecords = async () => {
    if (!isTauriRuntime()) {
      return;
    }
    setRecordsLoading(true);
    try {
      const records = await invoke<GameRecordSummary[]>("fetch_game_records");
      setGameRecords(records);
    } catch (err) {
      ingest({ type: "log", level: "warn", message: `fetch records failed: ${String(err)}` });
    } finally {
      setRecordsLoading(false);
    }
  };

  const copyPaipuUrl = async (record: GameRecordSummary) => {
    try {
      await navigator.clipboard.writeText(record.paipu_url);
      setCopiedUuid(record.uuid);
      setTimeout(() => setCopiedUuid(null), 2000);
    } catch (err) {
      console.error("copy failed", err);
    }
  };

  useEffect(() => {
    if (!isTauriRuntime() || !settingsReady) {
      return;
    }
    if (settings.autoplay_account.username && settings.autoplay_account.password) {
      void refreshGameRecords();
    }
  }, [settingsReady]);

  useEffect(() => {
    if (isTauriRuntime()) {
      return;
    }
    if (gameRecords.length === 0) {
      setGameRecords([
        {
          uuid: "260912-8259aca0-a3f1-4901-946d-da20c59bd0bc",
          start_time: 1726140000,
          end_time: 1726141800,
          mode_id: 12,
          room_name: "玉之间 四人南",
          rank: 1,
          score: 44700,
          point_change: 145,
          paipu_url: "https://game.maj-soul.com/1/?paipu=260912-8259aca0-a3f1-4901-946d-da20c59bd0bc_a14244521",
          players: [],
        },
        {
          uuid: "260609-66835b36-aa82-422e-a14d-9316863298f8",
          start_time: 1726130000,
          end_time: 1726131800,
          mode_id: 12,
          room_name: "玉之间 四人南",
          rank: 2,
          score: 31600,
          point_change: 67,
          paipu_url: "https://game.maj-soul.com/1/?paipu=260609-66835b36-aa82-422e-a14d-9316863298f8_a14244521",
          players: [],
        },
        {
          uuid: "260609-94056dfa-c267-4034-81a8-88f833d7a97a",
          start_time: 1726120000,
          end_time: 1726121800,
          mode_id: 12,
          room_name: "玉之间 四人南",
          rank: 3,
          score: 22900,
          point_change: -7,
          paipu_url: "https://game.maj-soul.com/1/?paipu=260609-94056dfa-c267-4034-81a8-88f833d7a97a_a14244521",
          players: [],
        },
        {
          uuid: "260913-57c2689f-b011-4d4c-b3b4-78d06e5c1307",
          start_time: 1726110000,
          end_time: 1726111800,
          mode_id: 12,
          room_name: "玉之间 四人南",
          rank: 4,
          score: 0,
          point_change: -205,
          paipu_url: "https://game.maj-soul.com/1/?paipu=260913-57c2689f-b011-4d4c-b3b4-78d06e5c1307_a14244521",
          players: [],
        },
      ]);
    }
  }, []);

  useEffect(() => {
    if (!isTauriRuntime()) {
      return;
    }
    invoke<Settings>("load_settings")
      .then((loaded) => {
        const normalized = normalizeSettings(loaded);
        lastSavedSettings.current = JSON.stringify(normalized);
        setSettings(normalized);
        setSettingsReady(true);
      })
      .catch((error) => {
        ingest({ type: "log", level: "warn", message: `${copy.zh.readFailed}: ${error}` });
        setSettingsReady(true);
      });
  }, [ingest, setSettings]);

  useEffect(() => {
    if (!isTauriRuntime()) {
      return;
    }
    void refreshModelChoices();
  }, []);

  useEffect(() => {
    if (!isTauriRuntime()) {
      return;
    }
    let disposed = false;
    const syncCoreEvents = () => {
      invoke<CoreEventBatch>("get_core_event_batch", { after: coreEventCursor.current })
        .then((batch) => {
          if (disposed) return;
          for (const record of batch.events) {
            ingest(record.event);
          }
          coreEventCursor.current = batch.cursor;
        })
        .catch((error) => {
          if (!disposed) {
            ingest({ type: "log", level: "warn", message: `core event sync failed: ${String(error)}` });
          }
        });
    };
    syncCoreEvents();
    const timer = window.setInterval(syncCoreEvents, 700);
    return () => {
      disposed = true;
      window.clearInterval(timer);
    };
  }, [ingest]);

  useEffect(() => {
    if (!isTauriRuntime()) {
      return;
    }
    let disposed = false;
    const syncRuntimeSnapshot = () => {
      invoke<RuntimeSnapshot>("get_runtime_snapshot")
        .then((snapshot) => {
          if (disposed) return;
          setRuntimeRunning(snapshot.running);
          if (snapshot.status !== status) {
            ingest({ type: "runtime_status", status: snapshot.status });
          }
          if (!snapshot.running && snapshot.last_error && snapshot.last_error !== lastSnapshotError.current) {
            lastSnapshotError.current = snapshot.last_error;
            ingest({ type: "runtime_error", message: snapshot.last_error });
          }
          if (snapshot.running || !snapshot.last_error) {
            lastSnapshotError.current = null;
          }
        })
        .catch((error) => {
          if (!disposed) {
            ingest({ type: "log", level: "warn", message: `runtime snapshot failed: ${String(error)}` });
          }
        });
    };
    syncRuntimeSnapshot();
    const timer = window.setInterval(syncRuntimeSnapshot, 1000);
    return () => {
      disposed = true;
      window.clearInterval(timer);
    };
  }, [ingest, status]);

  useEffect(() => {
    if (isTauriRuntime()) {
      return;
    }
    const params = new URLSearchParams(window.location.search);
    if (params.get("demo") !== "running") {
      return;
    }
    ingest({ type: "runtime_status", status: "in_game" });
  }, [ingest]);

  useEffect(() => {
    if (!settingsReady || !isTauriRuntime()) {
      return;
    }
    const normalized = normalizeSettings(settings);
    const raw = JSON.stringify(normalized);
    if (raw === lastSavedSettings.current) {
      return;
    }
    const timer = window.setTimeout(() => {
      invoke("save_settings", { settings: normalized })
        .then(() => {
          lastSavedSettings.current = raw;
        })
        .catch((error) => {
          ingest({ type: "log", level: "error", message: `${t.saveFailed}: ${String(error)}` });
        });
    }, 450);
    return () => window.clearTimeout(timer);
  }, [ingest, settings, settingsReady, t.saveFailed]);

  const canStart =
    !launching &&
    !modelImporting &&
    !runtimeRunning &&
    (status === "idle" || status === "stopped" || status === "error");
  const inGame = ["logging_in", "matching", "reconnecting", "in_game", "stopping_after_game"].includes(status);
  const stopAlreadyScheduled = stopScheduled || status === "stopping_after_game";
  const statusText = t.status[status] ?? status;
  const accountName = account?.nickname || account?.username || settings.autoplay_account.username || t.accountWaiting;
  const accountTarget =
    account?.target_room && account?.target_mode
      ? `${t.rooms[account.target_room]} ${t.modes[account.target_mode]}`
      : "-";
  const visibleModelChoices = ensureCurrentModelChoice(modelChoices, settings.model_path);
  const selectedModelChoice = visibleModelChoices.find((choice) => choice.model_path === settings.model_path);

  async function saveSettings(nextSettings = settings) {
    const normalized = normalizeSettings(nextSettings);
    setSettings(normalized);
    if (!isTauriRuntime()) {
      ingest({ type: "log", level: "info", message: t.previewSave });
      return;
    }
    try {
      await invoke("save_settings", { settings: normalized });
      lastSavedSettings.current = JSON.stringify(normalized);
      ingest({ type: "log", level: "info", message: t.saveOk });
    } catch (error) {
      ingest({ type: "log", level: "error", message: `${t.saveFailed}: ${String(error)}` });
      throw error;
    }
  }

  async function refreshModelChoices() {
    if (!isTauriRuntime()) {
      setModelChoices([{ label: "mortal", model_path: defaultSettings.model_path, builtin: true }]);
      return;
    }
    try {
      const choices = await invoke<ModelChoice[]>("list_models");
      setModelChoices(mergeModelChoices(choices));
    } catch (error) {
      ingest({ type: "log", level: "warn", message: `${t.modelImportFailed}: ${String(error)}` });
    }
  }

  async function importModel() {
    if (modelImporting || runtimeRunning || launching) {
      return;
    }
    if (!isTauriRuntime()) {
      ingest({ type: "log", level: "info", message: t.previewImport });
      return;
    }
    try {
      const selected = await open({
        multiple: false,
        directory: false,
        title: t.modelImport,
        filters: [{ name: "safetensors", extensions: ["safetensors"] }],
      });
      if (typeof selected !== "string") {
        return;
      }
      setModelImporting(true);
      ingest({ type: "log", level: "info", message: t.modelImporting });
      const result = await invoke<ModelImportResult>("import_model", { sourcePath: selected });
      await refreshModelChoices();
      const nextSettings = normalizeSettings({ ...settings, model_path: result.model_path });
      await saveSettings(nextSettings);
      ingest({
        type: "log",
        level: "info",
        message: `${t.modelImportOk}: ${result.model_name}`,
      });
    } catch (error) {
      ingest({ type: "log", level: "error", message: `${t.modelImportFailed}: ${String(error)}` });
    } finally {
      setModelImporting(false);
    }
  }

  async function start() {
    if (!canStart) {
      return;
    }
    const normalized = normalizeSettings(settings);
    setLaunching(true);
    ingest({
      type: "account_snapshot",
      account: {
        refreshing: true,
        username: normalized.autoplay_account.username,
        account_id: null,
        nickname: null,
        level_id: null,
        level_score: null,
        rank_tier: null,
        target_mode: null,
        target_room: null,
      },
    });
    try {
      await saveSettings(normalized);
      if (!isTauriRuntime()) return;
      await invoke("start_autoplay", { settings: normalized });
      setRuntimeRunning(true);
      ingest({ type: "runtime_status", status: "logging_in" });
    } catch (error) {
      if (String(error).includes("autoplay is already running")) {
        setRuntimeRunning(true);
        ingest({ type: "runtime_status", status: "logging_in" });
      }
      ingest({ type: "log", level: "error", message: `${t.startFailed}: ${String(error)}` });
    } finally {
      setLaunching(false);
    }
  }

  async function stopAfterCurrentGame() {
    if (!isTauriRuntime()) return;
    try {
      await invoke("stop_after_current_game");
      ingest({ type: "stop_scheduled", after_current_game: true });
      ingest({ type: "runtime_status", status: "stopping_after_game" });
    } catch (error) {
      ingest({ type: "log", level: "error", message: `${t.stopFailed}: ${String(error)}` });
    }
  }

  async function emergencyStop() {
    if (!isTauriRuntime()) return;
    try {
      await invoke("emergency_stop");
      setEmergencyConfirmOpen(false);
      setRuntimeRunning(false);
      ingest({ type: "runtime_status", status: "stopped" });
    } catch (error) {
      ingest({ type: "log", level: "error", message: `${t.emergencyFailed}: ${String(error)}` });
    }
  }

  function updateSettings(next: Settings) {
    setSettings(normalizeSettings(next));
  }

  function updateLanguage(nextLanguage: Language) {
    updateSettings({ ...settings, ui_language: nextLanguage });
  }

  return (
    <div className="viewportFrame">
      <div
        className="appShell"
        lang={language}
        style={{
          width: BASE_WIDTH,
          height: BASE_HEIGHT,
          transform: `translate(-50%, -50%) scale(${viewportScale})`,
        }}
      >
      <header className="commandBar">
        <div className="brandCluster">
          <div className="brandMark">雀</div>
          <div>
            <h1>{t.appTitle}</h1>
            <p>{t.subtitle}</p>
          </div>
        </div>

        <div className="languageSwitch" data-testid="language-switch" aria-label={t.settings}>
          <Languages size={16} />
          {(Object.keys(languageNames) as Language[]).map((lang) => (
            <button
              key={lang}
              className={language === lang ? "active" : ""}
              onClick={() => updateLanguage(lang)}
              type="button"
            >
              {languageNames[lang]}
            </button>
          ))}
        </div>

        <div className={`statusBeacon status-${status}`} data-testid="phase-banner">
          <CircleDot size={15} />
          <span>{statusText}</span>
        </div>

        <div className="commandActions">
          <button className="primary" disabled={!canStart} onClick={start} type="button">
            <Play size={16} /> {t.launch}
          </button>
          <button disabled={!inGame || stopAlreadyScheduled} onClick={stopAfterCurrentGame} type="button">
            <TimerReset size={16} /> {stopAlreadyScheduled ? t.stopScheduled : t.stopAfterGame}
          </button>
          <button
            className="danger"
            disabled={!inGame}
            onClick={() => setEmergencyConfirmOpen(true)}
            type="button"
          >
            <Square size={16} /> {t.emergencyStop}
          </button>
        </div>
      </header>

      <main className="cockpit">
        <aside className="controlDeck">
          <PanelTitle icon={<KeyRound size={17} />} label={t.account} />
          <Field label={t.username}>
            <input
              value={settings.autoplay_account.username}
              onChange={(event) =>
                updateSettings({
                  ...settings,
                  autoplay_account: {
                    ...settings.autoplay_account,
                    username: event.target.value,
                  },
                })
              }
            />
          </Field>
          <Field label={t.password}>
            <input
              type="password"
              value={settings.autoplay_account.password}
              onChange={(event) =>
                updateSettings({
                  ...settings,
                  autoplay_account: {
                    ...settings.autoplay_account,
                    password: event.target.value,
                  },
                })
              }
            />
          </Field>
          <PanelTitle icon={<Bot size={17} />} label={t.model} />
          <div className="modelImportCard">
            <label className="modelSelectRow">
              <span>{t.modelSelect}</span>
              <select
                value={settings.model_path}
                disabled={modelImporting || runtimeRunning || launching}
                onChange={(event) => updateSettings({ ...settings, model_path: event.target.value })}
              >
                {visibleModelChoices.map((choice) => (
                  <option key={choice.model_path} value={choice.model_path}>
                    {choice.builtin ? `${choice.label} (${t.modelBundled})` : choice.label}
                  </option>
                ))}
              </select>
            </label>
            <small title={settings.model_path}>{selectedModelChoice?.builtin ? t.modelBundled : t.modelImported}</small>
            <div className="modelImportActions">
              <span className="modelImportHelpWrap">
                <button
                  className="modelImportHelp"
                  type="button"
                  aria-label={t.modelImportHelp}
                  aria-describedby="model-import-help"
                >
                  <CircleHelp size={15} />
                </button>
                <span id="model-import-help" className="modelImportTooltip" role="tooltip">
                  {t.modelImportHelp}
                </span>
              </span>
              <button
                disabled={modelImporting || runtimeRunning || launching}
                onClick={() => void importModel()}
                type="button"
              >
                <Upload size={15} /> {modelImporting ? t.modelImporting : t.modelImport}
              </button>
            </div>
          </div>

          <PanelTitle icon={<RadioTower size={17} />} label={t.match} />
          <label className="toggleRow">
            <input
              type="checkbox"
              checked={settings.autoplay.room_policy.type === "auto_highest"}
              onChange={(event) =>
                updateSettings({
                  ...settings,
                  autoplay: {
                    ...settings.autoplay,
                    room_policy: event.target.checked ? { type: "auto_highest" } : { type: "manual" },
                  },
                })
              }
            />
            <span>{t.autoHighest}</span>
          </label>
          <Field label={t.manualRoom}>
            <select
              value={settings.autoplay.manual_room}
              disabled={settings.autoplay.room_policy.type === "auto_highest"}
              onChange={(event) =>
                updateSettings({
                  ...settings,
                  autoplay: {
                    ...settings.autoplay,
                    manual_room: event.target.value as RoomChoice,
                  },
                })
              }
            >
              {Object.entries(t.rooms).map(([value, label]) => (
                <option key={value} value={value}>
                  {label}
                </option>
              ))}
            </select>
          </Field>
          <Field label={t.mode}>
            <select
              value={settings.autoplay.manual_mode}
              disabled={settings.autoplay.room_policy.type === "auto_highest"}
              onChange={(event) =>
                updateSettings({
                  ...settings,
                  autoplay: {
                    ...settings.autoplay,
                    manual_mode: event.target.value as ModeChoice,
                  },
                })
              }
            >
              {Object.entries(t.modes).map(([value, label]) => (
                <option key={value} value={value}>
                  {label}
                </option>
              ))}
            </select>
          </Field>

          <PanelTitle icon={<Gauge size={17} />} label={t.tempo} />
          <div className="fieldGrid">
            <Field label={t.minMs}>
              <input
                type="number"
                value={settings.autoplay.action_interval_ms.min}
                onChange={(event) =>
                  updateSettings({
                    ...settings,
                    autoplay: {
                      ...settings.autoplay,
                      action_interval_ms: {
                        ...settings.autoplay.action_interval_ms,
                        min: Number(event.target.value),
                      },
                    },
                  })
                }
              />
            </Field>
            <Field label={t.maxMs}>
              <input
                type="number"
                value={settings.autoplay.action_interval_ms.max}
                onChange={(event) =>
                  updateSettings({
                    ...settings,
                    autoplay: {
                      ...settings.autoplay,
                      action_interval_ms: {
                        ...settings.autoplay.action_interval_ms,
                        max: Number(event.target.value),
                      },
                    },
                  })
                }
              />
            </Field>
          </div>
          <Field label={t.maxGames}>
            <input
              type="number"
              placeholder={t.maxGamesPlaceholder}
              value={settings.autoplay.max_games ?? ""}
              onChange={(event) =>
                updateSettings({
                  ...settings,
                  autoplay: {
                    ...settings.autoplay,
                    max_games: event.target.value === "" ? null : Number(event.target.value),
                  },
                })
              }
            />
          </Field>

          <button className="saveButton" onClick={() => void saveSettings()} type="button">
            <Save size={16} /> {t.save}
          </button>
        </aside>

        <section className="arenaDeck">
          <GameTable
            table={table}
            labels={{
              self: t.self,
              noTable: t.noTable,
              riichi: t.riichi,
              tsumoWin: t.tsumoWin,
              ronWin: t.ronWin,
              dealIn: t.dealIn,
              winningHand: t.winningHand,
              exhaustiveDraw: t.exhaustiveDraw,
              abortiveDraw: t.abortiveDraw,
              winTile: t.winTile,
              points: t.points,
              han: t.han,
              fu: t.fu,
              allPlayersPay: t.allPlayersPay,
              winningMelds: t.winningMelds,
              formatTileName: (tile) => formatTileName(tile, language),
              topPlayer: t.topPlayer,
              leftPlayer: t.leftPlayer,
              rightPlayer: t.rightPlayer,
              dora: t.dora,
              honba: t.honba,
              deposit: t.deposit,
              roundEast: t.roundEast,
              roundSouth: t.roundSouth,
              roundWest: t.roundWest,
              roundNorth: t.roundNorth,
            }}
          />

          <div className="modelStrip">
            <div className="recommendation">
              <span>{t.modelDecision}</span>
              <strong>{modelDecision ? formatActionLabel(modelDecision.action_label, t, language) : "-"}</strong>
              <small>
                {modelDecision
                  ? `${Math.round(modelDecision.confidence * 100)}% ${t.relativeConfidence}`
                  : "-"}
              </small>
            </div>
            <div className="candidateTrack">
              {(modelDecision?.candidates ?? []).slice(0, 3).map((candidate) => (
                <div className="candidateRow" key={candidate.action_index}>
                  <span>{formatActionLabel(candidate.action_label, t, language)}</span>
                  <div className="confidenceBar">
                    <i style={{ width: `${Math.max(4, candidate.confidence * 100)}%` }} />
                  </div>
                  <code>{candidate.q_value.toFixed(2)}</code>
                </div>
              ))}
            </div>
          </div>
        </section>

        <aside className="telemetryRail">
          <TelemetryBlock title={t.players} icon={<Shield size={16} />} className="playersBlock">
            <div className="accountSnapshot liveAccount" data-testid="account-snapshot">
              <div>
                <span>{t.accountInfo}</span>
                <strong>{accountName}</strong>
              </div>
              <small className={account?.refreshing ? "isRefreshing" : ""}>
                {account?.refreshing
                  ? t.accountRefreshing
                  : account?.account_id
                    ? `${t.accountId} ${account.account_id}`
                    : t.accountWaiting}
              </small>
              <dl>
                <div>
                  <dt>{t.rank}</dt>
                  <dd>{formatRank(account?.rank_tier, account?.level_id, account?.level_score, language)}</dd>
                </div>
                <div>
                  <dt>{t.target}</dt>
                  <dd>{accountTarget}</dd>
                </div>
              </dl>
            </div>
            {(table?.players ?? []).length > 0 && (
              <div className="liveMatchPlayers">
                {(table?.players ?? []).map((player) => (
                  <div className="playerLine" key={player.seat}>
                    <span>{playerRelation(player.seat, table?.seat, t)}</span>
                    <strong>{player.points}</strong>
                    <em>#{player.seat}</em>
                    <small>{player.riichi ? t.riichi : ""}</small>
                  </div>
                ))}
              </div>
            )}
          </TelemetryBlock>

          <section className="telemetryBlock recordsBlock">
            <div className="telemetryTitle">
              <div className="recordsTitleLeft">
                <ScrollText size={16} />
                <h2>{t.gameRecords}</h2>
              </div>
              <button
                type="button"
                className="recordRefreshBtn"
                onClick={() => void refreshGameRecords()}
                disabled={recordsLoading}
                title={t.refreshRecords}
              >
                <RefreshCw size={13} className={recordsLoading ? "spinning" : ""} />
                <span>{recordsLoading ? t.refreshingRecords : t.refreshRecords}</span>
              </button>
            </div>

            <div className="recordsList">
              {gameRecords.length === 0 ? (
                <div className="emptyRecords">
                  <p>{recordsLoading ? t.refreshingRecords : t.noGameRecords}</p>
                </div>
              ) : (
                gameRecords.map((record) => {
                  const isCopied = copiedUuid === record.uuid;
                  return (
                    <div className="recordCard" key={record.uuid}>
                      <div className="recordHead">
                        <div className="recordHeadLeft">
                          <span className={`rankPill rank-${record.rank}`}>{formatPlacement(record.rank, language)}</span>
                          <span className="recordRoom">{formatRoomMode(record.mode_id, record.room_name, language)}</span>
                        </div>
                        <span className="recordTime">{formatRecordTime(record.end_time || record.start_time)}</span>
                      </div>
                      <div className="recordBody">
                        <div className="recordStats">
                          <span className="recordScore">
                            {record.score.toLocaleString()} {t.points.trim()}
                          </span>
                          <span className={`recordPt ${record.point_change >= 0 ? "positive" : "negative"}`}>
                            {record.point_change > 0 ? `+${record.point_change}` : record.point_change} pt
                          </span>
                        </div>
                        <div className="recordActions">
                          <button
                            type="button"
                            className={`actionBtn copyBtn ${isCopied ? "copied" : ""}`}
                            onClick={() => void copyPaipuUrl(record)}
                            title={t.copyPaipu}
                          >
                            {isCopied ? <Check size={12} /> : <Copy size={12} />}
                            <span>{isCopied ? t.copied : t.copyPaipu}</span>
                          </button>
                        </div>
                      </div>
                    </div>
                  );
                })
              )}
            </div>
          </section>
        </aside>
      </main>
      {emergencyConfirmOpen ? (
        <div className="confirmOverlay" role="dialog" aria-modal="true" aria-labelledby="emergency-title">
          <div className="confirmDialog">
            <div className="confirmIcon">
              <AlertTriangle size={22} />
            </div>
            <div>
              <h2 id="emergency-title">{t.emergencyStop}</h2>
              <p>{t.emergencyConfirm}</p>
            </div>
            <div className="confirmActions">
              <button type="button" onClick={() => setEmergencyConfirmOpen(false)}>
                {t.emergencyCancel}
              </button>
              <button className="danger" type="button" onClick={() => void emergencyStop()}>
                {t.emergencyConfirmAction}
              </button>
            </div>
          </div>
        </div>
      ) : null}
      </div>
    </div>
  );
}

function formatRank(
  tier: number | null | undefined,
  levelId: number | null | undefined,
  levelScore: number | null | undefined,
  language: Language,
) {
  if (!tier) return levelId ? String(levelId) : "-";
  const zh = ["", "初心", "雀士", "雀杰", "雀豪", "魂天"];
  const en = ["", "Novice", "Adept", "Expert", "Master", "Celestial"];
  const ja = ["", "初心", "雀士", "雀傑", "雀豪", "魂天"];
  const names = language === "en" ? en : language === "ja" ? ja : zh;
  const base = names[tier] ?? `Tier ${tier}`;
  const star = levelId ? levelId % 100 : null;
  const score = levelScore ?? null;
  if (!star) return score == null ? base : `${base} ${score}${language === "en" ? " pts" : "分"}`;
  if (language === "en") {
    return score == null ? `${base} ${star}` : `${base} ${star} · ${score} pts`;
  }
  const starText = language === "ja" ? `${toDisplayNumber(star, language)}星` : `${toDisplayNumber(star, language)}星`;
  return score == null ? `${base}${starText}` : `${base}${starText} ${score}分`;
}

function toDisplayNumber(value: number, language: Language) {
  if (language === "zh") return ["", "一", "二", "三", "四"][value] ?? String(value);
  if (language === "ja") return ["", "一", "二", "三", "四"][value] ?? String(value);
  return String(value);
}

function playerRelation(
  seat: number,
  selfSeat: number | undefined,
  labels: { self: string; topPlayer: string; leftPlayer: string; rightPlayer: string },
) {
  if (selfSeat === undefined) return `#${seat}`;
  const relative = (seat - selfSeat + 4) % 4;
  if (relative === 0) return labels.self;
  if (relative === 1) return labels.rightPlayer;
  if (relative === 2) return labels.topPlayer;
  return labels.leftPlayer;
}

function mergeModelChoices(choices: ModelChoice[]) {
  return ensureCurrentModelChoice(choices, defaultSettings.model_path);
}

function ensureCurrentModelChoice(choices: ModelChoice[], currentPath: string) {
  const deduped: ModelChoice[] = [];
  for (const choice of choices) {
    if (!deduped.some((existing) => existing.model_path === choice.model_path)) {
      deduped.push(choice);
    }
  }
  if (deduped.some((choice) => choice.model_path === currentPath)) {
    return deduped;
  }
  return [
    ...deduped,
    {
      label: modelPathLabel(currentPath),
      model_path: currentPath,
      builtin: currentPath === defaultSettings.model_path,
    },
  ];
}

function modelPathLabel(path: string) {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] || path || "model";
}

function formatActionLabel(
  label: string,
  copyText: {
    modelDiscard: string;
    modelReach: string;
    modelChiLow: string;
    modelChiMid: string;
    modelChiHigh: string;
    modelPon: string;
    modelKan: string;
    modelHora: string;
    modelRyukyoku: string;
    modelNone: string;
  },
  language: Language,
) {
  const discard = label.match(/^discard (.+)$/);
  if (discard) return `${copyText.modelDiscard} ${formatTileName(discard[1], language)}`;
  const reach = label.match(/^reach(?: (.+))?$/);
  if (reach) {
    return reach[1] ? `${copyText.modelReach} ${formatTileName(reach[1], language)}` : copyText.modelReach;
  }
  const map: Record<string, string> = {
    "chi low": copyText.modelChiLow,
    "chi mid": copyText.modelChiMid,
    "chi high": copyText.modelChiHigh,
    pon: copyText.modelPon,
    kan: copyText.modelKan,
    hora: copyText.modelHora,
    ryukyoku: copyText.modelRyukyoku,
    none: copyText.modelNone,
  };
  return map[label] ?? label;
}

function formatLogMessage(message: string, language: Language) {
  const discardAccepted = message.match(/^discard accepted: (.+)$/);
  if (discardAccepted) {
    const tile = formatTileName(discardAccepted[1], language);
    if (language === "en") return `Discarded ${tile}`;
    if (language === "ja") return `${tile}を打牌`;
    return `已打出 ${tile}`;
  }

  if (message.includes("discard refused without discard operation window")) {
    if (language === "en") return "No discard window; ignored one discard request";
    if (language === "ja") return "打牌できない状態のため、打牌要求を1回無視しました";
    return "当前没有出牌窗口，已忽略一次出牌请求";
  }

  if (message.startsWith("game connected:")) {
    if (language === "en") return "Game connected";
    if (language === "ja") return "対局に接続しました";
    return "已进入牌局";
  }

  if (message.includes("match queued successfully")) {
    if (language === "en") return "Queued for matchmaking";
    if (language === "ja") return "マッチング待機に入りました";
    return "已进入匹配队列";
  }

  const loginBegin = message.match(/^login begin: username=(.+)$/);
  if (loginBegin) {
    if (language === "en") return `Logging in as ${loginBegin[1]}`;
    if (language === "ja") return `${loginBegin[1]} でログイン中`;
    return `正在登录 ${loginBegin[1]}`;
  }

  const matchFound = message.match(/^match found:.*game_uuid=([^\\s]+).*/);
  if (matchFound) {
    if (language === "en") return `Match found (${matchFound[1]})`;
    if (language === "ja") return `対局が見つかりました (${matchFound[1]})`;
    return `匹配成功 (${matchFound[1]})`;
  }

  if (message.startsWith("runtime stopped after current game:")) {
    if (language === "en") return "Stopped after the current game";
    if (language === "ja") return "本局後に停止しました";
    return "已在本局结束后停止";
  }

  if (message.startsWith("core event sync failed:")) {
    if (language === "en") return "Failed to refresh runtime events";
    if (language === "ja") return "実行イベントの更新に失敗しました";
    return "刷新运行事件失败";
  }

  if (message.startsWith("runtime snapshot failed:")) {
    if (language === "en") return "Failed to refresh runtime status";
    if (language === "ja") return "実行状態の更新に失敗しました";
    return "刷新运行状态失败";
  }

  return message;
}

function formatTileName(tile: string, language: Language) {
  const honorNames: Record<Language, Record<string, string>> = {
    zh: { E: "东", S: "南", W: "西", N: "北", P: "白", F: "发", C: "中" },
    en: { E: "East", S: "South", W: "West", N: "North", P: "White", F: "Green", C: "Red" },
    ja: { E: "東", S: "南", W: "西", N: "北", P: "白", F: "發", C: "中" },
  };
  const honors = honorNames[language];
  if (honors[tile]) return honors[tile];
  const normalized = tile.replace(/r$/, "");
  const suited = normalized.match(/^([1-9])([mps])$/);
  if (!suited) return tile;
  const suitNames: Record<Language, Record<string, string>> = {
    zh: { m: "万", p: "筒", s: "索" },
    en: { m: "m", p: "p", s: "s" },
    ja: { m: "萬", p: "筒", s: "索" },
  };
  const suit = suitNames[language][suited[2]];
  const red = tile.endsWith("r") ? (language === "en" ? "red " : "赤") : "";
  if (language === "en") return `${red}${suited[1]}${suit}`;
  return `${red}${suited[1]}${suit}`;
}

function PanelTitle({ icon, label }: { icon: ReactNode; label: string }) {
  return (
    <div className="panelTitle">
      {icon}
      <h2>{label}</h2>
    </div>
  );
}

function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <label className="field">
      <span>{label}</span>
      {children}
    </label>
  );
}

function TelemetryBlock({
  title,
  icon,
  children,
  className,
}: {
  title: string;
  icon: ReactNode;
  children: ReactNode;
  className?: string;
}) {
  return (
    <section className={`telemetryBlock ${className ?? ""}`}>
      <div className="telemetryTitle">
        {icon}
        <h2>{title}</h2>
      </div>
      {children}
    </section>
  );
}

function formatRecordTime(timestamp: number) {
  if (!timestamp) return "";
  const d = new Date(timestamp * 1000);
  const m = (d.getMonth() + 1).toString().padStart(2, "0");
  const day = d.getDate().toString().padStart(2, "0");
  const h = d.getHours().toString().padStart(2, "0");
  const min = d.getMinutes().toString().padStart(2, "0");
  return `${m}-${day} ${h}:${min}`;
}

function formatPlacement(rank: number, language: Language): string {
  if (language === "en") {
    if (rank === 1) return "1st";
    if (rank === 2) return "2nd";
    if (rank === 3) return "3rd";
    return `${rank}th`;
  }
  return `${rank}位`;
}

const MODE_NAME_MAP: Record<number, Record<Language, string>> = {
  2: { zh: "铜之间 四人东", en: "Bronze 4-player East", ja: "銅の間 四人東" },
  3: { zh: "铜之间 四人南", en: "Bronze 4-player South", ja: "銅の間 四人南" },
  5: { zh: "银之间 四人东", en: "Silver 4-player East", ja: "銀の間 四人東" },
  6: { zh: "银之间 四人南", en: "Silver 4-player South", ja: "銀の間 四人南" },
  8: { zh: "金之间 四人东", en: "Gold 4-player East", ja: "金の間 四人東" },
  9: { zh: "金之间 四人南", en: "Gold 4-player South", ja: "金の間 四人南" },
  11: { zh: "玉之间 四人东", en: "Jade 4-player East", ja: "玉の間 四人東" },
  12: { zh: "玉之间 四人南", en: "Jade 4-player South", ja: "玉の間 四人南" },
  15: { zh: "王座之间 四人东", en: "Throne 4-player East", ja: "王座の間 四人東" },
  16: { zh: "王座之间 四人南", en: "Throne 4-player South", ja: "王座の間 四人南" },
  21: { zh: "铜之间 三人东", en: "Bronze 3-player East", ja: "銅の間 三人東" },
  22: { zh: "铜之间 三人南", en: "Bronze 3-player South", ja: "銅の間 三人南" },
  23: { zh: "银之间 三人东", en: "Silver 3-player East", ja: "銀の間 三人東" },
  24: { zh: "银之间 三人南", en: "Silver 3-player South", ja: "銀の間 三人南" },
  25: { zh: "金之间 三人东", en: "Gold 3-player East", ja: "金の間 三人東" },
  26: { zh: "金之间 三人南", en: "Gold 3-player South", ja: "金の間 三人南" },
  27: { zh: "玉之间 三人东", en: "Jade 3-player East", ja: "玉の間 三人東" },
  28: { zh: "玉之间 三人南", en: "Jade 3-player South", ja: "玉の間 三人南" },
  29: { zh: "王座之间 三人东", en: "Throne 3-player East", ja: "王座の間 三人東" },
  30: { zh: "王座之间 三人南", en: "Throne 3-player South", ja: "王座の間 三人南" },
};

function formatRoomMode(modeId: number, roomNameFallback: string, language: Language): string {
  const mapped = MODE_NAME_MAP[modeId];
  if (mapped && mapped[language]) {
    return mapped[language];
  }

  const fallback = roomNameFallback || (modeId > 0 ? `段位战 (${modeId})` : "段位战");

  if (language === "en") {
    return fallback
      .replace("段位战", "Ranked")
      .replace("铜之间", "Bronze")
      .replace("银之间", "Silver")
      .replace("金之间", "Gold")
      .replace("玉之间", "Jade")
      .replace("王座之间", "Throne")
      .replace("王座间", "Throne")
      .replace("四人东", "4-player East")
      .replace("四人南", "4-player South")
      .replace("三人东", "3-player East")
      .replace("三人南", "3-player South");
  }

  if (language === "ja") {
    return fallback
      .replace("段位战", "段位戦")
      .replace("铜之间", "銅の間")
      .replace("银之间", "銀の間")
      .replace("金之间", "金の間")
      .replace("玉之间", "玉の間")
      .replace("王座之间", "王座の間")
      .replace("王座间", "王座の間")
      .replace("四人东", "四人東")
      .replace("四人南", "四人南")
      .replace("三人东", "三人東")
      .replace("三人南", "三人南");
  }

  return fallback;
}
