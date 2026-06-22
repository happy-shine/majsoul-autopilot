use anyhow::{anyhow, Result};
use liqi::{
    codec::{decode_action_payload, notify_body, response_body},
    pb,
};
use mjai::bridge::{Bridge, Event};
use prost::Message;

use crate::{
    config::{match_sid, rank_tier_from_level_id, target_mode_for_rank_level, Mode, Room},
    login::{client_version_string, login_payload},
    routes::{
        game_gateway_tail, game_route_candidates, game_ws_urls, lobby_ws_url_candidates,
        prepare_login_body, request_route_candidates, route_body, route_id_from_ws_url,
        route_ws_url, GAME_ROUTE_IDS,
    },
    transport::LiqiSocket,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginSummary {
    pub account_id: u32,
    pub nickname: String,
    pub level_id: u32,
    pub level_score: u32,
    pub rank_tier: u32,
    pub target_mode: Mode,
    pub target_room: Room,
    pub access_token: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchStart {
    pub game_url: String,
    pub connect_token: String,
    pub game_uuid: String,
    pub match_mode_id: u32,
    pub location: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExistingGame {
    pub connect_token: String,
    pub game_uuid: String,
    pub location: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartMatchResult {
    Queued(String),
    Busy,
}

pub struct GameSession {
    socket: LiqiSocket,
    pub bridge: Bridge,
}

pub struct ProtocolClient {
    socket: LiqiSocket,
    route_id: String,
    route_prep: Option<LiqiSocket>,
    pub summary: LoginSummary,
}

pub async fn check_login(username: &str, password: &str, device_id: &str) -> Result<LoginSummary> {
    Ok(ProtocolClient::login(username, password, device_id)
        .await?
        .summary)
}

impl ProtocolClient {
    pub async fn login(username: &str, password: &str, device_id: &str) -> Result<Self> {
        let mut last_error = None;
        for (route_id, url) in lobby_route_options()? {
            match Self::login_on_route(username, password, device_id, route_id, &url).await {
                Ok(client) => return Ok(client),
                Err(err) => last_error = Some(err),
            }
        }
        Err(last_error.unwrap_or_else(|| anyhow!("login failed for all lobby routes")))
    }

    async fn login_on_route(
        username: &str,
        password: &str,
        device_id: &str,
        route_id: String,
        url: &str,
    ) -> Result<Self> {
        let mut socket = LiqiSocket::connect(&url).await?;
        let now_ms = current_time_millis();
        socket
            .request_raw(
                ".lq.Route.requestConnection",
                &route_body(1, &route_id, now_ms),
            )
            .await?;

        let login = login_payload(username, password, device_id, false);
        let response: pb::ResLogin = socket.request(".lq.Lobby.login", &login).await?;
        let summary = login_summary(response)?;
        let _ = socket
            .request::<_, pb::ResCommon>(
                ".lq.Lobby.loginBeat",
                &pb::ReqLoginBeat {
                    contract: crate::login::LOGIN_BEAT_CONTRACT.to_string(),
                },
            )
            .await;
        let _ = socket
            .request::<_, pb::ResCommon>(".lq.Lobby.loginSuccess", &pb::ReqCommon {})
            .await;
        let _ = socket
            .request::<_, pb::ResFetchInfo>(".lq.Lobby.fetchInfo", &pb::ReqCommon {})
            .await;
        Ok(Self {
            socket,
            route_id,
            route_prep: None,
            summary,
        })
    }

    pub fn set_match_target(&mut self, mode: Mode, room: Room) {
        self.summary.target_mode = mode;
        self.summary.target_room = room;
    }

    pub async fn refresh_account_summary(&mut self) -> Result<LoginSummary> {
        let response: pb::ResAccountInfo = self
            .socket
            .request(
                ".lq.Lobby.fetchAccountInfo",
                &pb::ReqAccountInfo {
                    account_id: self.summary.account_id,
                },
            )
            .await?;
        if let Some(error) = response.error {
            if error.code != 0 {
                return Err(anyhow!("fetchAccountInfo failed: code={}", error.code));
            }
        }
        let account = response
            .account
            .ok_or_else(|| anyhow!("fetchAccountInfo response missing account"))?;
        let level = account
            .level
            .ok_or_else(|| anyhow!("fetchAccountInfo response missing four-player level"))?;
        let (target_mode, target_room) = target_mode_for_rank_level(level.id);
        self.summary.nickname = account.nickname;
        self.summary.level_id = level.id;
        self.summary.level_score = level.score;
        self.summary.rank_tier = rank_tier_from_level_id(level.id);
        self.summary.target_mode = target_mode;
        self.summary.target_room = target_room;
        Ok(self.summary.clone())
    }

    pub async fn start_match(&mut self) -> Result<StartMatchResult> {
        self.prepare_game_route().await?;
        let sid = match_sid(&self.summary.target_mode, &self.summary.target_room)
            .ok_or_else(|| anyhow!("no match sid for current target"))?;
        let response: pb::ResCommon = self
            .socket
            .request(
                ".lq.Lobby.startUnifiedMatch",
                &start_match_payload(sid.clone()),
            )
            .await?;
        if let Some(error) = response.error {
            if error.code != 0 {
                if error.code == 1023 {
                    return Ok(StartMatchResult::Busy);
                }
                if error.code == 1304 {
                    return Ok(StartMatchResult::Queued(sid));
                }
                return Err(anyhow!(
                    "startUnifiedMatch failed: code={} str={:?} u32={:?}",
                    error.code,
                    error.str_params,
                    error.u32_params
                ));
            }
        }
        Ok(StartMatchResult::Queued(sid))
    }

    pub async fn wait_for_match_start(&mut self) -> Result<MatchStart> {
        loop {
            let raw = self.socket.next_binary_frame().await?;
            if raw.first().copied() != Some(0x01) {
                continue;
            }
            let notify = notify_body(&raw).map_err(|err| anyhow!(err))?;
            match notify.method.as_str() {
                ".lq.NotifyMatchGameStart" => {
                    let msg = pb::NotifyMatchGameStart::decode(notify.body.as_slice())?;
                    return Ok(MatchStart {
                        game_url: msg.game_url,
                        connect_token: msg.connect_token,
                        game_uuid: msg.game_uuid,
                        match_mode_id: msg.match_mode_id,
                        location: msg.location,
                    });
                }
                ".lq.NotifyMatchFailed" => return Err(anyhow!("match failed")),
                ".lq.NotifyMatchTimeout" => return Err(anyhow!("match timeout")),
                _ => {}
            }
        }
    }

    pub async fn fetch_existing_game(&mut self) -> Result<Option<ExistingGame>> {
        let response: pb::ResFetchGamingInfo = self
            .socket
            .request(".lq.Lobby.fetchGamingInfo", &pb::ReqCommon {})
            .await?;
        if let Some(error) = response.error {
            if error.code != 0 {
                return Err(anyhow!("fetchGamingInfo failed: code={}", error.code));
            }
        }
        Ok(response.game_info.map(|info| ExistingGame {
            connect_token: info.connect_token,
            game_uuid: info.game_uuid,
            location: info.location,
        }))
    }

    pub async fn connect_game(&mut self, start: &MatchStart) -> Result<(GameSession, Vec<Event>)> {
        tokio::time::sleep(std::time::Duration::from_millis(5500)).await;
        let mut last_error = None;
        let tail = game_gateway_tail(&start.location);
        for route in game_route_candidates(&start.location, &self.route_id) {
            let url = route_ws_url(&route, tail);
            for request_route in request_route_candidates(&route) {
                match connect_game_once(
                    self.summary.account_id,
                    &url,
                    &request_route,
                    &start.connect_token,
                    &start.game_uuid,
                )
                .await
                {
                    Ok(session) => return Ok(session),
                    Err(err) => last_error = Some(err),
                }
            }
        }

        for url in game_ws_urls(&start.game_url) {
            let mut request_routes = Vec::new();
            if let Some(route) = route_id_from_ws_url(&url) {
                request_routes.push(route);
            }
            if start.location.starts_with("route-")
                && !request_routes.iter().any(|route| route == &start.location)
            {
                request_routes.push(start.location.clone());
            }
            for route in GAME_ROUTE_IDS {
                if !request_routes.iter().any(|candidate| candidate == route) {
                    request_routes.push(route.to_string());
                }
            }

            for request_route in request_routes {
                match connect_game_once(
                    self.summary.account_id,
                    &url,
                    &request_route,
                    &start.connect_token,
                    &start.game_uuid,
                )
                .await
                {
                    Ok(session) => return Ok(session),
                    Err(err) => last_error = Some(err),
                }
            }
        }
        Err(last_error.unwrap_or_else(|| anyhow!("game websocket handshake failed for all routes")))
    }

    pub async fn connect_existing_game(
        &mut self,
        game: &ExistingGame,
    ) -> Result<(GameSession, Vec<Event>)> {
        self.prepare_game_route().await?;
        let mut last_error = None;
        let tail = game_gateway_tail(&game.location);
        for route in game_route_candidates(&game.location, &self.route_id) {
            let url = route_ws_url(&route, tail);
            for request_route in request_route_candidates(&route) {
                match connect_game_once(
                    self.summary.account_id,
                    &url,
                    &request_route,
                    &game.connect_token,
                    &game.game_uuid,
                )
                .await
                {
                    Ok(session) => return Ok(session),
                    Err(err) => last_error = Some(err),
                }
            }
        }
        Err(last_error.unwrap_or_else(|| anyhow!("existing game reconnect failed for all routes")))
    }

    pub async fn cancel_match(&mut self) -> Result<()> {
        let sid = match_sid(&self.summary.target_mode, &self.summary.target_room)
            .ok_or_else(|| anyhow!("no match sid for current target"))?;
        let response: pb::ResCommon = self
            .socket
            .request(
                ".lq.Lobby.cancelUnifiedMatch",
                &pb::ReqCancelUnifiedMatch { match_sid: sid },
            )
            .await?;
        if let Some(error) = response.error {
            if error.code != 0 {
                return Err(anyhow!("cancelUnifiedMatch failed: code={}", error.code));
            }
        }
        Ok(())
    }

    async fn prepare_game_route(&mut self) -> Result<()> {
        if self.summary.access_token.is_empty() {
            return Err(anyhow!("cannot prepare route without access token"));
        }
        let mut candidates = Vec::new();
        candidates.push(self.route_id.clone());
        for route in GAME_ROUTE_IDS {
            if !candidates.iter().any(|candidate| candidate == route) {
                candidates.push(route.to_string());
            }
        }

        let mut last_error = None;
        for route in candidates {
            let url = route_ws_url(&route, "gateway");
            match self.prepare_game_route_once(&route, &url).await {
                Ok(sock) => {
                    self.route_prep = Some(sock);
                    return Ok(());
                }
                Err(err) => last_error = Some(err),
            }
        }
        Err(last_error.unwrap_or_else(|| anyhow!("prepareLogin failed for all routes")))
    }

    async fn prepare_game_route_once(&self, route: &str, url: &str) -> Result<LiqiSocket> {
        let mut socket = LiqiSocket::connect(url).await?;
        socket
            .request_raw(
                ".lq.Route.requestConnection",
                &route_body(2, route, current_time_millis()),
            )
            .await?;
        socket
            .request_raw(
                ".lq.Lobby.prepareLogin",
                &prepare_login_body(&self.summary.access_token),
            )
            .await?;
        Ok(socket)
    }
}

impl GameSession {
    pub async fn next_events(&mut self) -> Result<Vec<Event>> {
        loop {
            let raw = self.socket.next_binary_frame().await?;
            if raw.first().copied() != Some(0x01) {
                continue;
            }
            if let Some(events) = parse_action_notify(&mut self.bridge, &raw)? {
                return Ok(events);
            }
        }
    }

    pub async fn input_operation(&mut self, req: pb::ReqSelfOperation) -> Result<pb::ResCommon> {
        self.socket
            .request(".lq.FastTest.inputOperation", &req)
            .await
    }

    pub async fn input_chi_peng_gang(&mut self, req: pb::ReqChiPengGang) -> Result<pb::ResCommon> {
        self.socket
            .request(".lq.FastTest.inputChiPengGang", &req)
            .await
    }

    pub async fn confirm_new_round(&mut self) -> Result<pb::ResCommon> {
        self.socket
            .request(".lq.FastTest.confirmNewRound", &pb::ReqCommon {})
            .await
    }

    pub async fn skip(&mut self) -> Result<pb::ResCommon> {
        self.input_chi_peng_gang(pb::ReqChiPengGang {
            cancel_operation: true,
            timeuse: 2,
            ..Default::default()
        })
        .await
    }
}

async fn connect_game_once(
    account_id: u32,
    url: &str,
    request_route: &str,
    connect_token: &str,
    game_uuid: &str,
) -> Result<(GameSession, Vec<Event>)> {
    let mut socket = LiqiSocket::connect(url).await?;
    socket
        .request_raw(
            ".lq.Route.requestConnection",
            &route_body(1, request_route, current_time_millis()),
        )
        .await?;
    let auth_raw = socket
        .request_raw(
            ".lq.FastTest.authGame",
            &crate::routes::auth_game_body(account_id, connect_token, game_uuid),
        )
        .await?;
    let (_, body) = response_body(&auth_raw).map_err(|err| anyhow!(err))?;
    let auth = pb::ResAuthGame::decode(body.as_slice())?;
    if let Some(error) = auth.error {
        if error.code != 0 {
            return Err(anyhow!("authGame failed: code={}", error.code));
        }
    }
    if auth.seat_list.len() != 4 {
        return Err(anyhow!("authGame returned non-4p seat list"));
    }
    let seat = auth
        .seat_list
        .iter()
        .position(|id| *id == account_id)
        .ok_or_else(|| anyhow!("authGame seat list does not contain account id"))?
        as u32;

    let mut bridge = Bridge::new(seat);
    let enter: pb::ResEnterGame = socket
        .request(".lq.FastTest.enterGame", &pb::ReqCommon {})
        .await?;
    let restore = if enter.error.as_ref().is_some_and(|err| err.code != 0) {
        let sync: pb::ResSyncGame = socket
            .request(
                ".lq.FastTest.syncGame",
                &pb::ReqSyncGame {
                    round_id: "-1".to_string(),
                    step: 1_000_000,
                },
            )
            .await?;
        if let Some(error) = sync.error {
            if error.code != 0 {
                return Err(anyhow!("syncGame failed: code={}", error.code));
            }
        }
        sync.game_restore
    } else {
        enter.game_restore
    };

    let mut initial_events = Vec::new();
    if let Some(restore) = restore {
        let passed_waiting_time = restore.passed_waiting_time;
        for action in restore.actions {
            initial_events.extend(bridge.handle_action_with_waiting(
                &action.name,
                &action.data,
                passed_waiting_time,
            )?);
        }
    }

    let clear: pb::ResCommon = socket
        .request(".lq.FastTest.clearLeaving", &pb::ReqCommon {})
        .await?;
    if let Some(error) = clear.error {
        if error.code != 0 && error.code != 2 {
            return Err(anyhow!("clearLeaving failed: code={}", error.code));
        }
    }

    Ok((GameSession { socket, bridge }, initial_events))
}

fn parse_action_notify(bridge: &mut Bridge, raw: &[u8]) -> Result<Option<Vec<Event>>> {
    let notify = notify_body(raw).map_err(|err| anyhow!(err))?;
    if matches!(
        notify.method.as_str(),
        ".lq.NotifyGameEndResult" | ".lq.NotifyGameTerminate"
    ) {
        return Ok(Some(vec![Event::EndGame]));
    }
    if notify.method != ".lq.ActionPrototype" {
        return Ok(None);
    }
    let mut action = pb::ActionPrototype::decode(notify.body.as_slice())?;
    action.data = decode_action_payload(&action.data);
    Ok(Some(bridge.handle_action(&action.name, &action.data)?))
}

fn lobby_route_options() -> Result<Vec<(String, String)>> {
    let routes = lobby_ws_url_candidates()
        .into_iter()
        .map(|url| {
            let route_id = url.split("://").nth(1)?.split('.').next()?.to_string();
            Some((route_id, url))
        })
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| anyhow!("invalid lobby route candidates"))?;
    if routes.is_empty() {
        return Err(anyhow!("no lobby route candidates"));
    }
    Ok(routes)
}

fn login_summary(response: pb::ResLogin) -> Result<LoginSummary> {
    if let Some(error) = response.error {
        if error.code != 0 {
            return Err(anyhow!(
                "login failed: code={} str={:?} u32={:?}",
                error.code,
                error.str_params,
                error.u32_params
            ));
        }
    }

    let account = response
        .account
        .ok_or_else(|| anyhow!("login response missing account"))?;
    let level = account
        .level
        .ok_or_else(|| anyhow!("login response missing four-player level"))?;
    let (target_mode, target_room) = target_mode_for_rank_level(level.id);

    Ok(LoginSummary {
        account_id: response.account_id,
        nickname: account.nickname,
        level_id: level.id,
        level_score: level.score,
        rank_tier: rank_tier_from_level_id(level.id),
        target_mode,
        target_room,
        access_token: response.access_token,
    })
}

pub fn start_match_payload(match_sid: String) -> pb::ReqStartUnifiedMatch {
    pb::ReqStartUnifiedMatch {
        match_sid,
        client_version_string: client_version_string(),
    }
}

fn current_time_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use liqi::codec::{encode_blocks, ProtoBlock};

    #[test]
    fn start_match_payload_uses_current_webgl_client_version() {
        let payload = start_match_payload("1:9".to_string());
        assert_eq!(payload.match_sid, "1:9");
        assert_eq!(payload.client_version_string, client_version_string());
    }

    #[test]
    fn action_notify_decodes_xored_action_payload_into_bridge_events() {
        let action = pb::ActionDiscardTile {
            seat: 0,
            tile: "1m".to_string(),
            moqie: false,
            ..Default::default()
        };
        let mut action_body = Vec::new();
        action.encode(&mut action_body).unwrap();
        let proto = pb::ActionPrototype {
            name: "ActionDiscardTile".to_string(),
            data: liqi::codec::encode_action_payload(&action_body),
            ..Default::default()
        };
        let mut proto_body = Vec::new();
        proto.encode(&mut proto_body).unwrap();
        let raw = [
            vec![1],
            encode_blocks(&[
                ProtoBlock::Bytes {
                    id: 1,
                    data: b".lq.ActionPrototype".to_vec(),
                },
                ProtoBlock::Bytes {
                    id: 2,
                    data: proto_body,
                },
            ]),
        ]
        .concat();

        let mut bridge = Bridge::new(0);
        let events = parse_action_notify(&mut bridge, &raw).unwrap().unwrap();
        assert_eq!(
            events,
            vec![Event::Dahai {
                actor: 0,
                pai: "1m".to_string(),
                tsumogiri: false
            }]
        );
    }

    #[test]
    fn game_end_notify_emits_end_game_like_python_bridge() {
        let raw = [
            vec![1],
            encode_blocks(&[
                ProtoBlock::Bytes {
                    id: 1,
                    data: b".lq.NotifyGameTerminate".to_vec(),
                },
                ProtoBlock::Bytes {
                    id: 2,
                    data: Vec::new(),
                },
            ]),
        ]
        .concat();

        let mut bridge = Bridge::new(0);
        let events = parse_action_notify(&mut bridge, &raw).unwrap().unwrap();
        assert_eq!(events, vec![Event::EndGame]);
    }

    #[test]
    fn lobby_route_options_preserve_fallback_routes() {
        let routes = lobby_route_options().unwrap();

        assert_eq!(
            routes,
            vec![
                (
                    "route-2".to_string(),
                    "wss://route-2.maj-soul.com:443/gateway".to_string()
                ),
                (
                    "route-3".to_string(),
                    "wss://route-3.maj-soul.com:8443/gateway".to_string()
                ),
                (
                    "route-4".to_string(),
                    "wss://route-4.maj-soul.com:443/gateway".to_string()
                ),
                (
                    "route-5".to_string(),
                    "wss://route-5.maj-soul.com:443/gateway".to_string()
                ),
                (
                    "route-6".to_string(),
                    "wss://route-6.maj-soul.com:443/gateway".to_string()
                ),
            ]
        );
    }
}
