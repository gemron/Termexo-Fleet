use std::collections::HashMap;

use fleet_protocol::{
    AccountModelSlot, AgentType, DifficultyTier, FleetNode, NodeStatus, RoutingMode, SlotHealth,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RouterError {
    #[error("No healthy account/model slot available for tier {0:?}")]
    NoEligibleSlot(DifficultyTier),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteRequest {
    pub difficulty: DifficultyTier,
    pub mode: RoutingMode,
    pub preferred_node_id: Option<String>,
    #[serde(default)]
    pub required_node_tags: Vec<String>,
    pub preferred_agent: Option<AgentType>,
    /// Used by L4 cross-model reviews to avoid choosing the same family as the author.
    pub exclude_model_family: Option<String>,
    pub now_ms: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteDecision {
    pub slot_id: String,
    pub node_id: String,
    pub agent_type: AgentType,
    pub account_profile_id: Option<String>,
    pub model_profile_id: Option<String>,
    pub model_name: String,
    pub model_family: String,
    pub reason: String,
}

/// Infers a coarse model family string so L4 reviews can enforce cross-family diversity.
pub fn infer_model_family(agent: &AgentType, model_name: &str) -> String {
    let lower = model_name.to_ascii_lowercase();
    if lower.contains("claude") || lower.contains("sonnet") || lower.contains("opus") || lower.contains("haiku") {
        return "anthropic".into();
    }
    if lower.contains("gpt")
        || lower.contains("o1")
        || lower.contains("o3")
        || lower.contains("o4")
        || lower.contains("codex")
        || lower.contains("sol")
        || lower.contains("astra")
    {
        return "openai".into();
    }
    if lower.contains("gemini") {
        return "google".into();
    }
    if lower.contains("deepseek") {
        return "deepseek".into();
    }
    if lower.contains("glm") {
        return "glm".into();
    }
    if lower.contains("kimi") {
        return "kimi".into();
    }
    if lower.contains("minimax") {
        return "minimax".into();
    }
    if lower.contains("grok") {
        return "xai".into();
    }
    match agent {
        AgentType::Claude => "anthropic".into(),
        AgentType::Codex => "openai".into(),
        AgentType::Antigravity => "google".into(),
        AgentType::Grok => "xai".into(),
        AgentType::Opencode => "opencode".into(),
        AgentType::Pi => "pi".into(),
    }
}

/// Evaluates task difficulty automatically from title, description and failure history.
pub fn evaluate_difficulty(title: &str, description: &str, retry_count: u32) -> DifficultyTier {
    let text = format!("{title}\n{description}").to_ascii_lowercase();

    let review_keywords = [
        "review", "audit", "security", "审查", "评审", "安全检查", "交叉验证", "验收",
    ];
    if review_keywords.iter().any(|kw| text.contains(kw)) {
        return DifficultyTier::L4;
    }

    let complex_keywords = [
        "architect", "refactor", "concurrency", "deadlock", "race condition", "migration",
        "performance", "memory leak", "distributed", "架构", "重构", "并发", "死锁", "性能", "迁移", "底层",
    ];
    let mut tier = if complex_keywords.iter().any(|kw| text.contains(kw)) || description.chars().count() > 600 {
        DifficultyTier::L3
    } else {
        let light_keywords = [
            "typo", "readme", "comment", "format", "lint", "rename", "copy", "文案", "注释", "格式化", "查看", "列出",
        ];
        if light_keywords.iter().any(|kw| text.contains(kw)) && description.chars().count() < 200 {
            DifficultyTier::L1
        } else {
            DifficultyTier::L2
        }
    };

    // Automatic escalation after 2 consecutive failures.
    if retry_count >= 2 {
        tier = tier.escalate();
    }
    tier
}

/// Checks whether a slot is healthy and has capacity at `now_ms`.
pub fn is_slot_available(slot: &AccountModelSlot, now_ms: i64) -> bool {
    if slot.active_count >= slot.max_concurrency.max(1) {
        return false;
    }
    match slot.health {
        SlotHealth::Healthy => true,
        SlotHealth::RateLimited => slot
            .cooldown_until_ms
            .is_some_and(|until| now_ms >= until),
        SlotHealth::AuthExpired | SlotHealth::Disabled => false,
    }
}

/// Single-node / compatibility helper that routes across slots without node metadata.
pub fn select_slot(
    slots: &[AccountModelSlot],
    request: &RouteRequest,
) -> Result<RouteDecision, RouterError> {
    select_slot_across_fleet(&[], slots, request)
}

/// Selects the best `(node, account, model)` slot across one or more Termexo nodes,
/// enforcing node online status, affinity tags, node concurrency ceilings, cross-node load
/// balancing, and L4 cross-family review diversity.
pub fn select_slot_across_fleet(
    nodes: &[FleetNode],
    slots: &[AccountModelSlot],
    request: &RouteRequest,
) -> Result<RouteDecision, RouterError> {
    let node_map: HashMap<&str, &FleetNode> = nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let mut node_active: HashMap<&str, u32> = HashMap::new();
    for slot in slots {
        *node_active.entry(slot.node_id.as_str()).or_insert(0) += slot.active_count;
    }

    // Try with preferred_node_id first; if that node is offline/full, failover across the fleet.
    let mut candidates = filter_candidates(&node_map, &node_active, slots, request, true, true);
    let mut failover_used = false;
    if candidates.is_empty() && request.preferred_node_id.is_some() {
        candidates = filter_candidates(&node_map, &node_active, slots, request, true, false);
        failover_used = !candidates.is_empty();
    }

    let (chosen_pool, relaxed_family) = if !candidates.is_empty() {
        (candidates, false)
    } else if request.exclude_model_family.is_some() {
        let relaxed = filter_candidates(&node_map, &node_active, slots, request, false, true);
        if !relaxed.is_empty() {
            (relaxed, true)
        } else {
            let failover_relaxed =
                filter_candidates(&node_map, &node_active, slots, request, false, false);
            failover_used = !failover_relaxed.is_empty();
            (failover_relaxed, true)
        }
    } else {
        (Vec::new(), false)
    };

    let best = chosen_pool
        .into_iter()
        .max_by_key(|slot| score_slot(slot, request, &node_map, &node_active))
        .ok_or(RouterError::NoEligibleSlot(request.difficulty))?;

    let reason = build_reason(best, request, relaxed_family, failover_used);
    Ok(RouteDecision {
        slot_id: best.id.clone(),
        node_id: best.node_id.clone(),
        agent_type: best.agent_type.clone(),
        account_profile_id: best.account_profile_id.clone(),
        model_profile_id: best.model_profile_id.clone(),
        model_name: best.model_name.clone(),
        model_family: best.model_family.clone(),
        reason,
    })
}

fn node_matches_constraints(
    node_map: &HashMap<&str, &FleetNode>,
    node_active: &HashMap<&str, u32>,
    node_id: &str,
    required_tags: &[String],
) -> bool {
    if node_map.is_empty() {
        return required_tags.is_empty();
    }
    let Some(node) = node_map.get(node_id) else {
        return false;
    };
    if node.status != NodeStatus::Online {
        return false;
    }
    let active = node_active.get(node_id).copied().unwrap_or(0);
    if active >= node.max_concurrency.max(1) {
        return false;
    }
    for required in required_tags {
        let needle = required.trim();
        if needle.is_empty() {
            continue;
        }
        if !node
            .tags
            .iter()
            .any(|tag| tag.eq_ignore_ascii_case(needle))
        {
            return false;
        }
    }
    true
}

fn filter_candidates<'a>(
    node_map: &HashMap<&str, &FleetNode>,
    node_active: &HashMap<&str, u32>,
    slots: &'a [AccountModelSlot],
    request: &RouteRequest,
    enforce_family_exclusion: bool,
    enforce_preferred_node: bool,
) -> Vec<&'a AccountModelSlot> {
    let filter_with_tier = |strict_tier: bool| -> Vec<&'a AccountModelSlot> {
        slots
            .iter()
            .filter(|slot| is_slot_available(slot, request.now_ms))
            .filter(|slot| {
                node_matches_constraints(
                    node_map,
                    node_active,
                    &slot.node_id,
                    &request.required_node_tags,
                )
            })
            .filter(|slot| {
                if !enforce_preferred_node {
                    return true;
                }
                request
                    .preferred_node_id
                    .as_deref()
                    .is_none_or(|node| slot.node_id == node)
            })
            .filter(|slot| {
                request
                    .preferred_agent
                    .as_ref()
                    .is_none_or(|agent| &slot.agent_type == agent)
            })
            .filter(|slot| {
                !strict_tier
                    || slot.supported_tiers.is_empty()
                    || slot.supported_tiers.contains(&request.difficulty)
            })
            .filter(|slot| {
                if !enforce_family_exclusion {
                    return true;
                }
                request
                    .exclude_model_family
                    .as_deref()
                    .is_none_or(|excluded| !slot.model_family.eq_ignore_ascii_case(excluded))
            })
            .collect()
    };

    let exact = filter_with_tier(true);
    if !exact.is_empty() {
        exact
    } else {
        filter_with_tier(false)
    }
}

fn score_slot(
    slot: &AccountModelSlot,
    request: &RouteRequest,
    node_map: &HashMap<&str, &FleetNode>,
    node_active: &HashMap<&str, u32>,
) -> i64 {
    let tier_match = if slot.supported_tiers.contains(&request.difficulty) {
        1000
    } else {
        0
    };
    let mode_score: i64 = match request.mode {
        RoutingMode::Cost => (6 - i64::from(slot.cost_tier.min(5))) * 120 + i64::from(slot.quality_tier) * 20,
        RoutingMode::Balanced => {
            let ideal_quality: i64 = match request.difficulty {
                DifficultyTier::L1 => 2,
                DifficultyTier::L2 => 3,
                DifficultyTier::L3 | DifficultyTier::L4 => 5,
            };
            let quality_fit = 150 - (i64::from(slot.quality_tier) - ideal_quality).abs() * 35;
            let cost_bonus = (6 - i64::from(slot.cost_tier.min(5))) * 30;
            quality_fit + cost_bonus
        }
        RoutingMode::Quality => i64::from(slot.quality_tier) * 140 - i64::from(slot.cost_tier) * 10,
    };
    let load_penalty = i64::from(slot.active_count) * 60;
    let node_headroom_bonus = node_map
        .get(slot.node_id.as_str())
        .map(|node| {
            let used = node_active.get(slot.node_id.as_str()).copied().unwrap_or(0);
            i64::from(node.max_concurrency.saturating_sub(used)) * 25
        })
        .unwrap_or(0);
    let priority_bonus = i64::from(slot.priority) * 10;
    let cooldown_penalty = if slot.health == SlotHealth::RateLimited { 50 } else { 0 };
    tier_match + mode_score + priority_bonus + node_headroom_bonus - load_penalty - cooldown_penalty
}

fn build_reason(
    slot: &AccountModelSlot,
    request: &RouteRequest,
    relaxed_family: bool,
    failover_used: bool,
) -> String {
    let mut parts = vec![
        format!("tier={}", request.difficulty.as_str()),
        format!("mode={:?}", request.mode).to_ascii_lowercase(),
        format!("node={}", slot.node_id),
        format!("slot={}({})", slot.label, slot.model_name),
    ];
    if failover_used {
        parts.push("node_failover".into());
    }
    if !request.required_node_tags.is_empty() {
        parts.push(format!("tags=[{}]", request.required_node_tags.join(",")));
    }
    if let Some(excluded) = &request.exclude_model_family {
        if relaxed_family {
            parts.push(format!("cross_family_relaxed(no alternative to {excluded})"));
        } else {
            parts.push(format!("cross_family({}!={excluded})", slot.model_family));
        }
    }
    parts.join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use fleet_protocol::TransportType;

    fn sample_node(id: &str, status: NodeStatus, tags: &[&str], max_concurrency: u32) -> FleetNode {
        FleetNode {
            id: id.into(),
            name: id.into(),
            transport_type: TransportType::Local,
            endpoint: format!("http://{id}:7421/mcp"),
            allow_insecure_tls: false,
            status,
            tags: tags.iter().map(|s| (*s).to_owned()).collect(),
            max_concurrency,
            last_heartbeat_ms: Some(1000),
            last_error: None,
        }
    }

    fn sample_slot(
        id: &str,
        node_id: &str,
        agent: AgentType,
        model: &str,
        tiers: Vec<DifficultyTier>,
        cost: u8,
        quality: u8,
    ) -> AccountModelSlot {
        let family = infer_model_family(&agent, model);
        AccountModelSlot {
            id: id.into(),
            node_id: node_id.into(),
            label: id.into(),
            agent_type: agent,
            account_profile_id: Some(format!("acc-{id}")),
            model_profile_id: Some(format!("prof-{id}")),
            model_name: model.into(),
            model_family: family,
            supported_tiers: tiers,
            cost_tier: cost,
            quality_tier: quality,
            max_concurrency: 2,
            active_count: 0,
            priority: 10,
            health: SlotHealth::Healthy,
            cooldown_until_ms: None,
        }
    }

    #[test]
    fn evaluates_difficulty_and_escalates_on_retries() {
        assert_eq!(evaluate_difficulty("Fix README typo", "update link", 0), DifficultyTier::L1);
        assert_eq!(evaluate_difficulty("Fix README typo", "update link", 2), DifficultyTier::L2);
        assert_eq!(evaluate_difficulty("Add user profile API", "CRUD endpoint", 0), DifficultyTier::L2);
        assert_eq!(evaluate_difficulty("Add user profile API", "CRUD endpoint", 2), DifficultyTier::L3);
        assert_eq!(evaluate_difficulty("Refactor scheduler concurrency", "deadlock fix", 0), DifficultyTier::L3);
        assert_eq!(evaluate_difficulty("Security review PR #42", "check auth bypass", 0), DifficultyTier::L4);
    }

    #[test]
    fn routes_by_difficulty_cost_and_cross_family_review() {
        let slots = vec![
            sample_slot("cheap", "local", AgentType::Opencode, "deepseek-v4-flash", vec![DifficultyTier::L1], 1, 2),
            sample_slot("claude-sonnet", "local", AgentType::Claude, "claude-sonnet-4-6", vec![DifficultyTier::L2], 3, 4),
            sample_slot("claude-opus", "local", AgentType::Claude, "claude-opus-4-6", vec![DifficultyTier::L3, DifficultyTier::L4], 5, 5),
            sample_slot("codex-pro", "local", AgentType::Codex, "gpt-5.4", vec![DifficultyTier::L3, DifficultyTier::L4], 4, 5),
        ];

        let l1 = select_slot(
            &slots,
            &RouteRequest {
                difficulty: DifficultyTier::L1,
                mode: RoutingMode::Balanced,
                preferred_node_id: None,
                required_node_tags: vec![],
                preferred_agent: None,
                exclude_model_family: None,
                now_ms: 1000,
            },
        )
        .unwrap();
        assert_eq!(l1.slot_id, "cheap");

        let l4 = select_slot(
            &slots,
            &RouteRequest {
                difficulty: DifficultyTier::L4,
                mode: RoutingMode::Quality,
                preferred_node_id: None,
                required_node_tags: vec![],
                preferred_agent: None,
                exclude_model_family: Some("anthropic".into()),
                now_ms: 1000,
            },
        )
        .unwrap();
        assert_eq!(l4.slot_id, "codex-pro");
        assert_eq!(l4.model_family, "openai");
    }

    #[test]
    fn enforces_multi_node_tag_affinity_load_balancing_and_offline_failover() {
        let nodes = vec![
            sample_node("win-main", NodeStatus::Offline, &["windows", "gpu"], 4),
            sample_node("mac-build", NodeStatus::Online, &["macos", "ios"], 4),
            sample_node("linux-ci", NodeStatus::Online, &["linux", "gpu"], 4),
        ];
        let mut s_win = sample_slot("s-win", "win-main", AgentType::Claude, "claude-opus-4-6", vec![DifficultyTier::L3], 5, 5);
        s_win.priority = 50;
        let s_mac = sample_slot("s-mac", "mac-build", AgentType::Claude, "claude-opus-4-6", vec![DifficultyTier::L3], 5, 5);
        let s_linux = sample_slot("s-linux", "linux-ci", AgentType::Claude, "claude-opus-4-6", vec![DifficultyTier::L3], 5, 5);
        let slots = vec![s_win, s_mac, s_linux];

        // Preferred node win-main is offline; requiring "gpu" fails over to linux-ci!
        let gpu_route = select_slot_across_fleet(
            &nodes,
            &slots,
            &RouteRequest {
                difficulty: DifficultyTier::L3,
                mode: RoutingMode::Quality,
                preferred_node_id: Some("win-main".into()),
                required_node_tags: vec!["gpu".into()],
                preferred_agent: None,
                exclude_model_family: None,
                now_ms: 1000,
            },
        )
        .unwrap();
        assert_eq!(gpu_route.node_id, "linux-ci");
        assert!(gpu_route.reason.contains("node_failover"));

        // Requiring "macos" picks mac-build.
        let mac_route = select_slot_across_fleet(
            &nodes,
            &slots,
            &RouteRequest {
                difficulty: DifficultyTier::L3,
                mode: RoutingMode::Balanced,
                preferred_node_id: None,
                required_node_tags: vec!["macos".into()],
                preferred_agent: None,
                exclude_model_family: None,
                now_ms: 1000,
            },
        )
        .unwrap();
        assert_eq!(mac_route.node_id, "mac-build");
    }

    #[test]
    fn fails_over_when_slot_is_rate_limited_or_expired_and_recovers_after_cooldown() {
        let mut primary = sample_slot("primary", "local", AgentType::Claude, "claude-opus-4-6", vec![DifficultyTier::L3], 5, 5);
        primary.priority = 50;
        primary.health = SlotHealth::RateLimited;
        primary.cooldown_until_ms = Some(5000);

        let mut expired = sample_slot("expired", "local", AgentType::Claude, "claude-opus-4-6", vec![DifficultyTier::L3], 5, 5);
        expired.health = SlotHealth::AuthExpired;

        let backup = sample_slot("backup", "local", AgentType::Codex, "gpt-5.4", vec![DifficultyTier::L3], 4, 5);
        let slots = vec![primary, expired, backup];

        let during = select_slot(
            &slots,
            &RouteRequest {
                difficulty: DifficultyTier::L3,
                mode: RoutingMode::Quality,
                preferred_node_id: None,
                required_node_tags: vec![],
                preferred_agent: None,
                exclude_model_family: None,
                now_ms: 2000,
            },
        )
        .unwrap();
        assert_eq!(during.slot_id, "backup");

        let after = select_slot(
            &slots,
            &RouteRequest {
                difficulty: DifficultyTier::L3,
                mode: RoutingMode::Quality,
                preferred_node_id: None,
                required_node_tags: vec![],
                preferred_agent: None,
                exclude_model_family: None,
                now_ms: 6000,
            },
        )
        .unwrap();
        assert_eq!(after.slot_id, "primary");
    }
}
