use anyhow::{anyhow, bail, ensure, Result};
use mjai::bridge;
use riichi::{
    mjai as riichi_mjai,
    state::{ActionCandidate, PlayerState},
    tile::Tile,
};
use std::str::FromStr;

#[derive(Debug, Clone)]
pub struct Observation {
    pub values: Vec<f32>,
    pub mask: Vec<bool>,
    pub channels: usize,
    pub width: usize,
}

#[derive(Debug, Clone)]
pub struct EngineDecision {
    pub action: usize,
    pub q_values: Vec<f32>,
    pub mask: Vec<bool>,
    pub is_greedy: bool,
}

pub trait NativeEngine {
    fn version(&self) -> u32;
    fn react_batch(&mut self, observations: &[Observation]) -> Result<Vec<EngineDecision>>;
}

pub struct NativeBot<E> {
    player_id: u8,
    state: PlayerState,
    log: Vec<riichi_mjai::EventExt>,
    engine: E,
    enable_rule_based_agari_guard: bool,
    last_decision: Option<EngineDecision>,
}

impl<E: NativeEngine> NativeBot<E> {
    pub fn new(player_id: u8, engine: E) -> Self {
        Self {
            player_id,
            state: PlayerState::new(player_id),
            log: Vec::new(),
            engine,
            enable_rule_based_agari_guard: true,
            last_decision: None,
        }
    }

    pub fn last_decision(&self) -> Option<&EngineDecision> {
        self.last_decision.as_ref()
    }

    pub fn react(&mut self, event: &bridge::Event) -> Result<Option<String>> {
        let Some(event) = convert_event(event)? else {
            return Ok(None);
        };

        match event {
            riichi_mjai::Event::EndKyoku => self.log.clear(),
            riichi_mjai::Event::EndGame => {}
            _ => self.log.push(riichi_mjai::EventExt::no_meta(event.clone())),
        }

        let cans = self.state.update(&event)?;
        if !cans.can_act() {
            return Ok(None);
        }

        let observation = encode_observation(&self.state, self.engine.version(), false);
        let mut observations = Vec::with_capacity(2);
        let kan_select_idx = if needs_kan_select(&self.state, cans) {
            observations.push(encode_observation(&self.state, self.engine.version(), true));
            Some(0)
        } else {
            None
        };
        let action_idx = observations.len();
        observations.push(observation);

        #[cfg(not(test))]
        let (jev_dec, kan_jev_dec) = (
            query_jev_decision(&self.state, &observations[action_idx].mask),
            kan_select_idx.map(|idx| query_jev_decision(&self.state, &observations[idx].mask)),
        );
        #[cfg(not(test))]
        let (decision, kan_decision) = (&jev_dec, kan_jev_dec.as_ref());

        #[cfg(test)]
        let decisions = self.engine.react_batch(&observations)?;
        #[cfg(test)]
        let (decision, kan_decision) = (&decisions[action_idx], kan_select_idx.map(|idx| &decisions[idx]));
        let action = if self.enable_rule_based_agari_guard
            && decision.action == 43
            && !self.state.rule_based_agari()
        {
            decision
                .q_values
                .iter()
                .enumerate()
                .filter(|(idx, _)| *idx != 43)
                .max_by(|(_, l), (_, r)| l.total_cmp(r))
                .map(|(idx, _)| idx)
                .unwrap_or(decision.action)
        } else {
            decision.action
        };
        let mut stored_decision = decision.clone();
        stored_decision.action = action;
        self.last_decision = Some(stored_decision);

        let reaction = decode_action(self.player_id, &self.state, cans, action, kan_decision)?;
        self.state.validate_reaction(&reaction)?;
        Ok(Some(serde_json::to_string(&reaction)?))
    }
}

fn encode_observation(state: &PlayerState, version: u32, at_kan_select: bool) -> Observation {
    let (obs, mask) = state.encode_obs(version, at_kan_select);
    let (channels, width) = obs.dim();
    Observation {
        values: obs.into_raw_vec_and_offset().0,
        mask: mask.to_vec(),
        channels,
        width,
    }
}

fn needs_kan_select(state: &PlayerState, cans: ActionCandidate) -> bool {
    (cans.can_ankan || cans.can_kakan)
        && state.ankan_candidates().len() + state.kakan_candidates().len() > 1
}

fn decode_action(
    actor: u8,
    state: &PlayerState,
    cans: ActionCandidate,
    action: usize,
    kan_decision: Option<&EngineDecision>,
) -> Result<riichi_mjai::Event> {
    let akas_in_hand = state.akas_in_hand();
    let event = match action {
        0..=36 => {
            ensure!(
                cans.can_discard,
                "engine chose discard without discard window"
            );
            let pai = Tile::try_from(action)?;
            let tsumogiri = state.last_self_tsumo().is_some_and(|tile| tile == pai);
            riichi_mjai::Event::Dahai {
                actor,
                pai,
                tsumogiri,
            }
        }
        37 => {
            ensure!(cans.can_riichi, "engine chose riichi without riichi window");
            riichi_mjai::Event::Reach { actor }
        }
        38 => {
            ensure!(cans.can_chi_low, "engine chose chi low without window");
            let pai = state
                .last_kawa_tile()
                .ok_or_else(|| anyhow!("chi low without last kawa tile"))?;
            let first = pai.next();
            let consumed = if can_akaize_chi_low(pai, akas_in_hand) {
                [first.akaize(), first.next().akaize()]
            } else {
                [first, first.next()]
            };
            riichi_mjai::Event::Chi {
                actor,
                target: cans.target_actor,
                pai,
                consumed,
            }
        }
        39 => {
            ensure!(cans.can_chi_mid, "engine chose chi mid without window");
            let pai = state
                .last_kawa_tile()
                .ok_or_else(|| anyhow!("chi mid without last kawa tile"))?;
            let consumed = if can_akaize_chi_mid(pai, akas_in_hand) {
                [pai.prev().akaize(), pai.next().akaize()]
            } else {
                [pai.prev(), pai.next()]
            };
            riichi_mjai::Event::Chi {
                actor,
                target: cans.target_actor,
                pai,
                consumed,
            }
        }
        40 => {
            ensure!(cans.can_chi_high, "engine chose chi high without window");
            let pai = state
                .last_kawa_tile()
                .ok_or_else(|| anyhow!("chi high without last kawa tile"))?;
            let last = pai.prev();
            let consumed = if can_akaize_chi_high(pai, akas_in_hand) {
                [last.prev().akaize(), last.akaize()]
            } else {
                [last.prev(), last]
            };
            riichi_mjai::Event::Chi {
                actor,
                target: cans.target_actor,
                pai,
                consumed,
            }
        }
        41 => {
            ensure!(cans.can_pon, "engine chose pon without window");
            let pai = state
                .last_kawa_tile()
                .ok_or_else(|| anyhow!("pon without last kawa tile"))?;
            let consumed = if can_akaize_pon(pai, akas_in_hand) {
                [pai.akaize(), pai.deaka()]
            } else {
                [pai.deaka(); 2]
            };
            riichi_mjai::Event::Pon {
                actor,
                target: cans.target_actor,
                pai,
                consumed,
            }
        }
        42 => decode_kan(actor, state, cans, kan_decision)?,
        43 => {
            ensure!(cans.can_agari(), "engine chose hora without agari window");
            riichi_mjai::Event::Hora {
                actor,
                target: cans.target_actor,
                deltas: None,
                ura_markers: None,
            }
        }
        44 => {
            ensure!(cans.can_ryukyoku, "engine chose ryukyoku without window");
            riichi_mjai::Event::Ryukyoku { deltas: None }
        }
        _ => riichi_mjai::Event::None,
    };
    Ok(event)
}

fn decode_kan(
    actor: u8,
    state: &PlayerState,
    cans: ActionCandidate,
    kan_decision: Option<&EngineDecision>,
) -> Result<riichi_mjai::Event> {
    ensure!(
        cans.can_daiminkan || cans.can_ankan || cans.can_kakan,
        "engine chose kan without kan window"
    );

    let ankan_candidates = state.ankan_candidates();
    let kakan_candidates = state.kakan_candidates();
    let tile = if let Some(decision) = kan_decision {
        let tile = Tile::try_from(decision.action)?;
        ensure!(
            ankan_candidates.contains(&tile) || kakan_candidates.contains(&tile),
            "kan-select action is not a kan candidate"
        );
        tile
    } else if cans.can_daiminkan {
        state
            .last_kawa_tile()
            .ok_or_else(|| anyhow!("daiminkan without last kawa tile"))?
    } else if cans.can_ankan {
        *ankan_candidates
            .first()
            .ok_or_else(|| anyhow!("ankan without candidates"))?
    } else {
        *kakan_candidates
            .first()
            .ok_or_else(|| anyhow!("kakan without candidates"))?
    };

    if cans.can_daiminkan {
        let consumed = if tile.is_aka() {
            [tile.deaka(); 3]
        } else {
            [tile.akaize(), tile, tile]
        };
        Ok(riichi_mjai::Event::Daiminkan {
            actor,
            target: cans.target_actor,
            pai: tile,
            consumed,
        })
    } else if cans.can_ankan && ankan_candidates.contains(&tile.deaka()) {
        Ok(riichi_mjai::Event::Ankan {
            actor,
            consumed: [tile.akaize(), tile, tile, tile],
        })
    } else {
        let (pai, consumed) = if can_akaize_pon(tile, state.akas_in_hand()) {
            (tile.akaize(), [tile.deaka(); 3])
        } else {
            (tile.deaka(), [tile.akaize(), tile.deaka(), tile.deaka()])
        };
        Ok(riichi_mjai::Event::Kakan {
            actor,
            pai,
            consumed,
        })
    }
}

fn convert_event(event: &bridge::Event) -> Result<Option<riichi_mjai::Event>> {
    let event = match event {
        bridge::Event::StartGame { .. } | bridge::Event::Dora { .. } => return Ok(None),
        bridge::Event::StartKyoku {
            bakaze,
            dora_marker,
            honba,
            kyoku,
            kyotaku,
            oya,
            scores,
            tehais,
        } => riichi_mjai::Event::StartKyoku {
            bakaze: parse_tile(bakaze)?,
            dora_marker: parse_tile(dora_marker)?,
            kyoku: (*kyoku).try_into()?,
            honba: (*honba).try_into()?,
            kyotaku: (*kyotaku).try_into()?,
            oya: (*oya).try_into()?,
            scores: vec_to_array4(scores)?,
            tehais: tehais_to_array(tehais)?,
        },
        bridge::Event::Tsumo { actor, pai } => riichi_mjai::Event::Tsumo {
            actor: (*actor).try_into()?,
            pai: parse_tile(pai)?,
        },
        bridge::Event::Reach { actor } => riichi_mjai::Event::Reach {
            actor: (*actor).try_into()?,
        },
        bridge::Event::ReachAccepted { actor } => riichi_mjai::Event::ReachAccepted {
            actor: (*actor).try_into()?,
        },
        bridge::Event::Dahai {
            actor,
            pai,
            tsumogiri,
        } => riichi_mjai::Event::Dahai {
            actor: (*actor).try_into()?,
            pai: parse_tile(pai)?,
            tsumogiri: *tsumogiri,
        },
        bridge::Event::Chi {
            actor,
            target,
            pai,
            consumed,
        } => riichi_mjai::Event::Chi {
            actor: (*actor).try_into()?,
            target: (*target).try_into()?,
            pai: parse_tile(pai)?,
            consumed: vec_to_tile_array2(consumed)?,
        },
        bridge::Event::Pon {
            actor,
            target,
            pai,
            consumed,
        } => riichi_mjai::Event::Pon {
            actor: (*actor).try_into()?,
            target: (*target).try_into()?,
            pai: parse_tile(pai)?,
            consumed: vec_to_tile_array2(consumed)?,
        },
        bridge::Event::Daiminkan {
            actor,
            target,
            pai,
            consumed,
        } => riichi_mjai::Event::Daiminkan {
            actor: (*actor).try_into()?,
            target: (*target).try_into()?,
            pai: parse_tile(pai)?,
            consumed: vec_to_tile_array3(consumed)?,
        },
        bridge::Event::Ankan { actor, consumed } => riichi_mjai::Event::Ankan {
            actor: (*actor).try_into()?,
            consumed: vec_to_tile_array4(consumed)?,
        },
        bridge::Event::Kakan {
            actor,
            pai,
            consumed,
        } => riichi_mjai::Event::Kakan {
            actor: (*actor).try_into()?,
            pai: parse_tile(pai)?,
            consumed: vec_to_tile_array3(consumed)?,
        },
        bridge::Event::Hule { .. }
        | bridge::Event::NoTile { .. }
        | bridge::Event::LiuJu { .. }
        | bridge::Event::EndKyoku => riichi_mjai::Event::EndKyoku,
        bridge::Event::EndGame => riichi_mjai::Event::EndGame,
    };
    Ok(Some(event))
}

fn parse_tile(tile: &str) -> Result<Tile> {
    Tile::from_str(tile).map_err(|err| anyhow!("{err}"))
}

fn vec_to_array4(values: &[i32]) -> Result<[i32; 4]> {
    values
        .try_into()
        .map_err(|_| anyhow!("expected 4 scores, got {}", values.len()))
}

fn tehais_to_array(values: &[Vec<String>]) -> Result<[[Tile; 13]; 4]> {
    let mut ret = [[parse_tile("?")?; 13]; 4];
    if values.len() != 4 {
        bail!("expected 4 tehais, got {}", values.len());
    }
    for (seat, tehai) in values.iter().enumerate() {
        if tehai.len() != 13 {
            bail!("expected 13 tiles in tehai {seat}, got {}", tehai.len());
        }
        for (idx, tile) in tehai.iter().enumerate() {
            ret[seat][idx] = parse_tile(tile)?;
        }
    }
    Ok(ret)
}

fn vec_to_tile_array2(values: &[String]) -> Result<[Tile; 2]> {
    Ok([parse_tile(&values[0])?, parse_tile(&values[1])?])
}

fn vec_to_tile_array3(values: &[String]) -> Result<[Tile; 3]> {
    Ok([
        parse_tile(&values[0])?,
        parse_tile(&values[1])?,
        parse_tile(&values[2])?,
    ])
}

fn vec_to_tile_array4(values: &[String]) -> Result<[Tile; 4]> {
    Ok([
        parse_tile(&values[0])?,
        parse_tile(&values[1])?,
        parse_tile(&values[2])?,
        parse_tile(&values[3])?,
    ])
}

fn can_akaize_chi_low(pai: Tile, akas: [bool; 3]) -> bool {
    matches!(pai.to_string().as_str(), "3m" | "4m") && akas[0]
        || matches!(pai.to_string().as_str(), "3p" | "4p") && akas[1]
        || matches!(pai.to_string().as_str(), "3s" | "4s") && akas[2]
}

fn can_akaize_chi_mid(pai: Tile, akas: [bool; 3]) -> bool {
    matches!(pai.to_string().as_str(), "4m" | "6m") && akas[0]
        || matches!(pai.to_string().as_str(), "4p" | "6p") && akas[1]
        || matches!(pai.to_string().as_str(), "4s" | "6s") && akas[2]
}

fn can_akaize_chi_high(pai: Tile, akas: [bool; 3]) -> bool {
    matches!(pai.to_string().as_str(), "6m" | "7m") && akas[0]
        || matches!(pai.to_string().as_str(), "6p" | "7p") && akas[1]
        || matches!(pai.to_string().as_str(), "6s" | "7s") && akas[2]
}

fn can_akaize_pon(pai: Tile, akas: [bool; 3]) -> bool {
    matches!(pai.to_string().as_str(), "5m") && akas[0]
        || matches!(pai.to_string().as_str(), "5p") && akas[1]
        || matches!(pai.to_string().as_str(), "5s") && akas[2]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct ScriptedEngine {
        version: u32,
        actions: Vec<usize>,
    }

    impl NativeEngine for ScriptedEngine {
        fn version(&self) -> u32 {
            self.version
        }

        fn react_batch(&mut self, observations: &[Observation]) -> Result<Vec<EngineDecision>> {
            assert_eq!(observations.len(), 1);
            let action = self.actions.remove(0);
            let mut q_values = vec![0.0; 46];
            q_values[action] = 1.0;
            Ok(vec![EngineDecision {
                action,
                q_values,
                mask: observations[0].mask.clone(),
                is_greedy: true,
            }])
        }
    }

    fn start() -> bridge::Event {
        bridge::Event::StartKyoku {
            bakaze: "E".to_string(),
            dora_marker: "4p".to_string(),
            honba: 0,
            kyoku: 1,
            kyotaku: 0,
            oya: 0,
            scores: vec![25000; 4],
            tehais: vec![
                vec![
                    "1m", "1m", "2m", "3m", "4p", "5p", "6p", "2s", "3s", "4s", "E", "E", "P",
                ]
                .into_iter()
                .map(str::to_string)
                .collect(),
                vec!["?"; 13].into_iter().map(str::to_string).collect(),
                vec!["?"; 13].into_iter().map(str::to_string).collect(),
                vec!["?"; 13].into_iter().map(str::to_string).collect(),
            ],
        }
    }

    fn riichi_start() -> bridge::Event {
        bridge::Event::StartKyoku {
            bakaze: "E".to_string(),
            dora_marker: "4p".to_string(),
            honba: 0,
            kyoku: 1,
            kyotaku: 0,
            oya: 0,
            scores: vec![25000; 4],
            tehais: vec![
                vec![
                    "1m", "2m", "3m", "6m", "7m", "1p", "2p", "3p", "1s", "2s", "3s", "P", "P",
                ]
                .into_iter()
                .map(str::to_string)
                .collect(),
                vec!["?"; 13].into_iter().map(str::to_string).collect(),
                vec!["?"; 13].into_iter().map(str::to_string).collect(),
                vec!["?"; 13].into_iter().map(str::to_string).collect(),
            ],
        }
    }

    #[test]
    fn native_bot_decodes_scripted_discard_action_through_riichi_state() {
        let mut bot = NativeBot::new(
            0,
            ScriptedEngine {
                version: 4,
                actions: vec![0],
            },
        );
        assert!(bot.react(&start()).unwrap().is_none());
        let action = bot
            .react(&bridge::Event::Tsumo {
                actor: 0,
                pai: "7m".to_string(),
            })
            .unwrap()
            .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&action).unwrap(),
            serde_json::json!({"type":"dahai","actor":0,"pai":"1m","tsumogiri":false})
        );
    }

    #[test]
    fn native_bot_returns_reach_before_declaration_discard_like_python_wrapper() {
        let mut bot = NativeBot::new(
            0,
            ScriptedEngine {
                version: 4,
                actions: vec![37],
            },
        );
        assert!(bot.react(&riichi_start()).unwrap().is_none());
        let action = bot
            .react(&bridge::Event::Tsumo {
                actor: 0,
                pai: "9m".to_string(),
            })
            .unwrap()
            .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&action).unwrap(),
            serde_json::json!({"type":"reach","actor":0})
        );
    }
}


fn action_idx_to_label(idx: usize) -> String {
    match idx {
        0..=36 => Tile::try_from(idx)
            .map(|t| t.to_string())
            .unwrap_or_else(|_| format!("t{idx}")),
        37 => "reach".to_string(),
        38 => "chi_low".to_string(),
        39 => "chi_mid".to_string(),
        40 => "chi_high".to_string(),
        41 => "pon".to_string(),
        42 => "kan".to_string(),
        43 => "hora".to_string(),
        44 => "ryukyoku".to_string(),
        _ => "none".to_string(),
    }
}

fn describe_action_option(opt: &str) -> String {
    match opt {
        "hora" => "立即和牌（自摸或荣和得分）".to_string(),
        "reach" => "宣言立直（门前清听牌加1番并争夺里宝牌）".to_string(),
        "pon" => "碰牌（副露鸣牌）".to_string(),
        "chi_low" => "低位顺子吃牌（如用56吃4）".to_string(),
        "chi_mid" => "嵌张顺子吃牌（如用46吃5）".to_string(),
        "chi_high" => "高位顺子吃牌（如用34吃5）".to_string(),
        "kan" => "杠牌（暗杠/加杠/大明杠）".to_string(),
        "ryukyoku" => "九种九牌流局".to_string(),
        "none" => "跳过不鸣牌/不动作（保持门前清或保持当前听牌）".to_string(),
        other => format!("切出牌 {other}"),
    }
}

fn load_openrouter_api_key() -> String {
    if let Ok(val) = std::env::var("OPENROUTER_API_KEY") {
        let trimmed = val.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    let mut candidates = Vec::new();
    if let Ok(custom) = std::env::var("JEV_ENV_FILE") {
        candidates.push(custom);
    }
    candidates.push(".env".to_string());
    candidates.push("/Users/shine/proj/dc2dc/secrets/.env".to_string());

    for path in candidates {
        if let Ok(content) = std::fs::read_to_string(&path) {
            for line in content.lines() {
                let line = line.trim();
                if let Some(rest) = line.strip_prefix("OPENROUTER_API_KEY=") {
                    let val = rest.trim().trim_matches('"').trim_matches('\'');
                    if !val.is_empty() {
                        return val.to_string();
                    }
                }
            }
        }
    }
    String::new()
}

const JEV_INSTRUCTIONS: &str = "你是日本立直麻将（雀魂玉之间·四人南/东）顶级AI决策引擎。请严格依据 state 底部的 libriichi 预计算面板选出最优动作：\n\n1. 【和牌必胡 (hora)】：若候选含 hora，除All-Last（南4局/东4局）第4位未逆转外，100%必选 hora。\n2. 【副露吃碰判断 (pon/chi vs none)】：\n   - 必须严格查看底部的【libriichi 副露(吃/碰)模拟器精确对比】！\n   - 只要模拟器判定为「❌ 严禁鸣牌」或「⚠️ 倾向不鸣牌 none」（例如已听牌破听、向听数未推进、无役副露导致EV暴跌），100%必选 none！\n   - 只有当模拟器明确判定为「✅ 推荐鸣牌」时，才选择对应的 pon 或 chi 动作。\n3. 【他家立直攻防】：\n   - 若底部出现 opponent_riichi_genbutsu（他家已立直告警），且自家当前 shanten >= 2（或低打点1向听），必须弃和防守！严格从 100%_safe_genbutsu_in_hand（对立直家100%安全现物）中选择切牌，绝不切生张点炮。\n4. 【立直 vs 默听 (reach vs 切牌)】：\n   - 当门前清听牌（shanten=0）且候选有 reach 时：若已存在【有役默听真听牌 (unconditional_tenpai_discards 非空)】且（自家处于南4局/东4局可锁顺位，或默听打点 EV >= 3000 已足够），优先不立直（直接切对应的听牌切牌默听）；若无役听牌（unconditional_tenpai_discards 为空）则必选 reach 立直！\n5. 【常态何切（无人立直）】：\n   - 直接严格遵循底部的【libriichi 综合最优何切推荐榜】，优先选择「推荐 #1」对应的切牌！";

fn jev_http_runtime() -> &'static (tokio::runtime::Runtime, reqwest::Client) {
    static RT: std::sync::OnceLock<(tokio::runtime::Runtime, reqwest::Client)> =
        std::sync::OnceLock::new();
    RT.get_or_init(|| {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build Jev HTTP tokio runtime");
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_millis(4200))
            .build()
            .expect("failed to build Jev reqwest client");
        (rt, client)
    })
}

fn query_jev_decision(state: &PlayerState, mask: &[bool]) -> EngineDecision {
    let legal: Vec<(usize, String)> = mask
        .iter()
        .copied()
        .enumerate()
        .filter(|&(_, b)| b)
        .map(|(i, _)| (i, action_idx_to_label(i)))
        .collect();

    let mut q_values = vec![f32::NEG_INFINITY; mask.len()];
    if legal.is_empty() {
        return EngineDecision {
            action: 45,
            q_values,
            mask: mask.to_vec(),
            is_greedy: true,
        };
    }
    if legal.len() == 1 {
        let idx = legal[0].0;
        q_values[idx] = 1.0;
        return EngineDecision {
            action: idx,
            q_values,
            mask: mask.to_vec(),
            is_greedy: true,
        };
    }

    let options: Vec<String> = legal.iter().map(|(_, s)| s.clone()).collect();
    let mut criteria = serde_json::Map::new();
    for opt in &options {
        criteria.insert(
            opt.clone(),
            serde_json::Value::String(describe_action_option(opt)),
        );
    }

    let context = state.jev_context_info(mask);
    let payload = serde_json::json!({
        "model": "typesafe/jev-1.14",
        "state": [{"type": "text", "text": context}],
        "questions": {
            "action": {
                "type": "choice",
                "instructions": JEV_INSTRUCTIONS,
                "criteria": criteria,
            }
        }
    });

    let api_key = load_openrouter_api_key();
    let body_bytes = serde_json::to_vec(&payload).unwrap_or_default();

    let mut chosen_idx = legal
        .iter()
        .find(|&&(_, ref lbl)| lbl == "none")
        .map(|&(i, _)| i)
        .unwrap_or(legal[0].0);
    for &(i, _) in &legal {
        q_values[i] = 0.0;
    }

    let t0 = std::time::Instant::now();
    let mut probs_map = serde_json::Map::new();
    for opt in &options {
        probs_map.insert(opt.clone(), serde_json::json!(0.0));
    }
    let mut chosen_label = action_idx_to_label(chosen_idx);
    let mut err_msg: Option<String> = None;

    let res: Result<serde_json::Value, String> = std::thread::scope(|s| {
        s.spawn(|| {
            let (rt, client) = jev_http_runtime();
            rt.block_on(async {
                let mut last_err = String::new();
                for attempt in 0..2 {
                    if attempt > 0 {
                        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                    }
                    let req = client
                        .post("https://openrouter.ai/api/alpha/decisions")
                        .header("Authorization", format!("Bearer {api_key}"))
                        .header("Content-Type", "application/json")
                        .header("HTTP-Referer", "https://github.com/majsoul-autopilot")
                        .header("X-Title", "majsoul-jev-live")
                        .body(body_bytes.clone());
                    match req.send().await {
                        Ok(resp) => match resp.bytes().await {
                            Ok(bytes) => match serde_json::from_slice::<serde_json::Value>(&bytes) {
                                Ok(val) => return Ok(val),
                                Err(e) => last_err = format!("json parse error: {e}"),
                            },
                            Err(e) => last_err = format!("read body error: {e}"),
                        },
                        Err(e) => last_err = format!("http send error: {e}"),
                    }
                }
                Err(last_err)
            })
        })
        .join()
        .unwrap_or_else(|_| Err("jev worker thread panicked".to_string()))
    });

    match res {
        Ok(val) => {
            if let Some(ans) = val.get("answers").and_then(|v| v.get("action")) {
                if let Some(dist) = ans.get("probabilities").and_then(|v| v.as_object()) {
                    for &(i, ref lbl) in &legal {
                        if let Some(p) = dist.get(lbl).and_then(|v| v.as_f64()) {
                            q_values[i] = p as f32;
                            probs_map.insert(lbl.clone(), serde_json::json!(p));
                        }
                    }
                }
                if let Some(c_str) = ans.get("choice").and_then(|v| v.as_str()) {
                    if let Some(&(i, ref lbl)) = legal.iter().find(|&&(_, ref l)| l == c_str) {
                        chosen_idx = i;
                        chosen_label = lbl.clone();
                    }
                } else if let Some(&(i, ref lbl)) = legal.iter().max_by(|&&(i1, _), &&(i2, _)| {
                    q_values[i1]
                        .partial_cmp(&q_values[i2])
                        .unwrap_or(std::cmp::Ordering::Equal)
                }) {
                    chosen_idx = i;
                    chosen_label = lbl.clone();
                }
            }
        }
        Err(e) => {
            err_msg = Some(e);
            probs_map.insert(chosen_label.clone(), serde_json::json!(1.0));
        }
    }

    let latency_ms = t0.elapsed().as_millis() as u64;
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let mut log_entry = serde_json::json!({
        "ts": ts,
        "latency_ms": latency_ms,
        "options": options,
        "choice": chosen_label,
        "probs": probs_map,
    });
    if let Some(err) = err_msg {
        if let Some(obj) = log_entry.as_object_mut() {
            obj.insert("error".to_string(), serde_json::Value::String(err));
        }
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/jev_live_decisions.jsonl")
    {
        use std::io::Write;
        let _ = writeln!(file, "{}", log_entry);
    }

    EngineDecision {
        action: chosen_idx,
        q_values,
        mask: mask.to_vec(),
        is_greedy: true,
    }
}
