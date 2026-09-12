import type { Language, ModeChoice, RoomChoice, RuntimeStatus } from "./types";

export const languageNames: Record<Language, string> = {
  zh: "中文",
  en: "English",
  ja: "日本語",
};

type Copy = {
  appTitle: string;
  subtitle: string;
  launch: string;
  stopAfterGame: string;
  stopScheduled: string;
  emergencyStop: string;
  emergencyConfirm: string;
  emergencyCancel: string;
  emergencyConfirmAction: string;
  save: string;
  account: string;
  username: string;
  password: string;
  accountInfo: string;
  accountRefreshing: string;
  accountWaiting: string;
  accountId: string;
  rank: string;
  target: string;
  model: string;
  modelSelect: string;
  modelBundled: string;
  modelImported: string;
  modelImport: string;
  modelImporting: string;
  modelImportOk: string;
  modelImportFailed: string;
  modelImportHelp: string;
  previewImport: string;
  match: string;
  autoHighest: string;
  manualRoom: string;
  mode: string;
  tempo: string;
  minMs: string;
  maxMs: string;
  maxGames: string;
  maxGamesPlaceholder: string;
  current: string;
  roundEast: string;
  roundSouth: string;
  roundWest: string;
  roundNorth: string;
  roundUnit: string;
  noTable: string;
  players: string;
  self: string;
  riichi: string;
  tsumoWin: string;
  ronWin: string;
  dealIn: string;
  winningHand: string;
  exhaustiveDraw: string;
  abortiveDraw: string;
  winTile: string;
  points: string;
  han: string;
  fu: string;
  yaku: string;
  allPlayersPay: string;
  winningMelds: string;
  eventStream: string;
  logs: string;
  gameRecords: string;
  copyPaipu: string;
  copied: string;
  openInMortal: string;
  noGameRecords: string;
  refreshRecords: string;
  refreshingRecords: string;
  modelDecision: string;
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
  relativeConfidence: string;
  qValue: string;
  legal: string;
  invalid: string;
  topPlayer: string;
  leftPlayer: string;
  rightPlayer: string;
  dora: string;
  honba: string;
  deposit: string;
  saveOk: string;
  previewSave: string;
  readFailed: string;
  saveFailed: string;
  startFailed: string;
  stopFailed: string;
  emergencyFailed: string;
  settings: string;
  roomPolicy: string;
  statusLabel: string;
  status: Record<RuntimeStatus, string>;
  rooms: Record<RoomChoice, string>;
  modes: Record<ModeChoice, string>;
};

export const copy: Record<Language, Copy> = {
  zh: {
    appTitle: "Majsoul Autopilot",
    subtitle: "自动对局控制台",
    launch: "启动",
    stopAfterGame: "本局后停止",
    stopScheduled: "已安排本局后停止",
    emergencyStop: "紧急停止",
    emergencyConfirm: "确定要立刻强制停止自动打牌吗？这会断开当前运行。",
    emergencyCancel: "取消",
    emergencyConfirmAction: "确认停止",
    save: "保存设置",
    account: "账号",
    username: "邮箱 / 用户名",
    password: "密码",
    accountInfo: "当前账号",
    accountRefreshing: "刷新中",
    accountWaiting: "等待登录",
    accountId: "账号 ID",
    rank: "段位",
    target: "目标",
    model: "模型",
    modelSelect: "选择模型",
    modelBundled: "内置模型",
    modelImported: "已导入模型",
    modelImport: "导入 safetensors",
    modelImporting: "正在导入模型",
    modelImportOk: "模型已导入",
    modelImportFailed: "模型导入失败",
    modelImportHelp:
      "只支持已导出的 .safetensors 模型目录：程序用 Rust/Candle 直接加载权重和 model_config.json；.pth 是 PyTorch 训练检查点，格式依赖 Python/PyTorch，不能在当前运行时直接读取。",
    previewImport: "Web 预览模式：跳过模型导入",
    match: "房间",
    autoHighest: "自动选择当前段位最高房间",
    manualRoom: "手动房间",
    mode: "场次",
    tempo: "打牌节奏",
    minMs: "最小 ms",
    maxMs: "最大 ms",
    maxGames: "连续局数",
    maxGamesPlaceholder: "空 = 一直打",
    current: "当前对局",
    roundEast: "东",
    roundSouth: "南",
    roundWest: "西",
    roundNorth: "北",
    roundUnit: "局",
    noTable: "暂无牌局",
    players: "玩家",
    self: "自家",
    riichi: "立直",
    tsumoWin: "自摸",
    ronWin: "荣和",
    dealIn: "点炮",
    winningHand: "胡牌手牌",
    exhaustiveDraw: "荒牌流局",
    abortiveDraw: "途中流局",
    winTile: "和牌",
    points: "点",
    han: "番",
    fu: "符",
    yaku: "番种",
    allPlayersPay: "每家",
    winningMelds: "副露",
    eventStream: "结构化事件",
    logs: "日志",
    gameRecords: "近期牌谱",
    copyPaipu: "复制牌谱",
    copied: "已复制",
    openInMortal: "Mortal 跑谱",
    noGameRecords: "暂无牌谱记录",
    refreshRecords: "刷新牌谱",
    refreshingRecords: "正在拉取...",
    modelDecision: "模型推荐",
    modelDiscard: "打出",
    modelReach: "立直",
    modelChiLow: "吃牌",
    modelChiMid: "吃牌",
    modelChiHigh: "吃牌",
    modelPon: "碰",
    modelKan: "杠",
    modelHora: "和牌",
    modelRyukyoku: "九种九牌",
    modelNone: "跳过",
    relativeConfidence: "相对置信度",
    qValue: "Q 值",
    legal: "可用",
    invalid: "不可用",
    topPlayer: "对家",
    leftPlayer: "上家",
    rightPlayer: "下家",
    dora: "宝牌指示牌",
    honba: "本场",
    deposit: "供托",
    saveOk: "设置已保存",
    previewSave: "Web 预览模式：跳过保存设置",
    readFailed: "读取 settings.json 失败",
    saveFailed: "保存设置失败",
    startFailed: "启动失败",
    stopFailed: "停止失败",
    emergencyFailed: "紧急停止失败",
    settings: "设置",
    roomPolicy: "房间策略",
    statusLabel: "运行阶段",
    status: {
      idle: "未启动",
      logging_in: "登录中",
      matching: "匹配中",
      reconnecting: "重连中",
      in_game: "对局中",
      stopping_after_game: "本局后停止",
      stopped: "已停止",
      error: "异常",
    },
    rooms: {
      bronze: "铜之间",
      silver: "银之间",
      gold: "金之间",
      jade: "玉之间",
      throne: "王座间",
    },
    modes: {
      four_player_east: "四人东",
      four_player_south: "四人南",
    },
  },
  en: {
    appTitle: "Majsoul Autopilot",
    subtitle: "Autoplay console",
    launch: "Start",
    stopAfterGame: "Stop after game",
    stopScheduled: "Stop scheduled",
    emergencyStop: "Emergency stop",
    emergencyConfirm: "Force stop autoplay now? This disconnects the current run.",
    emergencyCancel: "Cancel",
    emergencyConfirmAction: "Stop now",
    save: "Save settings",
    account: "Account",
    username: "Email / Username",
    password: "Password",
    accountInfo: "Current account",
    accountRefreshing: "Refreshing",
    accountWaiting: "Waiting for login",
    accountId: "Account ID",
    rank: "Rank",
    target: "Target",
    model: "Model",
    modelSelect: "Select model",
    modelBundled: "Bundled model",
    modelImported: "Imported model",
    modelImport: "Import safetensors",
    modelImporting: "Importing model",
    modelImportOk: "Model imported",
    modelImportFailed: "Model import failed",
    modelImportHelp:
      "Only exported .safetensors model directories are supported: the app loads weights and model_config.json directly through Rust/Candle. .pth is a PyTorch training checkpoint that depends on Python/PyTorch and cannot be read directly by this runtime.",
    previewImport: "Web preview: model import skipped",
    match: "Room",
    autoHighest: "Use highest room for current rank",
    manualRoom: "Manual room",
    mode: "Mode",
    tempo: "Action tempo",
    minMs: "Min ms",
    maxMs: "Max ms",
    maxGames: "Games",
    maxGamesPlaceholder: "Blank = unlimited",
    current: "Current game",
    roundEast: "East",
    roundSouth: "South",
    roundWest: "West",
    roundNorth: "North",
    roundUnit: "",
    noTable: "No game yet",
    players: "Players",
    self: "Self",
    riichi: "Riichi",
    tsumoWin: "Tsumo",
    ronWin: "Ron",
    dealIn: " dealt in",
    winningHand: "Winning hand",
    exhaustiveDraw: "Exhaustive draw",
    abortiveDraw: "Abortive draw",
    winTile: "Winning tile",
    points: " pts",
    han: " han",
    fu: " fu",
    yaku: "Yaku",
    allPlayersPay: " all",
    winningMelds: "Open melds",
    eventStream: "Structured events",
    logs: "Logs",
    gameRecords: "Recent Matches",
    copyPaipu: "Copy Paipu",
    copied: "Copied",
    openInMortal: "Review in Mortal",
    noGameRecords: "No match records yet",
    refreshRecords: "Refresh",
    refreshingRecords: "Fetching...",
    modelDecision: "Model recommendation",
    modelDiscard: "Discard",
    modelReach: "Riichi",
    modelChiLow: "Chi",
    modelChiMid: "Chi",
    modelChiHigh: "Chi",
    modelPon: "Pon",
    modelKan: "Kan",
    modelHora: "Win",
    modelRyukyoku: "Abortive draw",
    modelNone: "Skip",
    relativeConfidence: "relative confidence",
    qValue: "Q",
    legal: "legal",
    invalid: "invalid",
    topPlayer: "Across",
    leftPlayer: "Left",
    rightPlayer: "Right",
    dora: "Dora indicator",
    honba: "Honba",
    deposit: "Deposit",
    saveOk: "Settings saved",
    previewSave: "Web preview: settings save skipped",
    readFailed: "Failed to read settings.json",
    saveFailed: "Failed to save settings",
    startFailed: "Failed to start",
    stopFailed: "Failed to stop",
    emergencyFailed: "Emergency stop failed",
    settings: "Settings",
    roomPolicy: "Room policy",
    statusLabel: "Phase",
    status: {
      idle: "Idle",
      logging_in: "Logging in",
      matching: "Matching",
      reconnecting: "Reconnecting",
      in_game: "In game",
      stopping_after_game: "Stopping after game",
      stopped: "Stopped",
      error: "Error",
    },
    rooms: {
      bronze: "Bronze",
      silver: "Silver",
      gold: "Gold",
      jade: "Jade",
      throne: "Throne",
    },
    modes: {
      four_player_east: "4-player East",
      four_player_south: "4-player South",
    },
  },
  ja: {
    appTitle: "Majsoul Autopilot",
    subtitle: "自動対局コンソール",
    launch: "開始",
    stopAfterGame: "対局後に停止",
    stopScheduled: "対局後停止を予約済み",
    emergencyStop: "緊急停止",
    emergencyConfirm: "今すぐ自動対局を強制停止しますか？現在の実行を切断します。",
    emergencyCancel: "キャンセル",
    emergencyConfirmAction: "停止する",
    save: "設定を保存",
    account: "アカウント",
    username: "メール / ユーザー名",
    password: "パスワード",
    accountInfo: "現在のアカウント",
    accountRefreshing: "更新中",
    accountWaiting: "ログイン待ち",
    accountId: "アカウント ID",
    rank: "段位",
    target: "目標",
    model: "モデル",
    modelSelect: "モデルを選択",
    modelBundled: "内蔵モデル",
    modelImported: "インポート済みモデル",
    modelImport: "safetensors をインポート",
    modelImporting: "モデルをインポート中",
    modelImportOk: "モデルをインポートしました",
    modelImportFailed: "モデルのインポートに失敗",
    modelImportHelp:
      "エクスポート済みの .safetensors モデルディレクトリのみ対応しています。アプリは Rust/Candle で重みと model_config.json を直接読み込みます。.pth は Python/PyTorch 依存の学習チェックポイントなので、このランタイムでは直接読み込めません。",
    previewImport: "Web プレビュー: モデルインポートをスキップ",
    match: "部屋",
    autoHighest: "現在段位の最高部屋を自動選択",
    manualRoom: "手動部屋",
    mode: "対局種別",
    tempo: "打牌間隔",
    minMs: "最小 ms",
    maxMs: "最大 ms",
    maxGames: "連続対局数",
    maxGamesPlaceholder: "空欄 = 無制限",
    current: "現在の対局",
    roundEast: "東",
    roundSouth: "南",
    roundWest: "西",
    roundNorth: "北",
    roundUnit: "局",
    noTable: "対局なし",
    players: "プレイヤー",
    self: "自家",
    riichi: "リーチ",
    tsumoWin: "ツモ",
    ronWin: "ロン",
    dealIn: " 放銃",
    winningHand: "和了手牌",
    exhaustiveDraw: "荒牌流局",
    abortiveDraw: "途中流局",
    winTile: "和了牌",
    points: "点",
    han: "翻",
    fu: "符",
    yaku: "役",
    allPlayersPay: "オール",
    winningMelds: "副露",
    eventStream: "構造化イベント",
    logs: "ログ",
    gameRecords: "直近の牌譜",
    copyPaipu: "牌譜をコピー",
    copied: "コピー済",
    openInMortal: "Mortal で検討",
    noGameRecords: "牌譜記録はありません",
    refreshRecords: "牌譜を更新",
    refreshingRecords: "取得中...",
    modelDecision: "モデル推奨",
    modelDiscard: "打牌",
    modelReach: "リーチ",
    modelChiLow: "チー",
    modelChiMid: "チー",
    modelChiHigh: "チー",
    modelPon: "ポン",
    modelKan: "カン",
    modelHora: "和了",
    modelRyukyoku: "九種九牌",
    modelNone: "スキップ",
    relativeConfidence: "相対信頼度",
    qValue: "Q 値",
    legal: "有効",
    invalid: "無効",
    topPlayer: "対面",
    leftPlayer: "上家",
    rightPlayer: "下家",
    dora: "ドラ表示牌",
    honba: "本場",
    deposit: "供託",
    saveOk: "設定を保存しました",
    previewSave: "Web プレビュー: 設定保存をスキップ",
    readFailed: "settings.json の読み込みに失敗",
    saveFailed: "設定保存に失敗",
    startFailed: "起動に失敗",
    stopFailed: "停止に失敗",
    emergencyFailed: "緊急停止に失敗",
    settings: "設定",
    roomPolicy: "部屋ポリシー",
    statusLabel: "進行状況",
    status: {
      idle: "未起動",
      logging_in: "ログイン中",
      matching: "マッチング中",
      reconnecting: "再接続中",
      in_game: "対局中",
      stopping_after_game: "対局後停止",
      stopped: "停止済み",
      error: "異常",
    },
    rooms: {
      bronze: "銅の間",
      silver: "銀の間",
      gold: "金の間",
      jade: "玉の間",
      throne: "王座の間",
    },
    modes: {
      four_player_east: "四人東",
      four_player_south: "四人南",
    },
  },
};
