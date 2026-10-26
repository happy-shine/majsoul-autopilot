use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BotAction {
    Dahai { tile: String, tsumogiri: bool },
    None,
    Reach { tile: String, tsumogiri: bool },
    Chi { consumed: Vec<String> },
    Pon { consumed: Vec<String> },
    Daiminkan { consumed: Vec<String> },
    Ankan { consumed: Vec<String> },
    Kakan { tile: String },
    Hora { tsumo: bool },
    Ryukyoku,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationContext {
    pub source: String,
    pub seat: u32,
    pub received_key: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingAction {
    pub action: BotAction,
    pub context: Option<OperationContext>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Operation {
    pub r#type: u32,
    pub combination: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionState {
    pub current_context: Option<OperationContext>,
    pub operations: Vec<Operation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RpcPlan {
    InputOperation {
        r#type: u32,
        tile: Option<String>,
        moqie: bool,
        timeuse: u32,
    },
    Skip,
    IgnoreStale,
    RefuseNoDiscardWindow,
    ChiPengGang {
        r#type: u32,
        index: u32,
        timeuse: u32,
    },
}

pub const OP_DISCARD: u32 = 1;
pub const OP_CHI: u32 = 2;
pub const OP_PENG: u32 = 3;
pub const OP_AN_GANG: u32 = 4;
pub const OP_MING_GANG: u32 = 5;
pub const OP_JIA_GANG: u32 = 6;
pub const OP_LIQI: u32 = 7;
pub const OP_ZIMO: u32 = 8;
pub const OP_HU: u32 = 9;
pub const OP_LIU_JU: u32 = 10;

pub fn plan_action(pending: &PendingAction, state: &ActionState) -> RpcPlan {
    if let Some(expected) = &pending.context {
        if state.current_context.as_ref() != Some(expected) {
            return RpcPlan::IgnoreStale;
        }
    }

    match &pending.action {
        BotAction::Dahai { tile, tsumogiri } => {
            if !state.operations.iter().any(|op| op.r#type == OP_DISCARD) {
                return RpcPlan::RefuseNoDiscardWindow;
            }
            RpcPlan::InputOperation {
                r#type: OP_DISCARD,
                tile: Some(mjai_tile_to_ms(tile)),
                moqie: *tsumogiri,
                timeuse: 3,
            }
        }
        BotAction::None => RpcPlan::Skip,
        BotAction::Reach { tile, tsumogiri } => {
            let requested_tile = mjai_tile_to_ms(tile);
            let valid_tiles = state
                .operations
                .iter()
                .find(|op| op.r#type == OP_LIQI)
                .map(|op| op.combination.as_slice())
                .unwrap_or(&[]);
            let (tile, _) = select_riichi_declaration_tile(&requested_tile, valid_tiles);
            RpcPlan::InputOperation {
                r#type: OP_LIQI,
                tile: Some(tile),
                moqie: *tsumogiri,
                timeuse: 3,
            }
        }
        BotAction::Chi { consumed } => RpcPlan::ChiPengGang {
            r#type: OP_CHI,
            index: select_chi_combination_index(consumed, &state.operations),
            timeuse: 3,
        },
        BotAction::Pon { .. } => RpcPlan::ChiPengGang {
            r#type: OP_PENG,
            index: 0,
            timeuse: 3,
        },
        BotAction::Daiminkan { .. } => RpcPlan::ChiPengGang {
            r#type: OP_MING_GANG,
            index: 0,
            timeuse: 3,
        },
        BotAction::Ankan { consumed } => RpcPlan::InputOperation {
            r#type: OP_AN_GANG,
            tile: consumed.first().map(|tile| mjai_tile_to_ms(tile)),
            moqie: false,
            timeuse: 3,
        },
        BotAction::Kakan { tile } => RpcPlan::InputOperation {
            r#type: OP_JIA_GANG,
            tile: Some(mjai_tile_to_ms(tile)),
            moqie: false,
            timeuse: 3,
        },
        BotAction::Hora { tsumo } => RpcPlan::InputOperation {
            r#type: if *tsumo { OP_ZIMO } else { OP_HU },
            tile: None,
            moqie: false,
            timeuse: 1,
        },
        BotAction::Ryukyoku => RpcPlan::InputOperation {
            r#type: OP_LIU_JU,
            tile: None,
            moqie: false,
            timeuse: 1,
        },
    }
}

pub fn select_riichi_declaration_tile(
    model_tile: &str,
    valid_tiles: &[String],
) -> (String, &'static str) {
    // Keep this aligned with Python's _select_riichi_declaration_tile.
    if valid_tiles.is_empty() {
        return (model_tile.to_string(), "model-no-candidates");
    }
    if valid_tiles.iter().any(|tile| tile == model_tile) {
        return (model_tile.to_string(), "model-exact");
    }

    let model_kind = candidate_kind(model_tile);
    // The server lists one tile per kind and shows the red five whenever the hand holds one,
    // even if a plain five is in hand too. The model only picks a plain five when it holds
    // one, so keep it and save the red dora.
    if is_plain_five(model_tile)
        && valid_tiles
            .iter()
            .any(|tile| candidate_kind(tile) == model_kind)
    {
        return (model_tile.to_string(), "model-plain-five");
    }
    for tile in valid_tiles {
        if candidate_kind(tile) == model_kind {
            return (tile.clone(), "model-red-equivalent");
        }
    }

    (valid_tiles[0].clone(), "fallback-first-candidate")
}

pub fn select_chi_combination_index(consumed: &[String], operations: &[Operation]) -> u32 {
    let Some(chi_op) = operations.iter().find(|op| op.r#type == OP_CHI) else {
        return 0;
    };
    let mut consumed_sorted = consumed
        .iter()
        .map(|tile| mjai_tile_to_ms(tile))
        .collect::<Vec<_>>();
    consumed_sorted.sort();

    for (idx, combination) in chi_op.combination.iter().enumerate() {
        let mut combo_sorted = combination
            .split('|')
            .map(str::to_string)
            .collect::<Vec<_>>();
        combo_sorted.sort();
        if combo_sorted == consumed_sorted {
            return idx as u32;
        }
    }
    0
}

fn is_plain_five(tile: &str) -> bool {
    tile.len() == 2 && tile.starts_with('5') && matches!(tile.chars().nth(1), Some('m' | 'p' | 's'))
}

pub fn candidate_kind(tile: &str) -> String {
    if tile.len() == 2
        && tile.starts_with('0')
        && matches!(tile.chars().nth(1), Some('m' | 'p' | 's'))
    {
        format!("5{}", tile.chars().nth(1).unwrap())
    } else {
        tile.to_string()
    }
}

fn mjai_tile_to_ms(tile: &str) -> String {
    match tile {
        "5mr" => "0m",
        "5pr" => "0p",
        "5sr" => "0s",
        "E" => "1z",
        "S" => "2z",
        "W" => "3z",
        "N" => "4z",
        "P" => "5z",
        "F" => "6z",
        "C" => "7z",
        other => other,
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(key: u64) -> OperationContext {
        OperationContext {
            source: "ActionDealTile".to_string(),
            seat: 0,
            received_key: key,
        }
    }

    #[test]
    fn stale_operation_window_is_ignored_before_rpc() {
        let plan = plan_action(
            &PendingAction {
                action: BotAction::Dahai {
                    tile: "1m".to_string(),
                    tsumogiri: false,
                },
                context: Some(ctx(1)),
            },
            &ActionState {
                current_context: Some(ctx(2)),
                operations: vec![Operation {
                    r#type: OP_DISCARD,
                    combination: vec![],
                }],
            },
        );
        assert_eq!(plan, RpcPlan::IgnoreStale);
    }

    #[test]
    fn discard_without_discard_operation_window_is_refused() {
        let plan = plan_action(
            &PendingAction {
                action: BotAction::Dahai {
                    tile: "1m".to_string(),
                    tsumogiri: false,
                },
                context: Some(ctx(1)),
            },
            &ActionState {
                current_context: Some(ctx(1)),
                operations: vec![],
            },
        );
        assert_eq!(plan, RpcPlan::RefuseNoDiscardWindow);
    }

    #[test]
    fn riichi_uses_model_tile_when_candidate_matches() {
        let valid = vec!["1m".to_string(), "3p".to_string()];
        assert_eq!(
            select_riichi_declaration_tile("3p", &valid),
            ("3p".to_string(), "model-exact")
        );
    }

    #[test]
    fn riichi_keeps_plain_five_when_server_lists_red_five() {
        let valid = vec!["0m".to_string(), "3p".to_string()];
        assert_eq!(
            select_riichi_declaration_tile("5m", &valid),
            ("5m".to_string(), "model-plain-five")
        );
    }

    #[test]
    fn riichi_uses_red_equivalent_candidate() {
        let valid = vec!["5m".to_string(), "3p".to_string()];
        assert_eq!(
            select_riichi_declaration_tile("0m", &valid),
            ("5m".to_string(), "model-red-equivalent")
        );
    }

    #[test]
    fn riichi_matches_python_first_candidate_when_model_tile_is_invalid() {
        let valid = vec!["1m".to_string(), "3p".to_string()];
        assert_eq!(
            select_riichi_declaration_tile("9s", &valid),
            ("1m".to_string(), "fallback-first-candidate")
        );
    }

    #[test]
    fn reach_action_plans_liqi_operation_with_selected_tile() {
        let plan = plan_action(
            &PendingAction {
                action: BotAction::Reach {
                    tile: "5m".to_string(),
                    tsumogiri: true,
                },
                context: Some(ctx(1)),
            },
            &ActionState {
                current_context: Some(ctx(1)),
                operations: vec![Operation {
                    r#type: OP_LIQI,
                    combination: vec!["0m".to_string(), "3p".to_string()],
                }],
            },
        );
        assert_eq!(
            plan,
            RpcPlan::InputOperation {
                r#type: OP_LIQI,
                tile: Some("5m".to_string()),
                moqie: true,
                timeuse: 3
            }
        );
    }

    #[test]
    fn chi_action_selects_matching_combination_index() {
        let plan = plan_action(
            &PendingAction {
                action: BotAction::Chi {
                    consumed: vec!["4m".to_string(), "2m".to_string()],
                },
                context: Some(ctx(1)),
            },
            &ActionState {
                current_context: Some(ctx(1)),
                operations: vec![Operation {
                    r#type: OP_CHI,
                    combination: vec!["1m|2m".to_string(), "2m|4m".to_string()],
                }],
            },
        );
        assert_eq!(
            plan,
            RpcPlan::ChiPengGang {
                r#type: OP_CHI,
                index: 1,
                timeuse: 3
            }
        );
    }

    #[test]
    fn pon_and_daiminkan_use_chi_peng_gang_rpc() {
        let state = ActionState {
            current_context: Some(ctx(1)),
            operations: vec![],
        };
        let pon = plan_action(
            &PendingAction {
                action: BotAction::Pon {
                    consumed: vec!["5p".to_string(), "5p".to_string()],
                },
                context: Some(ctx(1)),
            },
            &state,
        );
        assert_eq!(
            pon,
            RpcPlan::ChiPengGang {
                r#type: OP_PENG,
                index: 0,
                timeuse: 3
            }
        );

        let daiminkan = plan_action(
            &PendingAction {
                action: BotAction::Daiminkan {
                    consumed: vec!["E".to_string(), "E".to_string(), "E".to_string()],
                },
                context: Some(ctx(1)),
            },
            &state,
        );
        assert_eq!(
            daiminkan,
            RpcPlan::ChiPengGang {
                r#type: OP_MING_GANG,
                index: 0,
                timeuse: 3
            }
        );
    }

    #[test]
    fn ankan_and_kakan_use_self_operation_rpc() {
        let state = ActionState {
            current_context: Some(ctx(1)),
            operations: vec![],
        };
        let ankan = plan_action(
            &PendingAction {
                action: BotAction::Ankan {
                    consumed: vec![
                        "5mr".to_string(),
                        "5m".to_string(),
                        "5m".to_string(),
                        "5m".to_string(),
                    ],
                },
                context: Some(ctx(1)),
            },
            &state,
        );
        assert_eq!(
            ankan,
            RpcPlan::InputOperation {
                r#type: OP_AN_GANG,
                tile: Some("0m".to_string()),
                moqie: false,
                timeuse: 3
            }
        );

        let kakan = plan_action(
            &PendingAction {
                action: BotAction::Kakan {
                    tile: "5p".to_string(),
                },
                context: Some(ctx(1)),
            },
            &state,
        );
        assert_eq!(
            kakan,
            RpcPlan::InputOperation {
                r#type: OP_JIA_GANG,
                tile: Some("5p".to_string()),
                moqie: false,
                timeuse: 3
            }
        );
    }
}
