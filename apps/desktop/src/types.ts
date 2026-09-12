export type RoomPolicy = { type: "auto_highest" } | { type: "manual" };
export type RoomChoice = "bronze" | "silver" | "gold" | "jade" | "throne";
export type ModeChoice = "four_player_east" | "four_player_south";
export type Language = "zh" | "en" | "ja";

export type Settings = {
  model_path: string;
  ui_language: Language;
  autoplay_account: {
    username: string;
    password: string;
  };
  autoplay: {
    room_policy: RoomPolicy;
    manual_room: RoomChoice;
    manual_mode: ModeChoice;
    action_interval_ms: {
      min: number;
      max: number;
    };
    max_games: number | null;
  };
};

export type RuntimeStatus =
  | "idle"
  | "logging_in"
  | "matching"
  | "reconnecting"
  | "in_game"
  | "stopping_after_game"
  | "stopped"
  | "error";

export type RuntimeSnapshot = {
  running: boolean;
  status: RuntimeStatus;
  last_error: string | null;
  settings_path: string;
  runtime_log_path: string;
};

export type CoreEventRecord = {
  seq: number;
  event: CoreEvent;
};

export type CoreEventBatch = {
  cursor: number;
  events: CoreEventRecord[];
};

export type ModelImportResult = {
  model_path: string;
  model_name: string;
};

export type ModelChoice = {
  label: string;
  model_path: string;
  builtin: boolean;
};

export type BridgeEvent = {
  type: string;
  [key: string]: unknown;
};

export type TableSnapshot = {
  seat: number;
  bakaze: string;
  kyoku: number;
  honba: number;
  kyotaku: number;
  oya: number;
  dora_markers: string[];
  scores: number[];
  players: PlayerSnapshot[];
  last_event: BridgeEvent | null;
};

export type PlayerSnapshot = {
  seat: number;
  points: number;
  hand: string[];
  hand_count: number;
  discards: DiscardSnapshot[];
  melds: MeldSnapshot[];
  riichi: boolean;
  is_self: boolean;
};

export type DiscardSnapshot = {
  tile: string;
  tsumogiri: boolean;
  riichi: boolean;
};

export type MeldSnapshot = {
  kind: "chi" | "pon" | "daiminkan" | "ankan" | "kakan";
  target: number | null;
  called_tile: string | null;
  consumed: string[];
};

export type ModelDecision = {
  action_index: number;
  action_label: string;
  confidence: number;
  candidates: ModelCandidate[];
  is_greedy: boolean;
};

export type ModelCandidate = {
  action_index: number;
  action_label: string;
  q_value: number;
  confidence: number;
  legal: boolean;
};

export type ActionAck = {
  ok: boolean;
  message: string;
};

export type AccountSnapshot = {
  refreshing: boolean;
  username: string;
  account_id: number | null;
  nickname: string | null;
  level_id: number | null;
  level_score: number | null;
  rank_tier: number | null;
  target_mode: ModeChoice | null;
  target_room: RoomChoice | null;
};

export type LogItem = {
  level: "info" | "warn" | "error";
  message: string;
  at: number;
};

export type GameRecordPlayer = {
  account_id: number;
  nickname: string;
  seat: number;
  rank: number;
  score: number;
  point_change: number;
  is_self: boolean;
};

export type GameRecordSummary = {
  uuid: string;
  start_time: number;
  end_time: number;
  mode_id: number;
  room_name: string;
  rank: number;
  score: number;
  point_change: number;
  paipu_url: string;
  players: GameRecordPlayer[];
};

export type CoreEvent =
  | { type: "runtime_status"; status: RuntimeStatus }
  | { type: "account_snapshot"; account: AccountSnapshot }
  | { type: "table_snapshot"; table: TableSnapshot }
  | { type: "game_event"; event: BridgeEvent }
  | { type: "model_decision"; decision: ModelDecision }
  | { type: "action_planned"; action: unknown }
  | { type: "action_ack"; ack: ActionAck }
  | { type: "log"; level: "info" | "warn" | "error"; message: string }
  | { type: "game_completed"; games_done: number }
  | { type: "stop_scheduled"; after_current_game: boolean }
  | { type: "runtime_error"; message: string }
  | { type: "game_records"; records: GameRecordSummary[] };

export const defaultSettings: Settings = {
  model_path: "models/mortal",
  ui_language: "zh",
  autoplay_account: {
    username: "",
    password: "",
  },
  autoplay: {
    room_policy: { type: "auto_highest" },
    manual_room: "bronze",
    manual_mode: "four_player_east",
    action_interval_ms: { min: 800, max: 1600 },
    max_games: null,
  },
};
