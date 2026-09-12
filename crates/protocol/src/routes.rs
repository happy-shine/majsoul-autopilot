use liqi::codec::{encode_blocks, ProtoBlock};

pub const DEFAULT_ROUTE_ID: &str = "route-2";
pub const LOBBY_ROUTE_IDS: [&str; 5] = ["route-2", "route-3", "route-4", "route-5", "route-6"];
pub const GAME_ROUTE_IDS: [&str; 5] = ["route-6", "route-5", "route-4", "route-3", "route-2"];

pub fn route_ws_url(route_id: &str, tail: &str) -> String {
    let port = if route_id == "route-3" { 8443 } else { 443 };
    format!(
        "wss://{route_id}.maj-soul.com:{port}/{}",
        tail.trim_matches('/')
    )
}

pub fn lobby_ws_url_candidates() -> Vec<String> {
    LOBBY_ROUTE_IDS
        .iter()
        .map(|route_id| route_ws_url(route_id, "gateway"))
        .collect()
}

pub const CLIENT_PLATFORM: &str = "Web";

pub fn route_body(kind: u64, route_id: &str, timestamp_ms: u64) -> Vec<u8> {
    encode_blocks(&[
        ProtoBlock::Varint { id: 2, value: kind },
        ProtoBlock::Bytes {
            id: 3,
            data: route_id.as_bytes().to_vec(),
        },
        ProtoBlock::Varint {
            id: 4,
            value: timestamp_ms,
        },
        ProtoBlock::Bytes {
            id: 6,
            data: CLIENT_PLATFORM.as_bytes().to_vec(),
        },
    ])
}

pub fn prepare_login_body(access_token: &str) -> Vec<u8> {
    encode_blocks(&[
        ProtoBlock::Bytes {
            id: 1,
            data: access_token.as_bytes().to_vec(),
        },
        ProtoBlock::Varint { id: 2, value: 0 },
    ])
}

pub fn auth_game_body(account_id: u32, token: &str, game_uuid: &str) -> Vec<u8> {
    encode_blocks(&[
        ProtoBlock::Varint {
            id: 1,
            value: account_id.into(),
        },
        ProtoBlock::Bytes {
            id: 2,
            data: token.as_bytes().to_vec(),
        },
        ProtoBlock::Bytes {
            id: 3,
            data: game_uuid.as_bytes().to_vec(),
        },
        ProtoBlock::Bytes {
            id: 4,
            data: Vec::new(),
        },
        ProtoBlock::Bytes {
            id: 5,
            data: Vec::new(),
        },
        ProtoBlock::Varint { id: 6, value: 0 },
    ])
}

pub fn game_ws_urls(game_url: &str) -> Vec<String> {
    if game_url.starts_with("ws://") || game_url.starts_with("wss://") {
        let url = if game_url.ends_with("/gateway") {
            game_url.to_string()
        } else {
            format!("{}/gateway", game_url.trim_end_matches('/'))
        };
        return vec![url];
    }
    vec![
        format!("ws://{}/gateway", game_url.trim_end_matches('/')),
        format!("wss://{}/gateway", game_url.trim_end_matches('/')),
    ]
}

pub fn game_gateway_tail(location: &str) -> &'static str {
    if location == "local" {
        "game-gateway"
    } else {
        "game-gateway-zone"
    }
}

pub fn game_route_candidates(location: &str, preferred: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    for route in std::iter::once(preferred)
        .chain(std::iter::once(location))
        .chain(GAME_ROUTE_IDS.iter().copied())
    {
        if route.starts_with("route-") && !candidates.iter().any(|item| item == route) {
            candidates.push(route.to_string());
        }
    }
    candidates
}

pub fn request_route_candidates(host_route_id: &str) -> Vec<String> {
    let mut candidates = vec![host_route_id.to_string()];
    if DEFAULT_ROUTE_ID != host_route_id {
        candidates.push(DEFAULT_ROUTE_ID.to_string());
    }
    candidates
}

pub fn route_id_from_ws_url(url: &str) -> Option<String> {
    let host = url.split("://").nth(1)?.split('/').next()?;
    let first = host.split('.').next()?;
    first.starts_with("route-").then(|| first.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_three_uses_8443_and_other_routes_use_443() {
        assert_eq!(
            route_ws_url("route-3", "gateway"),
            "wss://route-3.maj-soul.com:8443/gateway"
        );
        assert_eq!(
            route_ws_url("route-2", "gateway"),
            "wss://route-2.maj-soul.com:443/gateway"
        );
        assert_eq!(
            route_ws_url("route-6", "game-gateway-zone"),
            "wss://route-6.maj-soul.com:443/game-gateway-zone"
        );
    }

    #[test]
    fn lobby_candidates_match_python_order() {
        assert_eq!(
            lobby_ws_url_candidates(),
            vec![
                "wss://route-2.maj-soul.com:443/gateway",
                "wss://route-3.maj-soul.com:8443/gateway",
                "wss://route-4.maj-soul.com:443/gateway",
                "wss://route-5.maj-soul.com:443/gateway",
                "wss://route-6.maj-soul.com:443/gateway",
            ]
        );
    }

    #[test]
    fn route_body_matches_python_shape() {
        assert_eq!(
            hex::encode(route_body(2, "route-2", 1_717_000_000_000)),
            "10021a07726f7574652d322080a4b3a9fc313203576562"
        );
    }

    #[test]
    fn prepare_login_body_matches_python_shape() {
        assert_eq!(
            hex::encode(prepare_login_body("token")),
            "0a05746f6b656e1000"
        );
    }

    #[test]
    fn auth_game_body_keeps_empty_fields_like_python_raw_body() {
        assert_eq!(
            hex::encode(auth_game_body(42, "tok", "uuid")),
            "082a1203746f6b1a047575696422002a003000"
        );
    }

    #[test]
    fn game_ws_urls_match_python_fallback_shape() {
        assert_eq!(
            game_ws_urls("server.example.com"),
            vec![
                "ws://server.example.com/gateway".to_string(),
                "wss://server.example.com/gateway".to_string()
            ]
        );
        assert_eq!(
            game_ws_urls("wss://route-6.maj-soul.com/game-gateway-zone"),
            vec!["wss://route-6.maj-soul.com/game-gateway-zone/gateway".to_string()]
        );
    }

    #[test]
    fn route_id_can_be_extracted_from_route_ws_url() {
        assert_eq!(
            route_id_from_ws_url("wss://route-6.maj-soul.com:443/gateway"),
            Some("route-6".to_string())
        );
        assert_eq!(route_id_from_ws_url("ws://1.2.3.4/gateway"), None);
    }
}
