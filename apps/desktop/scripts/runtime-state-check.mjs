import { spawn } from "node:child_process";
import { chromium } from "playwright";

const port = 1422;
const url = `http://127.0.0.1:${port}`;
const server = spawn(
  process.platform === "win32" ? "npm.cmd" : "npm",
  ["run", "dev", "--", "--host", "127.0.0.1", "--port", String(port)],
  { stdio: ["ignore", "pipe", "pipe"] },
);

const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

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
  if (!ready) {
    throw new Error("Vite dev server did not become ready");
  }

  const browser = await chromium.launch();
  const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
  await page.addInitScript(() => {
    const callbacks = {};
    let nextCallbackId = 1;
    let runtimeRunning = false;
    const settings = {
      model_path: "models/mortal",
      ui_language: "zh",
      autoplay_account: {
        username: "test@example.com",
        password: "password",
      },
      autoplay: {
        room_policy: { type: "manual" },
        manual_room: "bronze",
        manual_mode: "four_player_east",
        action_interval_ms: { min: 800, max: 1600 },
        max_games: null,
      },
    };

    window.__invokeCalls = [];
    const coreEvents = [
      {
        seq: 1,
        event: {
          type: "account_snapshot",
          account: {
            refreshing: false,
            username: "test@example.com",
            account_id: 123456,
            nickname: "FrontendOk",
            level_id: 10301,
            level_score: 542,
            rank_tier: 3,
            target_mode: "four_player_south",
            target_room: "gold",
          },
        },
      },
      {
        seq: 2,
        event: {
          type: "table_snapshot",
          table: {
            seat: 3,
            bakaze: "E",
            kyoku: 2,
            honba: 0,
            kyotaku: 0,
            oya: 1,
            dora_markers: ["3m"],
            scores: [25000, 19800, 25000, 30200],
            players: [
              { seat: 0, points: 25000, hand: Array(13).fill("?"), hand_count: 13, discards: [], melds: [], riichi: false, is_self: false },
              { seat: 1, points: 19800, hand: Array(13).fill("?"), hand_count: 13, discards: [{ tile: "S", tsumogiri: false, riichi: false }], melds: [], riichi: false, is_self: false },
              { seat: 2, points: 25000, hand: Array(13).fill("?"), hand_count: 13, discards: [], melds: [], riichi: true, is_self: false },
              { seat: 3, points: 30200, hand: ["1m", "2m", "3m"], hand_count: 13, discards: [{ tile: "N", tsumogiri: false, riichi: false }], melds: [], riichi: false, is_self: true },
            ],
            last_event: { type: "dahai", actor: 3, pai: "N", tsumogiri: false },
          },
        },
      },
      {
        seq: 3,
        event: {
          type: "model_decision",
          decision: {
            action_index: 30,
            action_label: "discard N",
            confidence: 0.42,
            is_greedy: true,
            candidates: [
              { action_index: 30, action_label: "discard N", q_value: 0.1, confidence: 0.42, legal: true },
            ],
          },
        },
      },
      { seq: 4, event: { type: "action_ack", ack: { ok: true, message: "discard broadcast ack matched N" } } },
      { seq: 5, event: { type: "game_event", event: { type: "dahai", actor: 3, pai: "N", tsumogiri: false } } },
      { seq: 6, event: { type: "log", level: "info", message: "frontend batch sync ok" } },
    ];
    let runtimeStatus = "idle";
    let lastError = null;
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
            running: runtimeRunning,
            status: runtimeStatus,
            last_error: lastError,
            settings_path: "settings.json",
            runtime_log_path: "logs/runtime/gui_autoplay.log",
          };
        }
        if (command === "get_core_event_batch") {
          if (!runtimeRunning) return { cursor: args.after ?? 0, events: [] };
          const after = args.after ?? 0;
          const events = coreEvents.filter((record) => record.seq > after);
          return {
            cursor: events.at(-1)?.seq ?? after,
            events,
          };
        }
        if (command === "start_autoplay") {
          runtimeRunning = true;
          runtimeStatus = "logging_in";
          return null;
        }
        if (command === "stop_after_current_game") {
          runtimeStatus = "stopping_after_game";
          return null;
        }
        if (command === "emergency_stop") {
          runtimeRunning = false;
          runtimeStatus = "stopped";
          return null;
        }
        if (command === "plugin:event|listen") return 1;
        if (command === "plugin:event|unlisten") return null;
        throw new Error(`unhandled invoke: ${command}`);
      },
      convertFileSrc(path) {
        return path;
      },
    };
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = {
      unregisterListener() {},
    };
  });

  await page.goto(url, { waitUntil: "networkidle" });
  const initialText = await page.locator("body").innerText();
  if (initialText.includes("纯 Rust") || initialText.includes("Pure Rust") || initialText.includes("Rust 製")) {
    throw new Error("runtime implementation wording leaked into user-facing subtitle");
  }
  const startButton = page.getByRole("button", { name: /启动/ });
  await startButton.waitFor();
  if (await startButton.isDisabled()) {
    throw new Error("start button should be enabled before runtime starts");
  }
  await startButton.click();
  await page.waitForTimeout(150);
  if (!(await startButton.isDisabled())) {
    throw new Error("start button stayed enabled after start_autoplay returned");
  }
  await page.waitForSelector('[data-testid="account-snapshot"]');
  const accountText = await page.locator('[data-testid="account-snapshot"]').innerText();
  if (
    !(
      (accountText.includes("test@example.com") && accountText.includes("刷新中")) ||
      (accountText.includes("FrontendOk") && accountText.includes("雀杰一星 542分"))
    )
  ) {
    throw new Error(`account snapshot was not refreshed immediately: ${accountText}`);
  }
  const phaseText = await page.locator('[data-testid="phase-banner"]').innerText();
  if (!phaseText.includes("登录中")) {
    throw new Error(`phase banner did not show runtime progress: ${phaseText}`);
  }
  const bodyText = await page.locator("body").innerText();
  if (!bodyText.includes("状态: 登录中")) {
    throw new Error("runtime status transition was not logged");
  }
  const modelImportText = await page.locator(".modelImportCard").innerText();
  if (
    !modelImportText.includes("导入 safetensors") ||
    !modelImportText.includes("仅支持 .safetensors") ||
    modelImportText.includes("导入模型目录") ||
    modelImportText.includes("导入模型文件") ||
    (await page.locator(".modelImportCard input").count()) !== 0 ||
    (await page.locator(".modelImportCard select").count()) !== 1 ||
    (await page.locator(".modelImportCard button").count()) !== 1
  ) {
    throw new Error(`model panel should use one dropdown plus one file import button: ${modelImportText}`);
  }
  await page.waitForTimeout(900);
  const syncedText = await page.locator("body").innerText();
  for (const expected of [
    "FrontendOk",
    "雀杰一星 542分",
    "金之间 四人南",
    "东二局",
    "0本场",
    "0供托",
    "30200",
    "打出 北",
    "frontend batch sync ok",
    "\"dahai\"",
  ]) {
    if (!syncedText.includes(expected)) {
      throw new Error(`core event batch did not update visible UI with ${expected}: ${syncedText}`);
    }
  }
  const candidateText = await page.locator(".candidateTrack").innerText();
  const candidateLines = candidateText.split(/\n/).filter((line) => line.trim().length > 0);
  if (candidateLines.length > 6) {
    throw new Error(`candidate list rendered too many rows: ${candidateText}`);
  }
  await page.getByRole("button", { name: "English" }).click();
  await page.waitForTimeout(250);
  const englishText = await page.locator("body").innerText();
  for (const expected of ["FrontendOk", "Expert 1 · 542 pts", "Gold 4-player South", "East 2", "Discard North"]) {
    if (!englishText.includes(expected)) {
      throw new Error(`English UI did not localize ${expected}: ${englishText}`);
    }
  }
  await page.getByRole("button", { name: "中文" }).click();
  await page.waitForTimeout(150);
  const stopButton = page.getByRole("button", { name: /本局后停止/ });
  await stopButton.click();
  await page.waitForTimeout(150);
  const stopScheduledButton = page.getByRole("button", { name: /已安排本局后停止/ });
  await stopScheduledButton.waitFor();
  if (!(await stopScheduledButton.isDisabled())) {
    throw new Error("stop-after-current-game button did not become disabled after scheduling");
  }
  await page.getByRole("button", { name: /紧急停止/ }).click();
  await page.waitForTimeout(150);
  const emergencyDialog = page.getByRole("dialog");
  await emergencyDialog.waitFor();
  const emergencyDialogText = await emergencyDialog.innerText();
  if (!emergencyDialogText.includes("确定要立刻强制停止自动打牌吗")) {
    throw new Error(`emergency stop confirmation did not appear: ${emergencyDialogText}`);
  }
  let emergencyCalls = await page.evaluate(
    () => window.__invokeCalls.filter((call) => call.command === "emergency_stop").length,
  );
  if (emergencyCalls !== 0) {
    throw new Error("emergency_stop was invoked before confirmation");
  }
  await page.getByRole("button", { name: "取消" }).click();
  await page.waitForTimeout(150);
  if (await emergencyDialog.isVisible()) {
    throw new Error("emergency stop confirmation did not close after cancel");
  }
  await page.getByRole("button", { name: /紧急停止/ }).click();
  await page.getByRole("button", { name: "确认停止" }).click();
  await page.waitForTimeout(150);
  emergencyCalls = await page.evaluate(
    () => window.__invokeCalls.filter((call) => call.command === "emergency_stop").length,
  );
  if (emergencyCalls !== 1) {
    throw new Error(`expected one emergency_stop call after confirmation, got ${emergencyCalls}`);
  }
  await page.evaluate(() => {
    window.__TAURI_INTERNALS__.invoke = async (command, args = {}) => {
      window.__invokeCalls.push({ command, args });
      if (command === "load_settings") {
        return {
          model_path: "models/mortal",
          ui_language: "zh",
          autoplay_account: { username: "test@example.com", password: "password" },
          autoplay: {
            room_policy: { type: "manual" },
            manual_room: "bronze",
            manual_mode: "four_player_east",
            action_interval_ms: { min: 800, max: 1600 },
            max_games: null,
          },
        };
      }
      if (command === "save_settings") return null;
      if (command === "get_runtime_snapshot") {
        return {
          running: false,
          status: "error",
          last_error: "login timed out after 20s",
          settings_path: "settings.json",
          runtime_log_path: "logs/runtime/gui_autoplay.log",
        };
      }
      if (command === "get_core_event_batch") {
        return { cursor: args.after ?? 0, events: [] };
      }
      if (command === "plugin:event|listen") return 1;
      if (command === "plugin:event|unlisten") return null;
      return null;
    };
  });
  await page.waitForTimeout(1200);
  const errorText = await page.locator("body").innerText();
  if (!errorText.includes("异常") || !errorText.includes("login timed out after 20s")) {
    throw new Error("runtime snapshot error did not replace stale logging-in state");
  }
  await page.waitForTimeout(1200);
  const repeatedErrorText = await page.locator("body").innerText();
  const errorCount = (repeatedErrorText.match(/login timed out after 20s/g) ?? []).length;
  if (errorCount !== 1) {
    throw new Error(`runtime snapshot error was logged ${errorCount} times`);
  }
  const startCalls = await page.evaluate(
    () => window.__invokeCalls.filter((call) => call.command === "start_autoplay").length,
  );
  if (startCalls !== 1) {
    throw new Error(`expected exactly one start_autoplay call, got ${startCalls}`);
  }

  await browser.close();
  console.log("runtime state check passed");
} finally {
  server.kill("SIGTERM");
}
