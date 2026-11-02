use super::action::ActionCandidate;
use super::item::{ChiPon, KawaItem, Sutehai};
use crate::algo::agari::AgariCalculator;
use crate::algo::sp::Candidate;
use crate::hand::tiles_to_string;
use crate::tile::Tile;
use crate::{must_tile, tu8};
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

    /// Check whether `tile_deaka` is 100% safe genbutsu against player `rel`
    /// (includes `rel`'s own kawa AND any tile discarded by any player at or after `rel`'s riichi declaration).
    #[must_use]
    pub fn is_genbutsu_against(&self, rel: usize, tile_deaka: Tile) -> bool {
        if self.kawa_overview[rel]
            .iter()
            .any(|t| t.deaka() == tile_deaka)
        {
            return true;
        }
        let riichi_ts = self.kawa[rel]
            .iter()
            .enumerate()
            .find_map(|(row, item)| {
                item.as_ref()
                    .and_then(|k| k.sutehai.is_riichi.then_some(row * 4 + rel))
            });
        if let Some(r_ts) = riichi_ts {
            for p in 0..4 {
                for (row, item) in self.kawa[p].iter().enumerate() {
                    if row * 4 + p >= r_ts {
                        if let Some(k) = item {
                            if k.sutehai.tile.deaka() == tile_deaka {
                                return true;
                            }
                        }
                    }
                }
            }
        }
        false
    }

    /// Evaluate danger score (0 = 100% genbutsu, lower = safer) and label of discarding `t` against `rel`.
    #[must_use]
    pub fn eval_tile_danger_against(&self, rel: usize, t: Tile) -> (i32, &'static str) {
        let d = t.deaka();
        let tid = d.as_usize();
        let id = d.as_u8();
        if self.is_genbutsu_against(rel, d) {
            return (0, "100%安全现物");
        }
        let seen = self.tiles_seen[tid];
        let is_rel_oya = rel as u8 == self.oya;
        let oya_penalty = if is_rel_oya { 8 } else { 0 };
        let dora_penalty = if t.is_aka() || self.dora_factor[tid] > 0 {
            22
        } else {
            0
        };
        let multi_in_hand_bonus = if self.tehai[tid] >= 2 { -4 } else { 0 };

        if id >= 27 {
            let rel_jikaze = must_tile!(tu8!(E) + (rel as u8 + 4 - self.oya) % 4);
            let is_yakuhai_for_rel = d == self.bakaze || d == rel_jikaze || id >= 31;
            let (base, lbl) = match (seen, is_yakuhai_for_rel) {
                (4.., _) => (4, "绝张字牌(已见4张)"),
                (3, false) => (8, "已见3张客风(仅单骑)"),
                (3, true) => (12, "已见3张役牌(仅单骑)"),
                (2, false) => (18, "已见2张客风"),
                (2, true) => (28, "已见2张役牌"),
                (_, false) => (36, "生张客风"),
                (_, true) => (48, "生张役牌"),
            };
            return (
                (base + oya_penalty + dora_penalty + multi_in_hand_bonus).max(2),
                lbl,
            );
        }

        let suit_base = (tid / 9) * 9;
        let num = (id % 9) + 1; // 1..=9
        let is_gen = |n: u8| -> bool {
            let check_tile = must_tile!(suit_base + (n as usize - 1));
            self.is_genbutsu_against(rel, check_tile)
        };
        let is_kabe = |n: u8| -> bool { self.tiles_seen[suit_base + (n as usize - 1)] >= 4 };

        let (base, lbl) = match num {
            1 => {
                if is_gen(4) || is_kabe(2) || is_kabe(3) {
                    if seen >= 3 {
                        (10, "已见3张筋/壁19牌")
                    } else {
                        (14, "筋/壁19老头牌")
                    }
                } else if seen >= 3 {
                    (24, "已见3张无筋19牌")
                } else {
                    (34, "无筋19老头牌")
                }
            }
            9 => {
                if is_gen(6) || is_kabe(8) || is_kabe(7) {
                    if seen >= 3 {
                        (10, "已见3张筋/壁19牌")
                    } else {
                        (14, "筋/壁19老头牌")
                    }
                } else if seen >= 3 {
                    (24, "已见3张无筋19牌")
                } else {
                    (34, "无筋19老头牌")
                }
            }
            2 => {
                if is_gen(5) || is_kabe(3) {
                    (20, "筋/壁28牌")
                } else {
                    (62, "无筋28生张")
                }
            }
            8 => {
                if is_gen(5) || is_kabe(7) {
                    (20, "筋/壁28牌")
                } else {
                    (62, "无筋28生张")
                }
            }
            3 => {
                if is_gen(6) || is_kabe(4) {
                    (26, "筋/壁37牌")
                } else {
                    (74, "无筋37高危生张")
                }
            }
            7 => {
                if is_gen(4) || is_kabe(6) {
                    (26, "筋/壁37牌")
                } else {
                    (74, "无筋37高危生张")
                }
            }
            4 => {
                let low_safe = is_gen(1) || is_kabe(2) || is_kabe(3);
                let high_safe = is_gen(7) || is_kabe(5) || is_kabe(6);
                match (low_safe, high_safe) {
                    (true, true) => (24, "双筋/壁4牌"),
                    (true, false) | (false, true) => (46, "半筋4牌"),
                    (false, false) => (88, "无筋456极危生张"),
                }
            }
            5 => {
                let low_safe = is_gen(2) || is_kabe(3) || is_kabe(4);
                let high_safe = is_gen(8) || is_kabe(6) || is_kabe(7);
                match (low_safe, high_safe) {
                    (true, true) => (24, "双筋/壁5牌"),
                    (true, false) | (false, true) => (48, "半筋5牌"),
                    (false, false) => (92, "无筋5极危生张"),
                }
            }
            6 => {
                let low_safe = is_gen(3) || is_kabe(4) || is_kabe(5);
                let high_safe = is_gen(9) || is_kabe(7) || is_kabe(8);
                match (low_safe, high_safe) {
                    (true, true) => (24, "双筋/壁6牌"),
                    (true, false) | (false, true) => (46, "半筋6牌"),
                    (false, false) => (88, "无筋456极危生张"),
                }
            }
            _ => (60, "数牌"),
        };

        (
            (base + oya_penalty + dora_penalty + multi_in_hand_bonus).max(4),
            lbl,
        )
    }

    #[must_use]
    pub fn eval_tile_danger_multi(&self, threats: &[usize], t: Tile) -> (i32, &'static str) {
        if threats.is_empty() {
            return (0, "安全");
        }
        let mut max_d = -1;
        let mut sum_d = 0;
        let mut worst_lbl = "100%安全现物";
        for &rel in threats {
            let (d, lbl) = self.eval_tile_danger_against(rel, t);
            sum_d += d;
            if d > max_d {
                max_d = d;
                worst_lbl = lbl;
            }
        }
        let combined = if threats.len() > 1 {
            max_d + (sum_d - max_d) / 3
        } else {
            max_d
        };
        (combined, worst_lbl)
    }

    /// Compute exact Dama vs Riichi Ron point metrics for discarding `discard_tile` at tenpai (`shanten == 0`).
    fn eval_tenpai_dama_vs_riichi(&self, discard_tile: Tile) -> (bool, i32, i32, i32, bool) {
        let discard_tid = discard_tile.deaka().as_usize();
        if self.tehai[discard_tid] == 0 {
            return (false, 0, 0, 0, self.at_furiten);
        }
        let mut tehai_3n1 = self.tehai;
        tehai_3n1[discard_tid] -= 1;

        let mut akas = self.akas_in_hand;
        match discard_tile.as_u8() {
            tu8!(5mr) => akas[0] = false,
            tu8!(5pr) => akas[1] = false,
            tu8!(5sr) => akas[2] = false,
            _ => (),
        }
        let base_doras: u8 = self
            .dora_indicators
            .iter()
            .map(|ind| tehai_3n1[ind.next().as_usize()])
            .sum::<u8>()
            + akas.iter().filter(|&&b| b).count() as u8;

        let is_oya = self.is_oya();
        let mut is_furiten = self.at_furiten;
        let mut all_have_dama_yaku = true;
        let mut total_wait_cnt = 0i32;
        let mut min_dama_ron = i32::MAX;
        let mut weighted_dama_ron = 0i64;
        let mut weighted_riichi_ron = 0i64;

        for (tsumo_tid, &seen) in self.tiles_seen.iter().enumerate() {
            if tsumo_tid == discard_tid || tehai_3n1[tsumo_tid] == 4 {
                continue;
            }
            let mut tehai_3n2 = tehai_3n1;
            tehai_3n2[tsumo_tid] += 1;
            if crate::algo::shanten::calc_all(&tehai_3n2, self.tehai_len_div3) > -1 {
                continue;
            }
            if self.discarded_tiles[tsumo_tid] {
                is_furiten = true;
            }
            let rem = (4u8.saturating_sub(seen)) as i32;
            if rem <= 0 {
                continue;
            }
            total_wait_cnt += rem;
            let win_doras = base_doras + self.dora_factor[tsumo_tid];
            let agari_calc = AgariCalculator {
                tehai: &tehai_3n2,
                is_menzen: self.is_menzen,
                chis: &self.chis,
                pons: &self.pons,
                minkans: &self.minkans,
                ankans: &self.ankans,
                bakaze: self.bakaze.as_u8(),
                jikaze: self.jikaze.as_u8(),
                winning_tile: tsumo_tid as u8,
                is_ron: true,
            };
            if let Some(dama_agari) = agari_calc.agari(0, win_doras) {
                let pts = dama_agari.point(is_oya).ron;
                min_dama_ron = min_dama_ron.min(pts);
                weighted_dama_ron += (pts as i64) * (rem as i64);
            } else {
                all_have_dama_yaku = false;
                min_dama_ron = 0;
            }
            if let Some(riichi_agari) = agari_calc.agari(1, win_doras) {
                let pts = riichi_agari.point(is_oya).ron;
                weighted_riichi_ron += (pts as i64) * (rem as i64);
            }
        }

        if total_wait_cnt == 0 {
            return (false, 0, 0, 0, is_furiten);
        }
        let avg_dama = (weighted_dama_ron / (total_wait_cnt as i64)) as i32;
        let avg_riichi = (weighted_riichi_ron / (total_wait_cnt as i64)) as i32;
        let final_min_dama = if min_dama_ron == i32::MAX { 0 } else { min_dama_ron };
        (all_have_dama_yaku, final_min_dama, avg_dama, avg_riichi, is_furiten)
    }

    #[must_use]
    pub fn jev_context_info(&self, mask: &[bool]) -> String {
        let mut out = self.brief_info();
        let sh_now = self.real_time_shanten();

        let riichi_rels: Vec<usize> = (1..4).filter(|&r| self.riichi_declared[r]).collect();
        let any_opp_riichi = !riichi_rels.is_empty();
        let any_oya_riichi = riichi_rels.iter().any(|&r| r as u8 == self.oya);

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

            let tables_opt = self.single_player_tables().ok();
            let sp_cands: Vec<&Candidate> = tables_opt
                .as_ref()
                .map(|t| {
                    t.max_ev_table
                        .iter()
                        .filter(|c| {
                            mask.get(c.tile.as_usize()).copied().unwrap_or(false)
                                || mask.get(c.tile.akaize().as_usize()).copied().unwrap_or(false)
                        })
                        .collect()
                })
                .unwrap_or_default();

            let mut keep_cands: Vec<&Candidate> = sp_cands
                .iter()
                .copied()
                .filter(|c| !c.shanten_down)
                .collect();
            if keep_cands.is_empty() {
                keep_cands = sp_cands.clone();
            }

            let top_ev = keep_cands
                .iter()
                .map(|c| c.exp_values.first().copied().unwrap_or(0.0))
                .fold(0.0_f32, f32::max);
            let top_uke = keep_cands
                .iter()
                .map(|c| c.num_required_tiles)
                .max()
                .unwrap_or(0);

            let is_chitoi_trap = sh_now >= 2
                && keep_cands
                    .first()
                    .is_some_and(|c| c.required_tiles.len() == 7 && c.num_required_tiles <= 21);

            keep_cands.sort_by(|a, b| {
                let ev_a = a.exp_values.first().copied().unwrap_or(0.0);
                let ev_b = b.exp_values.first().copied().unwrap_or(0.0);
                let wp_a = a.win_probs.first().copied().unwrap_or(0.0) * 100.0;
                let wp_b = b.win_probs.first().copied().unwrap_or(0.0) * 100.0;
                let (_, prio_a, conn_a) = classify_tile(a.tile);
                let (_, prio_b, conn_b) = classify_tile(b.tile);
                let (dan_a, _) = self.eval_tile_danger_multi(&riichi_rels, a.tile);
                let (dan_b, _) = self.eval_tile_danger_multi(&riichi_rels, b.tile);

                if top_ev <= 1.0 {
                    b.num_required_tiles
                        .cmp(&a.num_required_tiles)
                        .then_with(|| dan_a.cmp(&dan_b))
                        .then_with(|| prio_b.cmp(&prio_a))
                } else if is_chitoi_trap && (ev_a - ev_b).abs() <= top_ev * 0.08 {
                    conn_a
                        .cmp(&conn_b)
                        .then_with(|| prio_b.cmp(&prio_a))
                        .then_with(|| b.num_required_tiles.cmp(&a.num_required_tiles))
                        .then_with(|| ev_b.total_cmp(&ev_a))
                } else {
                    let close = (ev_a - ev_b).abs() <= top_ev * 0.08;
                    if close {
                        let score_a = (ev_a / top_ev) * 100.0
                            + wp_a * 1.5
                            + (a.num_required_tiles as f32) * 0.6
                            + (prio_a as f32) * 3.0
                            - (dan_a as f32) * 0.45;
                        let score_b = (ev_b / top_ev) * 100.0
                            + wp_b * 1.5
                            + (b.num_required_tiles as f32) * 0.6
                            + (prio_b as f32) * 3.0
                            - (dan_b as f32) * 0.45;
                        score_b.total_cmp(&score_a)
                    } else {
                        ev_b.total_cmp(&ev_a)
                    }
                }
            });

            // Determine whether we should fold / mawashi against opponent riichi
            let min_keep_danger = keep_cands
                .iter()
                .map(|c| self.eval_tile_danger_multi(&riichi_rels, c.tile).0)
                .min()
                .unwrap_or(99);
            let is_desperate_all_last = self.is_all_last && self.rank == 3 && !self.is_oya();

            let should_fold_against_riichi = if !any_opp_riichi {
                false
            } else if sh_now >= 2 {
                true
            } else if sh_now == 1 {
                let high_value_push = !is_desperate_all_last
                    && self.at_turn <= 11
                    && top_uke >= 12
                    && top_ev >= if any_oya_riichi { 3200.0 } else { 2200.0 }
                    && min_keep_danger <= 75;
                let safe_mawashi_available = min_keep_danger <= 24 && top_ev >= 1100.0;
                !(high_value_push || safe_mawashi_available || is_desperate_all_last)
            } else {
                // sh_now == 0: already tenpai, only fold if discard is super dangerous & hand is tiny late
                min_keep_danger >= 68
                    && top_ev < if any_oya_riichi { 2400.0 } else { 1500.0 }
                    && self.at_turn >= 12
                    && !is_desperate_all_last
            };

            // Upgrade 1: Dedicated Riichi (`reach`) vs Dama (`切牌`) simulator when `can_riichi` is true
            let mut recommend_reach_first: Option<(String, f32, i32, i32, usize, u8)> = None;
            if self.last_cans.can_riichi && mask.get(37).copied().unwrap_or(false) {
                if let Some(&best_c) = keep_cands.first() {
                    let chosen_tile = if mask.get(best_c.tile.as_usize()).copied().unwrap_or(false) {
                        best_c.tile
                    } else {
                        best_c.tile.akaize()
                    };
                    let ev_riichi = best_c.exp_values.first().copied().unwrap_or(0.0);
                    let ukeire = best_c.num_required_tiles;
                    let wait_kinds = best_c.required_tiles.len();
                    let (has_dama_yaku, min_dama_ron, avg_dama_ron, avg_riichi_ron, is_furiten_wait) =
                        self.eval_tenpai_dama_vs_riichi(chosen_tile);
                    let (discard_danger, danger_lbl) =
                        self.eval_tile_danger_multi(&riichi_rels, chosen_tile);

                    let mangan_thresh = if self.is_oya() { 11600 } else { 7700 };
                    let all_last_dama_lock = self.is_all_last
                        && has_dama_yaku
                        && min_dama_ron >= 1000
                        && ((self.rank == 0 && self.scores[0] >= 30000)
                            || (self.rank == 3
                                && self.scores[0]
                                    + min_dama_ron
                                    + (self.kyotaku as i32) * 1000
                                    + (self.honba as i32) * 300
                                    > self.scores[1..].iter().copied().min().unwrap_or(0)));

                    let should_riichi = if should_fold_against_riichi || self.tiles_left < 4 {
                        false
                    } else if is_furiten_wait && (has_dama_yaku || ukeire < 6 || self.at_turn > 9) {
                        false
                    } else if has_dama_yaku && min_dama_ron >= mangan_thresh {
                        false
                    } else if all_last_dama_lock {
                        false
                    } else if any_opp_riichi
                        && has_dama_yaku
                        && min_dama_ron >= 2000
                        && (ukeire <= 4 || self.at_turn >= 11 || discard_danger >= 45)
                    {
                        false
                    } else if any_opp_riichi
                        && !has_dama_yaku
                        && (ukeire <= 2 || (self.at_turn >= 13 && ukeire <= 3) || discard_danger >= 68)
                    {
                        false
                    } else if self.tiles_left <= 8 && has_dama_yaku && ukeire <= 3 {
                        false
                    } else {
                        true
                    };

                    let verdict_str = if should_fold_against_riichi {
                        "❌ 严禁立直（他家已立直且宣言牌极危/打点不足，应弃和切安全现物！）"
                    } else if should_riichi {
                        "✅ 强烈推荐立直（必选 reach！宣言后下步切立直宣言牌，切勿直接选牌名否则变默听！）"
                    } else {
                        "✅ 推荐有役默听（不选 reach，直接切出听牌切牌默听偷现）"
                    };

                    out.push_str(&format!(
                        "\n\n【libriichi 立直(reach) vs 默听(切牌) 精确对比】:\n  - 最优听牌切牌: 切 {} (进张={}种{}枚, 宣言牌安全度={}[危险度={}])\n  - 选 reach (宣言立直): 立直综合EV={:.1}, 立直基础荣和均打点={} (含立直+1番, 尚未计一发与里宝牌加成)\n  - 选 直接切 {} (不立直默听): 全部进张有役={}, 默听最低荣和={}, 默听平均荣和={}, 振听={}\n  => 【立直最终结论】: {}",
                        chosen_tile,
                        wait_kinds,
                        ukeire,
                        danger_lbl,
                        discard_danger,
                        ev_riichi,
                        avg_riichi_ron,
                        chosen_tile,
                        has_dama_yaku,
                        min_dama_ron,
                        avg_dama_ron,
                        is_furiten_wait,
                        verdict_str
                    ));

                    if should_riichi {
                        recommend_reach_first = Some((
                            chosen_tile.to_string(),
                            ev_riichi,
                            avg_riichi_ron,
                            avg_dama_ron,
                            wait_kinds,
                            ukeire,
                        ));
                    }
                }
            }

            // Upgrade 2: Unified Attack / Mawashi / Defense Discard Recommendation Table
            let disc_aka = self.discard_candidates_aka();
            let mut all_legal_tiles: Vec<Tile> = Vec::new();
            for (idx, &flag) in disc_aka.iter().enumerate() {
                if flag && mask.get(idx).copied().unwrap_or(false) {
                    let t = must_tile!(idx);
                    if !all_legal_tiles.iter().any(|x| x.deaka() == t.deaka()) {
                        all_legal_tiles.push(t);
                    }
                }
            }

            if should_fold_against_riichi {
                // Sort ALL legal tiles in hand by lowest danger against riichi opponents
                all_legal_tiles.sort_by(|&ta, &tb| {
                    let (da, _) = self.eval_tile_danger_multi(&riichi_rels, ta);
                    let (db, _) = self.eval_tile_danger_multi(&riichi_rels, tb);
                    let cand_a = keep_cands.iter().find(|c| c.tile.deaka() == ta.deaka());
                    let cand_b = keep_cands.iter().find(|c| c.tile.deaka() == tb.deaka());
                    let keep_a = cand_a.is_some();
                    let keep_b = cand_b.is_some();
                    let eva = cand_a
                        .and_then(|c| c.exp_values.first().copied())
                        .unwrap_or(0.0);
                    let evb = cand_b
                        .and_then(|c| c.exp_values.first().copied())
                        .unwrap_or(0.0);
                    let cnt_a = self.tehai[ta.deaka().as_usize()];
                    let cnt_b = self.tehai[tb.deaka().as_usize()];

                    // Group 0 (100% genbutsu) strictly first; within close danger (<=6 diff), prefer keeping shanten or pair/triplet
                    if da == 0 || db == 0 {
                        da.cmp(&db)
                            .then_with(|| keep_b.cmp(&keep_a))
                            .then_with(|| cnt_b.cmp(&cnt_a))
                            .then_with(|| evb.total_cmp(&eva))
                    } else if (da - db).abs() <= 6 {
                        keep_b
                            .cmp(&keep_a)
                            .then_with(|| da.cmp(&db))
                            .then_with(|| cnt_b.cmp(&cnt_a))
                            .then_with(|| evb.total_cmp(&eva))
                    } else {
                        da.cmp(&db)
                    }
                });

                out.push_str("\n\n【libriichi 综合最优何切推荐榜（他家已立直·弃和/兜牌安全度优先模式）】:");
                for (rank, &t) in all_legal_tiles.iter().take(6).enumerate() {
                    let (dan, dan_lbl) = self.eval_tile_danger_multi(&riichi_rels, t);
                    let cand = sp_cands.iter().find(|c| c.tile.deaka() == t.deaka());
                    let (ev, wp, uke_k, uke_n, sh_tag) = if let Some(c) = cand {
                        (
                            c.exp_values.first().copied().unwrap_or(0.0),
                            c.win_probs.first().copied().unwrap_or(0.0) * 100.0,
                            c.required_tiles.len(),
                            c.num_required_tiles,
                            if c.shanten_down { "退向安牌" } else { "保向听兜牌" },
                        )
                    } else {
                        (0.0, 0.0, 0, 0, "弃和拆牌")
                    };
                    let mut variants = vec![t.deaka().to_string()];
                    if mask.get(t.akaize().as_usize()).copied().unwrap_or(false)
                        && t.akaize() != t.deaka()
                    {
                        if !mask.get(t.deaka().as_usize()).copied().unwrap_or(false) {
                            variants = vec![t.akaize().to_string()];
                        } else {
                            variants.push(t.akaize().to_string());
                        }
                    }
                    out.push_str(&format!(
                        "\n  推荐 #{}: 切 {} [{} | 危险度={} | {}] -> EV={:.1}, 和率={:.1}%, 进张={}种{}枚",
                        rank + 1,
                        variants.join("/"),
                        dan_lbl,
                        dan,
                        sh_tag,
                        ev,
                        wp,
                        uke_k,
                        uke_n
                    ));
                }
            } else if !keep_cands.is_empty() {
                if any_opp_riichi && sh_now == 1 && min_keep_danger <= 24 {
                    // Safe mawashi at 1-shanten: sort safe keep-shanten tiles ahead of dangerous ones
                    keep_cands.sort_by(|a, b| {
                        let (da, _) = self.eval_tile_danger_multi(&riichi_rels, a.tile);
                        let (db, _) = self.eval_tile_danger_multi(&riichi_rels, b.tile);
                        let safe_a = da <= 26;
                        let safe_b = db <= 26;
                        let eva = a.exp_values.first().copied().unwrap_or(0.0);
                        let evb = b.exp_values.first().copied().unwrap_or(0.0);
                        safe_b
                            .cmp(&safe_a)
                            .then_with(|| evb.total_cmp(&eva))
                            .then_with(|| da.cmp(&db))
                    });
                }

                out.push_str("\n\n【libriichi 综合最优何切推荐榜（已综合 EV + 和牌率 + 进张广度 + 攻防牌理）】:");
                let mut rank_offset = 0usize;
                if let Some((ref r_tile, r_ev, r_ron, d_ron, r_kinds, r_uke)) = recommend_reach_first {
                    out.push_str(&format!(
                        "\n  推荐 #1: reach [宣言立直（必选 reach！下一步再切 {}）] -> 立直EV={:.1}, 立直均荣和={}, 默听均荣和={}, 进张={}种{}枚",
                        r_tile, r_ev, r_ron, d_ron, r_kinds, r_uke
                    ));
                    rank_offset = 1;
                }
                for (idx, c) in keep_cands.iter().take(6 - rank_offset).enumerate() {
                    let ev = c.exp_values.first().copied().unwrap_or(0.0);
                    let wp = c.win_probs.first().copied().unwrap_or(0.0) * 100.0;
                    let (role_str, _, _) = classify_tile(c.tile);
                    let (dan, dan_lbl) = self.eval_tile_danger_multi(&riichi_rels, c.tile);
                    let mut variants = Vec::new();
                    if mask.get(c.tile.deaka().as_usize()).copied().unwrap_or(false) {
                        variants.push(c.tile.deaka().to_string());
                    }
                    if mask.get(c.tile.akaize().as_usize()).copied().unwrap_or(false)
                        && c.tile.akaize() != c.tile.deaka()
                    {
                        variants.push(c.tile.akaize().to_string());
                    }
                    if variants.is_empty() {
                        variants.push(c.tile.to_string());
                    }
                    let safety_note = if any_opp_riichi {
                        format!(" | 对立直:{}(危={})", dan_lbl, dan)
                    } else {
                        String::new()
                    };
                    let dama_note = if recommend_reach_first.is_some() {
                        " (注:直接选此牌代表放弃立直转为默听!)"
                    } else {
                        ""
                    };
                    out.push_str(&format!(
                        "\n  推荐 #{}: 切 {} [{}{}] -> EV={:.1}, 和率={:.1}%, 进张={}种{}枚{}",
                        idx + 1 + rank_offset,
                        variants.join("/"),
                        role_str,
                        safety_note,
                        ev,
                        wp,
                        c.required_tiles.len(),
                        c.num_required_tiles,
                        dama_note
                    ));
                }
            }
        }

        // Upgrade 3: Calibrated Naki (Pon / Chi / Kan) simulator
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

            let mut best_recommended_meld: Option<(&str, f32)> = None;

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
                                        let (da, _) = self.eval_tile_danger_multi(&riichi_rels, a.tile);
                                        let (db, _) = self.eval_tile_danger_multi(&riichi_rels, b.tile);
                                        sa.cmp(&sb).then_with(|| {
                                            let eva = a.exp_values.first().copied().unwrap_or(0.0);
                                            let evb = b.exp_values.first().copied().unwrap_or(0.0);
                                            if any_opp_riichi && (eva - evb).abs() <= 400.0 {
                                                db.cmp(&da)
                                            } else {
                                                eva.total_cmp(&evb)
                                            }
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
                                let (post_danger, _) =
                                    self.eval_tile_danger_multi(&riichi_rels, best.tile);
                                let is_yakuhai_pon = act_idx == 41
                                    && (pai.deaka() == self.bakaze
                                        || pai.deaka() == self.jikaze
                                        || pai.deaka().as_u8() >= 31);

                                let (is_rec, verdict) = if sh_now == 0 && sh_after > 0 {
                                    (false, "❌ 严禁鸣牌（已听牌，鸣牌触发喰替禁手导致破听！必选 none）")
                                } else if sh_now == 0 && self.is_menzen {
                                    (false, "❌ 严禁鸣牌（已门前清听牌，鸣牌丧失立直与门清自摸！必选 none）")
                                } else if sh_now == 0 && (ev_after <= ev_none * 1.25 || uke_after < uke_none) {
                                    (false, "❌ 严禁鸣牌（已听牌，鸣牌未显著提升打点与进张！必选 none）")
                                } else if sh_now > 0 && sh_after >= sh_now {
                                    (false, "❌ 严禁鸣牌（向听数未推进！必选 none）")
                                } else if any_opp_riichi {
                                    if sh_after >= 2 {
                                        (false, "❌ 严禁鸣牌（他家已立直且鸣牌后仍>=2向听，短手牌极易放铳！必选 none）")
                                    } else if sh_after == 1 {
                                        if ev_after >= 2600.0 && wp_after >= 24.0 && post_danger <= 22 {
                                            (true, "✅ 推荐鸣牌（高打点1向听且鸣后切安全牌）")
                                        } else {
                                            (false, "❌ 严禁鸣牌（他家已立直，1向听打点不足或鸣后切牌危险！必选 none）")
                                        }
                                    } else if ev_after >= 1300.0 && post_danger <= 35
                                        || (ev_after >= 3600.0 && post_danger <= 65)
                                    {
                                        (true, "✅ 推荐鸣牌（顶立直一鸣听牌且打点/切牌安全度达标）")
                                    } else {
                                        (false, "❌ 严禁鸣牌（他家已立直，无役/低打点或鸣后需切高危生张！必选 none）")
                                    }
                                } else if sh_after == 0 {
                                    // Calling into immediate Tenpai (no opponent riichi)
                                    if ev_after >= 180.0 {
                                        if self.is_menzen
                                            && self.at_turn <= 7
                                            && ev_none >= ev_after * 1.85
                                            && wp_none >= 32.0
                                        {
                                            (false, "⚠️ 倾向不鸣牌 none（早巡门前好形高打点1向听，门清立直期望远高于副露）")
                                        } else {
                                            (true, "✅ 推荐鸣牌（一鸣直接有役听牌，抢和率与速度极佳）")
                                        }
                                    } else if self.tiles_left <= 12 && ev_none < 250.0 && uke_after >= 4 {
                                        (true, "✅ 推荐鸣牌（尾巡形式听牌，争夺流局听牌罚符/连庄）")
                                    } else {
                                        (false, "❌ 严禁鸣牌（无役形式听牌且巡目尚早，无法和牌！必选 none）")
                                    }
                                } else if sh_after == 1 {
                                    // Calling from 2-shanten to 1-shanten (no opponent riichi)
                                    if is_yakuhai_pon {
                                        if self.is_menzen
                                            && self.at_turn <= 5
                                            && ev_none >= ev_after * 1.50
                                            && wp_none >= 26.0
                                        {
                                            (false, "⚠️ 倾向不鸣牌 none（早巡门前高打点好牌，不急于碰单番役牌）")
                                        } else if ev_after >= 280.0 {
                                            (true, "✅ 推荐鸣牌（役牌碰推进至1向听，有役且速度大幅提升）")
                                        } else {
                                            (false, "⚠️ 倾向不鸣牌 none（手牌过碎或打点过低）")
                                        }
                                    } else if ev_after < 520.0
                                        || (ev_none > 1200.0 && ev_after < ev_none * 0.45)
                                    {
                                        (false, "❌ 严禁鸣牌（无役或1000点低打点2向听副露，破坏门清与防守！必选 none）")
                                    } else if ev_after >= ev_none * 0.88
                                        || (wp_after >= wp_none * 1.40 && ev_after >= 700.0)
                                        || (!self.is_menzen && wp_after >= wp_none * 1.20 && ev_after >= 520.0)
                                    {
                                        (true, "✅ 推荐鸣牌（高效率有役1向听推进）")
                                    } else {
                                        (false, "⚠️ 倾向不鸣牌 none（门前立直期望更高，副露打点损失较大）")
                                    }
                                } else if sh_after == 2 {
                                    // Calling from 3-shanten to 2-shanten
                                    if is_yakuhai_pon
                                        && (ev_after >= 550.0
                                            || (!self.is_menzen && ev_after >= 380.0)
                                            || (self.doras_owned[0] >= 2 && ev_after >= 420.0)
                                            || (self.at_turn >= 6 && ev_after >= 400.0 && wp_after >= wp_none * 1.35))
                                    {
                                        (true, "✅ 推荐鸣牌（役牌碰推进至2向听，锁役提速）")
                                    } else if !is_yakuhai_pon
                                        && self.doras_owned[0] >= 3
                                        && ev_after >= 1500.0
                                        && ev_after >= ev_none * 1.15
                                        && wp_after >= wp_none * 1.35
                                    {
                                        (true, "✅ 推荐鸣牌（多宝牌高打点强行提速）")
                                    } else {
                                        (false, "❌ 严禁鸣牌（2向听以上远巡副露打点低且严重削弱防守！必选 none）")
                                    }
                                } else {
                                    (false, "❌ 严禁鸣牌（鸣牌后仍>=3向听，手牌过碎严禁副露！必选 none）")
                                };

                                if is_rec {
                                    let score = ev_after + wp_after * 15.0;
                                    if best_recommended_meld.is_none_or(|(_, s)| score > s) {
                                        best_recommended_meld = Some((act_name, score));
                                    }
                                }

                                out.push_str(&format!(
                                    "\n  - 选 {} (鸣牌后最优切 {}): 鸣牌后向听数={} (原={}), 鸣牌后EV={:.1} (原={:.1}), 鸣牌后和率={:.1}% (原={:.1}%), 进张={}枚 -> 判定: {}",
                                    act_name, best.tile, sh_after, sh_now, ev_after, ev_none, wp_after, wp_none, uke_after, verdict
                                ));
                            }
                        }
                    }
                }
            }

            if let Some((best_meld_name, _)) = best_recommended_meld {
                out.push_str(&format!(
                    "\n  => 【副露最终结论】: ✅ 推荐鸣牌，请直接选择 {}（优于选 none）",
                    best_meld_name
                ));
            } else {
                out.push_str(
                    "\n  => 【副露最终结论】: ❌ 严禁鸣牌，100%必选 none（跳过不鸣牌，保持门前清与防守安全）",
                );
            }
        }

        // Kan (Ankan / Kakan / Daiminkan) simulator when mask[42] is true
        if (self.last_cans.can_ankan || self.last_cans.can_kakan || self.last_cans.can_daiminkan)
            && mask.get(42).copied().unwrap_or(false)
        {
            if self.last_cans.can_daiminkan {
                out.push_str("\n\n【libriichi 杠牌(kan)评估】: ❌ 严禁大明杠（必选 none！大明杠白送三家新宝牌与里宝牌且不进向听）");
            } else if any_opp_riichi {
                out.push_str("\n\n【libriichi 杠牌(kan)评估】: ❌ 严禁杠牌（他家已立直，暗杠/加杠会翻出新宝牌与里宝牌送分！请勿选 kan）");
            } else if self.last_cans.can_ankan {
                if let Some(&t) = self.ankan_candidates.first() {
                    let mut sim = self.clone();
                    let ev = crate::mjai::Event::Ankan {
                        actor: self.player_id,
                        consumed: [t.akaize(), t, t, t],
                    };
                    if sim.update(&ev).is_ok() && sim.real_time_shanten() <= sh_now && sh_now <= 1 {
                        out.push_str(&format!(
                            "\n\n【libriichi 杠牌(kan)评估】: ✅ 可选暗杠 {} (不破向听且增加符数与新宝牌)",
                            t
                        ));
                    } else {
                        out.push_str(&format!(
                            "\n\n【libriichi 杠牌(kan)评估】: ❌ 严禁暗杠 {} (破坏顺子搭子或向听过远！请勿选 kan)",
                            t
                        ));
                    }
                }
            } else if self.last_cans.can_kakan {
                if sh_now <= 1 {
                    out.push_str("\n\n【libriichi 杠牌(kan)评估】: ✅ 可选加杠 (1向听以内无人立直可加杠补牌)");
                } else {
                    out.push_str("\n\n【libriichi 杠牌(kan)评估】: ❌ 严禁加杠 (向听数>=2加杠白送对手新宝牌！请勿选 kan)");
                }
            }
        }

        for &rel in &riichi_rels {
            let mut safe_in_hand = Vec::new();
            for (tid, &cnt) in self.tehai.iter().enumerate() {
                if cnt > 0 {
                    let t = must_tile!(tid);
                    if self.is_genbutsu_against(rel, t) {
                        safe_in_hand.push(t.to_string());
                    }
                }
            }
            out.push_str(&format!(
                "\nopponent_riichi_genbutsu: player_rel={} (is_oya={}) IS IN RIICHI! 100%_safe_genbutsu_in_hand={:?} | should_fold={}",
                rel,
                rel as u8 == self.oya,
                safe_in_hand,
                if sh_now >= 2 {
                    true
                } else {
                    sh_now == 1 && self.at_turn >= 12
                }
            ));
        }
        out
    }
}
