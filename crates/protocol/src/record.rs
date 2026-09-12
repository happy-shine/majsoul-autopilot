use liqi::pb;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameRecordPlayer {
    pub account_id: u32,
    pub nickname: String,
    pub seat: u32,
    pub rank: u32,
    pub score: i32,
    pub point_change: i32,
    pub is_self: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameRecordSummary {
    pub uuid: String,
    pub start_time: u64,
    pub end_time: u64,
    pub mode_id: u32,
    pub room_name: String,
    pub rank: u32,
    pub score: i32,
    pub point_change: i32,
    pub paipu_url: String,
    pub players: Vec<GameRecordPlayer>,
}

pub fn mode_id_to_room_name(mode_id: u32) -> String {
    match mode_id {
        2 => "铜之间 四人东".to_string(),
        3 => "铜之间 四人南".to_string(),
        5 => "银之间 四人东".to_string(),
        6 => "银之间 四人南".to_string(),
        8 => "金之间 四人东".to_string(),
        9 => "金之间 四人南".to_string(),
        11 => "玉之间 四人东".to_string(),
        12 => "玉之间 四人南".to_string(),
        15 => "王座之间 四人东".to_string(),
        16 => "王座之间 四人南".to_string(),
        21 => "铜之间 三人东".to_string(),
        22 => "铜之间 三人南".to_string(),
        23 => "银之间 三人东".to_string(),
        24 => "银之间 三人南".to_string(),
        25 => "金之间 三人东".to_string(),
        26 => "金之间 三人南".to_string(),
        27 => "玉之间 三人东".to_string(),
        28 => "玉之间 三人南".to_string(),
        29 => "王座之间 三人东".to_string(),
        30 => "王座之间 三人南".to_string(),
        0 => "段位战".to_string(),
        other => format!("段位战 ({other})"),
    }
}

pub fn parse_game_record_list(
    res: pb::ResGameRecordList,
    self_account_id: u32,
) -> Vec<GameRecordSummary> {
    res.record_list
        .into_iter()
        .map(|game| parse_record_game(game, self_account_id))
        .collect()
}

pub fn parse_record_game(game: pb::RecordGame, self_account_id: u32) -> GameRecordSummary {
    let mode_id = game
        .config
        .as_ref()
        .and_then(|c| c.meta.as_ref())
        .map(|m| m.mode_id)
        .unwrap_or(0);

    let room_name = mode_id_to_room_name(mode_id);

    let mut players_raw: Vec<(u32, i32, i32, i32)> = Vec::new(); // (seat, total_point, part_point_1, grading_score)
    if let Some(result) = &game.result {
        for p in &result.players {
            players_raw.push((p.seat, p.total_point, p.part_point_1, p.grading_score));
        }
    }

    // Sort by placement: total_point desc, then part_point_1 desc, then seat asc (tie break)
    players_raw.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| b.2.cmp(&a.2))
            .then_with(|| a.0.cmp(&b.0))
    });

    let mut players: Vec<GameRecordPlayer> = Vec::new();
    for (idx, &(seat, _total_point, part_point_1, grading_score)) in players_raw.iter().enumerate() {
        let rank = (idx + 1) as u32;
        let account = game.accounts.iter().find(|a| a.seat == seat);
        let robot = game.robots.iter().find(|r| r.seat == seat);

        let (account_id, nickname) = if let Some(acc) = account {
            (acc.account_id, acc.nickname.clone())
        } else if let Some(rob) = robot {
            (rob.account_id, rob.nickname.clone())
        } else {
            (0, format!("电脑 #{seat}"))
        };

        let is_self = self_account_id > 0 && account_id == self_account_id;
        let score = part_point_1;

        players.push(GameRecordPlayer {
            account_id,
            nickname,
            seat,
            rank,
            score,
            point_change: grading_score,
            is_self,
        });
    }

    let self_player = players.iter().find(|p| p.is_self).or_else(|| players.first());
    let (rank, score, point_change) = match self_player {
        Some(p) => (p.rank, p.score, p.point_change),
        None => (1, 0, 0),
    };

    let target_account_id = if self_account_id > 0 {
        self_account_id
    } else {
        self_player.map(|p| p.account_id).unwrap_or(0)
    };

    let paipu_url = if target_account_id > 0 {
        format!("https://game.maj-soul.com/1/?paipu={}_a{}", game.uuid, target_account_id)
    } else {
        format!("https://game.maj-soul.com/1/?paipu={}", game.uuid)
    };

    GameRecordSummary {
        uuid: game.uuid,
        start_time: game.start_time as u64,
        end_time: game.end_time as u64,
        mode_id,
        room_name,
        rank,
        score,
        point_change,
        paipu_url,
        players,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_record_game_ranks_correctly() {
        let game = pb::RecordGame {
            uuid: "260912-12345678".to_string(),
            start_time: 1726140000,
            end_time: 1726141800,
            config: Some(pb::GameConfig {
                meta: Some(pb::GameMetaData {
                    mode_id: 12,
                    ..Default::default()
                }),
                ..Default::default()
            }),
            accounts: vec![
                pb::record_game::AccountInfo {
                    account_id: 14244521,
                    seat: 2,
                    nickname: "SXYSS".to_string(),
                    ..Default::default()
                },
                pb::record_game::AccountInfo {
                    account_id: 20000001,
                    seat: 0,
                    nickname: "Opponent1".to_string(),
                    ..Default::default()
                },
                pb::record_game::AccountInfo {
                    account_id: 20000002,
                    seat: 1,
                    nickname: "Opponent2".to_string(),
                    ..Default::default()
                },
                pb::record_game::AccountInfo {
                    account_id: 20000003,
                    seat: 3,
                    nickname: "Opponent3".to_string(),
                    ..Default::default()
                },
            ],
            result: Some(pb::GameEndResult {
                players: vec![
                    pb::game_end_result::PlayerItem {
                        seat: 0,
                        total_point: 38500,
                        part_point_1: 38500,
                        grading_score: 85,
                        ..Default::default()
                    },
                    pb::game_end_result::PlayerItem {
                        seat: 1,
                        total_point: 24700,
                        part_point_1: 24700,
                        grading_score: -15,
                        ..Default::default()
                    },
                    pb::game_end_result::PlayerItem {
                        seat: 2,
                        total_point: 33800,
                        part_point_1: 33800,
                        grading_score: 45,
                        ..Default::default()
                    },
                    pb::game_end_result::PlayerItem {
                        seat: 3,
                        total_point: 3000,
                        part_point_1: 3000,
                        grading_score: -115,
                        ..Default::default()
                    },
                ],
            }),
            ..Default::default()
        };

        let summary = parse_record_game(game, 14244521);
        assert_eq!(summary.uuid, "260912-12345678");
        assert_eq!(summary.room_name, "玉之间 四人南");
        // seat 0: 38500 -> 1st
        // seat 2 (self): 33800 -> 2nd
        // seat 1: 24700 -> 3rd
        // seat 3: 3000 -> 4th
        assert_eq!(summary.rank, 2);
        assert_eq!(summary.score, 33800);
        assert_eq!(summary.point_change, 45);
        assert_eq!(
            summary.paipu_url,
            "https://game.maj-soul.com/1/?paipu=260912-12345678_a14244521"
        );
        assert_eq!(summary.players.len(), 4);
        assert_eq!(summary.players[0].rank, 1);
        assert_eq!(summary.players[0].nickname, "Opponent1");
        assert_eq!(summary.players[1].rank, 2);
        assert_eq!(summary.players[1].nickname, "SXYSS");
        assert!(summary.players[1].is_self);
    }
}
