import { mkdirSync } from "node:fs";
import { spawn } from "node:child_process";
import { chromium } from "playwright";

const port = 1424;
const url = `http://127.0.0.1:${port}`;
const screenshotDir = "/tmp/majsoul-autopilot-scenarios";
mkdirSync(screenshotDir, { recursive: true });

const viewports = [
  { id: "desktop", width: 1600, height: 1000 },
  { id: "compact", width: 1280, height: 860 },
  { id: "wide", width: 1920, height: 1080 },
];

const server = spawn(
  process.platform === "win32" ? "npm.cmd" : "npm",
  ["run", "dev", "--", "--host", "127.0.0.1", "--port", String(port)],
  { stdio: ["ignore", "pipe", "pipe"] },
);

const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

const baseAccount = {
  refreshing: false,
  username: "370501542@qq.com",
  account_id: 23744444,
  nickname: "SsssssssY",
  level_id: 10301,
  level_score: 683,
  rank_tier: 3,
  target_mode: "four_player_south",
  target_room: "gold",
};

const settings = {
  model_path: "models/mortal",
  ui_language: "zh",
  autoplay_account: { username: "370501542@qq.com", password: "password" },
  autoplay: {
    room_policy: { type: "auto_highest" },
    manual_room: "gold",
    manual_mode: "four_player_south",
    action_interval_ms: { min: 800, max: 1600 },
    max_games: null,
  },
};

function discardTiles(tiles, riichiIndex = -1) {
  return tiles.map((tile, index) => ({
    tile,
    tsumogiri: index % 3 === 1,
    riichi: index === riichiIndex,
  }));
}

function player(seat, points, hand, discards, melds = [], riichi = false, isSelf = false) {
  return {
    seat,
    points,
    hand: isSelf ? hand : [],
    hand_count: isSelf ? hand.length : hand,
    discards,
    melds,
    riichi,
    is_self: isSelf,
  };
}

function table({
  seat = 3,
  bakaze = "E",
  kyoku = 1,
  honba = 0,
  kyotaku = 0,
  dora = ["6p"],
  players,
  last_event = null,
}) {
  return {
    seat,
    bakaze,
    kyoku,
    honba,
    kyotaku,
    oya: 0,
    dora_markers: dora,
    scores: players.map((item) => item.points),
    players,
    last_event,
  };
}

const scenarioFilter = process.env.SCENARIO_FILTER ?? "";

const scenarios = [
  {
    id: "01-idle-empty",
    status: "idle",
    running: false,
    account: null,
    table: null,
    model: null,
    events: [],
    logs: [{ level: "info", message: "ready" }],
  },
  {
    id: "02-login-matching",
    status: "matching",
    running: true,
    account: { ...baseAccount, refreshing: true, nickname: null, account_id: null, level_id: null, level_score: null },
    table: null,
    model: null,
    events: [],
    logs: [
      { level: "info", message: "login begin: username=370501542@qq.com" },
      { level: "info", message: "match queued successfully" },
    ],
  },
  {
    id: "03-east1-initial",
    status: "in_game",
    running: true,
    account: baseAccount,
    table: table({
      players: [
        player(0, 25000, 13, [], []),
        player(1, 25000, 13, [], []),
        player(2, 25000, 13, [], []),
        player(3, 25000, ["1m", "7m", "9m", "5pr", "2p", "6p", "7s", "4s", "5s", "F", "E", "9s", "4p", "6s"], [], [], false, true),
      ],
    }),
    model: {
      action_index: 1,
      action_label: "discard 1m",
      confidence: 0.5,
      is_greedy: true,
      candidates: [
        { action_index: 1, action_label: "discard 1m", q_value: 0.27, confidence: 0.5, legal: true },
        { action_index: 2, action_label: "discard 2s", q_value: -0.33, confidence: 0.32, legal: true },
        { action_index: 3, action_label: "discard F", q_value: -0.5, confidence: 0.26, legal: true },
      ],
    },
    events: [{ type: "start_kyoku", bakaze: "E", kyoku: 1, honba: 0, kyotaku: 0 }],
    logs: [{ level: "info", message: "game connected: initial_events=0 operation_window=0" }],
  },
  {
    id: "04-after-first-discards",
    status: "in_game",
    running: true,
    account: baseAccount,
    table: table({
      kyoku: 2,
      dora: ["4p"],
      players: [
        player(0, 24600, 14, discardTiles(["5p", "E", "9s"]), []),
        player(1, 26000, 13, discardTiles(["8p", "2m"]), []),
        player(2, 25000, 13, discardTiles(["4m", "9p", "2s", "W"]), []),
        player(3, 24400, ["1m", "2m", "3m", "7m", "8m", "4p", "5p", "6p", "2s", "3s", "4s", "E", "S", "P"], discardTiles(["1p", "N"]), [], false, true),
      ],
      last_event: { type: "dahai", actor: 3, pai: "N", tsumogiri: false },
    }),
    model: {
      action_index: 7,
      action_label: "discard 7m",
      confidence: 0.61,
      is_greedy: true,
      candidates: [
        { action_index: 7, action_label: "discard 7m", q_value: 0.7, confidence: 0.61, legal: true },
        { action_index: 8, action_label: "discard 8m", q_value: 0.2, confidence: 0.28, legal: true },
        { action_index: 34, action_label: "none", q_value: -0.1, confidence: 0.11, legal: true },
      ],
    },
    events: [
      { type: "dahai", actor: 3, pai: "N", tsumogiri: false },
      { type: "tsumo", actor: 0, pai: "?" },
    ],
    logs: [
      { level: "info", message: "discard accepted: N" },
      { level: "warn", message: "discard refused without discard operation window" },
    ],
  },
  {
    id: "05-riichi-window",
    status: "in_game",
    running: true,
    account: baseAccount,
    table: table({
      kyoku: 3,
      kyotaku: 1,
      players: [
        player(0, 24000, 13, discardTiles(["5p", "E", "9s", "1m"], 3), [], true),
        player(1, 25000, 13, discardTiles(["8p", "2m", "7s"]), []),
        player(2, 26000, 13, discardTiles(["4m", "9p", "2s", "W"]), []),
        player(3, 25000, ["2m", "3m", "4m", "5m", "6m", "7p", "8p", "9p", "2s", "3s", "4s", "P", "F", "C"], discardTiles(["1p", "N", "E"]), [], false, true),
      ],
      last_event: { type: "reach", actor: 0 },
    }),
    model: {
      action_index: 12,
      action_label: "reach 4m",
      confidence: 0.74,
      is_greedy: true,
      candidates: [
        { action_index: 12, action_label: "reach 4m", q_value: 1.2, confidence: 0.74, legal: true },
        { action_index: 13, action_label: "discard C", q_value: 0.5, confidence: 0.18, legal: true },
        { action_index: 14, action_label: "discard F", q_value: 0.2, confidence: 0.08, legal: true },
      ],
    },
    events: [
      { type: "reach", actor: 0 },
      { type: "dahai", actor: 0, pai: "1m", tsumogiri: false },
    ],
    logs: [{ level: "info", message: "riichi decision prepared" }],
  },
  {
    id: "06-meld-heavy",
    status: "in_game",
    running: true,
    account: baseAccount,
    table: table({
      bakaze: "S",
      kyoku: 1,
      dora: ["4p", "8s", "1m", "E", "C"],
      players: [
        player(0, 19800, 11, discardTiles(["5p", "E", "9s", "1m", "3p"]), [
          { kind: "pon", target: 2, called_tile: "C", consumed: ["C", "C"] },
        ]),
        player(1, 31200, 10, discardTiles(["8p", "2m", "7s", "4s"]), [
          { kind: "chi", target: 0, called_tile: "7s", consumed: ["5s", "6s"] },
        ]),
        player(2, 24600, 10, discardTiles(["4m", "9p", "2s", "W", "6m"]), [
          { kind: "daiminkan", target: 1, called_tile: "8m", consumed: ["8m", "8m", "8m"] },
        ]),
        player(3, 24400, ["1m", "2m", "3m", "4p", "5p", "6p", "2s", "3s", "4s", "E", "S"], discardTiles(["1p", "N", "E", "9m"]), [
          { kind: "pon", target: 1, called_tile: "8p", consumed: ["8p", "8p"] },
        ], false, true),
      ],
    }),
    model: {
      action_index: 20,
      action_label: "discard E",
      confidence: 0.45,
      is_greedy: true,
      candidates: [
        { action_index: 20, action_label: "discard E", q_value: 0.15, confidence: 0.45, legal: true },
        { action_index: 21, action_label: "discard S", q_value: -0.1, confidence: 0.3, legal: true },
        { action_index: 22, action_label: "pon", q_value: -0.7, confidence: 0.25, legal: true },
      ],
    },
    events: [
      { type: "chi", actor: 1, target: 0, pai: "7s", consumed: ["5s", "6s"] },
      { type: "pon", actor: 3, target: 1, pai: "8p", consumed: ["8p", "8p"] },
    ],
    logs: [{ level: "info", message: "meld-heavy scenario with side calls" }],
  },
  {
    id: "07-late-four-row-rivers",
    status: "in_game",
    running: true,
    account: baseAccount,
    table: table({
      bakaze: "S",
      kyoku: 3,
      honba: 2,
      kyotaku: 1,
      players: [
        player(0, 42000, 4, discardTiles(["5p", "E", "9s", "1m", "3p", "7p", "W", "2m", "4m", "5m", "6m", "1s", "2s", "3s", "4s", "5s", "6s", "P", "F", "C"], 8), []),
        player(1, 16800, 7, discardTiles(["8p", "2m", "7s", "4s", "9m", "1p", "2p", "3p", "4p", "5p", "6p", "7p", "8p", "9p", "E", "S", "W", "N", "P"], 12), []),
        player(2, 9200, 5, discardTiles(["4m", "9p", "2s", "W", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "4p", "5p", "6p", "7p", "8p", "9p", "C"], 4), []),
        player(3, 32000, ["1m", "2m", "3m", "4p", "5p", "6p", "2s", "3s"], discardTiles(["1p", "N", "E", "9m", "8s", "7s", "6s", "5s", "4s", "3s", "2s", "1s", "P", "F", "C", "W", "S"], 10), [
          { kind: "ankan", target: null, called_tile: null, consumed: ["9p", "9p", "9p", "9p"] },
          { kind: "pon", target: 0, called_tile: "E", consumed: ["E", "E"] },
        ], false, true),
      ],
    }),
    model: {
      action_index: 28,
      action_label: "hora",
      confidence: 0.98,
      is_greedy: true,
      candidates: [
        { action_index: 28, action_label: "hora", q_value: 12.4, confidence: 0.98, legal: true },
        { action_index: 29, action_label: "discard 1m", q_value: 0.1, confidence: 0.01, legal: true },
        { action_index: 30, action_label: "none", q_value: -2, confidence: 0.01, legal: true },
      ],
    },
    events: [
      { type: "tsumo", actor: 3, pai: "?" },
      { type: "dahai", actor: 2, pai: "C", tsumogiri: false },
    ],
    logs: [{ level: "warn", message: "late-round stress scenario with long rivers and long diagnostic text abcdefghijklmnopqrstuvwxyz0123456789" }],
  },
  {
    id: "08-hule-result",
    status: "in_game",
    running: true,
    account: baseAccount,
    table: table({
      bakaze: "S",
      kyoku: 4,
      honba: 1,
      kyotaku: 0,
      players: [
        player(0, 18000, 13, discardTiles(["5p", "E", "9s"]), []),
        player(1, 42000, 13, discardTiles(["8p", "2m", "7s"]), []),
        player(2, 12000, 13, discardTiles(["4m", "9p", "2s", "W"]), []),
        player(
          3,
          28000,
          ["1m", "2m", "3m", "7m", "8m", "9m", "4p", "5p", "6p", "2s", "3s", "4s", "C"],
          discardTiles(["1p", "N", "E"]),
          [{ kind: "pon", target: 1, called_tile: "P", consumed: ["P", "P"] }],
          false,
          true,
        ),
      ],
      last_event: {
        type: "hule",
        actor: 3,
        target: null,
        pai: "C",
        zimo: true,
        title: "満貫",
        count: 5,
        fu: 40,
        fans: [
          { name: "門前清自摸和", val: 1, id: 1 },
          { name: "ドラ", val: 4, id: 34 },
        ],
        point_sum: 8000,
        hand: ["1m", "2m", "3m", "7m", "8m", "9m", "4p", "5p", "6p", "2s", "3s", "4s", "C"],
        ming: [],
      },
    }),
    model: null,
    events: [
      {
        type: "hule",
        actor: 3,
        target: null,
        pai: "C",
        zimo: true,
        title: "満貫",
        count: 5,
        fu: 40,
        fans: [
          { name: "門前清自摸和", val: 1, id: 1 },
          { name: "ドラ", val: 4, id: 34 },
        ],
        point_sum: 8000,
        hand: ["1m", "2m", "3m", "7m", "8m", "9m", "4p", "5p", "6p", "2s", "3s", "4s", "C"],
        ming: [],
      },
    ],
    logs: [{ level: "info", message: "round ended with hule" }],
  },
  {
    id: "09-ron-result",
    status: "in_game",
    running: true,
    account: baseAccount,
    table: table({
      bakaze: "E",
      kyoku: 4,
      honba: 0,
      kyotaku: 2,
      players: [
        player(0, 33000, 13, discardTiles(["5p", "E", "9s", "1m"]), []),
        player(1, 31000, 13, discardTiles(["8p", "2m", "7s", "4s"]), []),
        player(2, 17000, 13, discardTiles(["4m", "9p", "2s", "W", "6m"]), []),
        player(3, 19000, ["1m", "2m", "3m", "4p", "5p", "6p", "2s", "3s", "4s", "E", "S", "W", "N"], discardTiles(["1p", "N", "E", "C"]), [], false, true),
      ],
      last_event: {
        type: "hule",
        actor: 1,
        target: 3,
        pai: "C",
        zimo: false,
        title: "跳満",
        count: 6,
        fu: 30,
        fans: [
          { name: "立直", val: 1, id: 1 },
          { name: "一発", val: 1, id: 2 },
          { name: "ドラ", val: 4, id: 34 },
        ],
        point_sum: 12000,
        hand: ["1m", "2m", "3m", "4p", "5p", "6p", "2s", "3s", "4s", "E", "S", "W", "N"],
        ming: [],
      },
    }),
    model: null,
    events: [
      {
        type: "hule",
        actor: 1,
        target: 3,
        pai: "C",
        zimo: false,
        title: "跳満",
        count: 6,
        fu: 30,
        fans: [
          { name: "立直", val: 1, id: 1 },
          { name: "一発", val: 1, id: 2 },
          { name: "ドラ", val: 4, id: 34 },
        ],
        point_sum: 12000,
        hand: ["1m", "2m", "3m", "4p", "5p", "6p", "2s", "3s", "4s", "E", "S", "W", "N"],
        ming: [],
      },
    ],
    logs: [{ level: "info", message: "round ended with ron" }],
  },
  {
    id: "10-no-tile-result",
    status: "in_game",
    running: true,
    account: baseAccount,
    table: table({
      bakaze: "S",
      kyoku: 2,
      honba: 3,
      kyotaku: 1,
      players: [
        player(0, 26000, 0, discardTiles(["5p", "E", "9s", "1m", "3p", "7p", "W", "2m", "4m", "5m", "6m", "1s", "2s", "3s", "4s", "5s", "6s", "P"], 8), []),
        player(1, 27000, 0, discardTiles(["8p", "2m", "7s", "4s", "9m", "1p", "2p", "3p", "4p", "5p", "6p", "7p", "8p", "9p", "E", "S", "W"], 12), []),
        player(2, 25000, 0, discardTiles(["4m", "9p", "2s", "W", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "4p", "5p", "6p", "7p"], 4), []),
        player(3, 22000, [], discardTiles(["1p", "N", "E", "9m", "8s", "7s", "6s", "5s", "4s", "3s", "2s", "1s", "P", "F", "C"], 10), [], false, true),
      ],
      last_event: { type: "no_tile", liujumanguan: false },
    }),
    model: null,
    events: [{ type: "no_tile", liujumanguan: false }],
    logs: [{ level: "info", message: "round ended with exhaustive draw" }],
  },
  {
    id: "11-liu-ju-result",
    status: "in_game",
    running: true,
    account: baseAccount,
    table: table({
      bakaze: "E",
      kyoku: 1,
      honba: 0,
      kyotaku: 0,
      players: [
        player(0, 25000, 13, discardTiles(["5p", "E"]), []),
        player(1, 25000, 13, discardTiles(["8p", "2m"]), []),
        player(2, 25000, 13, discardTiles(["4m", "9p"]), []),
        player(3, 25000, ["1m", "9m", "1p", "9p", "1s", "9s", "E", "S", "W", "N", "P", "F", "C"], discardTiles(["1p"]), [], false, true),
      ],
      last_event: { type: "liu_ju", actor: 3, reason: 1 },
    }),
    model: null,
    events: [{ type: "liu_ju", actor: 3, reason: 1 }],
    logs: [{ level: "info", message: "round ended with abortive draw" }],
  },
  {
    id: "12-extreme-names-and-melds",
    status: "in_game",
    running: true,
    account: { ...baseAccount, nickname: "超長名字SsssssssYLongNameTest" },
    table: table({
      bakaze: "S",
      kyoku: 4,
      honba: 4,
      kyotaku: 2,
      players: [
        player(0, 1200, 5, discardTiles(["5p", "E", "9s", "1m", "3p", "7p", "W", "2m", "4m", "5m"]), [
          { kind: "pon", target: 2, called_tile: "P", consumed: ["P", "P"] },
          { kind: "kakan", target: 3, called_tile: "E", consumed: ["E", "E", "E"] },
        ], true),
        player(1, 58000, 4, discardTiles(["8p", "2m", "7s", "4s", "9m", "1p", "2p", "3p"], 3), [
          { kind: "chi", target: 2, called_tile: "3s", consumed: ["1s", "2s"] },
          { kind: "pon", target: 0, called_tile: "N", consumed: ["N", "N"] },
        ]),
        player(2, 9000, 6, discardTiles(["4m", "9p", "2s", "W", "6m", "7m", "8m"], 2), [
          { kind: "daiminkan", target: 1, called_tile: "7p", consumed: ["7p", "7p", "7p"] },
        ]),
        player(3, 31800, ["1m", "2m", "3m", "4p", "5p", "6p", "2s", "3s"], discardTiles(["1p", "N", "E", "9m", "8s", "7s", "6s", "5s"], 5), [
          { kind: "ankan", target: null, called_tile: null, consumed: ["9p", "9p", "9p", "9p"] },
          { kind: "pon", target: 0, called_tile: "F", consumed: ["F", "F"] },
        ], false, true),
      ],
    }),
    model: {
      action_index: 30,
      action_label: "none",
      confidence: 0.67,
      is_greedy: true,
      candidates: [
        { action_index: 30, action_label: "none", q_value: 0.5, confidence: 0.67, legal: true },
        { action_index: 22, action_label: "pon", q_value: -0.2, confidence: 0.2, legal: true },
        { action_index: 28, action_label: "hora", q_value: -0.8, confidence: 0.13, legal: true },
      ],
    },
    events: [
      { type: "kakan", actor: 0, pai: "E", consumed: ["E", "E", "E"] },
      { type: "pon", actor: 3, target: 0, pai: "F", consumed: ["F", "F"] },
    ],
    logs: [{ level: "warn", message: "extreme layout scenario with many melds and long account name" }],
  },
  {
    id: "13-late-all-melds",
    status: "in_game",
    running: true,
    account: { ...baseAccount, nickname: "SsssssssY" },
    table: table({
      bakaze: "S",
      kyoku: 4,
      honba: 6,
      kyotaku: 3,
      dora: ["1m", "5pr", "C"],
      players: [
        player(0, 9800, 4, discardTiles(["5p", "E", "9s", "1m", "3p", "7p", "W", "2m", "4m", "5m", "6m", "1s", "2s", "3s"], 6), [
          { kind: "chi", target: 3, called_tile: "4m", consumed: ["2m", "3m"] },
          { kind: "pon", target: 2, called_tile: "P", consumed: ["P", "P"] },
        ], true),
        player(1, 44200, 3, discardTiles(["8p", "2m", "7s", "4s", "9m", "1p", "2p", "3p", "4p", "5p", "6p", "7p", "8p"], 10), [
          { kind: "chi", target: 2, called_tile: "6s", consumed: ["4s", "5s"] },
          { kind: "pon", target: 0, called_tile: "F", consumed: ["F", "F"] },
          { kind: "daiminkan", target: 3, called_tile: "9p", consumed: ["9p", "9p", "9p"] },
        ]),
        player(2, 18200, 4, discardTiles(["4m", "9p", "2s", "W", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "4p"], 5), [
          { kind: "pon", target: 1, called_tile: "S", consumed: ["S", "S"] },
          { kind: "kakan", target: 3, called_tile: "7m", consumed: ["7m", "7m", "7m"] },
        ], true),
        player(3, 27800, ["1m", "2m", "3m", "4p", "5p"], discardTiles(["1p", "N", "E", "9m", "8s", "7s", "6s", "5s", "4s", "3s", "2s", "1s", "P"], 9), [
          { kind: "ankan", target: null, called_tile: null, consumed: ["8m", "8m", "8m", "8m"] },
          { kind: "pon", target: 0, called_tile: "C", consumed: ["C", "C"] },
          { kind: "chi", target: 0, called_tile: "6p", consumed: ["4p", "5p"] },
        ], false, true),
      ],
    }),
    model: {
      action_index: 28,
      action_label: "hora",
      confidence: 0.88,
      is_greedy: true,
      candidates: [
        { action_index: 28, action_label: "hora", q_value: 8.1, confidence: 0.88, legal: true },
        { action_index: 29, action_label: "none", q_value: 1.0, confidence: 0.08, legal: true },
        { action_index: 30, action_label: "discard 1m", q_value: -0.7, confidence: 0.04, legal: true },
      ],
    },
    events: [
      { type: "reach", actor: 0 },
      { type: "reach", actor: 2 },
      { type: "pon", actor: 1, target: 0, pai: "F", consumed: ["F", "F"] },
      { type: "chi", actor: 3, target: 0, pai: "6p", consumed: ["4p", "5p"] },
    ],
    logs: [{ level: "warn", message: "late all-melds stress scenario" }],
  },
  {
    id: "14-error-stopped",
    status: "error",
    running: false,
    account: baseAccount,
    table: null,
    model: null,
    events: [],
    logs: [{ level: "error", message: "runtime stopped after current game: websocket closed cleanly" }],
  },
];

function coreEventsFor(scenario) {
  let seq = 1;
  const records = [{ seq: seq++, event: { type: "runtime_status", status: scenario.status } }];
  if (scenario.account) records.push({ seq: seq++, event: { type: "account_snapshot", account: scenario.account } });
  if (scenario.table) records.push({ seq: seq++, event: { type: "table_snapshot", table: scenario.table } });
  if (scenario.model) records.push({ seq: seq++, event: { type: "model_decision", decision: scenario.model } });
  for (const event of scenario.events) records.push({ seq: seq++, event: { type: "game_event", event } });
  for (const log of scenario.logs) records.push({ seq: seq++, event: { type: "log", ...log } });
  return records;
}

function installTauriMock(page, scenario) {
  return page.addInitScript(({ settings, scenario, coreEvents }) => {
    const callbacks = {};
    let nextCallbackId = 1;
    window.__invokeCalls = [];
    window.__TAURI_INTERNALS__ = {
      callbacks,
      transformCallback(callback) {
        const id = nextCallbackId;
        nextCallbackId += 1;
        callbacks[id] = callback;
        return id;
      },
      unregisterCallback(id) {
        delete callbacks[id];
      },
      runCallback(id, payload) {
        callbacks[id]?.(payload);
      },
      async invoke(command, args = {}) {
        window.__invokeCalls.push({ command, args });
        if (command === "load_settings") return settings;
        if (command === "save_settings") return null;
        if (command === "get_runtime_snapshot") {
          return {
            running: scenario.running,
            status: scenario.status,
            last_error: scenario.status === "error" ? scenario.logs.at(-1)?.message ?? "error" : null,
            settings_path: "settings.json",
            runtime_log_path: "logs/runtime/gui_autoplay.log",
          };
        }
        if (command === "get_core_event_batch") {
          const after = args.after ?? 0;
          const events = coreEvents.filter((record) => record.seq > after);
          return { cursor: events.at(-1)?.seq ?? after, events };
        }
        if (command === "start_autoplay") return null;
        if (command === "stop_after_current_game") return null;
        if (command === "emergency_stop") return null;
        if (command === "plugin:event|listen") return 1;
        if (command === "plugin:event|unlisten") return null;
        throw new Error(`unhandled invoke: ${command}`);
      },
      convertFileSrc(path) {
        return path;
      },
    };
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
  }, { settings, scenario, coreEvents: coreEventsFor(scenario) });
}

function compactRect(el) {
  if (!el) return null;
  const rect = el.getBoundingClientRect();
  return {
    left: Math.round(rect.left),
    top: Math.round(rect.top),
    right: Math.round(rect.right),
    bottom: Math.round(rect.bottom),
    width: Math.round(rect.width),
    height: Math.round(rect.height),
    scrollWidth: el.scrollWidth,
    clientWidth: el.clientWidth,
    scrollHeight: el.scrollHeight,
    clientHeight: el.clientHeight,
  };
}

function assertScenario(scenario, metrics) {
  const errors = [];
  if (metrics.overflowX || metrics.overflowY) errors.push("document overflow");
  if (!metrics.appShell || metrics.appShell.left < 0 || metrics.appShell.top < 0) errors.push("app shell outside viewport");
  if (scenario.table) {
    if (!metrics.tableArea || metrics.tableArea.width < 480 || metrics.tableArea.height < 480) errors.push("table too small");
    if (Math.abs(metrics.tableArea.width - metrics.tableArea.height) > 8) errors.push("table is not square");
    if (metrics.outsideTableTiles.length > 0) errors.push(`table tiles outside table: ${JSON.stringify(metrics.outsideTableTiles.slice(0, 3))}`);
    if (metrics.riverCenterOverlaps.length > 0) errors.push(`river overlaps center: ${metrics.riverCenterOverlaps.join(",")}`);
    if (metrics.riverCrossOverlaps.length > 0) errors.push(`rivers overlap each other: ${metrics.riverCrossOverlaps.slice(0, 5).join(",")}`);
    if (metrics.handMeldOverflow) errors.push("hand melds overflow hand area");
    if (metrics.seatRiverOverlaps.length > 0) errors.push(`river overlaps side hand panel: ${metrics.seatRiverOverlaps.join(",")}`);
    if (!metrics.selfRiverFlowsLeftToRight) errors.push("self river order is reversed");
    if (!metrics.toimenRiverFlowsRightToLeft) errors.push("toimen river order is reversed");
    if (!metrics.shimochaRiverFlowsBottomToTop) errors.push("shimocha river order is reversed");
    if (!metrics.kamichaRiverFlowsTopToBottom) errors.push("kamicha river order is reversed");
    if (!metrics.sideHandsSideways) errors.push("side concealed hands are not rotated sideways");
    if (!metrics.sideHandSpacingOk) errors.push("side concealed hands are too tightly overlapped");
    if (!metrics.sideMeldsSideways) errors.push("side meld tiles are not rotated with side seats");
    if (!metrics.sideMeldStacksVertical) errors.push("side meld tiles are not stacked vertically");
    if (!metrics.sideCalledTilesHorizontal) errors.push("side called tiles should be horizontal within side meld stacks");
    if (!metrics.sideMeldFacesSideways) errors.push("side meld faces should be sideways");
    if (!metrics.sideMeldFacesFacingCorrect) errors.push("side meld faces should face their physical seats");
    if (!metrics.toimenVisibleFacesFacingCorrect) errors.push("toimen visible faces should face the top seat");
    if (metrics.calledTilePositionErrors.length > 0) {
      errors.push(`called tile position wrong: ${metrics.calledTilePositionErrors.join(",")}`);
    }
    if (metrics.sideMeldOverlaps.length > 0) errors.push(`side melds overlap table elements: ${metrics.sideMeldOverlaps.join(",")}`);
    if (metrics.redArrowClassCount > 0) errors.push("called tile red arrow marker classes should not exist");
    if (scenario.id === "06-meld-heavy" && metrics.meldPlacementErrors.length > 0) {
      errors.push(`meld placement wrong: ${metrics.meldPlacementErrors.join(",")}`);
    }
    if (scenario.id === "07-late-four-row-rivers" && (metrics.ankanBackTiles !== 2 || metrics.ankanFaceTiles !== 2)) {
      errors.push(`ankan should be 2 backs + 2 faces, got backs=${metrics.ankanBackTiles} faces=${metrics.ankanFaceTiles}`);
    }
    if (scenario.id === "12-extreme-names-and-melds" && metrics.kakanStacks === 0) {
      errors.push("kakan should render as an added tile on the original pon stack");
    }
    if (scenario.id === "12-extreme-names-and-melds" && !metrics.kakanCalledTilesKeepDirection) {
      errors.push("kakan should keep the original called tile sideways");
    }
    if (scenario.id === "12-extreme-names-and-melds" && metrics.kakanCalledAddedOverlapMax > 6) {
      errors.push(`kakan added tile covers original called tile: ${metrics.kakanCalledAddedOverlapMax}`);
    }
    if (scenario.id === "13-late-all-melds" && metrics.kakanStacks === 0) {
      errors.push("kakan should render as an added tile on the original pon stack");
    }
    if (scenario.id === "13-late-all-melds" && !metrics.kakanCalledTilesKeepDirection) {
      errors.push("kakan should keep the original called tile sideways");
    }
    if (scenario.id === "13-late-all-melds" && metrics.kakanCalledAddedOverlapMax > 6) {
      errors.push(`kakan added tile covers original called tile: ${metrics.kakanCalledAddedOverlapMax}`);
    }
    if (scenario.id === "08-hule-result" && !metrics.roundResultText.includes("自家 自摸")) {
      errors.push(`hule result banner missing winner: ${metrics.roundResultText}`);
    }
    if (scenario.id === "08-hule-result" && !metrics.roundResultText.includes("8000点")) {
      errors.push(`hule result banner missing points: ${metrics.roundResultText}`);
    }
    if (scenario.id === "08-hule-result" && (!metrics.roundResultText.includes("5番") || !metrics.roundResultText.includes("40符") || !metrics.roundResultText.includes("番种"))) {
      errors.push(`hule result banner missing fan/fu/yaku details: ${metrics.roundResultText}`);
    }
    if (scenario.id === "08-hule-result" && !metrics.roundResultText.includes("和牌 中")) {
      errors.push(`hule result banner leaked raw tile code: ${metrics.roundResultText}`);
    }
    if (scenario.id === "08-hule-result" && metrics.winningHandTiles < 13) {
      errors.push(`hule result missing winner hand tiles: ${metrics.winningHandTiles}`);
    }
    if (scenario.id === "08-hule-result" && metrics.winningMeldTiles < 3) {
      errors.push(`hule result missing winner meld tiles: ${metrics.winningMeldTiles}`);
    }
    if (scenario.id === "09-ron-result" && !metrics.roundResultText.includes("荣和")) {
      errors.push(`ron result banner missing ron text: ${metrics.roundResultText}`);
    }
    if (scenario.id === "09-ron-result" && !metrics.roundResultText.includes("自家点炮")) {
      errors.push(`ron result banner missing deal-in player: ${metrics.roundResultText}`);
    }
    if (scenario.id === "09-ron-result" && !metrics.roundResultText.includes("12000点")) {
      errors.push(`ron result banner missing points: ${metrics.roundResultText}`);
    }
    if (scenario.id === "09-ron-result" && (!metrics.roundResultText.includes("6番") || !metrics.roundResultText.includes("30符"))) {
      errors.push(`ron result banner missing ron fan/fu details: ${metrics.roundResultText}`);
    }
    if (scenario.id === "09-ron-result" && metrics.winningHandTiles < 14) {
      errors.push(`ron result missing winner hand tiles: ${metrics.winningHandTiles}`);
    }
    if (scenario.id === "10-no-tile-result" && !metrics.roundResultText.includes("荒牌流局")) {
      errors.push(`no-tile result banner missing: ${metrics.roundResultText}`);
    }
    if (scenario.id === "11-liu-ju-result" && !metrics.roundResultText.includes("途中流局")) {
      errors.push(`liu-ju result banner missing: ${metrics.roundResultText}`);
    }
    if (scenario.id.includes("riichi") && metrics.riichiSlots === 0) errors.push("riichi scenario has no riichi river slot");
    if (metrics.modelRows > 3) errors.push("too many model candidate rows");
    if (metrics.seatWindTags !== 4) errors.push(`expected 4 seat wind tags, got ${metrics.seatWindTags}`);
    if (!metrics.bodyText.includes("宝牌指示牌")) errors.push("dora indicator label is missing");
    if (!metrics.shimochaTsumoRightSide) errors.push("shimocha tsumo tile is not on the right side");
  } else if (metrics.tableTileCount > 0) {
    errors.push("empty state rendered table tiles");
  }
  if (scenario.account && !metrics.bodyText.includes(scenario.account.nickname ?? scenario.account.username)) {
    errors.push("account name missing");
  }
  if (scenario.table && metrics.playerLines !== 4) errors.push(`expected 4 player lines, got ${metrics.playerLines}`);
  if (metrics.clippedPlayerRows?.length > 0) errors.push(`player rows clipped: ${metrics.clippedPlayerRows.join(",")}`);
  if (metrics.playersBlock && metrics.playersBlock.scrollWidth > metrics.playersBlock.clientWidth + 1) errors.push("players block horizontal overflow");
  if (metrics.recordsBlock && metrics.recordsBlock.scrollWidth > metrics.recordsBlock.clientWidth + 1) errors.push("records block horizontal overflow");
  if (metrics.bodyText.includes("完成局数") || metrics.bodyText.includes("ACK") || metrics.bodyText.includes("等待动作")) {
    errors.push("removed status wording returned");
  }
  if (metrics.bodyText.includes("discard refused without discard operation window")) {
    errors.push("internal discard-refused log leaked");
  }
  if (errors.length > 0) {
    throw new Error(`${scenario.id}: ${errors.join("; ")}\n${JSON.stringify(metrics, null, 2)}`);
  }
}

try {
  let ready = false;
  for (let i = 0; i < 80; i += 1) {
    try {
      const response = await fetch(url);
      if (response.ok) {
        ready = true;
        break;
      }
    } catch {
      await wait(250);
    }
  }
  if (!ready) throw new Error("Vite dev server did not become ready");

  const browser = await chromium.launch();
  try {
    const results = [];
    for (const scenario of scenarios.filter((item) => !scenarioFilter || item.id.includes(scenarioFilter))) {
      for (const viewport of viewports) {
        const page = await browser.newPage({ viewport: { width: viewport.width, height: viewport.height }, deviceScaleFactor: 1 });
        const consoleErrors = [];
        page.on("console", (message) => {
          if (message.type() === "error") consoleErrors.push(message.text());
        });
        await installTauriMock(page, scenario);
        await page.goto(url, { waitUntil: "networkidle" });
        await page.waitForSelector(".appShell");
        await page.waitForTimeout(900);
        const screenshot =
          viewport.id === "desktop"
            ? `${screenshotDir}/${scenario.id}.png`
            : `${screenshotDir}/${viewport.id}-${scenario.id}.png`;
        await page.screenshot({ path: screenshot, fullPage: true });
        const metrics = await page.evaluate(() => {
        const rect = (selector) => {
          const el = document.querySelector(selector);
          if (!el) return null;
          const item = el.getBoundingClientRect();
          return {
            left: Math.round(item.left),
            top: Math.round(item.top),
            right: Math.round(item.right),
            bottom: Math.round(item.bottom),
            width: Math.round(item.width),
            height: Math.round(item.height),
            scrollWidth: el.scrollWidth,
            clientWidth: el.clientWidth,
            scrollHeight: el.scrollHeight,
            clientHeight: el.clientHeight,
          };
        };
        const tableArea = document.querySelector(".tableArea");
        const tableRect = tableArea?.getBoundingClientRect();
        const tableTiles = Array.from(document.querySelectorAll(".tableArea .tile")).map((node) => {
          const item = node.getBoundingClientRect();
          return { title: node.getAttribute("title"), left: item.left, right: item.right, top: item.top, bottom: item.bottom };
        });
        const center = document.querySelector(".tableCenterAbsolute")?.getBoundingClientRect();
        const riverSelectors = [".seatLayer-self", ".seatLayer-toimen", ".seatLayer-kamicha", ".seatLayer-shimocha"];
        const riverCenterOverlaps = center
          ? riverSelectors.filter((selector) => {
              const slots = Array.from(document.querySelectorAll(`${selector} .localRiverSlot`));
              return slots.some((slot) => {
                const item = slot.getBoundingClientRect();
                return !(item.right < center.left || item.left > center.right || item.bottom < center.top || item.top > center.bottom);
              });
            })
          : [];
        return {
          appShell: rect(".appShell"),
          tableArea: rect(".tableArea"),
          playersBlock: rect(".playersBlock"),
          recordsBlock: rect(".recordsBlock"),
          handContainer: rect(".seatLayer-self .localHand"),
          handMelds: rect(".seatLayer-self .localMelds"),
          playerLines: document.querySelectorAll(".playerLine").length,
          modelRows: document.querySelectorAll(".candidateRow").length,
          clippedPlayerRows: (() => {
            const block = document.querySelector(".playersBlock")?.getBoundingClientRect();
            if (!block) return [];
            return Array.from(document.querySelectorAll(".playersBlock .playerLine"))
              .map((line, index) => ({ index, rect: line.getBoundingClientRect() }))
              .filter(({ rect }) => rect.top < block.top || rect.bottom > block.bottom)
              .map(({ index }) => index);
          })(),
          overflowX: document.documentElement.scrollWidth > window.innerWidth,
          overflowY: document.documentElement.scrollHeight > window.innerHeight,
          outsideTableTiles:
            tableRect == null
              ? []
              : tableTiles.filter((tile) => tile.left < tableRect.left || tile.right > tableRect.right || tile.top < tableRect.top || tile.bottom > tableRect.bottom),
          riverCenterOverlaps,
          tableTileCount: document.querySelectorAll(".tableArea .tile").length,
          riichiSlots: document.querySelectorAll(".localRiverRiichiSlot").length,
          riverCrossOverlaps: (() => {
            const slots = Array.from(document.querySelectorAll(".localRiverSlot")).map((slot, index) => {
              const item = slot.getBoundingClientRect();
              const pile =
                slot.closest(".seatLayer-self")?.className ||
                slot.closest(".seatLayer-toimen")?.className ||
                slot.closest(".seatLayer-kamicha")?.className ||
                slot.closest(".seatLayer-shimocha")?.className ||
                "";
              return { index, pile, left: item.left, right: item.right, top: item.top, bottom: item.bottom };
            });
            const overlaps = [];
            for (let i = 0; i < slots.length; i += 1) {
              for (let j = i + 1; j < slots.length; j += 1) {
                if (slots[i].pile === slots[j].pile) continue;
                const a = slots[i];
                const b = slots[j];
                const separated = a.right < b.left || b.right < a.left || a.bottom < b.top || b.bottom < a.top;
                if (!separated) overlaps.push(`${a.pile}#${a.index}-${b.pile}#${b.index}`);
              }
            }
            return overlaps;
          })(),
          seatRiverOverlaps: (() => {
            const checks = [
              [".seatLayer-kamicha .localHandTiles", ".seatLayer-kamicha .localRiverSlot"],
              [".seatLayer-shimocha .localHandTiles", ".seatLayer-shimocha .localRiverSlot"],
              [".seatLayer-toimen .localHandTiles", ".seatLayer-toimen .localRiverSlot"],
            ];
            const overlaps = [];
            for (const [seatSelector, slotSelector] of checks) {
              const seat = document.querySelector(seatSelector)?.getBoundingClientRect();
              if (!seat) continue;
              const hit = Array.from(document.querySelectorAll(slotSelector)).some((slot) => {
                const item = slot.getBoundingClientRect();
                return !(item.right < seat.left || item.left > seat.right || item.bottom < seat.top || item.top > seat.bottom);
              });
              if (hit) overlaps.push(`${seatSelector}-${slotSelector}`);
            }
            return overlaps;
          })(),
          handMeldOverflow: (() => {
            const hand = document.querySelector(".seatLayer-self .localHand")?.getBoundingClientRect();
            const melds = document.querySelector(".seatLayer-self .localMelds")?.getBoundingClientRect();
            return Boolean(hand && melds && (melds.left < hand.left || melds.right > hand.right || melds.top < hand.top || melds.bottom > hand.bottom));
          })(),
          shimochaRiverFlowsBottomToTop: (() => {
            const slots = Array.from(document.querySelectorAll(".seatLayer-shimocha .localRiverSlot"));
            if (slots.length < 2) return true;
            const first = slots[0].getBoundingClientRect();
            const second = slots[1].getBoundingClientRect();
            return first.top > second.top;
          })(),
          kamichaRiverFlowsTopToBottom: (() => {
            const slots = Array.from(document.querySelectorAll(".seatLayer-kamicha .localRiverSlot"));
            if (slots.length < 2) return true;
            const first = slots[0].getBoundingClientRect();
            const second = slots[1].getBoundingClientRect();
            return first.top < second.top;
          })(),
          selfRiverFlowsLeftToRight: (() => {
            const slots = Array.from(document.querySelectorAll(".seatLayer-self .localRiverSlot"));
            if (slots.length < 2) return true;
            const first = slots[0].getBoundingClientRect();
            const second = slots[1].getBoundingClientRect();
            return first.left < second.left;
          })(),
          toimenRiverFlowsRightToLeft: (() => {
            const slots = Array.from(document.querySelectorAll(".seatLayer-toimen .localRiverSlot"));
            if (slots.length < 2) return true;
            const first = slots[0].getBoundingClientRect();
            const second = slots[1].getBoundingClientRect();
            return first.left > second.left;
          })(),
          sideHandsSideways: (() => {
            const tiles = Array.from(document.querySelectorAll(".seatLayer-kamicha .localHandTiles .tileBack, .seatLayer-shimocha .localHandTiles .tileBack"));
            return tiles.length === 0 || tiles.every((tile) => {
              const rect = tile.getBoundingClientRect();
              return rect.width > rect.height;
            });
          })(),
          sideHandSpacingOk: (() => {
            const groups = [".seatLayer-kamicha .localHandTiles .tileBack", ".seatLayer-shimocha .localHandTiles .tileBack"];
            return groups.every((selector) => {
              const tiles = Array.from(document.querySelectorAll(selector)).map((tile) => tile.getBoundingClientRect());
              if (tiles.length < 2) return true;
              return tiles.slice(1).every((rect, index) => {
                const previous = tiles[index];
                const delta = Math.abs(rect.top - previous.top);
                const minHeight = Math.min(rect.height, previous.height);
                return delta >= minHeight - 1;
              });
            });
          })(),
          sideHandDeltas: (() => {
            const groups = [".seatLayer-kamicha .localHandTiles .tileBack", ".seatLayer-shimocha .localHandTiles .tileBack"];
            return groups.map((selector) => {
              const tiles = Array.from(document.querySelectorAll(selector)).map((tile) => tile.getBoundingClientRect());
              return tiles.slice(1, 5).map((rect, index) => Math.round(Math.abs(rect.top - tiles[index].top)));
            });
          })(),
          sideMeldsSideways: (() => {
            const tiles = Array.from(document.querySelectorAll(".seatLayer-kamicha .localMelds .tile, .seatLayer-shimocha .localMelds .tile"));
            return tiles.length === 0 || tiles.every((tile) => {
              const rect = tile.getBoundingClientRect();
              return rect.width > rect.height;
            });
          })(),
          sideMeldStacksVertical: (() => {
            const groups = Array.from(document.querySelectorAll(".seatLayer-kamicha .localMelds .meld, .seatLayer-shimocha .localMelds .meld"));
            return groups.every((group) => {
              const children = Array.from(group.children);
              if (children.length < 2) return true;
              const centers = children.map((child) => {
                const rect = child.getBoundingClientRect();
                return {
                  x: rect.left + rect.width / 2,
                  y: rect.top + rect.height / 2,
                };
              });
              const xSpread = Math.max(...centers.map((item) => item.x)) - Math.min(...centers.map((item) => item.x));
              const ySpread = Math.max(...centers.map((item) => item.y)) - Math.min(...centers.map((item) => item.y));
              return ySpread > xSpread + 12 && xSpread <= 18;
            });
          })(),
          sideCalledTilesHorizontal: (() => {
            const called = Array.from(
              document.querySelectorAll(".seatLayer-kamicha .localMelds .calledTileWrapper, .seatLayer-kamicha .localMelds .kakanCalledStack, .seatLayer-shimocha .localMelds .calledTileWrapper, .seatLayer-shimocha .localMelds .kakanCalledStack"),
            );
            return called.length === 0 || called.every((node) => node.getAttribute("data-called-relative") != null);
          })(),
          sideMeldFacesSideways: (() => {
            const layers = [document.querySelector(".seatLayer-kamicha"), document.querySelector(".seatLayer-shimocha")].filter(Boolean);
            return layers.every((layer) => getComputedStyle(layer).transform !== "none");
          })(),
          sideMeldFacesFacingCorrect: (() => {
            const rotations = {
              kamicha: document.querySelector(".seatLayer-kamicha")?.getAttribute("style") ?? "",
              shimocha: document.querySelector(".seatLayer-shimocha")?.getAttribute("style") ?? "",
            };
            return rotations.kamicha.includes("90deg") && rotations.shimocha.includes("270deg");
          })(),
          toimenVisibleFacesFacingCorrect: (() => {
            return (document.querySelector(".seatLayer-toimen")?.getAttribute("style") ?? "").includes("180deg");
          })(),
          calledTilePositionErrors: (() => {
            const expectedIndex = (relative, length) => {
              const middle = Math.floor((length - 1) / 2);
              if (relative === 3) return 0;
              if (relative === 1) return length - 1;
              return middle;
            };
            const errors = [];
            for (const meld of document.querySelectorAll(".meldFrom1, .meldFrom2, .meldFrom3")) {
              const children = Array.from(meld.children);
              const calledIndex = children.findIndex((child) => child.classList.contains("calledTileWrapper") || child.classList.contains("kakanCalledStack") || child.matches(".tile[data-called-relative]"));
              if (calledIndex < 0) continue;
              const relative = Number.parseInt(meld.getAttribute("data-called-relative") ?? "-1", 10);
              const expected = expectedIndex(relative, children.length);
              if (calledIndex !== expected) errors.push(`${relative}:${calledIndex}->${expected}`);
            }
            return errors;
          })(),
          sideMeldOverlaps: (() => {
            const subjects = Array.from(
              document.querySelectorAll(".seatLayer-kamicha .localMelds .tile, .seatLayer-kamicha .localMelds .calledTileWrapper, .seatLayer-shimocha .localMelds .tile, .seatLayer-shimocha .localMelds .calledTileWrapper"),
            ).map((node, index) => ({ index, rect: node.getBoundingClientRect() }));
            const targets = Array.from(
              document.querySelectorAll(".localRiverSlot, .seatLayer-kamicha .localHandTiles .tile, .seatLayer-shimocha .localHandTiles .tile"),
            ).map((node, index) => ({ index, className: node.className, rect: node.getBoundingClientRect() }));
            const overlaps = [];
            for (const subject of subjects) {
              for (const target of targets) {
                const a = subject.rect;
                const b = target.rect;
                const separated = a.right <= b.left + 2 || b.right <= a.left + 2 || a.bottom <= b.top + 2 || b.bottom <= a.top + 2;
                if (!separated) overlaps.push(`${subject.index}-${target.index}`);
              }
            }
            return overlaps.slice(0, 10);
          })(),
          redArrowClassCount: document.querySelectorAll(".tileCalled, [class*='tileCalledFrom']").length,
          roundResultText: document.querySelector(".roundResultBanner")?.textContent ?? "",
          winningHandTiles: document.querySelectorAll(".winningHandReveal .tile").length,
          winningMeldTiles: document.querySelectorAll(".winningMeldReveal .tile").length,
          seatWindTags: document.querySelectorAll(".seatWindTag").length,
          shimochaTsumoRightSide: (() => {
            const tsumo = document.querySelector(".seatLayer-shimocha .localTsumo")?.getBoundingClientRect();
            const hand = document.querySelector(".seatLayer-shimocha .localHandTiles")?.getBoundingClientRect();
            if (!tsumo || !hand) return true;
            return tsumo.top >= hand.bottom - 1;
          })(),
          kakanStacks: document.querySelectorAll(".kakanCalledStack .kakanAddedTile").length,
          kakanCalledTilesKeepDirection: (() => {
            const stacks = Array.from(document.querySelectorAll(".kakanCalledStack"));
            return stacks.length === 0 || stacks.every((stack) => {
              const called = stack.querySelector(":scope > .kakanBaseTile .tile");
              if (!called) return false;
              const rect = called.getBoundingClientRect();
              return rect.width > rect.height;
            });
          })(),
          kakanCalledAddedOverlapMax: (() => {
            const stacks = Array.from(document.querySelectorAll(".kakanCalledStack"));
            let maxOverlap = 0;
            for (const stack of stacks) {
              const called = stack.querySelector(":scope > .kakanBaseTile .tile");
              const added = stack.querySelector(":scope > .kakanAddedTile");
              if (!called || !added) continue;
              const a = called.getBoundingClientRect();
              const b = added.getBoundingClientRect();
              const overlapX = Math.max(0, Math.min(a.right, b.right) - Math.max(a.left, b.left));
              const overlapY = Math.max(0, Math.min(a.bottom, b.bottom) - Math.max(a.top, b.top));
              maxOverlap = Math.max(maxOverlap, Math.round(Math.min(overlapX, overlapY)));
            }
            return maxOverlap;
          })(),
          ankanBackTiles: document.querySelectorAll(".meldAnkan .tileBack").length,
          ankanFaceTiles: document.querySelectorAll(".meldAnkan .tile:not(.tileBack)").length,
          calledTileDirections: [1, 2, 3].map((relative) => ({
            relative,
            count: document.querySelectorAll(`[data-called-relative="${relative}"]`).length,
          })),
          meldPlacementErrors: (() => {
            const errors = [];
            const check = (name, handSelector, meldSelector, predicate) => {
              const hand = document.querySelector(handSelector)?.getBoundingClientRect();
              const meld = document.querySelector(meldSelector)?.getBoundingClientRect();
              if (!hand || !meld) return;
              if (!predicate(hand, meld)) errors.push(name);
            };
            check(
              "toimen meld should be on screen-left hand side",
              ".seatLayer-toimen .localHandTiles",
              ".seatLayer-toimen .localMelds",
              (hand, meld) => meld.right <= hand.left + 1,
            );
            check(
              "kamicha meld should be below hand",
              ".seatLayer-kamicha .localHandTiles",
              ".seatLayer-kamicha .localMelds",
              (hand, meld) => meld.top >= hand.bottom - 1,
            );
            check(
              "shimocha meld should be above hand",
              ".seatLayer-shimocha .localHandTiles",
              ".seatLayer-shimocha .localMelds",
              (hand, meld) => meld.bottom <= hand.top + 1,
            );
            return errors;
          })(),
          bodyText: document.body.innerText,
        };
      });
        if (consoleErrors.length > 0) throw new Error(`${viewport.id}/${scenario.id}: console errors: ${consoleErrors.join(" | ")}`);
        assertScenario(scenario, metrics);
        results.push({
          id: scenario.id,
          viewport: viewport.id,
          screenshot,
          playerLines: metrics.playerLines,
          modelRows: metrics.modelRows,
        });
        await page.close();
      }
    }
    console.log(JSON.stringify({ screenshotDir, results }, null, 2));
  } finally {
    await browser.close();
  }
} finally {
  server.kill("SIGTERM");
}
