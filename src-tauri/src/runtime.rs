use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use fleet_connector::{
    classify_terminal_output, CreateTermexoTaskRequest, TermexoMcpClient, TermexoTaskBoardSnapshot,
    TermexoTaskRecord, TerminalSignal,
};
use fleet_mcp_server::{BoxFuture, FleetMcpHandler, RunningMcpServer};
use fleet_orchestrator::{
    build_fleet_task_description, build_upstream_handoff, check_spawn_guardrails,
    collect_worktree_diff_stat, create_isolated_worktree, find_ready_dag_tasks,
    validate_dag_acyclic,
};
use fleet_protocol::{
    AccountModelSlot, AgentType, AnalyticsBucket, AttentionItem, AttentionKind, DifficultyTier,
    ExecutionMode, ExecutionRecord, FleetAnalytics, FleetNode, GuardrailPolicy, Mission,
    MissionStatus, MissionTask, NodeLiveSnapshot, NodeProbeResult, NodeStatus,
    NotificationSettings, PlannedDagStep, RoutingMode, SlotHealth, TaskRole, TaskStatus,
    TermexoSettingsSnapshot, TermexoTerminalSummary, TermexoWorkspaceSummary, TransportType,
    WorkflowTemplate,
};
use fleet_router::{
    evaluate_difficulty, infer_model_family, select_slot_across_fleet, RouteRequest,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::{Mutex, RwLock};

use crate::storage::{delete_secret, load_secret, save_secret, FleetDatabase};

const DEFAULT_FLEET_MCP_PORT: u16 = 7425;
const RATE_LIMIT_COOLDOWN_MS: i64 = 5 * 60 * 1000;
const FLEET_MCP_TOKEN_KEY: &str = "fleet-mcp-server-token";
const FLEET_MCP_REMOTE_URL_KEY: &str = "fleet_mcp_remote_url";
const NOTIFICATION_SETTINGS_KEY: &str = "notification_settings";

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FleetStateOverview {
    pub nodes: Vec<FleetNode>,
    pub snapshots: HashMap<String, NodeLiveSnapshot>,
    pub slots: Vec<AccountModelSlot>,
    pub missions: Vec<Mission>,
    pub tasks: Vec<MissionTask>,
    pub attention: Vec<AttentionItem>,
    pub execution_records: Vec<ExecutionRecord>,
    pub analytics: FleetAnalytics,
    pub notification_settings: NotificationSettings,
    pub mcp_server_url: String,
    pub mcp_remote_url: Option<String>,
    pub mcp_server_token: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpsertNodeInput {
    pub id: Option<String>,
    pub name: String,
    pub transport_type: TransportType,
    pub endpoint: String,
    pub token: Option<String>,
    pub tags: Vec<String>,
    pub max_concurrency: u32,
    #[serde(default)]
    pub allow_insecure_tls: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateMissionInput {
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
    #[serde(default)]
    pub execution_mode: ExecutionMode,
    #[serde(default)]
    pub preferred_terminal_id: Option<String>,
    pub difficulty: Option<DifficultyTier>,
    pub preferred_agent: Option<AgentType>,
    pub guardrails: GuardrailPolicy,
    pub fleet_mcp_profile_id: Option<String>,
    pub auto_dispatch: bool,
}

pub struct FleetRuntime {
    pub db: Arc<FleetDatabase>,
    operations: Mutex<()>,
    snapshots: RwLock<HashMap<String, NodeLiveSnapshot>>,
    attention: RwLock<Vec<AttentionItem>>,
    token_cache: RwLock<HashMap<String, String>>,
    mcp_token: String,
    mcp_url: RwLock<String>,
    mcp_server: RwLock<Option<RunningMcpServer>>,
}

impl FleetRuntime {
    pub fn new(db: Arc<FleetDatabase>) -> Result<Arc<Self>, String> {
        let mcp_token = match load_secret(FLEET_MCP_TOKEN_KEY).ok().flatten() {
            Some(existing) => existing,
            None => {
                let generated = format!("tf_{}", uuid::Uuid::new_v4().simple());
                let _ = save_secret(FLEET_MCP_TOKEN_KEY, &generated);
                generated
            }
        };
        Ok(Arc::new(Self {
            db,
            operations: Mutex::new(()),
            snapshots: RwLock::new(HashMap::new()),
            attention: RwLock::new(Vec::new()),
            token_cache: RwLock::new(HashMap::new()),
            mcp_token,
            mcp_url: RwLock::new(format!("http://127.0.0.1:{DEFAULT_FLEET_MCP_PORT}/mcp")),
            mcp_server: RwLock::new(None),
        }))
    }

    pub async fn start_mcp_server(self: &Arc<Self>) -> Result<(), String> {
        let port = self
            .db
            .read_setting("fleet_mcp_port")?
            .and_then(|v| v.parse::<u16>().ok())
            .unwrap_or(DEFAULT_FLEET_MCP_PORT);
        let running = fleet_mcp_server::start_local_server(port, self.clone()).await?;
        *self.mcp_url.write().await = running.url();
        *self.mcp_server.write().await = Some(running);
        Ok(())
    }

    pub async fn overview(&self) -> Result<FleetStateOverview, String> {
        let execution_records = self.db.list_execution_records()?;
        let analytics = compute_analytics(&execution_records);
        let notification_settings = self.notification_settings();
        Ok(FleetStateOverview {
            nodes: self.db.list_nodes()?,
            snapshots: self.snapshots.read().await.clone(),
            slots: self.db.list_slots()?,
            missions: self.db.list_missions()?,
            tasks: self.db.list_tasks()?,
            attention: self.attention.read().await.clone(),
            execution_records,
            analytics,
            notification_settings,
            mcp_server_url: self.mcp_url.read().await.clone(),
            mcp_remote_url: self
                .db
                .read_setting(FLEET_MCP_REMOTE_URL_KEY)?
                .filter(|url| !url.is_empty()),
            mcp_server_token: self.mcp_token.clone(),
        })
    }

    pub fn save_mcp_remote_url(&self, input: &str) -> Result<Option<String>, String> {
        let url = validate_remote_mcp_url(input)?;
        self.db
            .write_setting(FLEET_MCP_REMOTE_URL_KEY, url.as_deref().unwrap_or_default())?;
        Ok(url)
    }

    pub async fn probe_node(
        &self,
        endpoint: &str,
        token: &str,
        allow_insecure_tls: bool,
    ) -> NodeProbeResult {
        let endpoint = endpoint.trim();
        if endpoint.is_empty() {
            return NodeProbeResult {
                reachable: false,
                latency_ms: 0,
                workspace_count: 0,
                terminal_count: 0,
                supports_account_routing: false,
                supports_mcp_profiles: false,
                error: Some("Node MCP endpoint is required".into()),
            };
        }
        match TermexoMcpClient::new_with_options(endpoint, token.trim(), allow_insecure_tls) {
            Ok(client) => client.probe().await,
            Err(err) => NodeProbeResult {
                reachable: false,
                latency_ms: 0,
                workspace_count: 0,
                terminal_count: 0,
                supports_account_routing: false,
                supports_mcp_profiles: false,
                error: Some(err.to_string()),
            },
        }
    }

    pub fn notification_settings(&self) -> NotificationSettings {
        self.db
            .read_setting(NOTIFICATION_SETTINGS_KEY)
            .ok()
            .flatten()
            .and_then(|raw| serde_json::from_str::<NotificationSettings>(&raw).ok())
            .unwrap_or_default()
    }

    pub async fn save_notification_settings(
        &self,
        settings: NotificationSettings,
    ) -> Result<NotificationSettings, String> {
        let payload = serde_json::to_string(&settings)
            .map_err(|e| format!("Failed to serialize notification settings: {e}"))?;
        self.db.write_setting(NOTIFICATION_SETTINGS_KEY, &payload)?;
        Ok(settings)
    }

    pub async fn send_test_notification(&self) -> Result<String, String> {
        let settings = self.notification_settings();
        if !settings.webhook_enabled && !settings.wechat_via_termexo {
            return Err("请先启用 Webhook 或 Termexo 微信联动中的至少一项通知通道".into());
        }
        let mut delivered = Vec::new();
        let mut errors = Vec::new();

        if settings.webhook_enabled {
            let url = settings.webhook_url.trim();
            if url.is_empty() {
                errors.push("Webhook URL 为空".to_owned());
            } else {
                match send_webhook_json(
                    url,
                    "test",
                    "Termexo Fleet 测试通知",
                    "这是一条来自 Termexo Fleet 指挥官的连通性测试告警。",
                )
                .await
                {
                    Ok(()) => delivered.push(format!("Webhook ({url})")),
                    Err(err) => errors.push(format!("Webhook 失败: {err}")),
                }
            }
        }

        if settings.wechat_via_termexo {
            match self
                .dispatch_wechat_via_termexo(
                    "test",
                    "Termexo Fleet 测试通知",
                    None,
                    None,
                    None,
                )
                .await
            {
                Ok(node_name) => delivered.push(format!("Termexo 微信联动 ({node_name})")),
                Err(err) => errors.push(format!("Termexo 微信联动失败: {err}")),
            }
        }

        if delivered.is_empty() {
            Err(errors.join("；"))
        } else if errors.is_empty() {
            Ok(format!("已成功发送测试通知至：{}", delivered.join("、")))
        } else {
            Ok(format!(
                "部分发送成功（{}），存在告警：{}",
                delivered.join("、"),
                errors.join("；")
            ))
        }
    }

    pub async fn upsert_node(&self, input: UpsertNodeInput) -> Result<FleetNode, String> {
        let _operation = self.operations.lock().await;
        let id = input
            .id
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| format!("node-{}", &uuid::Uuid::new_v4().simple().to_string()[..8]));
        let endpoint = input.endpoint.trim().to_owned();
        if endpoint.is_empty() {
            return Err("Node MCP endpoint is required".into());
        }
        let node = FleetNode {
            id: id.clone(),
            name: if input.name.trim().is_empty() {
                "Local Termexo".into()
            } else {
                input.name.trim().to_owned()
            },
            transport_type: input.transport_type,
            endpoint,
            allow_insecure_tls: input.allow_insecure_tls,
            status: NodeStatus::Offline,
            tags: input.tags,
            max_concurrency: input.max_concurrency.max(1),
            last_heartbeat_ms: None,
            last_error: None,
        };
        let token = input
            .token
            .as_deref()
            .map(str::trim)
            .filter(|token| !token.is_empty());
        let client = if let Some(token) = token {
            TermexoMcpClient::new_with_options(&node.endpoint, token, node.allow_insecure_tls)
                .map_err(|error| error.to_string())?
        } else {
            self.client_for_node(&node).await?
        };
        let workspaces = client
            .workspace_list()
            .await
            .map_err(|error| error.to_string())?;
        if let Some(token) = token {
            save_secret(&node_token_key(&id), token)?;
            self.token_cache.write().await.insert(id, token.to_owned());
        }
        self.sync_connected_node(node, &client, workspaces).await
    }

    pub async fn remove_node(&self, node_id: &str) -> Result<(), String> {
        let _operation = self.operations.lock().await;
        if self.db.list_tasks()?.iter().any(|task| {
            task.assigned_node_id.as_deref() == Some(node_id) && task.status.is_active()
        }) {
            return Err("Stop the active tasks on this node before removing it".into());
        }
        let _ = delete_secret(&node_token_key(node_id));
        self.token_cache.write().await.remove(node_id);
        self.snapshots.write().await.remove(node_id);
        self.db.delete_node(node_id)
    }

    pub async fn sync_node(&self, node_id: &str) -> Result<FleetNode, String> {
        let _operation = self.operations.lock().await;
        self.sync_node_locked(node_id).await
    }

    async fn sync_node_locked(&self, node_id: &str) -> Result<FleetNode, String> {
        let mut node = self
            .db
            .list_nodes()?
            .into_iter()
            .find(|n| n.id == node_id)
            .ok_or_else(|| format!("Node not found: {node_id}"))?;

        let client = match self.client_for_node(&node).await {
            Ok(client) => client,
            Err(error) => {
                node.status = NodeStatus::Offline;
                node.last_error = Some(error.clone());
                self.db.save_node(&node)?;
                return Err(error);
            }
        };

        let workspaces = match client.workspace_list().await {
            Ok(list) => list,
            Err(error) => {
                node.status = NodeStatus::Offline;
                node.last_error = Some(error.to_string());
                self.db.save_node(&node)?;
                return Err(error.to_string());
            }
        };
        self.sync_connected_node(node, &client, workspaces).await
    }

    async fn sync_connected_node(
        &self,
        mut node: FleetNode,
        client: &TermexoMcpClient,
        workspaces: Vec<TermexoWorkspaceSummary>,
    ) -> Result<FleetNode, String> {
        let terminals = client.terminal_list(None).await.unwrap_or_default();
        let settings = client.settings_get().await.unwrap_or_default();
        let now = now_ms();

        node.status = NodeStatus::Online;
        node.last_heartbeat_ms = Some(now);
        node.last_error = None;
        self.db.save_node(&node)?;

        self.sync_slots_from_settings(&node.id, &settings)?;
        self.sync_running_tasks(&node, client, &workspaces, &terminals)
            .await?;

        let snapshot = NodeLiveSnapshot {
            node_id: node.id.clone(),
            workspaces,
            terminals,
            settings,
            synced_at_ms: now,
        };
        self.snapshots
            .write()
            .await
            .insert(node.id.clone(), snapshot);
        Ok(node)
    }

    pub async fn sync_all_nodes(&self) -> Result<(), String> {
        let nodes = self.db.list_nodes()?;
        for node in nodes {
            let _ = self.sync_node(&node.id).await;
        }
        let missions = self.db.list_missions()?;
        for mission in missions {
            if mission.status == MissionStatus::Running {
                let _ = self.advance_mission_dag(&mission.id).await;
            }
        }
        Ok(())
    }

    pub async fn save_slot(&self, slot: AccountModelSlot) -> Result<AccountModelSlot, String> {
        let _operation = self.operations.lock().await;
        let mut slot = slot;
        if let Some(current) = self
            .db
            .list_slots()?
            .into_iter()
            .find(|item| item.id == slot.id)
        {
            slot.active_count = current.active_count;
        }
        self.db.save_slot(&slot)?;
        Ok(slot)
    }

    pub async fn reset_slot_health(&self, slot_id: &str) -> Result<AccountModelSlot, String> {
        let _operation = self.operations.lock().await;
        let mut slot = self
            .db
            .list_slots()?
            .into_iter()
            .find(|s| s.id == slot_id)
            .ok_or_else(|| format!("Slot not found: {slot_id}"))?;
        slot.health = SlotHealth::Healthy;
        slot.cooldown_until_ms = None;
        self.db.save_slot(&slot)?;
        Ok(slot)
    }

    pub async fn create_mission(&self, input: CreateMissionInput) -> Result<(Mission, MissionTask), String> {
        let _operation = self.operations.lock().await;
        if input.workflow_template == WorkflowTemplate::PlannerDag
            && input.execution_mode == ExecutionMode::Terminal
        {
            return Err(
                "Planner DAG requires task-board execution to attach the Fleet MCP profile".into(),
            );
        }
        let now = now_ms();
        let mission_id = format!("m-{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
        let (base_difficulty, auto_difficulty) = match input.difficulty {
            Some(tier) => (tier, false),
            None => (
                evaluate_difficulty(&input.title, &input.goal_prompt, 0),
                true,
            ),
        };
        let mission = Mission {
            id: mission_id.clone(),
            title: input.title.trim().to_owned(),
            goal_prompt: input.goal_prompt.trim().to_owned(),
            node_id: input.node_id.clone(),
            workspace_id: input.workspace_id.clone(),
            workspace_name: self
                .snapshots
                .read()
                .await
                .get(&input.node_id)
                .and_then(|snapshot| {
                    snapshot
                        .workspaces
                        .iter()
                        .find(|workspace| workspace.id == input.workspace_id)
                })
                .map(|workspace| workspace.name.clone()),
            project_id: input.project_id.clone(),
            routing_mode: input.routing_mode,
            workflow_template: input.workflow_template,
            required_node_tags: input.required_node_tags.clone(),
            guardrails: input.guardrails,
            fleet_mcp_profile_id: input.fleet_mcp_profile_id.filter(|v| !v.trim().is_empty()),
            status: MissionStatus::Draft,
            created_at_ms: now,
            updated_at_ms: now,
        };

        let created_tasks = match input.workflow_template {
            WorkflowTemplate::Direct => vec![new_mission_task(
                &mission,
                None,
                0,
                vec![],
                TaskRole::Worker,
                input.execution_mode,
                input.preferred_terminal_id.clone(),
                mission.title.clone(),
                mission.goal_prompt.clone(),
                String::new(),
                base_difficulty,
                auto_difficulty,
                input.preferred_agent,
                input.required_node_tags.clone(),
                now,
            )],
            WorkflowTemplate::PlannerDag => vec![new_mission_task(
                &mission,
                None,
                0,
                vec![],
                TaskRole::Planner,
                input.execution_mode,
                input.preferred_terminal_id.clone(),
                format!("[Planner] {}", mission.title),
                mission.goal_prompt.clone(),
                "提交结构化且无环的 DAG 执行计划（fleet_submit_plan）".into(),
                DifficultyTier::L3,
                false,
                input.preferred_agent,
                input.required_node_tags.clone(),
                now,
            )],
            WorkflowTemplate::ImplementVerifyReview => {
                let impl_task = new_mission_task(
                    &mission,
                    None,
                    0,
                    vec![],
                    TaskRole::Worker,
                    input.execution_mode,
                    input.preferred_terminal_id.clone(),
                    format!("[1/3 实现] {}", mission.title),
                    mission.goal_prompt.clone(),
                    "完成代码实现并保持改动清晰可测".into(),
                    base_difficulty,
                    auto_difficulty,
                    input.preferred_agent,
                    input.required_node_tags.clone(),
                    now,
                );
                let verify_task = new_mission_task(
                    &mission,
                    None,
                    0,
                    vec![impl_task.id.clone()],
                    TaskRole::Worker,
                    ExecutionMode::Task,
                    None,
                    format!("[2/3 测试验证] {}", mission.title),
                    format!(
                        "针对上游实现补充并运行单元/集成测试，确认无编译与回归问题。\n原始目标：{}",
                        mission.goal_prompt
                    ),
                    "构建与相关测试全部通过".into(),
                    DifficultyTier::L2,
                    false,
                    None,
                    input.required_node_tags.clone(),
                    now + 1,
                );
                let review_task = new_mission_task(
                    &mission,
                    None,
                    0,
                    vec![impl_task.id.clone(), verify_task.id.clone()],
                    TaskRole::Reviewer,
                    ExecutionMode::Task,
                    None,
                    format!("[3/3 L4 异构审查] {}", mission.title),
                    format!(
                        "以独立审查者身份检查上游实现与测试的正确性、并发安全、边界条件与潜在缺陷。\n原始目标：{}",
                        mission.goal_prompt
                    ),
                    "给出明确审查结论与必要修复建议".into(),
                    DifficultyTier::L4,
                    false,
                    None,
                    input.required_node_tags.clone(),
                    now + 2,
                );
                vec![impl_task, verify_task, review_task]
            }
        };

        validate_dag_acyclic(&created_tasks).map_err(|e| e.to_string())?;
        self.db.save_mission_tasks(&mission, &created_tasks)?;

        let entry_task_id = created_tasks[0].id.clone();
        if input.auto_dispatch {
            let dispatched = self.dispatch_task_locked(&entry_task_id).await?;
            let updated_mission = self
                .db
                .list_missions()?
                .into_iter()
                .find(|m| m.id == mission_id)
                .unwrap_or(mission);
            return Ok((updated_mission, dispatched));
        }

        Ok((mission, created_tasks[0].clone()))
    }

    /// Advances all ready (`Pending` with all `depends_on` completed) DAG tasks in a mission,
    /// and reconciles overall mission completion state.
    pub async fn advance_mission_dag(&self, mission_id: &str) -> Result<Vec<String>, String> {
        let _operation = self.operations.lock().await;
        self.advance_mission_dag_locked(mission_id).await
    }

    async fn advance_mission_dag_locked(&self, mission_id: &str) -> Result<Vec<String>, String> {
        let tasks = self.mission_tasks(mission_id)?;
        let mut dispatched = Vec::new();
        for id in find_ready_dag_tasks(&tasks) {
            if self.dispatch_task_locked(&id).await.is_ok() {
                dispatched.push(id);
            }
        }
        self.reconcile_mission(mission_id).await?;
        Ok(dispatched)
    }

    fn mission_tasks(&self, mission_id: &str) -> Result<Vec<MissionTask>, String> {
        Ok(self
            .db
            .list_tasks()?
            .into_iter()
            .filter(|task| task.mission_id == mission_id)
            .collect())
    }

    async fn reconcile_mission(&self, mission_id: &str) -> Result<(), String> {
        let mut mission = self
            .db
            .list_missions()?
            .into_iter()
            .find(|mission| mission.id == mission_id)
            .ok_or_else(|| format!("Mission not found: {mission_id}"))?;
        let mut tasks = self.mission_tasks(mission_id)?;
        if tasks.is_empty() {
            return Ok(());
        }

        let mut blocked: HashSet<String> = tasks
            .iter()
            .filter(|task| matches!(task.status, TaskStatus::Failed | TaskStatus::Cancelled))
            .map(|task| task.id.clone())
            .collect();
        loop {
            let previous = blocked.len();
            for task in &tasks {
                if task.status == TaskStatus::Pending
                    && task.depends_on.iter().any(|id| blocked.contains(id))
                {
                    blocked.insert(task.id.clone());
                }
            }
            if blocked.len() == previous {
                break;
            }
        }

        let mut changed_tasks = Vec::new();
        for task in &mut tasks {
            if task.status != TaskStatus::Pending {
                continue;
            }
            let error = task
                .depends_on
                .iter()
                .find(|id| blocked.contains(*id))
                .map(|id| format!("Blocked by upstream task: {id}"));
            let had_blocker = task
                .last_error
                .as_deref()
                .is_some_and(|message| message.starts_with("Blocked by upstream task:"));
            if (error.is_some() || had_blocker) && error != task.last_error {
                task.last_error = error;
                task.updated_at_ms = now_ms();
                changed_tasks.push(task.clone());
            }
        }

        let previous_status = mission.status.clone();
        if tasks
            .iter()
            .all(|task| task.status == TaskStatus::Completed)
        {
            mission.status = MissionStatus::Completed;
        } else if !tasks.iter().any(|task| {
            task.status.is_active()
                || (task.status == TaskStatus::Pending && !blocked.contains(&task.id))
        }) {
            if tasks.iter().any(|task| task.status == TaskStatus::Failed) {
                mission.status = MissionStatus::Failed;
            } else if tasks
                .iter()
                .any(|task| task.status == TaskStatus::Cancelled)
            {
                mission.status = MissionStatus::Cancelled;
            }
        }

        if mission.status == previous_status {
            return self.db.save_task_changes(&changed_tasks, None, None);
        }
        mission.updated_at_ms = now_ms();
        self.db
            .save_task_changes(&changed_tasks, Some(&mission), None)?;
        let event = match mission.status {
            MissionStatus::Completed => ("completed", "任务完成", "所有阶段已完成。"),
            MissionStatus::Failed => ("failed", "任务失败", "任务存在失败阶段，请检查终端日志。"),
            _ => return Ok(()),
        };
        self.dispatch_notification(
            event.0,
            &format!("{}：{}", event.1, mission.title),
            event.2,
            Some(&mission.node_id),
            Some(&mission.workspace_id),
            None,
        )
        .await;
        Ok(())
    }

    pub async fn retry_task_with_escalation(&self, task_id: &str) -> Result<MissionTask, String> {
        let _operation = self.operations.lock().await;
        let mut task = self.task_by_id(task_id)?;
        if !matches!(task.status, TaskStatus::Failed | TaskStatus::Cancelled) {
            return Err("Only failed or cancelled tasks can be retried".into());
        }
        self.queue_retry(&mut task)?;
        self.dispatch_task_locked(task_id).await
    }

    fn queue_retry(&self, task: &mut MissionTask) -> Result<(), String> {
        task.retry_count = task.retry_count.saturating_add(1);
        if !task.auto_difficulty && task.difficulty != DifficultyTier::L4 {
            task.difficulty = task.difficulty.escalate();
        }
        task.status = TaskStatus::Pending;
        task.termexo_task_id = None;
        task.termexo_terminal_id = None;
        task.terminal_stop_requested = false;
        task.terminal_start_sequence = None;
        task.terminal_initial_status = None;
        task.provider_failure = None;
        task.result_summary = None;
        task.diff_stat = None;
        task.last_error = None;
        task.updated_at_ms = now_ms();
        self.db.save_task(task)
    }

    fn task_by_id(&self, task_id: &str) -> Result<MissionTask, String> {
        self.db
            .list_tasks()?
            .into_iter()
            .find(|task| task.id == task_id)
            .ok_or_else(|| format!("Task not found: {task_id}"))
    }

    fn task_slot(&self, task: &MissionTask) -> Result<Option<AccountModelSlot>, String> {
        Ok(self
            .db
            .list_slots()?
            .into_iter()
            .find(|slot| Some(slot.id.as_str()) == task.assigned_slot_id.as_deref()))
    }

    async fn task_client(&self, task: &MissionTask) -> Result<TermexoMcpClient, String> {
        let node = self
            .db
            .list_nodes()?
            .into_iter()
            .find(|node| Some(node.id.as_str()) == task.assigned_node_id.as_deref())
            .ok_or("Assigned Termexo node is unavailable")?;
        self.client_for_node(&node).await
    }

    pub async fn cancel_task(&self, task_id: &str) -> Result<MissionTask, String> {
        let _operation = self.operations.lock().await;
        let mut task = self.task_by_id(task_id)?;
        match task.status {
            TaskStatus::Cancelled => return Ok(task),
            TaskStatus::Completed | TaskStatus::Failed => {
                return Err("This task has already finished".into())
            }
            _ => {}
        }

        let was_active = task.status.is_active();
        if was_active {
            let client = self.task_client(&task).await?;
            if let Some(remote_id) = &task.termexo_task_id {
                let stopped = client
                    .task_stop(remote_id)
                    .await
                    .map_err(|error| error.to_string())?;
                if !matches!(
                    stopped.execution_state.as_str(),
                    "stopped" | "failed" | "cancelled"
                ) || stopped.id != *remote_id
                {
                    return Err("Termexo did not confirm that the task stopped".into());
                }
            } else if let Some(terminal_id) = &task.termexo_terminal_id {
                if !task.terminal_stop_requested {
                    client
                        .terminal_write(terminal_id, "\u{3}", false)
                        .await
                        .map_err(|error| error.to_string())?;
                    task.terminal_stop_requested = true;
                    task.updated_at_ms = now_ms();
                    self.db.save_task(&task)?;
                }
                return Ok(task);
            } else {
                return Err("No remote task or terminal is available to stop".into());
            }
        }

        task.status = TaskStatus::Cancelled;
        task.updated_at_ms = now_ms();
        let mut slot = if was_active {
            self.task_slot(&task)?
        } else {
            None
        };
        if let Some(slot) = &mut slot {
            slot.active_count = slot.active_count.saturating_sub(1);
        }
        self.db
            .save_task_changes(std::slice::from_ref(&task), None, slot.as_ref())?;
        if was_active {
            self.record_task_execution(&task, task.updated_at_ms);
        }
        self.reconcile_mission(&task.mission_id).await?;
        Ok(task)
    }

    pub async fn dispatch_task(&self, task_id: &str) -> Result<MissionTask, String> {
        let _operation = self.operations.lock().await;
        self.dispatch_task_locked(task_id).await
    }

    async fn dispatch_task_locked(&self, task_id: &str) -> Result<MissionTask, String> {
        let mut task = self.task_by_id(task_id)?;
        if task.status != TaskStatus::Pending {
            return Err("Only pending tasks can be dispatched".into());
        }
        let mut mission = self
            .db
            .list_missions()?
            .into_iter()
            .find(|item| item.id == task.mission_id)
            .ok_or_else(|| format!("Mission not found: {}", task.mission_id))?;
        let tasks = self.mission_tasks(&mission.id)?;
        if !find_ready_dag_tasks(&tasks).contains(&task.id) {
            return Err("Complete the upstream tasks before starting this stage".into());
        }
        if task.parent_task_id.is_some()
            && tasks
                .iter()
                .filter(|item| item.parent_task_id.is_some() && item.status.is_active())
                .count()
                >= mission.guardrails.max_concurrency as usize
        {
            return Err("Mission sub-agent concurrency limit reached".into());
        }
        match self.start_task(&mut task, &mut mission, &tasks).await {
            Ok(()) => Ok(task),
            Err(error) => {
                task.last_error = Some(error.clone());
                task.updated_at_ms = now_ms();
                self.db.save_task(&task)?;
                Err(error)
            }
        }
    }

    async fn start_task(
        &self,
        task: &mut MissionTask,
        mission: &mut Mission,
        tasks: &[MissionTask],
    ) -> Result<(), String> {
        if (task.difficulty == DifficultyTier::L4 || task.role == TaskRole::Reviewer)
            && task.exclude_model_family.is_none()
        {
            task.exclude_model_family = task.depends_on.iter().find_map(|id| {
                let dep = tasks.iter().find(|item| &item.id == id)?;
                let agent = dep.assigned_agent_type.as_ref()?;
                Some(infer_model_family(
                    agent,
                    dep.assigned_model_name.as_deref().unwrap_or(""),
                ))
            });
        }
        if task.auto_difficulty {
            task.difficulty = evaluate_difficulty(&task.title, &task.description, task.retry_count);
        }

        let mut nodes = self.db.list_nodes()?;
        let mut slots = self.db.list_slots()?;
        let all_tasks = if task.execution_mode == ExecutionMode::Terminal {
            self.db.list_tasks()?
        } else {
            Vec::new()
        };
        let mut workspaces = HashMap::new();
        let resume_created = task.termexo_task_id.is_some() && task.assigned_workspace_id.is_some();
        {
            let snapshots = self.snapshots.read().await;
            let origin = snapshots.get(&mission.node_id).and_then(|snapshot| {
                snapshot
                    .workspaces
                    .iter()
                    .find(|ws| ws.id == mission.workspace_id)
            });
            let worktree_node = task.worktree_path.as_ref().map(|_| {
                if task.assigned_workspace_id.is_some() {
                    task.assigned_node_id.as_deref().unwrap_or(&mission.node_id)
                } else {
                    &mission.node_id
                }
            });
            nodes.retain(|node| {
                if (task.isolate_worktree || task.worktree_path.is_some())
                    && (node.transport_type != TransportType::Local
                        || !is_loopback_endpoint(&node.endpoint))
                    || worktree_node.is_some_and(|id| id != node.id)
                    || resume_created && Some(node.id.as_str()) != task.assigned_node_id.as_deref()
                {
                    return false;
                }
                let Some(snapshot) = snapshots.get(&node.id) else {
                    return false;
                };
                let workspace = if resume_created {
                    snapshot
                        .workspaces
                        .iter()
                        .find(|ws| Some(ws.id.as_str()) == task.assigned_workspace_id.as_deref())
                } else {
                    match_workspace(mission, origin, &node.id, &snapshot.workspaces)
                };
                let Some(workspace) = workspace else {
                    return false;
                };
                workspaces.insert(node.id.clone(), workspace.clone());
                if task.execution_mode == ExecutionMode::Terminal {
                    slots.retain(|slot| {
                        slot.node_id != node.id
                            || snapshot.terminals.iter().any(|terminal| {
                                terminal_matches_slot(
                                    terminal,
                                    slot,
                                    &workspace.id,
                                    task.preferred_terminal_id.as_deref(),
                                ) && !terminal_in_use(terminal, &node.id, &all_tasks)
                            })
                    });
                }
                true
            });
        }

        if nodes.is_empty() {
            return Err(if task.isolate_worktree || task.worktree_path.is_some() {
                "No local Termexo node can provide the requested workspace and Worktree isolation"
                    .into()
            } else {
                "No Termexo node has a matching workspace. Check the workspace name on the target node.".into()
            });
        }

        if resume_created {
            slots.retain(|slot| Some(slot.id.as_str()) == task.assigned_slot_id.as_deref());
        }
        let effective_tags = if task.required_node_tags.is_empty() {
            mission.required_node_tags.clone()
        } else {
            task.required_node_tags.clone()
        };
        let decision = select_slot_across_fleet(
            &nodes,
            &slots,
            &RouteRequest {
                difficulty: task.difficulty,
                mode: mission.routing_mode,
                preferred_node_id: task
                    .assigned_node_id
                    .clone()
                    .or_else(|| Some(mission.node_id.clone())),
                required_node_tags: effective_tags,
                preferred_agent: task.preferred_agent.clone(),
                exclude_model_family: task.exclude_model_family.clone(),
                now_ms: now_ms(),
            },
        )
        .map_err(|error| {
            if let Some(family) = &task.exclude_model_family {
                format!("{error}; review requires a model family other than {family}")
            } else {
                error.to_string()
            }
        })?;
        let node = nodes
            .iter()
            .find(|node| node.id == decision.node_id)
            .ok_or("Assigned Termexo node is unavailable")?;
        let workspace = workspaces
            .get(&decision.node_id)
            .ok_or("Assigned workspace is unavailable")?;
        let client = self.client_for_node(node).await?;
        let slot = slots
            .iter_mut()
            .find(|slot| slot.id == decision.slot_id)
            .ok_or("Assigned routing slot is unavailable")?;

        task.assigned_node_id = Some(decision.node_id);
        task.assigned_workspace_id = Some(workspace.id.clone());
        task.assigned_slot_id = Some(decision.slot_id);
        task.assigned_agent_type = Some(decision.agent_type);
        task.assigned_model_name = Some(decision.model_name);
        task.assigned_account_profile_id = decision.account_profile_id;
        task.routing_reason = Some(decision.reason);
        let upstream = build_upstream_handoff(task, tasks);
        let description = build_fleet_task_description(
            &task.description,
            &mission.id,
            &task.id,
            &task.parent_token,
            task.role,
            task.depth,
            mission.guardrails.max_depth,
            upstream.as_deref(),
        );

        if task.execution_mode == ExecutionMode::Terminal {
            let live = client
                .terminal_list(Some(&workspace.id))
                .await
                .map_err(|error| error.to_string())?;
            let terminal = live
                .iter()
                .find(|terminal| {
                    terminal_matches_slot(
                        terminal,
                        slot,
                        &workspace.id,
                        task.preferred_terminal_id.as_deref(),
                    ) && !terminal_in_use(terminal, &node.id, &all_tasks)
                })
                .ok_or(
                    "No matching live Termexo terminal found for this account, model and workspace",
                )?;
            task.terminal_initial_status = Some(terminal.status.clone());
            task.terminal_start_sequence = client
                .terminal_read(&terminal.id, Some(1))
                .await
                .ok()
                .map(|output| output.sequence);
            client
                .terminal_write(&terminal.id, &description, true)
                .await
                .map_err(|error| error.to_string())?;
            task.termexo_terminal_id = Some(terminal.id.clone());
            self.reserve_task(task, mission, slot, TaskStatus::Running)?;
            return Ok(());
        }

        let remote_id = if resume_created {
            task.termexo_task_id
                .clone()
                .ok_or("Missing queued remote task")?
        } else {
            let board = client
                .task_list(&workspace.id)
                .await
                .map_err(|error| error.to_string())?;
            let project = if node.id == mission.node_id && !mission.project_id.trim().is_empty() {
                board
                    .projects
                    .iter()
                    .find(|project| project.id == mission.project_id)
                    .ok_or("The configured Termexo project is unavailable in this workspace")?
            } else {
                let project = board
                    .projects
                    .iter()
                    .find(|project| project.path == workspace.project_path)
                    .or_else(|| (board.projects.len() == 1).then(|| &board.projects[0]))
                    .ok_or("No unambiguous task-board project matches the target workspace")?;
                if node.id == mission.node_id {
                    mission.project_id = project.id.clone();
                }
                project
            };

            let mcp_profile_ids = self.task_mcp_profile(mission, task, node).await?;
            if task.role == TaskRole::Planner
                && client.supports_task_mcp_profiles().await == Some(false)
            {
                return Err("This Termexo node cannot attach a task-level MCP profile".into());
            }
            if task.isolate_worktree && task.worktree_path.is_none() {
                let repo = project.path.clone();
                let id = task.id.clone();
                let worktree = tokio::task::spawn_blocking(move || {
                    create_isolated_worktree(Path::new(&repo), &id)
                })
                .await
                .map_err(|error| error.to_string())?
                .map_err(|error| format!("Could not create the isolated Git worktree: {error}"))?;
                task.worktree_path = Some(worktree.path);
                task.worktree_branch = Some(worktree.branch);
                self.db.save_task(task)?;
            }
            let created = client
                .task_create(&CreateTermexoTaskRequest {
                    workspace_id: workspace.id.clone(),
                    project_id: project.id.clone(),
                    title: task.title.clone(),
                    agent_type: slot.agent_type.as_str().to_owned(),
                    description: Some(description),
                    acceptance_criteria: (!task.acceptance_criteria.trim().is_empty())
                        .then(|| task.acceptance_criteria.clone()),
                    priority: Some(
                        match task.difficulty {
                            DifficultyTier::L1 => "low",
                            DifficultyTier::L2 => "medium",
                            DifficultyTier::L3 | DifficultyTier::L4 => "high",
                        }
                        .into(),
                    ),
                    profile_id: slot.model_profile_id.clone(),
                    account_profile_id: slot.account_profile_id.clone(),
                    mcp_profile_ids,
                    model_name: (!slot.model_name.trim().is_empty())
                        .then(|| slot.model_name.clone()),
                    working_directory: task.worktree_path.clone(),
                })
                .await
                .map_err(|error| error.to_string())?;
            task.termexo_task_id = Some(created.id.clone());
            created.id
        };

        self.reserve_task(task, mission, slot, TaskStatus::Queued)?;
        let started = client
            .task_execute(&remote_id)
            .await
            .map_err(|error| error.to_string())?;
        task.termexo_terminal_id = started.terminal_id;
        task.status = TaskStatus::Running;
        task.updated_at_ms = now_ms();
        self.db.save_task(task)
    }

    fn reserve_task(
        &self,
        task: &mut MissionTask,
        mission: &mut Mission,
        slot: &mut AccountModelSlot,
        status: TaskStatus,
    ) -> Result<(), String> {
        task.status = status;
        task.last_error = None;
        task.updated_at_ms = now_ms();
        mission.status = MissionStatus::Running;
        mission.updated_at_ms = task.updated_at_ms;
        slot.active_count = slot.active_count.saturating_add(1);
        self.db
            .save_task_changes(std::slice::from_ref(task), Some(mission), Some(slot))
    }

    async fn task_mcp_profile(
        &self,
        mission: &Mission,
        task: &MissionTask,
        node: &FleetNode,
    ) -> Result<Option<Vec<String>>, String> {
        let planner = task.role == TaskRole::Planner;
        if !planner && task.depth >= mission.guardrails.max_depth {
            return Ok(None);
        }
        if !matches!(
            task.assigned_agent_type,
            Some(AgentType::Claude | AgentType::Codex | AgentType::Opencode)
        ) {
            return if planner {
                Err(
                    "Planner requires Claude Code, Codex CLI, or OpenCode for task-level MCP"
                        .into(),
                )
            } else {
                Ok(None)
            };
        }
        let snapshots = self.snapshots.read().await;
        let selected_name = mission.fleet_mcp_profile_id.as_deref().and_then(|id| {
            snapshots
                .get(&mission.node_id)?
                .settings
                .mcp_profiles
                .iter()
                .find(|profile| profile.id == id)
                .map(|profile| profile.name.as_str())
        });
        let profiles = &snapshots
            .get(&node.id)
            .ok_or("Node capabilities are unavailable")?
            .settings
            .mcp_profiles;
        let profile = if node.id == mission.node_id {
            mission
                .fleet_mcp_profile_id
                .as_deref()
                .and_then(|id| profiles.iter().find(|profile| profile.id == id))
        } else {
            selected_name.and_then(|name| {
                unique_match(profiles.iter().filter(|profile| profile.name == name))
            })
        };
        let profile = profile.or_else(|| {
            unique_match(
                profiles
                    .iter()
                    .filter(|item| item.name.eq_ignore_ascii_case("Termexo Fleet")),
            )
        });
        if planner && profile.is_none() {
            return Err(format!(
                "Configure a Termexo Fleet MCP profile on node '{}' before starting Planner",
                node.name
            ));
        }
        Ok(profile.map(|item| vec![item.id.clone()]))
    }

    pub async fn reply_terminal(
        &self,
        node_id: &str,
        terminal_id: &str,
        message: &str,
        submit: bool,
    ) -> Result<(), String> {
        let node = self
            .db
            .list_nodes()?
            .into_iter()
            .find(|n| n.id == node_id)
            .ok_or_else(|| format!("Node not found: {node_id}"))?;
        let client = self.client_for_node(&node).await?;
        client
            .terminal_write(terminal_id, message, submit)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub async fn read_terminal_tail(
        &self,
        node_id: &str,
        terminal_id: &str,
    ) -> Result<String, String> {
        let node = self
            .db
            .list_nodes()?
            .into_iter()
            .find(|n| n.id == node_id)
            .ok_or_else(|| format!("Node not found: {node_id}"))?;
        let client = self.client_for_node(&node).await?;
        let snapshot = client
            .terminal_read(terminal_id, Some(12_000))
            .await
            .map_err(|e| e.to_string())?;
        Ok(snapshot.data)
    }

    async fn client_for_node(&self, node: &FleetNode) -> Result<TermexoMcpClient, String> {
        let cached = self.token_cache.read().await.get(&node.id).cloned();
        let token = match cached {
            Some(token) => token,
            None => {
                let loaded = load_secret(&node_token_key(&node.id))?
                    .ok_or_else(|| format!("Missing MCP access token for node {}", node.name))?;
                self.token_cache
                    .write()
                    .await
                    .insert(node.id.clone(), loaded.clone());
                loaded
            }
        };
        TermexoMcpClient::new_with_options(&node.endpoint, token, node.allow_insecure_tls)
            .map_err(|e| e.to_string())
    }

    fn sync_slots_from_settings(
        &self,
        node_id: &str,
        settings: &TermexoSettingsSnapshot,
    ) -> Result<(), String> {
        let existing = self.db.list_slots()?;
        let mut by_id: HashMap<String, AccountModelSlot> =
            existing.into_iter().map(|s| (s.id.clone(), s)).collect();

        for model_profile in &settings.model_profiles {
            for (agent_type, enabled, model_name) in [
                (
                    AgentType::Claude,
                    model_profile.claude_enabled,
                    &model_profile.claude_model,
                ),
                (
                    AgentType::Codex,
                    model_profile.codex_enabled,
                    &model_profile.codex_model,
                ),
            ] {
                if !enabled {
                    continue;
                }
                let matching_accounts: Vec<_> = settings
                    .account_profiles
                    .iter()
                    .filter(|acc| acc.agent_type == agent_type.as_str())
                    .collect();
                let accounts_or_default: Vec<Option<&fleet_protocol::TermexoAccountProfileSummary>> =
                    if matching_accounts.is_empty() {
                        vec![None]
                    } else {
                        matching_accounts.into_iter().map(Some).collect()
                    };

                for account in accounts_or_default {
                    let acc_id = account.map(|a| a.id.as_str()).unwrap_or("default");
                    let slot_id = format!(
                        "{node_id}:{}:{}:{acc_id}",
                        agent_type.as_str(),
                        model_profile.id
                    );
                    if by_id.contains_key(&slot_id) {
                        continue;
                    }
                    let model = if model_name.trim().is_empty() {
                        agent_type.as_str().to_owned()
                    } else {
                        model_name.clone()
                    };
                    let (tiers, cost, quality) = infer_default_slot_profile(&model);
                    let label = match account {
                        Some(acc) => format!("{} · {} ({})", model_profile.name, acc.name, model),
                        None => format!("{} ({})", model_profile.name, model),
                    };
                    let slot = AccountModelSlot {
                        id: slot_id.clone(),
                        node_id: node_id.to_owned(),
                        label,
                        agent_type: agent_type.clone(),
                        account_profile_id: account.map(|a| a.id.clone()),
                        model_profile_id: Some(model_profile.id.clone()),
                        model_family: infer_model_family(&agent_type, &model),
                        model_name: model,
                        supported_tiers: tiers,
                        cost_tier: cost,
                        quality_tier: quality,
                        max_concurrency: 2,
                        active_count: 0,
                        priority: if model_profile.is_default { 20 } else { 10 },
                        health: match account {
                            Some(acc) if !acc.authenticated && !model_profile.has_credential => {
                                SlotHealth::AuthExpired
                            }
                            _ => SlotHealth::Healthy,
                        },
                        cooldown_until_ms: None,
                    };
                    self.db.save_slot(&slot)?;
                    by_id.insert(slot_id, slot);
                }
            }
        }

        for account in &settings.account_profiles {
            let Some(agent_type) = AgentType::parse(&account.agent_type) else {
                continue;
            };
            if matches!(agent_type, AgentType::Claude | AgentType::Codex) {
                continue;
            }
            let slot_id = format!("{node_id}:{}:{}", agent_type.as_str(), account.id);
            if by_id.contains_key(&slot_id) {
                continue;
            }
            let slot = AccountModelSlot {
                id: slot_id.clone(),
                node_id: node_id.to_owned(),
                label: format!("{} · {}", agent_type.as_str(), account.name),
                model_family: infer_model_family(&agent_type, agent_type.as_str()),
                model_name: String::new(),
                agent_type,
                account_profile_id: Some(account.id.clone()),
                model_profile_id: None,
                supported_tiers: vec![DifficultyTier::L1, DifficultyTier::L2],
                cost_tier: 2,
                quality_tier: 3,
                max_concurrency: 2,
                active_count: 0,
                priority: 12,
                health: if account.authenticated {
                    SlotHealth::Healthy
                } else {
                    SlotHealth::AuthExpired
                },
                cooldown_until_ms: None,
            };
            self.db.save_slot(&slot)?;
            by_id.insert(slot_id, slot);
        }

        Ok(())
    }

    async fn sync_running_tasks(
        &self,
        node: &FleetNode,
        client: &TermexoMcpClient,
        workspaces: &[TermexoWorkspaceSummary],
        terminals: &[TermexoTerminalSummary],
    ) -> Result<(), String> {
        let missions: HashMap<_, _> = self
            .db
            .list_missions()?
            .into_iter()
            .map(|mission| (mission.id.clone(), mission))
            .collect();
        let mut tasks = self.db.list_tasks()?;
        let planned: HashSet<_> = tasks
            .iter()
            .filter_map(|task| task.parent_task_id.clone())
            .collect();
        let now = now_ms();
        let mut attention = Vec::new();
        let mut boards = HashMap::new();

        for terminal in terminals {
            let kind = match terminal.status.as_str() {
                "WAITING_APPROVAL" => AttentionKind::WaitingApproval,
                "WAITING_INPUT" => AttentionKind::WaitingInput,
                _ => continue,
            };
            attention.push(AttentionItem {
                id: format!("att-term-{}", terminal.id),
                node_id: node.id.clone(),
                node_name: node.name.clone(),
                workspace_id: Some(terminal.workspace_id.clone()),
                terminal_id: Some(terminal.id.clone()),
                mission_task_id: None,
                title: terminal.name.clone(),
                detail: format!("{} ({})", terminal.agent_type, terminal.model),
                kind,
                detected_at_ms: now,
            });
        }

        for task in &mut tasks {
            if task.assigned_node_id.as_deref() != Some(&node.id) || !task.status.is_active() {
                continue;
            }
            let Some(mission) = missions.get(&task.mission_id) else {
                continue;
            };
            let remote = if let Some(id) = &task.termexo_task_id {
                find_remote_task(client, task, mission, workspaces, &mut boards, id).await
            } else {
                None
            };
            if task.execution_mode == ExecutionMode::Task && remote.is_none() {
                continue;
            }

            let old_status = task.status.clone();
            if let Some(remote) = &remote {
                task.assigned_workspace_id = Some(remote.workspace_id.clone());
                if remote.terminal_id.is_some() {
                    task.termexo_terminal_id.clone_from(&remote.terminal_id);
                }
                task.status = match remote.execution_state.as_str() {
                    "completed" => TaskStatus::Completed,
                    "failed" => TaskStatus::Failed,
                    "stopped" | "cancelled" if task.provider_failure.is_some() => {
                        TaskStatus::Failed
                    }
                    "stopped" | "cancelled" => TaskStatus::Cancelled,
                    "waiting" => TaskStatus::WaitingInput,
                    "idle" if task.status == TaskStatus::Queued && remote.stage == "todo" => {
                        TaskStatus::Pending
                    }
                    _ => TaskStatus::Running,
                };
                task.last_error = remote
                    .last_error
                    .clone()
                    .or_else(|| task.provider_failure.clone());
                if task.status == TaskStatus::Completed {
                    task.result_summary.clone_from(&remote.output_tail);
                }
            }

            let session = task
                .termexo_terminal_id
                .as_deref()
                .and_then(|id| terminals.iter().find(|terminal| terminal.id == id));
            let terminal_status = remote
                .as_ref()
                .and_then(|item| item.last_terminal_status.as_deref())
                .or_else(|| session.map(|terminal| terminal.status.as_str()));
            let output = if let Some(id) = &task.termexo_terminal_id {
                client.terminal_read(id, Some(16_000)).await.ok()
            } else {
                None
            };

            if task.execution_mode == ExecutionMode::Terminal {
                let Some(terminal) = session else {
                    continue;
                };
                let status_changed =
                    task.terminal_initial_status.as_deref() != Some(terminal.status.as_str());
                let new_output = output
                    .as_ref()
                    .and_then(|output| {
                        task.terminal_start_sequence
                            .map(|start| output.sequence > start)
                    })
                    .unwrap_or(false);
                if !task.terminal_stop_requested
                    && !status_changed
                    && !new_output
                    && matches!(
                        terminal.status.as_str(),
                        "IDLE" | "COMPLETED" | "WAITING_INPUT"
                    )
                {
                    continue;
                }
                if status_changed {
                    task.terminal_initial_status = None;
                }
                task.assigned_workspace_id = Some(terminal.workspace_id.clone());
                task.status = match terminal.status.as_str() {
                    "COMPLETED" if task.terminal_stop_requested => TaskStatus::Cancelled,
                    "COMPLETED" => TaskStatus::Completed,
                    "STOPPED" | "DISCONNECTED" | "FAILED" | "IDLE"
                        if task.terminal_stop_requested =>
                    {
                        TaskStatus::Cancelled
                    }
                    "STOPPED" | "DISCONNECTED" | "FAILED" => TaskStatus::Failed,
                    "WAITING_APPROVAL" => TaskStatus::WaitingApproval,
                    "WAITING_INPUT" => TaskStatus::WaitingInput,
                    "IDLE" if old_status == TaskStatus::Running => TaskStatus::Running,
                    _ => TaskStatus::Running,
                };
            }

            let mut slot = self.task_slot(task)?;
            let mut slot_changed = false;
            if !matches!(
                task.status,
                TaskStatus::Completed | TaskStatus::Cancelled | TaskStatus::Pending
            ) && !task.terminal_stop_requested
            {
                let signal = match terminal_status {
                    Some("RATE_LIMITED") => TerminalSignal::RateLimited,
                    Some("WAITING_APPROVAL") => TerminalSignal::WaitingApproval,
                    _ if matches!(task.status, TaskStatus::WaitingInput | TaskStatus::Failed) => {
                        output
                            .as_ref()
                            .map(|output| classify_recent_output(&output.data))
                            .unwrap_or(TerminalSignal::Normal)
                    }
                    _ => TerminalSignal::Normal,
                };
                let provider_problem = match signal {
                    TerminalSignal::RateLimited => Some((
                        SlotHealth::RateLimited,
                        AttentionKind::RateLimited,
                        "Provider rate limit detected",
                    )),
                    TerminalSignal::AuthExpired => Some((
                        SlotHealth::AuthExpired,
                        AttentionKind::AuthExpired,
                        "Provider authentication expired",
                    )),
                    TerminalSignal::ApiError => Some((
                        SlotHealth::RateLimited,
                        AttentionKind::TaskFailed,
                        "Provider API error detected",
                    )),
                    TerminalSignal::WaitingApproval => {
                        if task.status.is_active() {
                            task.status = TaskStatus::WaitingApproval;
                        }
                        None
                    }
                    _ => None,
                };

                if let Some((health, kind, detail)) = provider_problem {
                    if let Some(slot) = &mut slot {
                        slot.cooldown_until_ms = (health == SlotHealth::RateLimited)
                            .then_some(now + RATE_LIMIT_COOLDOWN_MS);
                        slot.health = health;
                        slot_changed = true;
                    }
                    attention.push(AttentionItem {
                        id: format!("att-provider-{}", task.id),
                        node_id: node.id.clone(),
                        node_name: node.name.clone(),
                        workspace_id: task.assigned_workspace_id.clone(),
                        terminal_id: task.termexo_terminal_id.clone(),
                        mission_task_id: Some(task.id.clone()),
                        title: task.title.clone(),
                        kind,
                        detail: detail.into(),
                        detected_at_ms: now,
                    });
                    task.last_error = Some(detail.into());
                    if task.status.is_active() {
                        if let Some(remote_id) = &task.termexo_task_id {
                            task.provider_failure = Some(detail.into());
                            match client.task_stop(remote_id).await {
                                Ok(stopped)
                                    if stopped.id == *remote_id
                                        && matches!(
                                            stopped.execution_state.as_str(),
                                            "stopped" | "failed" | "cancelled"
                                        ) =>
                                {
                                    task.status = TaskStatus::Failed;
                                }
                                _ => {
                                    task.last_error = Some(format!("{detail}; Termexo has not confirmed stopping the current attempt"));
                                }
                            }
                        }
                    }
                }
            }

            if task.status == TaskStatus::Completed
                && task.role == TaskRole::Planner
                && !planned.contains(&task.id)
            {
                task.status = TaskStatus::Failed;
                task.last_error = Some("Planner finished without submitting a DAG plan".into());
            }
            let finished = matches!(
                task.status,
                TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Cancelled
            );
            if finished {
                if task.result_summary.is_none() {
                    task.result_summary = output
                        .as_ref()
                        .map(|output| terminal_tail(&output.data, 25));
                }
                if task.status == TaskStatus::Completed {
                    if let Some(worktree) = task.worktree_path.as_deref() {
                        task.diff_stat = collect_worktree_diff_stat(Path::new(worktree));
                    }
                }
            }
            if !task.status.is_active() {
                if let Some(slot) = &mut slot {
                    slot.active_count = slot.active_count.saturating_sub(1);
                    slot_changed = true;
                }
            }
            task.updated_at_ms = now;
            self.db.save_task_changes(
                std::slice::from_ref(task),
                None,
                if slot_changed { slot.as_ref() } else { None },
            )?;
            if finished {
                self.record_task_execution(task, now);
            }
            if task.status == TaskStatus::Failed && task.retry_count < task.max_retries {
                self.queue_retry(task)?;
            }
        }

        let newly_raised: Vec<AttentionItem> = {
            let mut current = self.attention.write().await;
            let previous: HashSet<_> = current
                .iter()
                .filter(|item| item.node_id == node.id)
                .map(|item| item.id.clone())
                .collect();
            let raised = attention
                .iter()
                .filter(|item| !previous.contains(&item.id))
                .cloned()
                .collect();
            current.retain(|item| item.node_id != node.id);
            current.extend(attention);
            raised
        };
        for item in newly_raised {
            let event = match item.kind {
                AttentionKind::WaitingApproval => Some("approval"),
                AttentionKind::RateLimited | AttentionKind::AuthExpired => Some("slot_unhealthy"),
                AttentionKind::TaskFailed => Some("failed"),
                AttentionKind::WaitingInput => None,
            };
            if let Some(event) = event {
                self.dispatch_notification(
                    event,
                    &format!("{} · {}", item.node_name, item.title),
                    &item.detail,
                    Some(&item.node_id),
                    item.workspace_id.as_deref(),
                    item.terminal_id.as_deref(),
                )
                .await;
            }
        }
        Ok(())
    }

    async fn handle_list_nodes(&self, args: Value) -> Result<Value, String> {
        let parent_token = args
            .get("parent_token")
            .and_then(Value::as_str)
            .ok_or("Missing parent_token")?;
        let tasks = self.db.list_tasks()?;
        if !tasks.iter().any(|t| t.parent_token == parent_token) {
            return Err("Invalid parent_token".into());
        }
        let nodes = self.db.list_nodes()?;
        let snaps = self.snapshots.read().await;
        let items: Vec<Value> = nodes
            .into_iter()
            .map(|node| {
                let workspaces = snaps
                    .get(&node.id)
                    .map(|s| {
                        s.workspaces
                            .iter()
                            .map(|w| json!({ "id": w.id, "name": w.name, "projectPath": w.project_path }))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                json!({
                    "id": node.id,
                    "name": node.name,
                    "status": node.status,
                    "tags": node.tags,
                    "maxConcurrency": node.max_concurrency,
                    "workspaces": workspaces
                })
            })
            .collect();
        Ok(json!({ "nodes": items }))
    }

    async fn handle_submit_plan(&self, args: Value) -> Result<Value, String> {
        let _operation = self.operations.lock().await;
        let parent_token = args
            .get("parent_token")
            .and_then(Value::as_str)
            .ok_or("Missing parent_token")?;
        let raw_steps = args.get("steps").cloned().ok_or("Missing steps")?;
        let steps: Vec<PlannedDagStep> =
            serde_json::from_value(raw_steps).map_err(|e| format!("Invalid plan steps: {e}"))?;
        if steps.is_empty() {
            return Err("Plan must include at least one step".into());
        }

        let all_tasks = self.db.list_tasks()?;
        let mut planner = all_tasks
            .iter()
            .find(|t| t.parent_token == parent_token)
            .cloned()
            .ok_or("Invalid parent_token")?;
        let mission = self
            .db
            .list_missions()?
            .into_iter()
            .find(|m| m.id == planner.mission_id)
            .ok_or("Mission not found")?;

        if planner.role != TaskRole::Planner {
            return Err("Only a Planner task can submit a DAG plan".into());
        }
        let previous: Vec<_> = all_tasks
            .iter()
            .filter(|task| task.parent_task_id.as_deref() == Some(&planner.id))
            .filter(|task| task.id.starts_with("dag-"))
            .collect();
        if !previous.is_empty() {
            return Ok(json!({
                "missionId": mission.id,
                "createdTaskIds": previous.iter().map(|task| &task.id).collect::<Vec<_>>(),
                "dispatchedTaskIds": previous.iter().filter(|task| task.status.is_active())
                    .map(|task| &task.id).collect::<Vec<_>>()
            }));
        }
        if !planner.status.is_active() {
            return Err("The Planner task is no longer active".into());
        }
        if planner.depth >= mission.guardrails.max_depth {
            return Err("Plan exceeds the maximum sub-agent depth".into());
        }
        let existing_children = all_tasks
            .iter()
            .filter(|task| task.mission_id == mission.id && task.parent_task_id.is_some())
            .count();
        if steps.len()
            > (mission.guardrails.max_subagents as usize).saturating_sub(existing_children)
        {
            return Err("Plan exceeds the mission's total sub-agent limit".into());
        }
        if mission.guardrails.max_concurrency == 0 {
            return Err("Mission does not allow any concurrent sub-agents".into());
        }

        let mut key_to_id: HashMap<String, String> = HashMap::new();
        for step in &steps {
            if step.key.trim().is_empty()
                || step.title.trim().is_empty()
                || step.goal.trim().is_empty()
            {
                return Err("Each plan step requires a key, title and goal".into());
            }
            let id = format!("dag-{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
            if key_to_id.insert(step.key.clone(), id).is_some() {
                return Err(format!("Duplicate plan step key: {}", step.key));
            }
        }

        let now = now_ms();
        let mut created = Vec::new();
        for (idx, step) in steps.iter().enumerate() {
            let id = key_to_id[&step.key].clone();
            let mut depends_on = Vec::new();
            for dep_key in &step.depends_on_keys {
                let dep_id = key_to_id
                    .get(dep_key)
                    .ok_or_else(|| format!("Unknown depends_on_keys reference: {dep_key}"))?;
                depends_on.push(dep_id.clone());
            }
            let (difficulty, auto_difficulty) =
                match step.difficulty.as_deref().and_then(DifficultyTier::parse) {
                    Some(tier) => (tier, false),
                    None => (evaluate_difficulty(&step.title, &step.goal, 0), true),
                };
            let isolate = step.isolate_worktree.unwrap_or(
                mission.guardrails.isolate_worktree_by_default && step.role == TaskRole::Worker,
            );
            let mut task = new_mission_task(
                &mission,
                Some(planner.id.clone()),
                planner.depth + 1,
                depends_on,
                step.role,
                ExecutionMode::Task,
                None,
                step.title.clone(),
                step.goal.clone(),
                step.acceptance_criteria.clone(),
                difficulty,
                auto_difficulty,
                step.preferred_agent.clone(),
                step.required_node_tags.clone(),
                now + idx as i64,
            );
            task.id = id;
            task.isolate_worktree = isolate;
            created.push(task);
        }

        validate_dag_acyclic(&created).map_err(|e| e.to_string())?;
        planner.status = TaskStatus::Completed;
        planner.result_summary = Some(format!("Submitted DAG plan with {} steps", created.len()));
        planner.updated_at_ms = now;
        let mut slot = self.task_slot(&planner)?;
        if let Some(slot) = &mut slot {
            slot.active_count = slot.active_count.saturating_sub(1);
        }
        let created_ids: Vec<_> = created.iter().map(|task| task.id.clone()).collect();
        created.push(planner.clone());
        self.db.save_task_changes(&created, None, slot.as_ref())?;
        self.record_task_execution(&planner, now);

        let dispatched = self.advance_mission_dag_locked(&mission.id).await?;
        Ok(json!({
            "missionId": mission.id,
            "createdTaskIds": created_ids,
            "dispatchedTaskIds": dispatched
        }))
    }

    async fn handle_spawn_subagent(&self, args: Value) -> Result<Value, String> {
        let _operation = self.operations.lock().await;
        let parent_token = args
            .get("parent_token")
            .and_then(Value::as_str)
            .ok_or("Missing parent_token")?;
        let title = args
            .get("title")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or("Missing title")?;
        let goal = args
            .get("goal")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or("Missing goal")?;

        let all_tasks = self.db.list_tasks()?;
        let parent = all_tasks
            .iter()
            .find(|t| t.parent_token == parent_token)
            .cloned()
            .ok_or("Invalid parent_token")?;
        if !parent.status.is_active() {
            return Err("The parent task is no longer active".into());
        }
        let mission = self
            .db
            .list_missions()?
            .into_iter()
            .find(|m| m.id == parent.mission_id)
            .ok_or("Parent mission not found")?;
        let mission_tasks: Vec<MissionTask> = all_tasks
            .into_iter()
            .filter(|t| t.mission_id == mission.id)
            .collect();

        check_spawn_guardrails(&mission.guardrails, &parent, &mission_tasks, title)
            .map_err(|e| e.to_string())?;

        let (difficulty, auto_difficulty) = match args
            .get("difficulty")
            .and_then(Value::as_str)
            .and_then(DifficultyTier::parse)
        {
            Some(tier) => (tier, false),
            None => (evaluate_difficulty(title, goal, 0), true),
        };
        let preferred_agent = args
            .get("preferred_agent")
            .and_then(Value::as_str)
            .and_then(AgentType::parse);
        let required_node_tags: Vec<String> = args
            .get("required_node_tags")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        let acceptance_criteria = args
            .get("acceptance_criteria")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        let isolate_worktree = args
            .get("isolate_worktree")
            .and_then(Value::as_bool)
            .unwrap_or(mission.guardrails.isolate_worktree_by_default);

        let child_id = format!("sub-{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);

        let exclude_model_family = if difficulty == DifficultyTier::L4 {
            parent.assigned_agent_type.as_ref().map(|agent| {
                infer_model_family(agent, parent.assigned_model_name.as_deref().unwrap_or(""))
            })
        } else {
            None
        };

        let now = now_ms();
        let mut child = new_mission_task(
            &mission,
            Some(parent.id.clone()),
            parent.depth + 1,
            vec![],
            if difficulty == DifficultyTier::L4 {
                TaskRole::Reviewer
            } else {
                TaskRole::Worker
            },
            ExecutionMode::Task,
            None,
            title.to_owned(),
            goal.to_owned(),
            acceptance_criteria,
            difficulty,
            auto_difficulty,
            preferred_agent,
            required_node_tags,
            now,
        );
        child.id = child_id.clone();
        child.exclude_model_family = exclude_model_family;
        child.isolate_worktree = isolate_worktree;
        child.assigned_node_id.clone_from(&parent.assigned_node_id);
        self.db.save_task(&child)?;
        let dispatched = match self.dispatch_task_locked(&child_id).await {
            Ok(task) => task,
            Err(error) => {
                let task = self.task_by_id(&child_id)?;
                if task.termexo_task_id.is_none() && task.worktree_path.is_none() {
                    self.db.delete_task(&child_id)?;
                    return Err(error);
                }
                task
            }
        };
        Ok(json!({
            "subagentId": dispatched.id,
            "title": dispatched.title,
            "difficulty": dispatched.difficulty,
            "nodeId": dispatched.assigned_node_id,
            "agentType": dispatched.assigned_agent_type,
            "modelName": dispatched.assigned_model_name,
            "accountProfileId": dispatched.assigned_account_profile_id,
            "routingReason": dispatched.routing_reason,
            "termexoTaskId": dispatched.termexo_task_id,
            "termexoTerminalId": dispatched.termexo_terminal_id,
            "worktreePath": dispatched.worktree_path,
            "worktreeBranch": dispatched.worktree_branch,
            "lastError": dispatched.last_error,
            "status": dispatched.status
        }))
    }

    async fn handle_list_subagents(&self, args: Value) -> Result<Value, String> {
        let parent_token = args
            .get("parent_token")
            .and_then(Value::as_str)
            .ok_or("Missing parent_token")?;
        let tasks = self.db.list_tasks()?;
        let parent = tasks
            .iter()
            .find(|t| t.parent_token == parent_token)
            .ok_or("Invalid parent_token")?;
        let children: Vec<Value> = tasks
            .iter()
            .filter(|t| t.parent_task_id.as_deref() == Some(parent.id.as_str()))
            .map(|t| {
                json!({
                    "subagentId": t.id,
                    "title": t.title,
                    "status": t.status,
                    "difficulty": t.difficulty,
                    "nodeId": t.assigned_node_id,
                    "agentType": t.assigned_agent_type,
                    "modelName": t.assigned_model_name,
                    "worktreeBranch": t.worktree_branch,
                    "resultSummary": t.result_summary,
                    "diffStat": t.diff_stat,
                    "lastError": t.last_error
                })
            })
            .collect();
        Ok(json!({ "subagents": children }))
    }

    async fn handle_wait_subagents(&self, args: Value) -> Result<Value, String> {
        let parent_token = args
            .get("parent_token")
            .and_then(Value::as_str)
            .ok_or("Missing parent_token")?;
        let ids: Vec<String> = args
            .get("subagent_ids")
            .and_then(Value::as_array)
            .ok_or("Missing subagent_ids array")?
            .iter()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect();
        let timeout_secs = args
            .get("timeout_secs")
            .and_then(Value::as_u64)
            .unwrap_or(180)
            .min(1800);
        let deadline = tokio::time::Instant::now() + Duration::from_secs(timeout_secs);

        loop {
            let _ = self.sync_all_nodes().await;
            let tasks = self.db.list_tasks()?;
            let parent = tasks
                .iter()
                .find(|t| t.parent_token == parent_token)
                .ok_or("Invalid parent_token")?;
            let matched: Vec<&MissionTask> = tasks
                .iter()
                .filter(|t| {
                    t.parent_task_id.as_deref() == Some(parent.id.as_str())
                        && ids.iter().any(|id| id == &t.id)
                })
                .collect();
            let all_done = !matched.is_empty()
                && matched.iter().all(|t| {
                    matches!(
                        t.status,
                        TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Cancelled
                    )
                });
            if all_done || tokio::time::Instant::now() >= deadline {
                let results: Vec<Value> = matched
                    .into_iter()
                    .map(|t| {
                        json!({
                            "subagentId": t.id,
                            "title": t.title,
                            "status": t.status,
                            "worktreePath": t.worktree_path,
                            "worktreeBranch": t.worktree_branch,
                            "resultSummary": t.result_summary,
                            "diffStat": t.diff_stat,
                            "lastError": t.last_error
                        })
                    })
                    .collect();
                return Ok(json!({ "allCompleted": all_done, "subagents": results }));
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    }

    async fn handle_message_subagent(&self, args: Value) -> Result<Value, String> {
        let parent_token = args
            .get("parent_token")
            .and_then(Value::as_str)
            .ok_or("Missing parent_token")?;
        let subagent_id = args
            .get("subagent_id")
            .and_then(Value::as_str)
            .ok_or("Missing subagent_id")?;
        let message = args
            .get("message")
            .and_then(Value::as_str)
            .ok_or("Missing message")?;
        let tasks = self.db.list_tasks()?;
        let parent = tasks
            .iter()
            .find(|t| t.parent_token == parent_token)
            .ok_or("Invalid parent_token")?;
        let child = tasks
            .iter()
            .find(|t| {
                t.id == subagent_id && t.parent_task_id.as_deref() == Some(parent.id.as_str())
            })
            .ok_or("Sub-agent not found under this parent")?;
        let node_id = child
            .assigned_node_id
            .as_deref()
            .ok_or("Sub-agent has no assigned node")?;
        let terminal_id = child
            .termexo_terminal_id
            .as_deref()
            .ok_or("Sub-agent has no live terminal yet")?;
        self.reply_terminal(node_id, terminal_id, message, true)
            .await?;
        Ok(json!({ "subagentId": subagent_id, "sent": true }))
    }

    async fn handle_cancel_subagent(&self, args: Value) -> Result<Value, String> {
        let parent_token = args
            .get("parent_token")
            .and_then(Value::as_str)
            .ok_or("Missing parent_token")?;
        let subagent_id = args
            .get("subagent_id")
            .and_then(Value::as_str)
            .ok_or("Missing subagent_id")?;
        let tasks = self.db.list_tasks()?;
        let parent = tasks
            .iter()
            .find(|t| t.parent_token == parent_token)
            .ok_or("Invalid parent_token")?;
        let exists = tasks.iter().any(|t| {
            t.id == subagent_id && t.parent_task_id.as_deref() == Some(parent.id.as_str())
        });
        if !exists {
            return Err("Sub-agent not found under this parent".into());
        }
        self.cancel_task(subagent_id).await?;
        Ok(json!({ "subagentId": subagent_id, "cancelled": true }))
    }

    fn record_task_execution(&self, task: &MissionTask, finished_at_ms: i64) {
        let model_name = task
            .assigned_model_name
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .or_else(|| task.assigned_agent_type.as_ref().map(|a| a.as_str().to_owned()))
            .unwrap_or_else(|| "unknown".into());
        let record = ExecutionRecord {
            id: format!("rec-{}-{finished_at_ms}", task.id),
            mission_id: task.mission_id.clone(),
            task_id: task.id.clone(),
            task_title: task.title.clone(),
            node_id: task
                .assigned_node_id
                .clone()
                .unwrap_or_else(|| "unassigned".into()),
            slot_id: task.assigned_slot_id.clone(),
            agent_type: task.assigned_agent_type.clone(),
            model_name,
            difficulty: task.difficulty,
            status: task.status.clone(),
            started_at_ms: task.created_at_ms,
            finished_at_ms,
            duration_ms: (finished_at_ms - task.created_at_ms).max(0),
            retry_count: task.retry_count,
            escalated: task.retry_count > 0,
            estimated_cost_units: difficulty_cost_units(task.difficulty),
        };
        if let Err(err) = self.db.save_execution_record(&record) {
            tracing::warn!(%err, task_id = %task.id, "Failed to persist execution record");
        }
    }

    async fn dispatch_notification(
        &self,
        event: &str,
        title: &str,
        body: &str,
        node_id: Option<&str>,
        workspace_id: Option<&str>,
        terminal_id: Option<&str>,
    ) {
        let settings = self.notification_settings();
        let enabled_for_event = match event {
            "completed" => settings.notify_on_completed,
            "failed" => settings.notify_on_failed,
            "approval" => settings.notify_on_approval,
            "slot_unhealthy" => settings.notify_on_slot_unhealthy,
            _ => true,
        };
        if !enabled_for_event {
            return;
        }
        if settings.webhook_enabled && !settings.webhook_url.trim().is_empty() {
            if let Err(err) = send_webhook_json(settings.webhook_url.trim(), event, title, body).await {
                tracing::warn!(%err, event, "Webhook notification failed");
            }
        }
        if settings.wechat_via_termexo {
            if let Err(err) = self
                .dispatch_wechat_via_termexo(event, title, node_id, workspace_id, terminal_id)
                .await
            {
                tracing::warn!(%err, event, "Termexo WeChat notification failed");
            }
        }
    }

    async fn dispatch_wechat_via_termexo(
        &self,
        event: &str,
        title: &str,
        preferred_node_id: Option<&str>,
        preferred_workspace_id: Option<&str>,
        terminal_id: Option<&str>,
    ) -> Result<String, String> {
        let nodes = self.db.list_nodes()?;
        let target_node = preferred_node_id
            .and_then(|id| nodes.iter().find(|n| n.id == id && n.status == NodeStatus::Online))
            .or_else(|| nodes.iter().find(|n| n.status == NodeStatus::Online))
            .cloned()
            .ok_or_else(|| "没有在线的 Termexo 节点可用于发送微信通知".to_owned())?;
        let snaps = self.snapshots.read().await;
        let ws_id = preferred_workspace_id
            .map(str::to_owned)
            .or_else(|| {
                snaps
                    .get(&target_node.id)
                    .and_then(|s| s.workspaces.first().map(|w| w.id.clone()))
            })
            .ok_or_else(|| format!("节点 {} 暂无已同步的工作区", target_node.name))?;
        drop(snaps);

        let kind = match event {
            "completed" => "task_completed",
            "failed" => "task_failed",
            "approval" => "approval_required",
            _ => "fleet_alert",
        };
        let event_key = format!("fleet:{event}:{}", now_ms());
        let client = self.client_for_node(&target_node).await?;
        client
            .wechat_notify(&ws_id, &event_key, kind, terminal_id, Some(title))
            .await
            .map_err(|e| e.to_string())?;
        Ok(target_node.name)
    }
}

fn validate_remote_mcp_url(input: &str) -> Result<Option<String>, String> {
    let input = input.trim();
    if input.is_empty() {
        return Ok(None);
    }
    let url =
        reqwest::Url::parse(input).map_err(|_| "Enter a valid HTTPS URL for remote Fleet MCP")?;
    let local = url.host_str().is_none_or(|host| {
        let host = host.trim_matches(['[', ']']).trim_end_matches('.');
        host.eq_ignore_ascii_case("localhost")
            || host.ends_with(".localhost")
            || host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback() || ip.is_unspecified())
    });
    if url.scheme() != "https"
        || local
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(
            "Use a non-local HTTPS MCP URL without credentials, a query or a fragment".into(),
        );
    }
    Ok(Some(url.into()))
}

fn is_loopback_endpoint(endpoint: &str) -> bool {
    reqwest::Url::parse(endpoint).is_ok_and(|url| {
        url.host_str().is_some_and(|host| {
            host.eq_ignore_ascii_case("localhost")
                || host
                    .trim_matches(['[', ']'])
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
        })
    })
}

fn unique_match<T>(mut candidates: impl Iterator<Item = T>) -> Option<T> {
    let first = candidates.next()?;
    candidates.next().is_none().then_some(first)
}

fn match_workspace<'a>(
    mission: &Mission,
    origin: Option<&TermexoWorkspaceSummary>,
    node_id: &str,
    workspaces: &'a [TermexoWorkspaceSummary],
) -> Option<&'a TermexoWorkspaceSummary> {
    if node_id == mission.node_id {
        return workspaces
            .iter()
            .find(|workspace| workspace.id == mission.workspace_id);
    }
    let name = mission
        .workspace_name
        .as_deref()
        .or_else(|| origin.map(|workspace| workspace.name.as_str()))?;
    unique_match(
        workspaces
            .iter()
            .filter(|workspace| workspace.name.trim().eq_ignore_ascii_case(name.trim())),
    )
}

fn terminal_matches_slot(
    terminal: &TermexoTerminalSummary,
    slot: &AccountModelSlot,
    workspace_id: &str,
    preferred_terminal_id: Option<&str>,
) -> bool {
    terminal.live
        && terminal.workspace_id == workspace_id
        && matches!(
            terminal.status.as_str(),
            "IDLE" | "COMPLETED" | "WAITING_INPUT"
        )
        && terminal
            .agent_type
            .eq_ignore_ascii_case(slot.agent_type.as_str())
        && (terminal.model.is_empty()
            || slot.model_name.is_empty()
            || terminal.model.eq_ignore_ascii_case(&slot.model_name))
        && terminal.account_profile_id == slot.account_profile_id
        && preferred_terminal_id
            .filter(|id| !id.trim().is_empty())
            .is_none_or(|id| terminal.id == id)
}

fn terminal_in_use(
    terminal: &TermexoTerminalSummary,
    node_id: &str,
    tasks: &[MissionTask],
) -> bool {
    tasks.iter().any(|task| {
        task.status.is_active()
            && task.assigned_node_id.as_deref() == Some(node_id)
            && task.termexo_terminal_id.as_deref() == Some(terminal.id.as_str())
    })
}

async fn find_remote_task(
    client: &TermexoMcpClient,
    task: &MissionTask,
    mission: &Mission,
    workspaces: &[TermexoWorkspaceSummary],
    boards: &mut HashMap<String, Option<TermexoTaskBoardSnapshot>>,
    remote_id: &str,
) -> Option<TermexoTaskRecord> {
    // Older records did not store the assigned workspace. Resolve them by the remote task ID.
    let candidates: Vec<_> = match task.assigned_workspace_id.as_deref() {
        Some(id) => vec![id],
        None if task.assigned_node_id.as_deref() == Some(mission.node_id.as_str()) => {
            vec![mission.workspace_id.as_str()]
        }
        None => workspaces
            .iter()
            .map(|workspace| workspace.id.as_str())
            .collect(),
    };
    for id in candidates {
        if !boards.contains_key(id) {
            boards.insert(id.to_owned(), client.task_list(id).await.ok());
        }
        if let Some(remote) = boards
            .get(id)
            .and_then(Option::as_ref)
            .and_then(|board| board.tasks.iter().find(|remote| remote.id == remote_id))
        {
            return Some(remote.clone());
        }
    }
    None
}

fn terminal_tail(text: &str, lines: usize) -> String {
    let mut tail: Vec<_> = text
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.trim().is_empty())
        .rev()
        .take(lines)
        .collect();
    tail.reverse();
    tail.join("\n")
}

fn classify_recent_output(text: &str) -> TerminalSignal {
    classify_terminal_output(&terminal_tail(text, 12))
}

fn difficulty_cost_units(tier: DifficultyTier) -> u32 {
    match tier {
        DifficultyTier::L1 => 1,
        DifficultyTier::L2 => 3,
        DifficultyTier::L3 | DifficultyTier::L4 => 8,
    }
}

pub fn compute_analytics(records: &[ExecutionRecord]) -> FleetAnalytics {
    if records.is_empty() {
        return FleetAnalytics::default();
    }
    let total_runs = records.len() as u32;
    let completed_runs = records
        .iter()
        .filter(|r| r.status == TaskStatus::Completed)
        .count() as u32;
    let failed_runs = records
        .iter()
        .filter(|r| r.status == TaskStatus::Failed)
        .count() as u32;
    let escalated_runs = records.iter().filter(|r| r.escalated).count() as u32;
    let total_duration_ms: i64 = records.iter().map(|r| r.duration_ms.max(0)).sum();
    let total_cost_units: u32 = records.iter().map(|r| r.estimated_cost_units).sum();
    let baseline_l3_cost_units = total_runs.saturating_mul(8);
    let estimated_savings_percent = if baseline_l3_cost_units == 0 {
        0
    } else {
        baseline_l3_cost_units
            .saturating_sub(total_cost_units)
            .saturating_mul(100)
            / baseline_l3_cost_units
    };

    let build_buckets = |extract_key: fn(&ExecutionRecord) -> String| -> Vec<AnalyticsBucket> {
        let mut map: HashMap<String, (u32, u32, u32, i64, u32)> = HashMap::new();
        for rec in records {
            let key = extract_key(rec);
            let entry = map.entry(key).or_insert((0, 0, 0, 0, 0));
            entry.0 += 1;
            if rec.status == TaskStatus::Completed {
                entry.1 += 1;
            } else if rec.status == TaskStatus::Failed {
                entry.2 += 1;
            }
            entry.3 += rec.duration_ms.max(0);
            entry.4 += rec.estimated_cost_units;
        }
        let mut buckets: Vec<AnalyticsBucket> = map
            .into_iter()
            .map(|(key, (total, completed, failed, dur_sum, cost_units))| AnalyticsBucket {
                key,
                total,
                completed,
                failed,
                avg_duration_ms: if total > 0 { dur_sum / total as i64 } else { 0 },
                cost_units,
            })
            .collect();
        buckets.sort_by(|a, b| b.total.cmp(&a.total).then_with(|| a.key.cmp(&b.key)));
        buckets
    };

    let mut by_tier = build_buckets(|r| match r.difficulty {
        DifficultyTier::L1 => "L1".into(),
        DifficultyTier::L2 => "L2".into(),
        DifficultyTier::L3 => "L3".into(),
        DifficultyTier::L4 => "L4".into(),
    });
    by_tier.sort_by(|a, b| a.key.cmp(&b.key));

    let by_model = build_buckets(|r| r.model_name.clone());
    let by_node = build_buckets(|r| r.node_id.clone());

    FleetAnalytics {
        total_runs,
        completed_runs,
        failed_runs,
        success_rate_percent: (completed_runs.saturating_mul(100)) / total_runs,
        avg_duration_ms: total_duration_ms / total_runs as i64,
        escalated_runs,
        total_cost_units,
        baseline_l3_cost_units,
        estimated_savings_percent,
        by_tier,
        by_model,
        by_node,
    }
}

async fn send_webhook_json(url: &str, event: &str, title: &str, body: &str) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;
    let text = format!("[Termexo Fleet] {title}\n{body}");
    let payload = json!({
        "source": "Termexo Fleet",
        "event": event,
        "title": title,
        "detail": body,
        "text": text,
        "msg_type": "text",
        "content": { "text": text },
        "timestampMs": now_ms(),
    });
    let resp = client
        .post(url)
        .json(&payload)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    Ok(())
}

impl FleetMcpHandler for FleetRuntime {
    fn authorized(&self, token: &str) -> bool {
        !self.mcp_token.is_empty() && token == self.mcp_token
    }

    fn call_tool(&self, name: String, arguments: Value) -> BoxFuture<'_, Result<Value, String>> {
        Box::pin(async move {
            match name.as_str() {
                "fleet_list_nodes" => self.handle_list_nodes(arguments).await,
                "fleet_submit_plan" => self.handle_submit_plan(arguments).await,
                "fleet_spawn_subagent" => self.handle_spawn_subagent(arguments).await,
                "fleet_list_subagents" => self.handle_list_subagents(arguments).await,
                "fleet_wait_subagents" => self.handle_wait_subagents(arguments).await,
                "fleet_message_subagent" => self.handle_message_subagent(arguments).await,
                "fleet_cancel_subagent" => self.handle_cancel_subagent(arguments).await,
                _ => Err(format!("Unknown Fleet MCP tool: {name}")),
            }
        })
    }
}

#[allow(clippy::too_many_arguments)]
fn new_mission_task(
    mission: &Mission,
    parent_task_id: Option<String>,
    depth: u32,
    depends_on: Vec<String>,
    role: TaskRole,
    execution_mode: ExecutionMode,
    preferred_terminal_id: Option<String>,
    title: String,
    description: String,
    acceptance_criteria: String,
    difficulty: DifficultyTier,
    auto_difficulty: bool,
    preferred_agent: Option<AgentType>,
    required_node_tags: Vec<String>,
    now: i64,
) -> MissionTask {
    MissionTask {
        id: format!("t-{}", &uuid::Uuid::new_v4().simple().to_string()[..8]),
        mission_id: mission.id.clone(),
        parent_task_id,
        depth,
        depends_on,
        required_node_tags,
        role,
        execution_mode,
        preferred_terminal_id,
        title,
        description,
        acceptance_criteria,
        difficulty,
        auto_difficulty,
        preferred_agent,
        exclude_model_family: None,
        assigned_node_id: Some(mission.node_id.clone()),
        assigned_workspace_id: None,
        assigned_slot_id: None,
        assigned_agent_type: None,
        assigned_model_name: None,
        assigned_account_profile_id: None,
        routing_reason: None,
        termexo_task_id: None,
        termexo_terminal_id: None,
        terminal_stop_requested: false,
        terminal_start_sequence: None,
        terminal_initial_status: None,
        provider_failure: None,
        isolate_worktree: false,
        worktree_path: None,
        worktree_branch: None,
        status: TaskStatus::Pending,
        retry_count: 0,
        max_retries: 2,
        result_summary: None,
        diff_stat: None,
        last_error: None,
        parent_token: format!("pt_{}", uuid::Uuid::new_v4().simple()),
        created_at_ms: now,
        updated_at_ms: now,
    }
}

fn node_token_key(node_id: &str) -> String {
    format!("node-token:{node_id}")
}

fn infer_default_slot_profile(model: &str) -> (Vec<DifficultyTier>, u8, u8) {
    let lower = model.to_ascii_lowercase();
    if lower.contains("haiku")
        || lower.contains("mini")
        || lower.contains("flash")
        || lower.contains("lite")
    {
        (vec![DifficultyTier::L1, DifficultyTier::L2], 1, 2)
    } else if lower.contains("opus")
        || lower.contains("pro")
        || lower.contains("gpt-6")
        || lower.contains("gpt-5")
        || lower.contains("sol")
        || lower.contains("astra")
        || lower.contains("o3")
    {
        (vec![DifficultyTier::L3, DifficultyTier::L4], 5, 5)
    } else {
        (vec![DifficultyTier::L2, DifficultyTier::L3], 3, 4)
    }
}

#[cfg(test)]
#[path = "runtime_flow_tests.rs"]
mod flow_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{routing::post, Json, Router};
    use std::sync::Mutex as StdMutex;
    use tokio::net::TcpListener;

    async fn spawn_mock_termexo(
        node_label: &'static str,
        recorded_creates: Arc<StdMutex<Vec<Value>>>,
        completed_task_ids: Arc<StdMutex<Vec<String>>>,
    ) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
        let app = Router::new().route(
            "/mcp",
            post(move |Json(body): Json<Value>| {
                let creates = recorded_creates.clone();
                let completed = completed_task_ids.clone();
                async move {
                    let id = body.get("id").cloned().unwrap_or(json!(1));
                    let method = body.get("method").and_then(Value::as_str).unwrap_or("");
                    if method == "initialize" {
                        return Json(json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": { "protocolVersion": "2025-11-25", "capabilities": {} }
                        }));
                    }
                    if method == "notifications/initialized" {
                        return Json(json!({}));
                    }
                    let tool = body["params"]["name"].as_str().unwrap_or("");
                    let args = body["params"]["arguments"].clone();
                    let payload = match tool {
                        "workspace_list" => json!([{
                            "id": "w1",
                            "name": "DemoRepo",
                            "projectPath": "D:/nonexistent-repo-for-test",
                            "terminalIds": []
                        }]),
                        "terminal_list" => json!([]),
                        "settings_get" => json!({
                            "modelProfiles": [
                                {
                                    "id": "prof-claude",
                                    "name": "Anthropic",
                                    "provider": "Anthropic",
                                    "isDefault": true,
                                    "hasCredential": true,
                                    "claudeEnabled": true,
                                    "claudeModel": "claude-opus-4-6",
                                    "codexEnabled": false,
                                    "codexModel": ""
                                },
                                {
                                    "id": "prof-codex",
                                    "name": "OpenAI",
                                    "provider": "OpenAI",
                                    "isDefault": true,
                                    "hasCredential": true,
                                    "claudeEnabled": false,
                                    "claudeModel": "",
                                    "codexEnabled": true,
                                    "codexModel": "gpt-5.4"
                                }
                            ],
                            "accountProfiles": [
                                {
                                    "id": "acc-claude-1",
                                    "name": "Claude Max",
                                    "agentType": "claude",
                                    "authenticated": true,
                                    "isDefault": true,
                                    "isSystem": false
                                },
                                {
                                    "id": "acc-codex-1",
                                    "name": "Codex Pro",
                                    "agentType": "codex",
                                    "authenticated": true,
                                    "isDefault": true,
                                    "isSystem": false
                                }
                            ],
                            "mcpProfiles": [
                                { "id": "mcp-fleet", "name": "Termexo Fleet" }
                            ]
                        }),
                        "task_list" => {
                            let done = completed.lock().unwrap();
                            let tasks: Vec<Value> = done
                                .iter()
                                .map(|tid| {
                                    json!({
                                        "id": tid,
                                        "workspaceId": "w1",
                                        "projectId": "p1",
                                        "title": "Done",
                                        "stage": "completed",
                                        "executionState": "completed",
                                        "promptDeliveryState": "delivered",
                                        "agentType": "claude",
                                        "modelName": "claude-opus-4-6",
                                        "outputTail": "Implementation completed cleanly."
                                    })
                                })
                                .collect();
                            json!({
                                "workspaceId": "w1",
                                "projects": [{ "id": "p1", "workspaceId": "w1", "name": "Default", "path": "D:/nonexistent" }],
                                "tasks": tasks
                            })
                        }
                        "task_create" => {
                            let mut list = creates.lock().unwrap();
                            let task_id = format!("{node_label}-tx-{}", list.len() + 1);
                            let mut record = args.clone();
                            record["_node"] = json!(node_label);
                            list.push(record);
                            json!({
                                "id": task_id,
                                "workspaceId": "w1",
                                "projectId": "p1",
                                "title": args["title"],
                                "stage": "todo",
                                "executionState": "idle",
                                "promptDeliveryState": "idle",
                                "agentType": args["agentType"],
                                "modelName": args.get("modelName").and_then(Value::as_str).unwrap_or(""),
                                "accountProfileId": args.get("accountProfileId")
                            })
                        }
                        "task_execute" => json!({
                            "id": args["taskId"],
                            "workspaceId": "w1",
                            "projectId": "p1",
                            "title": "Running",
                            "stage": "executing",
                            "executionState": "running",
                            "promptDeliveryState": "delivered",
                            "agentType": "claude",
                            "modelName": "claude-opus-4-6",
                            "terminalId": "term-live-1"
                        }),
                        _ => json!({}),
                    };
                    Json(json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": { "structuredContent": payload, "isError": false }
                    }))
                }
            }),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (addr, handle)
    }

    #[tokio::test]
    async fn end_to_end_multi_node_planner_dag_and_cross_family_review() {
        let creates: Arc<StdMutex<Vec<Value>>> = Arc::new(StdMutex::new(Vec::new()));
        let completed_a: Arc<StdMutex<Vec<String>>> = Arc::new(StdMutex::new(Vec::new()));
        let completed_b: Arc<StdMutex<Vec<String>>> = Arc::new(StdMutex::new(Vec::new()));

        let (addr_a, handle_a) = spawn_mock_termexo("win", creates.clone(), completed_a.clone()).await;
        let (addr_b, handle_b) = spawn_mock_termexo("mac", creates.clone(), completed_b.clone()).await;

        let db_path = std::env::temp_dir().join(format!(
            "termexo-fleet-phase2-{}.db",
            uuid::Uuid::new_v4().simple()
        ));
        let db = Arc::new(FleetDatabase::open(db_path.clone()).unwrap());
        let runtime = FleetRuntime::new(db).unwrap();

        runtime
            .upsert_node(UpsertNodeInput {
                id: Some("win-main".into()),
                name: "Windows Main".into(),
                transport_type: TransportType::Local,
                endpoint: format!("http://{}:{}/mcp", addr_a.ip(), addr_a.port()),
                token: Some("tok-a".into()),
                tags: vec!["windows".into(), "local".into()],
                max_concurrency: 4,
                allow_insecure_tls: false,
            })
            .await
            .unwrap();
        runtime
            .upsert_node(UpsertNodeInput {
                id: Some("mac-build".into()),
                name: "Mac Studio".into(),
                transport_type: TransportType::Lan,
                endpoint: format!("http://{}:{}/mcp", addr_b.ip(), addr_b.port()),
                token: Some("tok-b".into()),
                tags: vec!["macos".into(), "ios".into()],
                max_concurrency: 4,
                allow_insecure_tls: false,
            })
            .await
            .unwrap();

        // 1. Create a PlannerDag mission on win-main.
        let (mission, planner_task) = runtime
            .create_mission(CreateMissionInput {
                title: "Build cross-platform auth".into(),
                goal_prompt: "Implement backend on Windows, build iOS on macOS, then L4 cross-model review".into(),
                node_id: "win-main".into(),
                workspace_id: "w1".into(),
                project_id: String::new(),
                routing_mode: RoutingMode::Quality,
                workflow_template: WorkflowTemplate::PlannerDag,
                required_node_tags: vec![],
                execution_mode: ExecutionMode::Task,
                preferred_terminal_id: None,
                difficulty: None,
                preferred_agent: Some(AgentType::Claude),
                guardrails: GuardrailPolicy::default(),
                fleet_mcp_profile_id: Some("mcp-fleet".into()),
                auto_dispatch: true,
            })
            .await
            .unwrap();
        assert_eq!(planner_task.role, TaskRole::Planner);

        // 2. Planner calls `fleet_list_nodes` and sees both connected Termexo nodes.
        let listed = runtime
            .call_tool(
                "fleet_list_nodes".into(),
                json!({ "parent_token": planner_task.parent_token }),
            )
            .await
            .unwrap();
        assert_eq!(listed["nodes"].as_array().unwrap().len(), 2);

        // 3. Planner calls `fleet_submit_plan` with a 2-stage DAG:
        //    step "impl" pinned to tag "macos" -> routes to mac-build!
        //    step "review" depends on "impl" with difficulty L4 -> waits until "impl" completes, then routes cross-family!
        let plan_resp = runtime
            .call_tool(
                "fleet_submit_plan".into(),
                json!({
                    "parent_token": planner_task.parent_token,
                    "steps": [
                        {
                            "key": "impl",
                            "title": "Build macOS signing module",
                            "goal": "Implement Keychain signing",
                            "difficulty": "L3",
                            "role": "worker",
                            "preferredAgent": "claude",
                            "requiredNodeTags": ["macos"],
                            "isolateWorktree": false
                        },
                        {
                            "key": "review",
                            "title": "Security review signing module",
                            "goal": "Cross-family security audit",
                            "dependsOnKeys": ["impl"],
                            "difficulty": "L4",
                            "role": "reviewer",
                            "isolateWorktree": false
                        }
                    ]
                }),
            )
            .await
            .unwrap();
        assert_eq!(plan_resp["createdTaskIds"].as_array().unwrap().len(), 2);
        assert_eq!(plan_resp["dispatchedTaskIds"].as_array().unwrap().len(), 1);

        let impl_task_id = plan_resp["dispatchedTaskIds"][0].as_str().unwrap().to_owned();
        let impl_task = runtime
            .db
            .list_tasks()
            .unwrap()
            .into_iter()
            .find(|t| t.id == impl_task_id)
            .unwrap();
        assert_eq!(impl_task.assigned_node_id.as_deref(), Some("mac-build"));
        assert_eq!(impl_task.assigned_agent_type, Some(AgentType::Claude));

        // 4. Mark the mac-build Termexo task completed and trigger `sync_all_nodes` -> DAG unlocks L4 review!
        completed_b
            .lock()
            .unwrap()
            .push(impl_task.termexo_task_id.clone().unwrap());
        runtime.sync_all_nodes().await.unwrap();

        let review_task = runtime
            .db
            .list_tasks()
            .unwrap()
            .into_iter()
            .find(|t| t.mission_id == mission.id && t.role == TaskRole::Reviewer)
            .unwrap();
        assert_eq!(review_task.status, TaskStatus::Running);
        // Upstream impl used Claude (anthropic), so L4 Reviewer must pick Codex (openai)!
        assert_eq!(review_task.assigned_agent_type, Some(AgentType::Codex));
        assert_eq!(review_task.exclude_model_family.as_deref(), Some("anthropic"));

        // Verify the review prompt received upstream handoff context!
        let creates_list = creates.lock().unwrap();
        let review_create = creates_list.last().unwrap();
        assert!(review_create["description"]
            .as_str()
            .unwrap()
            .contains("[Upstream Completed Tasks]"));

        handle_a.abort();
        handle_b.abort();
        let _ = std::fs::remove_file(db_path);
    }

    #[test]
    fn compute_analytics_calculates_savings_and_breakdowns() {
        let records = vec![
            ExecutionRecord {
                id: "r1".into(),
                mission_id: "m1".into(),
                task_id: "t1".into(),
                task_title: "Fix typo".into(),
                node_id: "win-main".into(),
                slot_id: Some("s1".into()),
                agent_type: Some(AgentType::Claude),
                model_name: "claude-haiku".into(),
                difficulty: DifficultyTier::L1,
                status: TaskStatus::Completed,
                started_at_ms: 1000,
                finished_at_ms: 4000,
                duration_ms: 3000,
                retry_count: 0,
                escalated: false,
                estimated_cost_units: 1,
            },
            ExecutionRecord {
                id: "r2".into(),
                mission_id: "m1".into(),
                task_id: "t2".into(),
                task_title: "Add endpoint".into(),
                node_id: "win-main".into(),
                slot_id: Some("s2".into()),
                agent_type: Some(AgentType::Codex),
                model_name: "gpt-5.4-mini".into(),
                difficulty: DifficultyTier::L2,
                status: TaskStatus::Completed,
                started_at_ms: 1000,
                finished_at_ms: 7000,
                duration_ms: 6000,
                retry_count: 1,
                escalated: true,
                estimated_cost_units: 3,
            },
            ExecutionRecord {
                id: "r3".into(),
                mission_id: "m2".into(),
                task_id: "t3".into(),
                task_title: "Security audit".into(),
                node_id: "mac-build".into(),
                slot_id: Some("s3".into()),
                agent_type: Some(AgentType::Claude),
                model_name: "claude-opus-4-6".into(),
                difficulty: DifficultyTier::L4,
                status: TaskStatus::Failed,
                started_at_ms: 2000,
                finished_at_ms: 11000,
                duration_ms: 9000,
                retry_count: 2,
                escalated: true,
                estimated_cost_units: 8,
            },
        ];

        let analytics = compute_analytics(&records);
        assert_eq!(analytics.total_runs, 3);
        assert_eq!(analytics.completed_runs, 2);
        assert_eq!(analytics.failed_runs, 1);
        assert_eq!(analytics.success_rate_percent, 66);
        assert_eq!(analytics.avg_duration_ms, 6000);
        assert_eq!(analytics.escalated_runs, 2);
        // Actual = 1 + 3 + 8 = 12 vs baseline 3 * 8 = 24 -> 50% savings
        assert_eq!(analytics.total_cost_units, 12);
        assert_eq!(analytics.baseline_l3_cost_units, 24);
        assert_eq!(analytics.estimated_savings_percent, 50);
        assert_eq!(analytics.by_tier.len(), 3);
        assert_eq!(analytics.by_tier[0].key, "L1");
        assert_eq!(analytics.by_node[0].key, "win-main");
        assert_eq!(analytics.by_node[0].total, 2);
    }

    #[tokio::test]
    async fn probe_node_reports_latency_and_capabilities() {
        let creates: Arc<StdMutex<Vec<Value>>> = Arc::new(StdMutex::new(Vec::new()));
        let completed: Arc<StdMutex<Vec<String>>> = Arc::new(StdMutex::new(Vec::new()));
        let (addr, handle) = spawn_mock_termexo("probe", creates, completed).await;

        let db_path = std::env::temp_dir().join(format!(
            "termexo-fleet-probe-{}.db",
            uuid::Uuid::new_v4().simple()
        ));
        let db = Arc::new(FleetDatabase::open(db_path.clone()).unwrap());
        let runtime = FleetRuntime::new(db).unwrap();

        let result = runtime
            .probe_node(&format!("http://{}:{}/mcp", addr.ip(), addr.port()), "tok", false)
            .await;
        assert!(result.reachable, "expected reachable: {:?}", result.error);
        assert_eq!(result.workspace_count, 1);
        assert_eq!(result.terminal_count, 0);

        handle.abort();
        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn send_test_notification_posts_webhook_json() {
        let received: Arc<StdMutex<Vec<Value>>> = Arc::new(StdMutex::new(Vec::new()));
        let received_clone = received.clone();
        let app = Router::new().route(
            "/hook",
            post(move |Json(body): Json<Value>| {
                let store = received_clone.clone();
                async move {
                    store.lock().unwrap().push(body);
                    Json(json!({ "ok": true }))
                }
            }),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let db_path = std::env::temp_dir().join(format!(
            "termexo-fleet-notify-{}.db",
            uuid::Uuid::new_v4().simple()
        ));
        let db = Arc::new(FleetDatabase::open(db_path.clone()).unwrap());
        let runtime = FleetRuntime::new(db).unwrap();

        runtime
            .save_notification_settings(NotificationSettings {
                webhook_enabled: true,
                webhook_url: format!("http://{}:{}/hook", addr.ip(), addr.port()),
                wechat_via_termexo: false,
                ..NotificationSettings::default()
            })
            .await
            .unwrap();

        let summary = runtime.send_test_notification().await.unwrap();
        assert!(summary.contains("Webhook"));
        let payloads = received.lock().unwrap();
        assert_eq!(payloads.len(), 1);
        assert_eq!(payloads[0]["source"], "Termexo Fleet");
        assert_eq!(payloads[0]["event"], "test");

        handle.abort();
        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn full_phase3_live_http_e2e() {
        let creates: Arc<StdMutex<Vec<Value>>> = Arc::new(StdMutex::new(Vec::new()));
        let completed_a: Arc<StdMutex<Vec<String>>> = Arc::new(StdMutex::new(Vec::new()));
        let completed_b: Arc<StdMutex<Vec<String>>> = Arc::new(StdMutex::new(Vec::new()));
        let failed_once_b: Arc<StdMutex<Option<String>>> = Arc::new(StdMutex::new(None));
        let wechat_calls: Arc<StdMutex<Vec<Value>>> = Arc::new(StdMutex::new(Vec::new()));
        let webhooks: Arc<StdMutex<Vec<Value>>> = Arc::new(StdMutex::new(Vec::new()));

        // Enhanced mock Termexo supporting tools/list (capabilities), terminal_read (429 rate-limit), and wechat_notify.
        let spawn_rich_termexo = |node_label: &'static str,
                                  creates_ref: Arc<StdMutex<Vec<Value>>>,
                                  completed_ref: Arc<StdMutex<Vec<String>>>,
                                  fail_once_ref: Arc<StdMutex<Option<String>>>,
                                  wechat_ref: Arc<StdMutex<Vec<Value>>>| async move {
            let app = Router::new().route(
                "/mcp",
                post(move |Json(body): Json<Value>| {
                    let creates = creates_ref.clone();
                    let completed = completed_ref.clone();
                    let fail_once = fail_once_ref.clone();
                    let wechat = wechat_ref.clone();
                    async move {
                        let id = body.get("id").cloned().unwrap_or(json!(1));
                        let method = body.get("method").and_then(Value::as_str).unwrap_or("");
                        if method == "initialize" {
                            return Json(json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "result": { "protocolVersion": "2025-11-25", "capabilities": {} }
                            }));
                        }
                        if method == "notifications/initialized" {
                            return Json(json!({}));
                        }
                        if method == "tools/list" {
                            return Json(json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "result": {
                                    "tools": [
                                        {
                                            "name": "task_create",
                                            "inputSchema": {
                                                "type": "object",
                                                "properties": {
                                                    "accountProfileId": { "type": "string" },
                                                    "mcpProfileIds": { "type": "array" }
                                                }
                                            }
                                        },
                                        { "name": "wechat_notify", "inputSchema": { "type": "object" } }
                                    ]
                                }
                            }));
                        }
                        let tool = body["params"]["name"].as_str().unwrap_or("");
                        let args = body["params"]["arguments"].clone();
                        let payload = match tool {
                            "workspace_list" => json!([{
                                "id": "w1",
                                "name": "DemoRepo",
                                "projectPath": "D:/nonexistent-repo-for-e2e",
                                "terminalIds": []
                            }]),
                            "terminal_list" => json!([]),
                            "settings_get" => json!({
                                "modelProfiles": [
                                    {
                                        "id": "prof-claude-fast",
                                        "name": "Claude Fast",
                                        "provider": "Anthropic",
                                        "isDefault": false,
                                        "hasCredential": true,
                                        "claudeEnabled": true,
                                        "claudeModel": "claude-haiku-4-5",
                                        "codexEnabled": false,
                                        "codexModel": ""
                                    },
                                    {
                                        "id": "prof-claude-pro",
                                        "name": "Claude Pro",
                                        "provider": "Anthropic",
                                        "isDefault": true,
                                        "hasCredential": true,
                                        "claudeEnabled": true,
                                        "claudeModel": "claude-opus-4-6",
                                        "codexEnabled": false,
                                        "codexModel": ""
                                    },
                                    {
                                        "id": "prof-codex",
                                        "name": "OpenAI",
                                        "provider": "OpenAI",
                                        "isDefault": true,
                                        "hasCredential": true,
                                        "claudeEnabled": false,
                                        "claudeModel": "",
                                        "codexEnabled": true,
                                        "codexModel": "gpt-5.4"
                                    }
                                ],
                                "accountProfiles": [
                                    {
                                        "id": "acc-claude-1",
                                        "name": "Claude Max",
                                        "agentType": "claude",
                                        "authenticated": true,
                                        "isDefault": true,
                                        "isSystem": false
                                    },
                                    {
                                        "id": "acc-codex-1",
                                        "name": "Codex Pro",
                                        "agentType": "codex",
                                        "authenticated": true,
                                        "isDefault": true,
                                        "isSystem": false
                                    }
                                ],
                                "mcpProfiles": [{ "id": "mcp-fleet", "name": "Termexo Fleet" }]
                            }),
                            "task_list" => {
                                let done = completed.lock().unwrap();
                                let fail_id = fail_once.lock().unwrap().clone();
                                let mut tasks: Vec<Value> = done
                                    .iter()
                                    .map(|tid| {
                                        json!({
                                            "id": tid,
                                            "workspaceId": "w1",
                                            "projectId": "p1",
                                            "title": "Done",
                                            "stage": "completed",
                                            "executionState": "completed",
                                            "promptDeliveryState": "delivered",
                                            "agentType": "claude",
                                            "modelName": "claude-opus-4-6",
                                            "outputTail": "Verified cleanly."
                                        })
                                    })
                                    .collect();
                                if let Some(fid) = fail_id {
                                    tasks.push(json!({
                                        "id": fid,
                                        "workspaceId": "w1",
                                        "projectId": "p1",
                                        "title": "RateLimited",
                                        "stage": "executing",
                                        "executionState": "failed",
                                        "promptDeliveryState": "delivered",
                                        "agentType": "claude",
                                        "modelName": "claude-haiku-4-5",
                                        "terminalId": "term-fail-429",
                                        "lastTerminalStatus": "RATE_LIMITED",
                                        "lastError": "HTTP 429 rate limit exceeded"
                                    }));
                                }
                                json!({
                                    "workspaceId": "w1",
                                    "projects": [{ "id": "p1", "workspaceId": "w1", "name": "Default", "path": "D:/nonexistent" }],
                                    "tasks": tasks
                                })
                            }
                            "terminal_read" => {
                                if args["terminalId"].as_str() == Some("term-fail-429") {
                                    json!({ "data": "API Error: 429 rate limit exceeded", "sequence": 1, "truncated": false })
                                } else {
                                    json!({ "data": "All unit tests passed.\nFLEET_E2E_OK", "sequence": 1, "truncated": false })
                                }
                            }
                            "task_create" => {
                                let mut list = creates.lock().unwrap();
                                let task_id = format!("{node_label}-tx-{}", list.len() + 1);
                                let mut record = args.clone();
                                record["_node"] = json!(node_label);
                                list.push(record);
                                json!({
                                    "id": task_id,
                                    "workspaceId": "w1",
                                    "projectId": "p1",
                                    "title": args["title"],
                                    "stage": "todo",
                                    "executionState": "idle",
                                    "promptDeliveryState": "idle",
                                    "agentType": args["agentType"],
                                    "modelName": args.get("modelName").and_then(Value::as_str).unwrap_or(""),
                                    "accountProfileId": args.get("accountProfileId")
                                })
                            }
                            "task_execute" => json!({
                                "id": args["taskId"],
                                "workspaceId": "w1",
                                "projectId": "p1",
                                "title": "Running",
                                "stage": "executing",
                                "executionState": "running",
                                "promptDeliveryState": "delivered",
                                "agentType": "claude",
                                "modelName": "claude-opus-4-6",
                                "terminalId": "term-live-1"
                            }),
                            "wechat_notify" => {
                                wechat.lock().unwrap().push(args);
                                json!({ "sent": true })
                            }
                            _ => json!({}),
                        };
                        Json(json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": { "structuredContent": payload, "isError": false }
                        }))
                    }
                }),
            );
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let handle = tokio::spawn(async move {
                let _ = axum::serve(listener, app).await;
            });
            (addr, handle)
        };

        let (addr_a, handle_a) = spawn_rich_termexo(
            "win",
            creates.clone(),
            completed_a.clone(),
            Arc::new(StdMutex::new(None)),
            wechat_calls.clone(),
        )
        .await;
        let (addr_b, handle_b) = spawn_rich_termexo(
            "mac",
            creates.clone(),
            completed_b.clone(),
            failed_once_b.clone(),
            wechat_calls.clone(),
        )
        .await;

        // Live Webhook receiver
        let webhooks_clone = webhooks.clone();
        let hook_app = Router::new().route(
            "/webhook",
            post(move |Json(body): Json<Value>| {
                let store = webhooks_clone.clone();
                async move {
                    store.lock().unwrap().push(body);
                    Json(json!({ "ok": true }))
                }
            }),
        );
        let hook_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let hook_addr = hook_listener.local_addr().unwrap();
        let hook_handle = tokio::spawn(async move {
            let _ = axum::serve(hook_listener, hook_app).await;
        });

        let db_path = std::env::temp_dir().join(format!(
            "termexo-fleet-live-http-e2e-{}.db",
            uuid::Uuid::new_v4().simple()
        ));
        let db = Arc::new(FleetDatabase::open(db_path.clone()).unwrap());
        let runtime = FleetRuntime::new(db).unwrap();

        // Start REAL Reverse Fleet MCP Server on an ephemeral port (0 -> OS assigns free port)
        let running_mcp = fleet_mcp_server::start_local_server(0, runtime.clone())
            .await
            .unwrap();
        let fleet_mcp_url = running_mcp.url();
        let fleet_mcp_token = runtime.overview().await.unwrap().mcp_server_token;

        // 1. Probe node over real HTTP -> verify reachability & capability flags
        let probe = runtime
            .probe_node(
                &format!("http://{}:{}/mcp", addr_b.ip(), addr_b.port()),
                "tok-b",
                false,
            )
            .await;
        assert!(probe.reachable);
        assert!(probe.supports_account_routing);
        assert!(probe.supports_mcp_profiles);
        assert_eq!(probe.workspace_count, 1);

        // 2. Connect both nodes & configure Webhook + Termexo WeChat notifications
        runtime
            .upsert_node(UpsertNodeInput {
                id: Some("win-main".into()),
                name: "Windows Main".into(),
                transport_type: TransportType::Local,
                endpoint: format!("http://{}:{}/mcp", addr_a.ip(), addr_a.port()),
                token: Some("tok-a".into()),
                tags: vec!["windows".into(), "local".into()],
                max_concurrency: 4,
                allow_insecure_tls: false,
            })
            .await
            .unwrap();
        runtime
            .upsert_node(UpsertNodeInput {
                id: Some("mac-build".into()),
                name: "Mac Studio".into(),
                transport_type: TransportType::Lan,
                endpoint: format!("http://{}:{}/mcp", addr_b.ip(), addr_b.port()),
                token: Some("tok-b".into()),
                tags: vec!["macos".into(), "ios".into()],
                max_concurrency: 4,
                allow_insecure_tls: false,
            })
            .await
            .unwrap();
        runtime
            .save_notification_settings(NotificationSettings {
                webhook_enabled: true,
                webhook_url: format!("http://{}:{}/webhook", hook_addr.ip(), hook_addr.port()),
                wechat_via_termexo: true,
                notify_on_completed: true,
                notify_on_failed: true,
                notify_on_approval: true,
                notify_on_slot_unhealthy: true,
            })
            .await
            .unwrap();

        // 3. Verify Reverse MCP rejects unauthenticated HTTP request with 401
        let http = reqwest::Client::new();
        let unauth = http
            .post(&fleet_mcp_url)
            .json(&json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {} }))
            .send()
            .await
            .unwrap();
        assert_eq!(unauth.status(), reqwest::StatusCode::UNAUTHORIZED);

        // 4. Create PlannerDag mission
        let (mission, planner) = runtime
            .create_mission(CreateMissionInput {
                title: "Full Phase 3 E2E Mission".into(),
                goal_prompt: "Plan, execute with failover, spawn subagent, L4 review, notify".into(),
                node_id: "win-main".into(),
                workspace_id: "w1".into(),
                project_id: String::new(),
                routing_mode: RoutingMode::Balanced,
                workflow_template: WorkflowTemplate::PlannerDag,
                required_node_tags: vec![],
                execution_mode: ExecutionMode::Task,
                preferred_terminal_id: None,
                difficulty: None,
                preferred_agent: Some(AgentType::Claude),
                guardrails: GuardrailPolicy::default(),
                fleet_mcp_profile_id: Some("mcp-fleet".into()),
                auto_dispatch: true,
            })
            .await
            .unwrap();

        // 5. Planner calls REAL HTTP POST /mcp -> fleet_submit_plan
        let submit_resp: Value = http
            .post(&fleet_mcp_url)
            .bearer_auth(&fleet_mcp_token)
            .json(&json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/call",
                "params": {
                    "name": "fleet_submit_plan",
                    "arguments": {
                        "parent_token": planner.parent_token,
                        "steps": [
                            {
                                "key": "impl",
                                "title": "Implement macOS keychain helper",
                                "goal": "Write helper",
                                "difficulty": "L1",
                                "role": "worker",
                                "preferredAgent": "claude",
                                "requiredNodeTags": ["macos"],
                                "isolateWorktree": false
                            },
                            {
                                "key": "review",
                                "title": "L4 Security Audit",
                                "goal": "Cross-family review",
                                "dependsOnKeys": ["impl"],
                                "difficulty": "L4",
                                "role": "reviewer",
                                "isolateWorktree": false
                            }
                        ]
                    }
                }
            }))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let impl_id = submit_resp["result"]["structuredContent"]["dispatchedTaskIds"][0]
            .as_str()
            .unwrap()
            .to_owned();
        let impl_task_before = runtime
            .db
            .list_tasks()
            .unwrap()
            .into_iter()
            .find(|t| t.id == impl_id)
            .unwrap();
        assert_eq!(impl_task_before.difficulty, DifficultyTier::L1);
        assert_eq!(impl_task_before.assigned_node_id.as_deref(), Some("mac-build"));

        // 6. Simulate HTTP 429 rate limit on impl_task -> sync triggers cooldown + automatic L1->L2 escalation retry!
        *failed_once_b.lock().unwrap() = impl_task_before.termexo_task_id.clone();
        runtime.sync_all_nodes().await.unwrap();
        *failed_once_b.lock().unwrap() = None;

        let impl_after_retry = runtime
            .db
            .list_tasks()
            .unwrap()
            .into_iter()
            .find(|t| t.id == impl_id)
            .unwrap();
        assert_eq!(impl_after_retry.status, TaskStatus::Running);
        assert_eq!(impl_after_retry.retry_count, 1);
        assert_eq!(impl_after_retry.difficulty, DifficultyTier::L2);

        // 7. Worker spawns an autonomous sub-agent via REAL HTTP POST /mcp (and duplicate spawn is blocked by guardrail!)
        let spawn_ok: Value = http
            .post(&fleet_mcp_url)
            .bearer_auth(&fleet_mcp_token)
            .json(&json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "tools/call",
                "params": {
                    "name": "fleet_spawn_subagent",
                    "arguments": {
                        "parent_token": impl_after_retry.parent_token,
                        "title": "Audit keychain edge cases",
                        "goal": "Check empty keychain error handling",
                        "difficulty": "L1",
                        "isolate_worktree": false
                    }
                }
            }))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(spawn_ok["result"]["isError"], json!(false));
        let subagent_id = spawn_ok["result"]["structuredContent"]["subagentId"]
            .as_str()
            .unwrap()
            .to_owned();

        let spawn_dup: Value = http
            .post(&fleet_mcp_url)
            .bearer_auth(&fleet_mcp_token)
            .json(&json!({
                "jsonrpc": "2.0",
                "id": 4,
                "method": "tools/call",
                "params": {
                    "name": "fleet_spawn_subagent",
                    "arguments": {
                        "parent_token": impl_after_retry.parent_token,
                        "title": "Audit keychain edge cases",
                        "goal": "Duplicate goal",
                        "difficulty": "L1",
                        "isolate_worktree": false
                    }
                }
            }))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(spawn_dup["result"]["isError"], json!(true));

        // 8. Complete sub-agent and impl -> unlocks L4 cross-family Reviewer -> complete Reviewer -> Mission completes!
        let sub_task = runtime
            .db
            .list_tasks()
            .unwrap()
            .into_iter()
            .find(|t| t.id == subagent_id)
            .unwrap();
        if let Some(tid) = sub_task.termexo_task_id.clone() {
            if sub_task.assigned_node_id.as_deref() == Some("mac-build") {
                completed_b.lock().unwrap().push(tid);
            } else {
                completed_a.lock().unwrap().push(tid);
            }
        }
        completed_b
            .lock()
            .unwrap()
            .push(impl_after_retry.termexo_task_id.clone().unwrap());
        runtime.sync_all_nodes().await.unwrap();

        let review_task = runtime
            .db
            .list_tasks()
            .unwrap()
            .into_iter()
            .find(|t| t.mission_id == mission.id && t.role == TaskRole::Reviewer)
            .unwrap();
        assert_eq!(review_task.status, TaskStatus::Running);
        assert_eq!(review_task.assigned_agent_type, Some(AgentType::Codex));
        if let Some(tid) = review_task.termexo_task_id.clone() {
            if review_task.assigned_node_id.as_deref() == Some("mac-build") {
                completed_b.lock().unwrap().push(tid);
            } else {
                completed_a.lock().unwrap().push(tid);
            }
        }
        runtime.sync_all_nodes().await.unwrap();

        // 9. Verify Mission Completed + SQLite Execution Ledger + Analytics + Webhook & WeChat notifications!
        let overview = runtime.overview().await.unwrap();
        let finished_mission = overview
            .missions
            .iter()
            .find(|m| m.id == mission.id)
            .unwrap();
        assert_eq!(finished_mission.status, MissionStatus::Completed);
        assert!(overview.execution_records.len() >= 4);
        assert!(overview.analytics.total_runs >= 4);
        assert!(overview.analytics.escalated_runs >= 1);
        assert!(overview.analytics.estimated_savings_percent > 0);

        let delivered_hooks = webhooks.lock().unwrap();
        assert!(
            delivered_hooks.iter().any(|h| h["event"] == "completed"),
            "expected completed webhook, got: {:?}",
            *delivered_hooks
        );
        let delivered_wechat = wechat_calls.lock().unwrap();
        assert!(
            delivered_wechat.iter().any(|w| w["kind"] == "task_completed"),
            "expected wechat_notify task_completed, got: {:?}",
            *delivered_wechat
        );

        drop(running_mcp);
        hook_handle.abort();
        handle_a.abort();
        handle_b.abort();
        let _ = std::fs::remove_file(db_path);
    }
}
