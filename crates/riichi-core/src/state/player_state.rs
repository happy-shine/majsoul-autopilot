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
    pub fn jev_context_info(&self) -> String {
        let mut out = self.brief_info();
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
            out.push_str(&format!("\nunconditional_tenpai_discards (有役默听真听牌切牌): {:?}", tiles));
        }
        if self.last_cans.can_pon {
            if let Some(kawa_t) = self.last_kawa_tile {
                let d = kawa_t.deaka();
                let is_yakuhai = d == self.bakaze || d == self.jikaze || d.as_u8() >= 31;
                out.push_str(&format!(
                    "\npon_target_tile: {} (is_1han_yakuhai_役牌={})",
                    kawa_t, is_yakuhai
                ));
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
