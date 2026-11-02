use anyhow::{anyhow, Result};
use liqi::{
    codec::{decode_action_payload, notify_body, response_body},
    pb,
};
use mjai::bridge::{Bridge, Event};
use prost::Message;

use crate::{
    config::{match_sid, rank_tier_from_level_id, target_mode_for_rank_level, Mode, Room},
    login::{
        client_version_string, login_payload_with_version, resolve_client_version, ClientVersion,
    },
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
    pub gold: u32,
    pub target_mode: Mode,
    pub target_room: Room,
    pub access_token: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CoinClaimReport {
    pub gold_before: u32,
    pub gold_after: u32,
    pub claimed: Vec<String>,
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
    Queued {
        match_sid: String,
        coin_claim: Option<CoinClaimReport>,
    },
    Busy,
    InsufficientCoins {
        gold: u32,
        coin_claim: CoinClaimReport,
    },
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
        let version = resolve_client_version().await;
        Self::login_with_version(username, password, device_id, &version).await
    }

    pub async fn login_with_version(
        username: &str,
        password: &str,
        device_id: &str,
        version: &ClientVersion,
    ) -> Result<Self> {
        let mut last_error = None;
        for (route_id, url) in lobby_route_options()? {
            match Self::login_on_route(username, password, device_id, route_id, &url, version).await {
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
        version: &ClientVersion,
    ) -> Result<Self> {
        let mut socket = LiqiSocket::connect(&url).await?;
        let now_ms = current_time_millis();
        socket
            .request_raw(
                ".lq.Route.requestConnection",
                &route_body(1, &route_id, now_ms),
            )
            .await?;

        let login = login_payload_with_version(username, password, device_id, false, version);
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

    pub fn sync_pending_account_updates(&mut self) {
        let mut updated_gold = None;
        self.socket.retain_pending_frames(|raw| {
            if raw.first().copied() != Some(0x01) {
                return true;
            }
            let Ok(notify) = notify_body(raw) else {
                return true;
            };
            if notify.method != ".lq.NotifyAccountUpdate" {
                return true;
            }
            if let Ok(msg) = pb::NotifyAccountUpdate::decode(notify.body.as_slice()) {
                if let Some(update) = msg.update {
                    for num in update.numerical {
                        if num.id == 100002 {
                            updated_gold = Some(num.r#final);
                        }
                    }
                }
            }
            false
        });
        if let Some(gold) = updated_gold {
            self.summary.gold = gold;
        }
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
        self.sync_pending_account_updates();
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
        if account.gold > 0 {
            self.summary.gold = account.gold;
        }
        self.summary.target_mode = target_mode;
        self.summary.target_room = target_room;
        Ok(self.summary.clone())
    }

    async fn claim_pending_mails(&mut self, timeout: std::time::Duration) -> u32 {
        let mut mail_claimed = 0u32;
        if let Ok(Ok(mail_info)) = tokio::time::timeout(
            timeout,
            self.socket
                .request::<_, pb::ResMailInfo>(".lq.Lobby.fetchMailInfo", &pb::ReqCommon {}),
        )
        .await
        {
            if mail_info.error.as_ref().map_or(0, |e| e.code) == 0 {
                for mail in mail_info.mails {
                    if !mail.attachments.is_empty() && !mail.take_attachment {
                        if let Ok(Ok(res)) = tokio::time::timeout(
                            timeout,
                            self.socket.request::<_, pb::ResCommon>(
                                ".lq.Lobby.takeAttachmentFromMail",
                                &pb::ReqTakeAttachment {
                                    mail_id: mail.mail_id,
                                },
                            ),
                        )
                        .await
                        {
                            if res.error.as_ref().map_or(0, |e| e.code) == 0 {
                                mail_claimed += 1;
                            }
                        }
                    }
                }
            }
        }
        mail_claimed
    }

    pub async fn claim_available_coins(&mut self) -> CoinClaimReport {
        use std::collections::{BTreeSet, HashMap, HashSet};
        const CLAIM_RPC_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

        self.sync_pending_account_updates();
        let gold_before = self.summary.gold;
        let mut claimed = Vec::new();

        // 1. Daily revive coin (e.g. 18,000 copper coins for Master tier) — claim FIRST while
        //    gold may be below threshold so daily perishable relief quota is never wasted.
        if let Ok(Ok(info)) = tokio::time::timeout(
            CLAIM_RPC_TIMEOUT,
            self.socket.request::<_, pb::ResReviveCoinInfo>(
                ".lq.Lobby.fetchReviveCoinInfo",
                &pb::ReqCommon {},
            ),
        )
        .await
        {
            if info.error.as_ref().map_or(0, |e| e.code) == 0 && !info.has_gained {
                if let Ok(Ok(res)) = tokio::time::timeout(
                    CLAIM_RPC_TIMEOUT,
                    self.socket
                        .request::<_, pb::ResCommon>(".lq.Lobby.gainReviveCoin", &pb::ReqCommon {}),
                )
                .await
                {
                    if res.error.as_ref().map_or(0, |e| e.code) == 0 {
                        claimed.push("revive_coin".to_string());
                    }
                }
            }
        }

        // 2. Sync daily copper-coin tasks so today's 3 daily tasks are active on the server.
        let _ = tokio::time::timeout(
            CLAIM_RPC_TIMEOUT,
            self.socket
                .request::<_, pb::ResDailyTask>(".lq.Lobby.fetchDailyTask", &pb::ReqCommon {}),
        )
        .await;

        // 3. Mail attachments (maintenance compensation, rank/event rewards).
        let mut total_mail_claimed = self.claim_pending_mails(CLAIM_RPC_TIMEOUT).await;

        // 4. Discover all currently active activities (especially Day-0 daily_sign events whose
        //    sign_in_data is empty before the first sign-in, progress_reward, and exchange shops).
        let mut active_daily_sign_ids = BTreeSet::new();
        let mut active_progress_reward_ids = BTreeSet::new();
        let mut active_exchange_ids = HashSet::new();
        if let Ok(Ok(act_list)) = tokio::time::timeout(
            CLAIM_RPC_TIMEOUT,
            self.socket.request::<_, pb::ResActivityList>(
                ".lq.Lobby.fetchActivityList",
                &pb::ReqCommon {},
            ),
        )
        .await
        {
            if act_list.error.as_ref().map_or(0, |e| e.code) == 0 {
                for a in act_list.activities {
                    if a.activity_id == 0 {
                        continue;
                    }
                    match a.r#type.as_str() {
                        "daily_sign" => {
                            active_daily_sign_ids.insert(a.activity_id);
                        }
                        "progress_reward" => {
                            active_progress_reward_ids.insert(a.activity_id);
                        }
                        "exchange" => {
                            active_exchange_ids.insert(a.activity_id);
                        }
                        _ => {}
                    }
                }
            }
        }

        // 5. Multi-pass cascading sweep over all regular & limited-time activity rewards.
        //    Completing stage N of newcomer/event quests often immediately unlocks stage N+1.
        let mut total_act_signin = 0u32;
        let mut total_act_tasks = 0usize;
        let mut total_period_tasks = 0usize;
        let mut total_random_tasks = 0usize;
        let mut total_flip_tasks = 0usize;
        let mut total_segment_tasks = 0u32;
        let mut total_point_rewards = 0u32;
        let mut total_progress_rewards = 0u32;
        let mut total_sub_event_rewards = 0u32;
        let mut latest_exchange_records: HashMap<u32, u32> = HashMap::new();

        let mut act_opt = tokio::time::timeout(
            CLAIM_RPC_TIMEOUT,
            self.socket.request::<_, pb::ResAccountActivityData>(
                ".lq.Lobby.fetchAccountActivityData",
                &pb::ReqCommon {},
            ),
        )
        .await
        .ok()
        .and_then(|r| r.ok())
        .filter(|a| a.error.as_ref().map_or(0, |e| e.code) == 0);

        for pass in 0..3 {
            let Some(act) = act_opt.take() else {
                break;
            };

            latest_exchange_records.clear();
            for rec in &act.exchange_records {
                latest_exchange_records.insert(rec.exchange_id, rec.count);
            }

            if pass == 0 {
                // 5a. Activity & 7-day login sign-ins (both existing sign_in_data and Day-0 active daily_sign).
                let mut sign_ids = active_daily_sign_ids.clone();
                for sign in &act.sign_in_data {
                    if sign.activity_id != 0 {
                        sign_ids.insert(sign.activity_id);
                    }
                }
                for activity_id in sign_ids {
                    if let Ok(Ok(res)) = tokio::time::timeout(
                        CLAIM_RPC_TIMEOUT,
                        self.socket.request::<_, pb::ResDoActivitySignIn>(
                            ".lq.Lobby.doActivitySignIn",
                            &pb::ReqDoActivitySignIn { activity_id },
                        ),
                    )
                    .await
                    {
                        if res.error.as_ref().map_or(0, |e| e.code) == 0 && !res.rewards.is_empty()
                        {
                            total_act_signin += 1;
                        }
                    }
                }

                // 5b. Accumulated point milestone rewards (game_point events).
                for ap in &act.accumulated_point_list {
                    if ap.activity_id == 0 || ap.point <= 0 {
                        continue;
                    }
                    let gained: HashSet<u32> = ap.gained_reward_list.iter().copied().collect();
                    let base = if ap.activity_id >= 100_000 {
                        ap.activity_id.saturating_mul(100)
                    } else {
                        ap.activity_id.saturating_mul(1000)
                    };
                    for tier in 1..=40u32 {
                        let reward_id = base.saturating_add(tier);
                        if gained.contains(&reward_id) {
                            continue;
                        }
                        if let Ok(Ok(res)) = tokio::time::timeout(
                            CLAIM_RPC_TIMEOUT,
                            self.socket.request::<_, pb::ResCommon>(
                                ".lq.Lobby.gainAccumulatedPointActivityReward",
                                &pb::ReqGainAccumulatedPointActivityReward {
                                    activity_id: ap.activity_id,
                                    reward_id,
                                },
                            ),
                        )
                        .await
                        {
                            if res.error.as_ref().map_or(0, |e| e.code) == 0 {
                                total_point_rewards += 1;
                            } else {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                }

                // 5c. Progress-bar activity rewards (e.g. 2500 / 5000 / 7500 / 10000 thresholds).
                let mut prog_map: HashMap<u32, HashSet<u32>> = HashMap::new();
                for aid in &active_progress_reward_ids {
                    prog_map.entry(*aid).or_default();
                }
                for pr in &act.progress_reward_data {
                    if pr.activity_id != 0 {
                        prog_map
                            .entry(pr.activity_id)
                            .or_default()
                            .extend(pr.rewarded_progresses.iter().copied());
                    }
                }
                for (activity_id, rewarded_set) in prog_map {
                    if let Ok(Ok(info)) = tokio::time::timeout(
                        CLAIM_RPC_TIMEOUT,
                        self.socket
                            .request::<_, pb::ResFetchProgressRewardActivityInfo>(
                                ".lq.Lobby.fetchProgressRewardActivityInfo",
                                &pb::ReqFetchProgressRewardActivityInfo { activity_id },
                            ),
                    )
                    .await
                    {
                        if info.error.as_ref().map_or(0, |e| e.code) == 0 && info.progress > 0 {
                            let targets: Vec<u32> = [2500u32, 5000, 7500, 10000]
                                .into_iter()
                                .filter(|t| *t <= info.progress && !rewarded_set.contains(t))
                                .collect();
                            if !targets.is_empty() {
                                if let Ok(Ok(res)) = tokio::time::timeout(
                                    CLAIM_RPC_TIMEOUT,
                                    self.socket
                                        .request::<_, pb::ResProgressRewardActivityReceive>(
                                            ".lq.Lobby.progressRewardActivityReceive",
                                            &pb::ReqProgressRewardActivityReceive {
                                                activity_id,
                                                progresses: targets,
                                            },
                                        ),
                                )
                                .await
                                {
                                    if res.error.as_ref().map_or(0, |e| e.code) == 0
                                        && !res.reward_items.is_empty()
                                    {
                                        total_progress_rewards += res.reward_items.len() as u32;
                                    }
                                }
                            }
                        }
                    }
                }

                // 5d. Upgrade, RPG, Gift inboxes, Spot, Story, Snowball, and MMO support rewards.
                for up in &act.upgrade_data {
                    if up.activity_id != 0
                        && up.groups.iter().any(|g| g.level > up.received_level)
                    {
                        if let Ok(Ok(res)) = tokio::time::timeout(
                            CLAIM_RPC_TIMEOUT,
                            self.socket.request::<_, pb::ResReceiveUpgradeActivityReward>(
                                ".lq.Lobby.receiveUpgradeActivityReward",
                                &pb::ReqReceiveUpgradeActivityReward {
                                    activity_id: up.activity_id,
                                },
                            ),
                        )
                        .await
                        {
                            if res.error.as_ref().map_or(0, |e| e.code) == 0
                                && !res.rewards.is_empty()
                            {
                                total_sub_event_rewards += 1;
                            }
                        }
                    }
                }

                for rpg in &act.rpg_data {
                    if rpg.activity_id != 0
                        && (rpg.current_state.is_some() || rpg.last_show_id > 0)
                    {
                        if let Ok(Ok(res)) = tokio::time::timeout(
                            CLAIM_RPC_TIMEOUT,
                            self.socket.request::<_, pb::ResReceiveRpgRewards>(
                                ".lq.Lobby.receiveRPGRewards",
                                &pb::ReqReceiveRpgRewards {
                                    activity_id: rpg.activity_id,
                                },
                            ),
                        )
                        .await
                        {
                            if res.error.as_ref().map_or(0, |e| e.code) == 0
                                && !res.items.is_empty()
                            {
                                total_sub_event_rewards += 1;
                            }
                        }
                    }
                }

                let mut gift_act_ids = BTreeSet::new();
                for feed in &act.feed_data {
                    if feed.activity_id != 0
                        && feed.gift_inbox.iter().any(|g| g.received == 0)
                    {
                        gift_act_ids.insert(feed.activity_id);
                    }
                }
                for fg in &act.friend_gift_data {
                    if fg.activity_id != 0 && fg.gift_inbox.iter().any(|g| g.received == 0) {
                        gift_act_ids.insert(fg.activity_id);
                    }
                }
                for activity_id in gift_act_ids {
                    if let Ok(Ok(res)) = tokio::time::timeout(
                        CLAIM_RPC_TIMEOUT,
                        self.socket.request::<_, pb::ResReceiveAllActivityGift>(
                            ".lq.Lobby.receiveAllActivityGift",
                            &pb::ReqReceiveAllActivityGift { activity_id },
                        ),
                    )
                    .await
                    {
                        if res.error.as_ref().map_or(0, |e| e.code) == 0 {
                            total_sub_event_rewards += 1;
                        }
                    }
                }

                for spot_act in &act.spot_data {
                    for spot in &spot_act.spots {
                        if spot.unlocked != 0 && spot.rewarded == 0 {
                            if let Ok(Ok(res)) = tokio::time::timeout(
                                CLAIM_RPC_TIMEOUT,
                                self.socket.request::<_, pb::ResReceiveActivitySpotReward>(
                                    ".lq.Lobby.receiveActivitySpotReward",
                                    &pb::ReqReceiveActivitySpotReward {
                                        unique_id: spot.unique_id,
                                    },
                                ),
                            )
                            .await
                            {
                                if res.error.as_ref().map_or(0, |e| e.code) == 0 {
                                    total_sub_event_rewards += 1;
                                }
                            }
                        }
                    }
                }

                for story_act in &act.story_data {
                    if story_act.activity_id == 0 {
                        continue;
                    }
                    for st in &story_act.unlocked_story {
                        let rewarded_endings: HashSet<u32> =
                            st.rewarded_ending.iter().copied().collect();
                        for &ending_id in &st.finished_ending {
                            if !rewarded_endings.contains(&ending_id) {
                                if let Ok(Ok(res)) = tokio::time::timeout(
                                    CLAIM_RPC_TIMEOUT,
                                    self.socket.request::<_, pb::ResStoryReward>(
                                        ".lq.Lobby.storyActivityReceiveEndingReward",
                                        &pb::ReqStoryActivityReceiveEndingReward {
                                            activity_id: story_act.activity_id,
                                            story_id: st.story_id,
                                            ending_id,
                                        },
                                    ),
                                )
                                .await
                                {
                                    if res.error.as_ref().map_or(0, |e| e.code) == 0 {
                                        total_sub_event_rewards += 1;
                                    }
                                }
                            }
                        }
                        if !st.finished_ending.is_empty() && st.finish_rewarded == 0 {
                            if let Ok(Ok(res)) = tokio::time::timeout(
                                CLAIM_RPC_TIMEOUT,
                                self.socket.request::<_, pb::ResStoryReward>(
                                    ".lq.Lobby.storyActivityReceiveFinishReward",
                                    &pb::ReqStoryActivityReceiveFinishReward {
                                        activity_id: story_act.activity_id,
                                        story_id: st.story_id,
                                    },
                                ),
                            )
                            .await
                            {
                                if res.error.as_ref().map_or(0, |e| e.code) == 0 {
                                    total_sub_event_rewards += 1;
                                }
                            }
                        }
                        if !st.finished_ending.is_empty() && st.all_finish_rewarded == 0 {
                            if let Ok(Ok(res)) = tokio::time::timeout(
                                CLAIM_RPC_TIMEOUT,
                                self.socket.request::<_, pb::ResStoryReward>(
                                    ".lq.Lobby.storyActivityReceiveAllFinishReward",
                                    &pb::ReqStoryActivityReceiveAllFinishReward {
                                        activity_id: story_act.activity_id,
                                        story_id: st.story_id,
                                    },
                                ),
                            )
                            .await
                            {
                                if res.error.as_ref().map_or(0, |e| e.code) == 0 {
                                    total_sub_event_rewards += 1;
                                }
                            }
                        }
                    }
                }

                for sb in &act.snowball_data {
                    if sb.activity_id == 0 {
                        continue;
                    }
                    let rewarded: HashSet<u32> = sb.rewarded_level.iter().copied().collect();
                    let levels: Vec<u32> = sb
                        .finished_level
                        .iter()
                        .copied()
                        .filter(|l| !rewarded.contains(l))
                        .collect();
                    if !levels.is_empty() {
                        if let Ok(Ok(res)) = tokio::time::timeout(
                            CLAIM_RPC_TIMEOUT,
                            self.socket.request::<_, pb::ResSnowballActivityReceiveReward>(
                                ".lq.Lobby.snowballActivityReceiveReward",
                                &pb::ReqSnowballActivityReceiveReward {
                                    activity_id: sb.activity_id,
                                    level: levels,
                                },
                            ),
                        )
                        .await
                        {
                            if res.error.as_ref().map_or(0, |e| e.code) == 0
                                && !res.rewards.is_empty()
                            {
                                total_sub_event_rewards += 1;
                            }
                        }
                    }
                }

                for mmo in &act.mmo_data {
                    if mmo.activity_id == 0 {
                        continue;
                    }
                    let has_support = mmo.support.as_ref().is_some_and(|s| {
                        s.receive_support_count > 0 || !s.support_record.is_empty()
                    });
                    if has_support {
                        if let Ok(Ok(res)) = tokio::time::timeout(
                            CLAIM_RPC_TIMEOUT,
                            self.socket.request::<_, pb::ResMmoActivityReceiveSupportReward>(
                                ".lq.Lobby.mmoActivityReceiveSupportReward",
                                &pb::ReqMmoActivityReceiveSupportReward {
                                    activity_id: mmo.activity_id,
                                },
                            ),
                        )
                        .await
                        {
                            if res.error.as_ref().map_or(0, |e| e.code) == 0
                                && !res.rewards.is_empty()
                            {
                                total_sub_event_rewards += 1;
                            }
                        }
                    }
                }
            }

            // 5e. Segment / multi-stage milestone tasks.
            let mut attempted_tasks = false;
            for seg in &act.segment_task_progress_list {
                if !seg.failed
                    && (seg.achieved_count > seg.reward_count || (seg.achieved && !seg.rewarded))
                {
                    let cnt = seg.achieved_count.saturating_sub(seg.reward_count).max(1);
                    if let Ok(Ok(res)) = tokio::time::timeout(
                        CLAIM_RPC_TIMEOUT,
                        self.socket.request::<_, pb::ResCompleteSegmentTaskReward>(
                            ".lq.Lobby.completeSegmentTaskReward",
                            &pb::ReqCompleteSegmentTaskReward {
                                task_id: seg.id,
                                count: cnt,
                            },
                        ),
                    )
                    .await
                    {
                        if res.error.as_ref().map_or(0, |e| e.code) == 0
                            && !res.rewards.is_empty()
                        {
                            total_segment_tasks += 1;
                            attempted_tasks = true;
                        }
                    }
                }
            }

            // Track unrewarded task IDs before completion so we accurately count only tasks that
            // actually transitioned to rewarded=true (ignoring future reward_limit_day locked tasks).
            let pre_task_rewarded: HashSet<u32> = act
                .task_progress_list
                .iter()
                .filter(|p| p.rewarded)
                .map(|p| p.id)
                .collect();
            let pre_period_rewarded: HashSet<u32> = act
                .period_task_progress_list
                .iter()
                .filter(|p| p.rewarded)
                .map(|p| p.id)
                .collect();
            let pre_random_rewarded: HashSet<u32> = act
                .random_task_progress_list
                .iter()
                .filter(|p| p.rewarded)
                .map(|p| p.id)
                .collect();
            let pre_flip_rewarded: HashSet<u32> = act
                .flip_task_progress_list
                .iter()
                .filter(|p| p.rewarded)
                .map(|p| p.id)
                .collect();

            // 5f. Regular activity tasks (batch with individual fallback).
            let task_ids: Vec<u32> = act
                .task_progress_list
                .iter()
                .filter(|p| p.achieved && !p.rewarded && !p.failed)
                .map(|p| p.id)
                .collect();
            if !task_ids.is_empty() {
                attempted_tasks = true;
                let batch_ok = tokio::time::timeout(
                    CLAIM_RPC_TIMEOUT,
                    self.socket.request::<_, pb::ResCommon>(
                        ".lq.Lobby.completeActivityTaskBatch",
                        &pb::ReqCompleteActivityTaskBatch {
                            task_list: task_ids.clone(),
                        },
                    ),
                )
                .await
                .ok()
                .and_then(|r| r.ok())
                .is_some_and(|res| res.error.as_ref().map_or(0, |e| e.code) == 0);
                if !batch_ok {
                    for task_id in &task_ids {
                        let _ = tokio::time::timeout(
                            CLAIM_RPC_TIMEOUT,
                            self.socket.request::<_, pb::ResCommon>(
                                ".lq.Lobby.completeActivityTask",
                                &pb::ReqCompleteActivityTask { task_id: *task_id },
                            ),
                        )
                        .await;
                    }
                }
            }

            // 5g. Period / newcomer growth / limited-time collaboration tasks (batch + individual fallback).
            let period_ids: Vec<u32> = act
                .period_task_progress_list
                .iter()
                .filter(|p| p.achieved && !p.rewarded && !p.failed)
                .map(|p| p.id)
                .collect();
            if !period_ids.is_empty() {
                attempted_tasks = true;
                let batch_ok = tokio::time::timeout(
                    CLAIM_RPC_TIMEOUT,
                    self.socket.request::<_, pb::ResCommon>(
                        ".lq.Lobby.completePeriodActivityTaskBatch",
                        &pb::ReqCompletePeriodActivityTaskBatch {
                            task_list: period_ids.clone(),
                        },
                    ),
                )
                .await
                .ok()
                .and_then(|r| r.ok())
                .is_some_and(|res| res.error.as_ref().map_or(0, |e| e.code) == 0);
                if !batch_ok {
                    for task_id in &period_ids {
                        let _ = tokio::time::timeout(
                            CLAIM_RPC_TIMEOUT,
                            self.socket.request::<_, pb::ResCommon>(
                                ".lq.Lobby.completePeriodActivityTask",
                                &pb::ReqCompleteActivityTask { task_id: *task_id },
                            ),
                        )
                        .await;
                    }
                }
            }

            // 5h. Random event tasks (batch + individual fallback).
            let random_ids: Vec<u32> = act
                .random_task_progress_list
                .iter()
                .filter(|p| p.achieved && !p.rewarded && !p.failed)
                .map(|p| p.id)
                .collect();
            if !random_ids.is_empty() {
                attempted_tasks = true;
                let batch_ok = tokio::time::timeout(
                    CLAIM_RPC_TIMEOUT,
                    self.socket.request::<_, pb::ResCommon>(
                        ".lq.Lobby.completeRandomActivityTaskBatch",
                        &pb::ReqCompleteActivityTaskBatch {
                            task_list: random_ids.clone(),
                        },
                    ),
                )
                .await
                .ok()
                .and_then(|r| r.ok())
                .is_some_and(|res| res.error.as_ref().map_or(0, |e| e.code) == 0);
                if !batch_ok {
                    for task_id in &random_ids {
                        let _ = tokio::time::timeout(
                            CLAIM_RPC_TIMEOUT,
                            self.socket.request::<_, pb::ResCommon>(
                                ".lq.Lobby.completeRandomActivityTask",
                                &pb::ReqCompleteActivityTask { task_id: *task_id },
                            ),
                        )
                        .await;
                    }
                }
            }

            // 5i. Flip-card / Bingo grid tasks.
            let flip_ids: Vec<u32> = act
                .flip_task_progress_list
                .iter()
                .filter(|p| p.achieved && !p.rewarded && !p.failed)
                .map(|p| p.id)
                .collect();
            if !flip_ids.is_empty() {
                attempted_tasks = true;
                let _ = tokio::time::timeout(
                    CLAIM_RPC_TIMEOUT,
                    self.socket.request::<_, pb::ResReceiveActivityFlipTaskBatch>(
                        ".lq.Lobby.receiveActivityFlipTaskBatch",
                        &pb::ReqReceiveActivityFlipTaskBatch {
                            task_list: flip_ids.clone(),
                        },
                    ),
                )
                .await;
                let batch_ok = tokio::time::timeout(
                    CLAIM_RPC_TIMEOUT,
                    self.socket.request::<_, pb::ResCompleteActivityFlipTaskBatch>(
                        ".lq.Lobby.completeActivityFlipTaskBatch",
                        &pb::ReqCompleteActivityFlipTaskBatch {
                            task_list: flip_ids.clone(),
                        },
                    ),
                )
                .await
                .ok()
                .and_then(|r| r.ok())
                .is_some_and(|res| res.error.as_ref().map_or(0, |e| e.code) == 0);
                if !batch_ok {
                    for task_id in &flip_ids {
                        let _ = tokio::time::timeout(
                            CLAIM_RPC_TIMEOUT,
                            self.socket.request::<_, pb::ResReceiveActivityFlipTask>(
                                ".lq.Lobby.receiveActivityFlipTask",
                                &pb::ReqReceiveActivityFlipTask { task_id: *task_id },
                            ),
                        )
                        .await;
                        let _ = tokio::time::timeout(
                            CLAIM_RPC_TIMEOUT,
                            self.socket.request::<_, pb::ResCommon>(
                                ".lq.Lobby.completeActivityFlipTask",
                                &pb::ReqCompleteActivityTask { task_id: *task_id },
                            ),
                        )
                        .await;
                    }
                }
            }

            if !attempted_tasks {
                break;
            }

            // Re-fetch activity data to verify which tasks actually became rewarded and to catch
            // newly unlocked chained tasks in the next cascade pass.
            let next_act = tokio::time::timeout(
                CLAIM_RPC_TIMEOUT,
                self.socket.request::<_, pb::ResAccountActivityData>(
                    ".lq.Lobby.fetchAccountActivityData",
                    &pb::ReqCommon {},
                ),
            )
            .await
            .ok()
            .and_then(|r| r.ok())
            .filter(|a| a.error.as_ref().map_or(0, |e| e.code) == 0);

            let Some(mut next) = next_act else {
                break;
            };

            let mut newly_task = next
                .task_progress_list
                .iter()
                .filter(|p| p.rewarded && !pre_task_rewarded.contains(&p.id))
                .count();
            let mut newly_period = next
                .period_task_progress_list
                .iter()
                .filter(|p| p.rewarded && !pre_period_rewarded.contains(&p.id))
                .count();

            // If a batch RPC returned code=0 without rewarding any item (e.g. mixed unlocked +
            // future-day locked tasks in the same batch), fall back once to individual RPCs.
            let mut retried_individually = false;
            if pass == 0 && !task_ids.is_empty() && newly_task == 0 {
                retried_individually = true;
                for task_id in &task_ids {
                    let _ = tokio::time::timeout(
                        CLAIM_RPC_TIMEOUT,
                        self.socket.request::<_, pb::ResCommon>(
                            ".lq.Lobby.completeActivityTask",
                            &pb::ReqCompleteActivityTask { task_id: *task_id },
                        ),
                    )
                    .await;
                }
            }
            if pass == 0 && !period_ids.is_empty() && newly_period == 0 {
                retried_individually = true;
                for task_id in &period_ids {
                    let _ = tokio::time::timeout(
                        CLAIM_RPC_TIMEOUT,
                        self.socket.request::<_, pb::ResCommon>(
                            ".lq.Lobby.completePeriodActivityTask",
                            &pb::ReqCompleteActivityTask { task_id: *task_id },
                        ),
                    )
                    .await;
                }
            }
            if retried_individually {
                if let Ok(Ok(refetched)) = tokio::time::timeout(
                    CLAIM_RPC_TIMEOUT,
                    self.socket.request::<_, pb::ResAccountActivityData>(
                        ".lq.Lobby.fetchAccountActivityData",
                        &pb::ReqCommon {},
                    ),
                )
                .await
                {
                    if refetched.error.as_ref().map_or(0, |e| e.code) == 0 {
                        next = refetched;
                        newly_task = next
                            .task_progress_list
                            .iter()
                            .filter(|p| p.rewarded && !pre_task_rewarded.contains(&p.id))
                            .count();
                        newly_period = next
                            .period_task_progress_list
                            .iter()
                            .filter(|p| p.rewarded && !pre_period_rewarded.contains(&p.id))
                            .count();
                    }
                }
            }

            let newly_random = next
                .random_task_progress_list
                .iter()
                .filter(|p| p.rewarded && !pre_random_rewarded.contains(&p.id))
                .count();
            let newly_flip = next
                .flip_task_progress_list
                .iter()
                .filter(|p| p.rewarded && !pre_flip_rewarded.contains(&p.id))
                .count();

            total_act_tasks += newly_task;
            total_period_tasks += newly_period;
            total_random_tasks += newly_random;
            total_flip_tasks += newly_flip;

            let progressed = newly_task + newly_period + newly_random + newly_flip;
            if progressed == 0 {
                break;
            }
            act_opt = Some(next);
        }

        if total_act_signin > 0 {
            claimed.push(format!("activity_sign_in({total_act_signin})"));
        }
        if total_act_tasks > 0 {
            claimed.push(format!("activity_tasks({total_act_tasks})"));
        }
        if total_period_tasks > 0 {
            claimed.push(format!("period_tasks({total_period_tasks})"));
        }
        if total_random_tasks > 0 {
            claimed.push(format!("random_tasks({total_random_tasks})"));
        }
        if total_flip_tasks > 0 {
            claimed.push(format!("flip_tasks({total_flip_tasks})"));
        }
        if total_segment_tasks > 0 {
            claimed.push(format!("segment_tasks({total_segment_tasks})"));
        }
        if total_point_rewards > 0 {
            claimed.push(format!("point_rewards({total_point_rewards})"));
        }
        if total_progress_rewards > 0 {
            claimed.push(format!("progress_rewards({total_progress_rewards})"));
        }
        if total_sub_event_rewards > 0 {
            claimed.push(format!("event_rewards({total_sub_event_rewards})"));
        }

        // 6. Bag coin pouches & expiring event-token -> copper-coin exchanges in active event shops.
        //    (Summon scrolls, bond gifts, rename scrolls, and selectable boxes are never touched.)
        const COPPER_EXCHANGE_RULES: &[(u32, u32, u32, u32, u32)] = &[
            // (exchange_id, activity_id, consume_id, consume_count, exchange_limit)
            (1002009, 1002, 309001, 1, 15),
            (1002010, 1002, 309002, 1, 15),
            (1005015, 1005, 309003, 1, 30),
            (1005016, 1005, 309004, 1, 30),
            (1005017, 1005, 309005, 1, 30),
            (1007013, 1007, 309006, 1, 13),
            (1013001, 1013, 309008, 1, 8),
            (1013002, 1013, 309009, 1, 8),
            (1013003, 1013, 309010, 1, 8),
            (1019005, 1019, 309011, 1, 12),
            (1025001, 1025, 309013, 1, 15),
            (1025002, 1025, 309014, 1, 15),
            (1025003, 1025, 309015, 1, 15),
            (1032005, 1032, 309017, 2, 15),
            (1063001, 1063, 309020, 1, 40),
            (1081001, 1081, 309024, 1, 40),
            (1085005, 1085, 309027, 1, 20),
            (1097012, 1097, 309029, 1, 12),
            (1108012, 1108, 309030, 30, 12),
            (1108013, 1108, 309030, 10, 999),
            (1168003, 1168, 309031, 2, 40),
            (1204011, 1204, 309039, 1, 15),
            (1204012, 1204, 309038, 1, 20),
            (1221011, 1221, 309043, 2, 20),
            (1221012, 1221, 309042, 2, 50),
            (1241011, 1241, 309046, 1, 15),
            (1241012, 1241, 309045, 1, 20),
            (22030611, 220306, 309054, 2, 20),
            (22030612, 220306, 309053, 2, 50),
            (22060611, 220606, 309058, 1, 15),
            (22060612, 220606, 309057, 1, 20),
            (22070611, 220706, 309061, 2, 20),
            (22070612, 220706, 309060, 2, 50),
            (22090811, 220908, 309067, 1, 15),
            (22090812, 220908, 309066, 1, 20),
            (23020212, 230202, 309071, 1, 15),
            (23020213, 230202, 309070, 1, 20),
            (23060313, 230603, 309074, 2, 20),
            (23060314, 230603, 309073, 2, 50),
            (23090213, 230902, 30900002, 1, 15),
            (23090214, 230902, 30900001, 1, 20),
            (24050313, 240503, 30900026, 2, 20),
            (24050314, 240503, 30900027, 2, 50),
            (25030212, 250302, 30900087, 2, 20),
            (25030213, 250302, 30900088, 2, 50),
        ];

        if let Ok(Ok(bag_res)) = tokio::time::timeout(
            CLAIM_RPC_TIMEOUT,
            self.socket
                .request::<_, pb::ResBagInfo>(".lq.Lobby.fetchBagInfo", &pb::ReqCommon {}),
        )
        .await
        {
            if bag_res.error.as_ref().map_or(0, |e| e.code) == 0 {
                if let Some(bag) = bag_res.bag {
                    let mut stacks: HashMap<u32, u32> = HashMap::new();
                    for item in bag.items {
                        if item.stack > 0 {
                            stacks.insert(item.item_id, item.stack);
                        }
                    }

                    let mut pouches_used = 0u32;
                    for coin_pouch_id in [300002u32, 300003] {
                        let cnt = stacks.get(&coin_pouch_id).copied().unwrap_or(0);
                        for _ in 0..cnt.min(20) {
                            if let Ok(Ok(res)) = tokio::time::timeout(
                                CLAIM_RPC_TIMEOUT,
                                self.socket.request::<_, pb::ResCommon>(
                                    ".lq.Lobby.useBagItem",
                                    &pb::ReqUseBagItem {
                                        item_id: coin_pouch_id,
                                    },
                                ),
                            )
                            .await
                            {
                                if res.error.as_ref().map_or(0, |e| e.code) == 0 {
                                    pouches_used += 1;
                                } else {
                                    break;
                                }
                            } else {
                                break;
                            }
                        }
                    }
                    if pouches_used > 0 {
                        claimed.push(format!("coin_pouches({pouches_used})"));
                    }

                    if !active_exchange_ids.is_empty() {
                        let mut exchanged_cnt = 0u32;
                        for &(ex_id, act_id, consume_id, consume_count, ex_limit) in
                            COPPER_EXCHANGE_RULES
                        {
                            if !active_exchange_ids.contains(&act_id) || consume_count == 0 {
                                continue;
                            }
                            let have = stacks.get(&consume_id).copied().unwrap_or(0);
                            let done = latest_exchange_records.get(&ex_id).copied().unwrap_or(0);
                            let rem_quota = ex_limit.saturating_sub(done);
                            let can_buy = (have / consume_count).min(rem_quota);
                            if can_buy > 0 {
                                if let Ok(Ok(res)) = tokio::time::timeout(
                                    CLAIM_RPC_TIMEOUT,
                                    self.socket.request::<_, pb::ResExchangeActivityItem>(
                                        ".lq.Lobby.exchangeActivityItem",
                                        &pb::ReqExchangeActivityItem {
                                            exchange_id: ex_id,
                                            count: can_buy,
                                        },
                                    ),
                                )
                                .await
                                {
                                    if res.error.as_ref().map_or(0, |e| e.code) == 0 {
                                        exchanged_cnt += can_buy;
                                        if let Some(st) = stacks.get_mut(&consume_id) {
                                            *st = st.saturating_sub(can_buy * consume_count);
                                        }
                                    }
                                }
                            }
                        }
                        if exchanged_cnt > 0 {
                            claimed.push(format!("event_exchange({exchanged_cnt})"));
                        }
                    }
                }
            }
        }

        // 7. Unclaimed completed achievements & completed achievement group milestones.
        if let Ok(Ok(ach)) = tokio::time::timeout(
            CLAIM_RPC_TIMEOUT,
            self.socket
                .request::<_, pb::ResAchievement>(".lq.Lobby.fetchAchievement", &pb::ReqCommon {}),
        )
        .await
        {
            if ach.error.as_ref().map_or(0, |e| e.code) == 0 {
                let rewarded_groups: HashSet<u32> = ach.rewarded_group.into_iter().collect();
                let mut ach_claimed = 0u32;
                for progress in ach
                    .progresses
                    .into_iter()
                    .filter(|p| p.achieved && !p.rewarded)
                    .take(200)
                {
                    if let Ok(Ok(res)) = tokio::time::timeout(
                        CLAIM_RPC_TIMEOUT,
                        self.socket.request::<_, pb::ResReceiveAchievementReward>(
                            ".lq.Lobby.receiveAchievementReward",
                            &pb::ReqReceiveAchievementReward {
                                achievement_id: progress.id,
                            },
                        ),
                    )
                    .await
                    {
                        if res.error.as_ref().map_or(0, |e| e.code) == 0 {
                            ach_claimed += 1;
                        } else {
                            break;
                        }
                    } else {
                        break;
                    }
                }
                if ach_claimed > 0 {
                    claimed.push(format!("achievements({ach_claimed})"));
                    let mut grp_claimed = 0u32;
                    for group_id in 1..=14u32 {
                        if rewarded_groups.contains(&group_id) {
                            continue;
                        }
                        if let Ok(Ok(res)) = tokio::time::timeout(
                            CLAIM_RPC_TIMEOUT,
                            self.socket
                                .request::<_, pb::ResReceiveAchievementGroupReward>(
                                    ".lq.Lobby.receiveAchievementGroupReward",
                                    &pb::ReqReceiveAchievementGroupReward { group_id },
                                ),
                        )
                        .await
                        {
                            if res.error.as_ref().map_or(0, |e| e.code) == 0 {
                                grp_claimed += 1;
                            }
                        }
                    }
                    if grp_claimed > 0 {
                        claimed.push(format!("achievement_groups({grp_claimed})"));
                    }
                }
            }
        }

        // 8. If any activity/achievement delivered rewards via mail, sweep mail once more,
        //    then sync NotifyAccountUpdate so gold_after reflects exact real-time copper coins.
        if !claimed.is_empty() {
            total_mail_claimed += self.claim_pending_mails(CLAIM_RPC_TIMEOUT).await;
        }
        if total_mail_claimed > 0 {
            claimed.insert(0, format!("mail({total_mail_claimed})"));
        }
        self.sync_pending_account_updates();

        CoinClaimReport {
            gold_before,
            gold_after: self.summary.gold,
            claimed,
        }
    }

    pub async fn fetch_game_record_list(
        &mut self,
        start: u32,
        count: u32,
        record_type: u32,
    ) -> Result<pb::ResGameRecordList> {
        let response: pb::ResGameRecordList = self
            .socket
            .request(
                ".lq.Lobby.fetchGameRecordList",
                &pb::ReqGameRecordList {
                    start,
                    count,
                    r#type: record_type,
                },
            )
            .await?;
        if let Some(ref error) = response.error {
            if error.code != 0 {
                return Err(anyhow!("fetchGameRecordList failed: code={}", error.code));
            }
        }
        Ok(response)
    }

    pub async fn fetch_game_records(
        &mut self,
        count: u32,
    ) -> Result<Vec<crate::record::GameRecordSummary>> {
        let res = self.fetch_game_record_list(0, count, 0).await?;
        Ok(crate::record::parse_game_record_list(
            res,
            self.summary.account_id,
        ))
    }

    pub async fn start_match(&mut self) -> Result<StartMatchResult> {
        self.prepare_game_route().await?;
        let sid = match_sid(&self.summary.target_mode, &self.summary.target_room)
            .ok_or_else(|| anyhow!("no match sid for current target"))?;
        let mut coin_claim = None;

        let payload = start_match_payload(sid.clone());
        let mut response: pb::ResCommon = self
            .socket
            .request(".lq.Lobby.startUnifiedMatch", &payload)
            .await?;

        if response.error.as_ref().map_or(0, |e| e.code) == 1304 {
            let report = self.claim_available_coins().await;
            let gained = !report.claimed.is_empty();
            coin_claim = Some(report);
            if gained {
                response = self
                    .socket
                    .request(".lq.Lobby.startUnifiedMatch", &payload)
                    .await?;
            }
        }

        if response.error.as_ref().map_or(0, |e| e.code) == 1304 {
            // Could be a stale match queue on the server; cancel and retry once.
            let _ = self.cancel_match().await;
            response = self
                .socket
                .request(".lq.Lobby.startUnifiedMatch", &payload)
                .await?;
        }

        if let Some(error) = response.error {
            if error.code != 0 {
                if error.code == 1023 {
                    return Ok(StartMatchResult::Busy);
                }
                if error.code == 1304 {
                    return Ok(StartMatchResult::InsufficientCoins {
                        gold: self.summary.gold,
                        coin_claim: coin_claim.unwrap_or_default(),
                    });
                }
                return Err(anyhow!(
                    "startUnifiedMatch failed: code={} str={:?} u32={:?}",
                    error.code,
                    error.str_params,
                    error.u32_params
                ));
            }
        }
        Ok(StartMatchResult::Queued {
            match_sid: sid,
            coin_claim,
        })
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
        gold: account.gold,
        target_mode,
        target_room,
        access_token: response.access_token,
    })
}

pub fn min_gold_for_room(room: &Room) -> u32 {
    match room {
        Room::Bronze => 0,
        Room::Silver => 1_500,
        Room::Gold => 6_000,
        Room::Jade => 15_000,
        Room::Throne => 30_000,
    }
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
    fn min_gold_thresholds_match_majsoul_rooms() {
        assert_eq!(min_gold_for_room(&Room::Bronze), 0);
        assert_eq!(min_gold_for_room(&Room::Silver), 1_500);
        assert_eq!(min_gold_for_room(&Room::Gold), 6_000);
        assert_eq!(min_gold_for_room(&Room::Jade), 15_000);
        assert_eq!(min_gold_for_room(&Room::Throne), 30_000);
    }

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

    #[tokio::test]
    async fn sync_pending_account_updates_consumes_copper_coin_notify_and_preserves_match_start(
    ) -> Result<()> {
        use futures_util::{SinkExt, StreamExt};
        use tokio::net::TcpListener;
        use tokio_tungstenite::tungstenite::Message as WsMessage;

        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;

        let update_msg = pb::NotifyAccountUpdate {
            update: Some(pb::AccountUpdate {
                numerical: vec![pb::account_update::NumericalUpdate {
                    id: 100002,
                    r#final: 134_218,
                }],
                ..Default::default()
            }),
        };
        let mut update_body = Vec::new();
        update_msg.encode(&mut update_body)?;
        let update_frame = [
            vec![0x01],
            encode_blocks(&[
                ProtoBlock::Bytes {
                    id: 1,
                    data: b".lq.NotifyAccountUpdate".to_vec(),
                },
                ProtoBlock::Bytes {
                    id: 2,
                    data: update_body,
                },
            ]),
        ]
        .concat();

        let match_msg = pb::NotifyMatchGameStart {
            game_url: "route-2.maj-soul.com:443".to_string(),
            connect_token: "tok".to_string(),
            game_uuid: "uuid-1".to_string(),
            match_mode_id: 6,
            location: "route-2".to_string(),
        };
        let mut match_body = Vec::new();
        match_msg.encode(&mut match_body)?;
        let match_frame = [
            vec![0x01],
            encode_blocks(&[
                ProtoBlock::Bytes {
                    id: 1,
                    data: b".lq.NotifyMatchGameStart".to_vec(),
                },
                ProtoBlock::Bytes {
                    id: 2,
                    data: match_body,
                },
            ]),
        ]
        .concat();

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            let _req = ws.next().await.unwrap().unwrap();
            ws.send(WsMessage::Binary(update_frame.into()))
                .await
                .unwrap();
            ws.send(WsMessage::Binary(match_frame.into()))
                .await
                .unwrap();
            ws.send(WsMessage::Binary(vec![0x03, 0x01, 0x00].into()))
                .await
                .unwrap();
        });

        let socket = LiqiSocket::connect(&format!("ws://{addr}/gateway")).await?;
        let mut client = ProtocolClient {
            socket,
            route_id: "route-2".to_string(),
            route_prep: None,
            summary: LoginSummary {
                account_id: 42,
                nickname: "tester".to_string(),
                level_id: 10101,
                level_score: 0,
                rank_tier: 1,
                gold: 88_888,
                target_mode: Mode::FourPlayerSouth,
                target_room: Room::Bronze,
                access_token: "token".to_string(),
            },
        };
        client.socket.request_raw(".lq.Test.echo", b"").await?;
        server.await?;

        client.sync_pending_account_updates();
        assert_eq!(client.summary.gold, 134_218);
        let start = client.wait_for_match_start().await?;
        assert_eq!(start.game_uuid, "uuid-1");
        Ok(())
    }
}
