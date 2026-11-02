use super::action::ActionCandidate;
use super::item::{ChiPon, KawaItem, Sutehai};
use crate::algo::sp::Candidate;
use crate::hand::tiles_to_string;
use crate::must_tile;
use crate::tile::Tile;
#[cfg(test)]
use anyhow::Result;
use std::iter;

use derivative::Derivative;
use tinyvec::{ArrayVec, TinyVec};

/// `PlayerState` is the core of the lib, which holds all the observable game
/// state information from a specific seat's perspective with the ability to
/// identify the legal actions the specified player can make upon an incoming
/// mjai event, along with some helper functions to build an actual agent.
/// Notably, `PlayerState` encodes observation features into numpy arrays which
/// serve as inputs for deep learning model.
#[derive(Clone, Derivative)]
#[derivative(Default)]
pub struct PlayerState {
    pub(super) player_id: u8,

    /// Does not include aka.
    #[derivative(Default(value = "[0; 34]"))]
    pub(super) tehai: [u8; 34],

    /// Does not consider yakunashi, but does consider other kinds of
    /// furiten.
    #[derivative(Default(value = "[false; 34]"))]
    pub(super) waits: [bool; 34],

    #[derivative(Default(value = "[0; 34]"))]
    pub(super) dora_factor: [u8; 34],

    /// For calculating `waits` and `doras_seen`, also for SPCalculator.
    #[derivative(Default(value = "[0; 34]"))]
    pub(super) tiles_seen: [u8; 34],

    /// For SPCalculator.
    pub(super) akas_seen: [bool; 3],

    #[derivative(Default(value = "[false; 34]"))]
    pub(super) keep_shanten_discards: [bool; 34],

    #[derivative(Default(value = "[false; 34]"))]
    pub(super) next_shanten_discards: [bool; 34],

    #[derivative(Default(value = "[false; 34]"))]
    pub(super) forbidden_tiles: [bool; 34],

    /// Used for furiten check.
    #[derivative(Default(value = "[false; 34]"))]
    pub(super) discarded_tiles: [bool; 34],

    pub(super) bakaze: Tile,
    pub(super) jikaze: Tile,
    /// Counts from 0 unlike mjai.
    pub(super) kyoku: u8,
    pub(super) honba: u8,
    pub(super) kyotaku: u8,
    /// Rotated to be relative, so `scores[0]` is the score of the player.
    pub(super) scores: [i32; 4],
    pub(super) rank: u8,
    /// Relative to `player_id`.
    pub(super) oya: u8,
    /// Including 西入 sudden death.
    pub(super) is_all_last: bool,
    pub(super) dora_indicators: ArrayVec<[Tile; 5]>,

    /// 24 is the theoretical max size of kawa, however, since None is included
    /// in the kawa, in some very rare cases (about one in a million hanchans),
    /// the size can exceed 24.
    ///
    /// Reference:
    /// <https://detail.chiebukuro.yahoo.co.jp/qa/question_detail/q1020002370>
    pub(super) kawa: [TinyVec<[Option<KawaItem>; 24]>; 4],
    pub(super) last_tedashis: [Option<Sutehai>; 4],
    pub(super) riichi_sutehais: [Option<Sutehai>; 4],

    /// Using 34-D arrays here may be more efficient, but I don't want to mess up
    /// with aka doras.
    pub(super) kawa_overview: [ArrayVec<[Tile; 24]>; 4],
    pub(super) fuuro_overview: [ArrayVec<[ArrayVec<[Tile; 4]>; 4]>; 4],
    /// In this field all `Tile` are deaka'd.
    pub(super) ankan_overview: [ArrayVec<[Tile; 4]>; 4],

    pub(super) riichi_declared: [bool; 4],
    pub(super) riichi_accepted: [bool; 4],

    pub(super) at_turn: u8,
    pub(super) tiles_left: u8,
    pub(super) intermediate_kan: ArrayVec<[Tile; 4]>,
    pub(super) intermediate_chi_pon: Option<ChiPon>,

    pub(super) shanten: i8,

    pub(super) last_self_tsumo: Option<Tile>,
    pub(super) last_kawa_tile: Option<Tile>,
    pub(super) last_cans: ActionCandidate,

    /// Both deaka'd
    pub(super) ankan_candidates: ArrayVec<[Tile; 3]>,
    pub(super) kakan_candidates: ArrayVec<[Tile; 3]>,
    pub(super) chankan_chance: Option<()>,

    pub(super) can_w_riichi: bool,
    pub(super) is_w_riichi: bool,
    pub(super) at_rinshan: bool,
    pub(super) at_ippatsu: bool,
    pub(super) at_furiten: bool,
    pub(super) to_mark_same_cycle_furiten: Option<()>,

    /// Used for 4-kan check.
    pub(super) kans_on_board: u8,

    pub(super) is_menzen: bool,
    /// For agari calc, all deaka'd.
    pub(super) chis: ArrayVec<[u8; 4]>,
    pub(super) pons: ArrayVec<[u8; 4]>,
    pub(super) minkans: ArrayVec<[u8; 4]>,
    pub(super) ankans: ArrayVec<[u8; 4]>,

    /// Including aka, originally for agari calc usage but also encoded as a
    /// feature to the obs.
    pub(super) doras_owned: [u8; 4],
    pub(super) doras_seen: u8,

    pub(super) akas_in_hand: [bool; 3],

    /// For shanten calc.
    pub(super) tehai_len_div3: u8,

    /// Used in can_riichi, also in single-player features to get the shanten
    /// for 3n+2.
    pub(super) has_next_shanten_discard: bool,
}

impl PlayerState {
    /// Panics if `player_id` is outside of range [0, 3].
    #[must_use]
    pub fn new(player_id: u8) -> Self {
        assert!(player_id < 4, "{player_id} is not in range [0, 3]");
        Self {
            player_id,
            ..Default::default()
        }
    }

    #[cfg(test)]
    pub(super) fn update_json(&mut self, mjai_json: &str) -> Result<ActionCandidate> {
        let event = serde_json::from_str(mjai_json)?;
        self.update(&event)
    }

    /// For debug only.
    ///
    /// Return a human readable description of the current state.
    #[must_use]
    pub fn brief_info(&self) -> String {
        let waits = self
            .waits
            .iter()
            .enumerate()
            .filter(|&(_, &b)| b)
            .map(|(i, _)| must_tile!(i))
            .collect::<Vec<_>>();

        let zipped_kawa = self.kawa[0]
            .iter()
            .chain(iter::repeat(&None))
            .zip(self.kawa[1].iter().chain(iter::repeat(&None)))
            .zip(self.kawa[2].iter().chain(iter::repeat(&None)))
            .zip(self.kawa[3].iter().chain(iter::repeat(&None)))
            .take_while(|row| !matches!(row, &(((None, None), None), None)))
            .enumerate()
            .map(|(i, (((a, b), c), d))| {
                format!(
                    "{i:2}. {}\t{}\t{}\t{}",
                    a.as_ref()
                        .map_or_else(|| "-".to_owned(), |item| item.to_string()),
                    b.as_ref()
                        .map_or_else(|| "-".to_owned(), |item| item.to_string()),
                    c.as_ref()
                        .map_or_else(|| "-".to_owned(), |item| item.to_string()),
                    d.as_ref()
                        .map_or_else(|| "-".to_owned(), |item| item.to_string()),
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        let can_discard = self.last_cans.can_discard;
        let mut sp_tables = Candidate::csv_header(can_discard).join("\t");
        if let Ok(tables) = self.single_player_tables() {
            for candidate in tables.max_ev_table {
                sp_tables.push('\n');
                sp_tables.push_str(&candidate.csv_row(can_discard).join("\t"));
            }
        }

        format!(
            r#"player (abs): {}
oya (rel): {}
kyoku: {}{}-{}
turn: {}
jikaze: {}
score (rel): {:?}
tehai: {}
fuuro: {:?}
ankan: {:?}
tehai len: {}
shanten: {} (actual: {})
furiten: {}
waits: {waits:?}
dora indicators: {:?}
doras owned: {:?}
doras seen: {}
action candidates: {:#?}
last self tsumo: {:?}
last kawa tile: {:?}
tiles left: {}
kawa:
{zipped_kawa}
single player table (max EV):
{sp_tables}"#,
            self.player_id,
            self.oya,
            self.bakaze,
            self.kyoku + 1,
            self.honba,
            self.at_turn,
            self.jikaze,
            self.scores,
            tiles_to_string(&self.tehai, self.akas_in_hand),
            self.fuuro_overview[0],
            self.ankan_overview[0],
            self.tehai_len_div3,
            self.shanten,
            self.real_time_shanten(),
            self.at_furiten,
            self.dora_indicators,
            self.doras_owned,
            self.doras_seen,
            self.last_cans,
            self.last_self_tsumo,
            self.last_kawa_tile,
            self.tiles_left,
        )
    }

    #[must_use]
    pub fn jev_context_info(&self, mask: &[bool]) -> String {
        let mut out = self.brief_info();
        let sh_now = self.real_time_shanten();

        if self.last_cans.can_agari() {
            if let Ok(pt) = self.agari_points(self.last_cans.can_ron_agari, &[]) {
                out.push_str(&format!(
                    "\nagari_points: can_win_now=true (ron={}, tsumo_ko={}, tsumo_oya={})",
                    pt.ron, pt.tsumo_ko, pt.tsumo_oya
                ));
            } else {
                out.push_str("\nagari_points: can_win_now=true");
            }
        }
        if self.last_cans.can_discard {
            let uncond = self.discard_candidates_with_unconditional_tenpai_aka();
            let tiles: Vec<String> = uncond
                .iter()
                .enumerate()
                .filter(|&(_, &b)| b)
                .map(|(i, _)| must_tile!(i).to_string())
                .collect();
            out.push_str(&format!(
                "\nunconditional_tenpai_discards (有役默听真听牌切牌): {:?}",
                tiles
            ));

            // Smart composite discard ranking (fixes 3-pair Chiitoitsu trap, guest wind priority, and ukeire/win_prob tie-breaks)
            if let Ok(tables) = self.single_player_tables() {
                let mut cands: Vec<_> = tables
                    .max_ev_table
                    .iter()
                    .filter(|c| !c.shanten_down && mask.get(c.tile.as_usize()).copied().unwrap_or(true))
                    .collect();
                if cands.is_empty() {
                    cands = tables
                        .max_ev_table
                        .iter()
                        .filter(|c| mask.get(c.tile.as_usize()).copied().unwrap_or(true))
                        .collect();
                }
                if !cands.is_empty() {
                    let top_ev = cands
                        .iter()
                        .map(|c| c.exp_values.first().copied().unwrap_or(0.0))
                        .fold(0.0_f32, f32::max);

                    let classify_tile = |t: Tile| -> (&'static str, i32, bool) {
                        let d = t.deaka();
                        let tid = d.as_usize();
                        let id = d.as_u8();
                        let cnt = self.tehai[tid];
                        let is_connected = if id < 27 {
                            let num = (id % 9) as i32;
                            let base = (tid / 9) * 9;
                            let mut conn = cnt >= 2;
                            for delta in [-2_i32, -1, 1, 2] {
                                let n = num + delta;
                                if (0..9).contains(&n) && self.tehai[base + n as usize] > 0 {
                                    conn = true;
                                }
                            }
                            conn
                        } else {
                            cnt >= 2
                        };
                        if id >= 31 {
                            if cnt >= 2 {
                                ("三元役牌对子/刻(1番)", 0, true)
                            } else {
                                ("孤立三元役牌(1番)", 3, false)
                            }
                        } else if d == self.bakaze && d == self.jikaze {
                            if cnt >= 2 {
                                ("连风役牌对子/刻(2番)", 0, true)
                            } else {
                                ("孤立连风役牌(2番)", 2, false)
                            }
                        } else if d == self.bakaze || d == self.jikaze {
                            if cnt >= 2 {
                                ("场/自风役牌对子/刻(1番)", 0, true)
                            } else {
                                ("孤立场/自风役牌(1番)", 4, false)
                            }
                        } else if id >= 27 {
                            if cnt >= 2 {
                                ("客风对子(0番)", 1, true)
                            } else {
                                ("孤立客风(0番无役优先切)", 6, false)
                            }
                        } else {
                            let num = id % 9;
                            if !is_connected {
                                if num == 0 || num == 8 {
                                    ("孤立老头牌(1/9)", 5, false)
                                } else {
                                    ("孤立中张数牌", 2, false)
                                }
                            } else if t.is_aka() {
                                ("赤宝牌搭子/面子", 0, true)
                            } else {
                                ("相连搭子/复合形数牌", 1, true)
                            }
                        }
                    };

                    // Detect 3-pair Chiitoitsu trap at shanten >= 2 (ukeire_kinds == 7 and ukeire_sum <= 21)
                    let is_chitoi_trap = sh_now >= 2
                        && cands.first().is_some_and(|c| c.required_tiles.len() == 7 && c.num_required_tiles <= 21);

                    cands.sort_by(|a, b| {
                        let ev_a = a.exp_values.first().copied().unwrap_or(0.0);
                        let ev_b = b.exp_values.first().copied().unwrap_or(0.0);
                        let wp_a = a.win_probs.first().copied().unwrap_or(0.0) * 100.0;
                        let wp_b = b.win_probs.first().copied().unwrap_or(0.0) * 100.0;
                        let (_, prio_a, conn_a) = classify_tile(a.tile);
                        let (_, prio_b, conn_b) = classify_tile(b.tile);

                        if top_ev <= 1.0 {
                            // shanten >= 4 or yakuless: sort by ukeire DESC, then isolated guest wind/terminal priority DESC
                            b.num_required_tiles
                                .cmp(&a.num_required_tiles)
                                .then_with(|| prio_b.cmp(&prio_a))
                        } else if is_chitoi_trap && (ev_a - ev_b).abs() <= top_ev * 0.08 {
                            // Never break connected 112p / 334s / 223m shapes before isolated tiles when EV is close!
                            conn_a
                                .cmp(&conn_b)
                                .then_with(|| prio_b.cmp(&prio_a))
                                .then_with(|| b.num_required_tiles.cmp(&a.num_required_tiles))
                                .then_with(|| ev_b.total_cmp(&ev_a))
                        } else {
                            // When two candidates have EV within 6% of each other, use composite score of EV + win_prob + ukeire + tile priority
                            let close = (ev_a - ev_b).abs() <= top_ev * 0.06;
                            if close {
                                let score_a = (ev_a / top_ev) * 100.0
                                    + wp_a * 1.5
                                    + (a.num_required_tiles as f32) * 0.6
                                    + (prio_a as f32) * 3.0;
                                let score_b = (ev_b / top_ev) * 100.0
                                    + wp_b * 1.5
                                    + (b.num_required_tiles as f32) * 0.6
                                    + (prio_b as f32) * 3.0;
                                score_b.total_cmp(&score_a)
                            } else {
                                ev_b.total_cmp(&ev_a)
                            }
                        }
                    });

                    out.push_str("\n\n【libriichi 综合最优何切推荐榜（已综合 EV + 和牌率 + 进张广度 + 客风/孤立牌牌理）】:");
                    for (rank, c) in cands.iter().take(6).enumerate() {
                        let ev = c.exp_values.first().copied().unwrap_or(0.0);
                        let wp = c.win_probs.first().copied().unwrap_or(0.0) * 100.0;
                        let (role_str, _, _) = classify_tile(c.tile);
                        let mut variants = vec![c.tile.to_string()];
                        if mask.get(c.tile.akaize().as_usize()).copied().unwrap_or(false)
                            && c.tile.akaize() != c.tile
                        {
                            variants.push(c.tile.akaize().to_string());
                        }
                        out.push_str(&format!(
                            "\n  推荐 #{}: 切 {} [{}] -> EV={:.1}, 和率={:.1}%, 进张={}种{}枚",
                            rank + 1,
                            variants.join("/"),
                            role_str,
                            ev,
                            wp,
                            c.required_tiles.len(),
                            c.num_required_tiles
                        ));
                    }
                }
            }
        }

        // Naki (Pon / Chi) simulator: compare passing (`none`) vs calling each legal meld
        if self.last_cans.can_pon
            || self.last_cans.can_chi_low
            || self.last_cans.can_chi_mid
            || self.last_cans.can_chi_high
        {
            let (ev_none, wp_none, uke_none) = self
                .single_player_tables()
                .ok()
                .and_then(|t| {
                    t.max_ev_table.first().map(|c| {
                        (
                            c.exp_values.first().copied().unwrap_or(0.0),
                            c.win_probs.first().copied().unwrap_or(0.0) * 100.0,
                            c.num_required_tiles,
                        )
                    })
                })
                .unwrap_or((0.0, 0.0, 0));

            out.push_str(&format!(
                "\n\n【libriichi 副露(吃/碰)模拟器精确对比】:\n  - 选 none (不鸣牌): 当前向听数={}, 门前EV={:.1}, 和率={:.1}%, 进张={}枚",
                sh_now, ev_none, wp_none, uke_none
            ));

            if let Some(pai) = self.last_kawa_tile {
                let d = pai.deaka().as_u8();
                let akas = self.akas_in_hand;
                let actor = self.player_id;
                let target = self.last_cans.target_actor;

                let meld_options: [(usize, &str, bool); 4] = [
                    (41, "pon", self.last_cans.can_pon),
                    (38, "chi_low", self.last_cans.can_chi_low),
                    (39, "chi_mid", self.last_cans.can_chi_mid),
                    (40, "chi_high", self.last_cans.can_chi_high),
                ];

                for (act_idx, act_name, can_flag) in meld_options {
                    if !can_flag || !mask.get(act_idx).copied().unwrap_or(true) {
                        continue;
                    }
                    let ev_opt = match act_idx {
                        41 => {
                            let use_aka = matches!(d, 4 if akas[0])
                                || matches!(d, 13 if akas[1])
                                || matches!(d, 22 if akas[2]);
                            let consumed = if use_aka {
                                [pai.akaize(), pai.deaka()]
                            } else {
                                [pai.deaka(); 2]
                            };
                            Some(crate::mjai::Event::Pon {
                                actor,
                                target,
                                pai,
                                consumed,
                            })
                        }
                        38 => {
                            let first = pai.next();
                            let use_aka = matches!(d, 2 if akas[0])
                                || matches!(d, 11 if akas[1])
                                || matches!(d, 20 if akas[2]);
                            let consumed = if use_aka {
                                [first.akaize(), first.next().akaize()]
                            } else {
                                [first, first.next()]
                            };
                            Some(crate::mjai::Event::Chi {
                                actor,
                                target,
                                pai,
                                consumed,
                            })
                        }
                        39 => {
                            let use_aka = matches!(d, 3 if akas[0])
                                || matches!(d, 12 if akas[1])
                                || matches!(d, 21 if akas[2]);
                            let consumed = if use_aka {
                                [pai.prev().akaize(), pai.next().akaize()]
                            } else {
                                [pai.prev(), pai.next()]
                            };
                            Some(crate::mjai::Event::Chi {
                                actor,
                                target,
                                pai,
                                consumed,
                            })
                        }
                        40 => {
                            let last = pai.prev();
                            let use_aka = matches!(d, 5 | 6 if akas[0])
                                || matches!(d, 14 | 15 if akas[1])
                                || matches!(d, 23 | 24 if akas[2]);
                            let consumed = if use_aka {
                                [last.prev().akaize(), last.akaize()]
                            } else {
                                [last.prev(), last]
                            };
                            Some(crate::mjai::Event::Chi {
                                actor,
                                target,
                                pai,
                                consumed,
                            })
                        }
                        _ => None,
                    };

                    if let Some(meld_ev) = ev_opt {
                        let mut sim = self.clone();
                        if sim.update(&meld_ev).is_ok() {
                            let (_, post_mask) = sim.encode_obs(4, false);
                            let sim_tables = sim.single_player_tables().ok();
                            let best_legal = sim_tables.as_ref().and_then(|t| {
                                t.max_ev_table
                                    .iter()
                                    .filter(|c| {
                                        post_mask.get(c.tile.as_usize()).copied().unwrap_or(false)
                                            || post_mask
                                                .get(c.tile.akaize().as_usize())
                                                .copied()
                                                .unwrap_or(false)
                                    })
                                    .max_by(|a, b| {
                                        let sa = !a.shanten_down;
                                        let sb = !b.shanten_down;
                                        sa.cmp(&sb).then_with(|| {
                                            let eva = a.exp_values.first().copied().unwrap_or(0.0);
                                            let evb = b.exp_values.first().copied().unwrap_or(0.0);
                                            eva.total_cmp(&evb)
                                        })
                                    })
                            });
                            if let Some(best) = best_legal {
                                let sh_after = if best.shanten_down {
                                    sim.real_time_shanten() + 1
                                } else {
                                    sim.real_time_shanten()
                                };
                                let ev_after = best.exp_values.first().copied().unwrap_or(0.0);
                                let wp_after = best.win_probs.first().copied().unwrap_or(0.0) * 100.0;
                                let uke_after = best.num_required_tiles;
                                let is_yakuhai_pon = act_idx == 41
                                    && (pai.deaka() == self.bakaze
                                        || pai.deaka() == self.jikaze
                                        || pai.deaka().as_u8() >= 31);
                                let verdict = if sh_now == 0 && sh_after > 0 {
                                    "❌ 严禁鸣牌（已听牌，鸣牌触发喰替禁手导致破听！必选 none）"
                                } else if sh_now == 0 && ev_after <= ev_none * 1.15 {
                                    "❌ 严禁鸣牌（已听牌，鸣牌不增打点反而暴露手牌！必选 none）"
                                } else if sh_after >= sh_now {
                                    "❌ 严禁鸣牌（向听数未推进！必选 none）"
                                } else if ev_after < 75.0
                                    || (ev_none > 120.0 && ev_after < ev_none * 0.25 && !is_yakuhai_pon)
                                {
                                    "❌ 严禁鸣牌（无役副露或EV暴跌超75%！必选 none）"
                                } else if is_yakuhai_pon
                                    || ev_after >= ev_none * 0.85
                                    || (sh_after <= 1 && wp_after >= wp_none * 1.35 && ev_after >= 80.0)
                                {
                                    "✅ 推荐鸣牌（推进向听且有役/高胜率）"
                                } else {
                                    "⚠️ 倾向不鸣牌 none（虽推进向听但打点EV损失较大）"
                                };
                                out.push_str(&format!(
                                    "\n  - 选 {} (鸣牌后最优切 {}): 鸣牌后向听数={} (原={}), 鸣牌后EV={:.1} (原={:.1}), 鸣牌后和率={:.1}% (原={:.1}%), 进张={}枚 -> 判定: {}",
                                    act_name, best.tile, sh_after, sh_now, ev_after, ev_none, wp_after, wp_none, uke_after, verdict
                                ));
                            }
                        }
                    }
                }
            }
        }

        for rel in 1..4 {
            if self.riichi_declared[rel] {
                let mut safe_in_hand = Vec::new();
                for (tid, &cnt) in self.tehai.iter().enumerate() {
                    if cnt > 0 {
                        let t = must_tile!(tid);
                        if self.kawa_overview[rel].contains(&t) {
                            safe_in_hand.push(t.to_string());
                        }
                    }
                }
                out.push_str(&format!(
                    "\nopponent_riichi_genbutsu: player_rel={} IS IN RIICHI! 100%_safe_genbutsu_in_hand={:?}",
                    rel, safe_in_hand
                ));
            }
        }
        out
    }
}
