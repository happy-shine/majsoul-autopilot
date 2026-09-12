import { create } from "zustand";
import type {
  ActionAck,
  AccountSnapshot,
  BridgeEvent,
  CoreEvent,
  LogItem,
  ModelDecision,
  RuntimeStatus,
  Settings,
  TableSnapshot,
} from "./types";
import { defaultSettings } from "./types";

type AppStore = {
  status: RuntimeStatus;
  settings: Settings;
  table: TableSnapshot | null;
  modelDecision: ModelDecision | null;
  events: BridgeEvent[];
  logs: LogItem[];
  ack: ActionAck | null;
  account: AccountSnapshot | null;
  gameRecords: import("./types").GameRecordSummary[];
  stopScheduled: boolean;
  setSettings: (settings: Settings) => void;
  patchSettings: (patch: Partial<Settings>) => void;
  setGameRecords: (records: import("./types").GameRecordSummary[]) => void;
  ingest: (event: CoreEvent) => void;
};

const statusLogText: Record<RuntimeStatus, string> = {
  idle: "状态: 未启动",
  logging_in: "状态: 登录中",
  matching: "状态: 匹配中",
  reconnecting: "状态: 重连中",
  in_game: "状态: 对局中",
  stopping_after_game: "状态: 本局后停止",
  stopped: "状态: 已停止",
  error: "状态: 异常",
};

function prependLog(logs: LogItem[], log: Omit<LogItem, "at">) {
  const now = Date.now();
  const latest = logs[0];
  if (latest && latest.level === log.level && latest.message === log.message && now - latest.at < 5_000) {
    return logs;
  }
  return [{ ...log, at: now }, ...logs].slice(0, 200);
}

export const useAppStore = create<AppStore>((set) => ({
  status: "idle",
  settings: defaultSettings,
  table: null,
  modelDecision: null,
  events: [],
  logs: [],
  ack: null,
  account: null,
  gameRecords: [],
  stopScheduled: false,
  setSettings: (settings) => set({ settings }),
  patchSettings: (patch) =>
    set((state) => ({
      settings: {
        ...state.settings,
        ...patch,
      },
    })),
  setGameRecords: (gameRecords) => set({ gameRecords }),
  ingest: (event) =>
    set((state) => {
      switch (event.type) {
        case "runtime_status":
          if (event.status === state.status) {
            return {};
          }
          return {
            status: event.status,
            stopScheduled:
              event.status === "stopping_after_game"
                ? true
                : ["idle", "logging_in", "matching", "stopped", "error"].includes(event.status)
                  ? false
                  : state.stopScheduled,
            logs: prependLog(state.logs, {
              level: event.status === "error" ? "error" : "info",
              message: statusLogText[event.status],
            }),
          };
        case "account_snapshot":
          return { account: event.account };
        case "table_snapshot":
          return { table: event.table };
        case "game_event":
          return { events: [event.event, ...state.events].slice(0, 80) };
        case "model_decision":
          return { modelDecision: event.decision };
        case "action_ack":
          return { ack: event.ack };
        case "game_completed":
          return {};
        case "stop_scheduled":
          return { stopScheduled: event.after_current_game };
        case "runtime_error":
          return {
            status: "error",
            logs: prependLog(state.logs, { level: "error", message: event.message }),
          };
        case "log":
          return {
            logs: prependLog(state.logs, { level: event.level, message: event.message }),
          };
        case "game_records":
          return { gameRecords: event.records };
        default:
          return {};
      }
    }),
}));
