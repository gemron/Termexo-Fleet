use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransportType {
    Local,
    Lan,
    Relay,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeStatus {
    Online,
    Degraded,
    Offline,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FleetNode {
    pub id: String,
    pub name: String,
    pub transport_type: TransportType,
    pub endpoint: String,
    #[serde(default)]
    pub allow_insecure_tls: bool,
    pub status: NodeStatus,
    pub tags: Vec<String>,
    pub max_concurrency: u32,
    pub last_heartbeat_ms: Option<i64>,
    pub last_error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentType {
    Claude,
    Codex,
    Opencode,
    Pi,
    Grok,
    Antigravity,
}

impl AgentType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Opencode => "opencode",
            Self::Pi => "pi",
            Self::Grok => "grok",
            Self::Antigravity => "antigravity",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "claude" => Some(Self::Claude),
            "codex" => Some(Self::Codex),
            "opencode" => Some(Self::Opencode),
            "pi" => Some(Self::Pi),
            "grok" => Some(Self::Grok),
            "antigravity" => Some(Self::Antigravity),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum DifficultyTier {
    L1,
    L2,
    L3,
    L4,
}

impl DifficultyTier {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::L1 => "L1",
            Self::L2 => "L2",
            Self::L3 => "L3",
            Self::L4 => "L4",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_uppercase().as_str() {
            "L1" => Some(Self::L1),
            "L2" => Some(Self::L2),
            "L3" => Some(Self::L3),
            "L4" => Some(Self::L4),
            _ => None,
        }
    }

    pub fn escalate(self) -> Self {
        match self {
            Self::L1 => Self::L2,
            Self::L2 => Self::L3,
            Self::L3 | Self::L4 => Self::L3,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoutingMode {
    Cost,
    Balanced,
    Quality,
}

impl Default for RoutingMode {
    fn default() -> Self {
        Self::Balanced
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowTemplate {
    Direct,
    ImplementVerifyReview,
    PlannerDag,
}

impl Default for WorkflowTemplate {
    fn default() -> Self {
        Self::Direct
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskRole {
    Planner,
    Worker,
    Reviewer,
    Shell,
}

impl Default for TaskRole {
    fn default() -> Self {
        Self::Worker
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionMode {
    Task,
    Terminal,
}

impl Default for ExecutionMode {
    fn default() -> Self {
        Self::Task
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlotHealth {
    Healthy,
    RateLimited,
    AuthExpired,
    Disabled,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountModelSlot {
    pub id: String,
    pub node_id: String,
    pub label: String,
    pub agent_type: AgentType,
    pub account_profile_id: Option<String>,
    pub model_profile_id: Option<String>,
    pub model_name: String,
    pub model_family: String,
    pub supported_tiers: Vec<DifficultyTier>,
    /// 1 = lowest cost, 5 = highest cost
    pub cost_tier: u8,
    /// 1 = lightweight, 5 = frontier reasoning
    pub quality_tier: u8,
    pub max_concurrency: u32,
    pub active_count: u32,
    pub priority: i32,
    pub health: SlotHealth,
    pub cooldown_until_ms: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuardrailPolicy {
    pub max_depth: u32,
    pub max_concurrency: u32,
    pub max_subagents: u32,
    pub isolate_worktree_by_default: bool,
}

impl Default for GuardrailPolicy {
    fn default() -> Self {
        Self {
            max_depth: 2,
            max_concurrency: 4,
            max_subagents: 12,
            isolate_worktree_by_default: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MissionStatus {
    Draft,
    Running,
    Paused,
    Completed,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    Queued,
    Running,
    WaitingInput,
    WaitingApproval,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mission {
    pub id: String,
    pub title: String,
    pub goal_prompt: String,
    pub node_id: String,
    pub workspace_id: String,
    pub project_id: String,
    pub routing_mode: RoutingMode,
    #[serde(default)]
    pub workflow_template: WorkflowTemplate,
    #[serde(default)]
    pub required_node_tags: Vec<String>,
    pub guardrails: GuardrailPolicy,
    pub fleet_mcp_profile_id: Option<String>,
    pub status: MissionStatus,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MissionTask {
    pub id: String,
    pub mission_id: String,
    pub parent_task_id: Option<String>,
    pub depth: u32,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub required_node_tags: Vec<String>,
    #[serde(default)]
    pub role: TaskRole,
    #[serde(default)]
    pub execution_mode: ExecutionMode,
    #[serde(default)]
    pub preferred_terminal_id: Option<String>,
    pub title: String,
    pub description: String,
    pub acceptance_criteria: String,
    pub difficulty: DifficultyTier,
    pub auto_difficulty: bool,
    pub preferred_agent: Option<AgentType>,
    pub exclude_model_family: Option<String>,
    pub assigned_node_id: Option<String>,
    pub assigned_slot_id: Option<String>,
    pub assigned_agent_type: Option<AgentType>,
    pub assigned_model_name: Option<String>,
    pub assigned_account_profile_id: Option<String>,
    #[serde(default)]
    pub routing_reason: Option<String>,
    pub termexo_task_id: Option<String>,
    pub termexo_terminal_id: Option<String>,
    pub worktree_path: Option<String>,
    pub worktree_branch: Option<String>,
    pub status: TaskStatus,
    pub retry_count: u32,
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    pub result_summary: Option<String>,
    pub diff_stat: Option<String>,
    pub last_error: Option<String>,
    pub parent_token: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

fn default_max_retries() -> u32 {
    2
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedDagStep {
    pub key: String,
    pub title: String,
    pub goal: String,
    #[serde(default)]
    pub depends_on_keys: Vec<String>,
    #[serde(default)]
    pub difficulty: Option<String>,
    #[serde(default)]
    pub role: TaskRole,
    #[serde(default)]
    pub required_node_tags: Vec<String>,
    #[serde(default)]
    pub preferred_agent: Option<AgentType>,
    #[serde(default)]
    pub isolate_worktree: Option<bool>,
    #[serde(default)]
    pub acceptance_criteria: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttentionKind {
    WaitingInput,
    WaitingApproval,
    RateLimited,
    AuthExpired,
    TaskFailed,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttentionItem {
    pub id: String,
    pub node_id: String,
    pub node_name: String,
    pub workspace_id: Option<String>,
    pub terminal_id: Option<String>,
    pub mission_task_id: Option<String>,
    pub title: String,
    pub kind: AttentionKind,
    pub detail: String,
    pub detected_at_ms: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TermexoWorkspaceSummary {
    pub id: String,
    pub name: String,
    pub project_path: String,
    #[serde(default)]
    pub terminal_ids: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TermexoTerminalSummary {
    pub id: String,
    pub workspace_id: String,
    pub name: String,
    pub agent_type: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub live: bool,
    pub account_profile_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TermexoAccountProfileSummary {
    pub id: String,
    pub name: String,
    pub agent_type: String,
    pub authenticated: bool,
    pub is_default: bool,
    #[serde(default)]
    pub is_system: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TermexoModelProfileSummary {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub is_default: bool,
    pub has_credential: bool,
    pub claude_enabled: bool,
    #[serde(default)]
    pub claude_model: String,
    pub codex_enabled: bool,
    #[serde(default)]
    pub codex_model: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TermexoMcpProfileSummary {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TermexoSettingsSnapshot {
    #[serde(default)]
    pub model_profiles: Vec<TermexoModelProfileSummary>,
    #[serde(default)]
    pub account_profiles: Vec<TermexoAccountProfileSummary>,
    #[serde(default)]
    pub mcp_profiles: Vec<TermexoMcpProfileSummary>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeLiveSnapshot {
    pub node_id: String,
    pub workspaces: Vec<TermexoWorkspaceSummary>,
    pub terminals: Vec<TermexoTerminalSummary>,
    pub settings: TermexoSettingsSnapshot,
    pub synced_at_ms: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeProbeResult {
    pub reachable: bool,
    pub latency_ms: u64,
    pub workspace_count: usize,
    pub terminal_count: usize,
    pub supports_account_routing: bool,
    pub supports_mcp_profiles: bool,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionRecord {
    pub id: String,
    pub mission_id: String,
    pub task_id: String,
    pub task_title: String,
    pub node_id: String,
    pub slot_id: Option<String>,
    pub agent_type: Option<AgentType>,
    pub model_name: String,
    pub difficulty: DifficultyTier,
    pub status: TaskStatus,
    pub started_at_ms: i64,
    pub finished_at_ms: i64,
    pub duration_ms: i64,
    pub retry_count: u32,
    pub escalated: bool,
    /// Relative cost units (L1=1, L2=3, L3=8, L4=8) used to measure routing efficiency.
    pub estimated_cost_units: u32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyticsBucket {
    pub key: String,
    pub total: u32,
    pub completed: u32,
    pub failed: u32,
    pub avg_duration_ms: i64,
    pub cost_units: u32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FleetAnalytics {
    pub total_runs: u32,
    pub completed_runs: u32,
    pub failed_runs: u32,
    pub success_rate_percent: u32,
    pub avg_duration_ms: i64,
    pub escalated_runs: u32,
    pub total_cost_units: u32,
    pub baseline_l3_cost_units: u32,
    pub estimated_savings_percent: u32,
    pub by_tier: Vec<AnalyticsBucket>,
    pub by_model: Vec<AnalyticsBucket>,
    pub by_node: Vec<AnalyticsBucket>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationSettings {
    pub webhook_enabled: bool,
    pub webhook_url: String,
    pub wechat_via_termexo: bool,
    pub notify_on_completed: bool,
    pub notify_on_failed: bool,
    pub notify_on_approval: bool,
    pub notify_on_slot_unhealthy: bool,
}

impl Default for NotificationSettings {
    fn default() -> Self {
        Self {
            webhook_enabled: false,
            webhook_url: String::new(),
            wechat_via_termexo: false,
            notify_on_completed: true,
            notify_on_failed: true,
            notify_on_approval: true,
            notify_on_slot_unhealthy: true,
        }
    }
}
