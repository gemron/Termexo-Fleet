use super::*;
use axum::{extract::State, routing::post, Json, Router};
use std::sync::Mutex as StdMutex;
use tokio::net::TcpListener;

#[derive(Default)]
struct Mock {
    workspace: &'static str,
    creates: StdMutex<Vec<Value>>,
    board_reads: StdMutex<Vec<String>>,
    state: StdMutex<&'static str>,
    writes: StdMutex<u32>,
    stops: StdMutex<u32>,
}

async fn mcp(State(mock): State<Arc<Mock>>, Json(body): Json<Value>) -> Json<Value> {
    let id = &body["id"];
    let method = body["method"].as_str().unwrap();
    if method == "initialize" {
        return Json(
            json!({"jsonrpc":"2.0","id":id,"result":{"protocolVersion":"2025-11-25","capabilities":{}}}),
        );
    }
    if method == "notifications/initialized" {
        return Json(json!({}));
    }
    if method == "tools/list" {
        return Json(json!({"jsonrpc":"2.0","id":id,"result":{"tools":[
            {"name":"task_create","inputSchema":{"properties":{"accountProfileId":{},"mcpProfileIds":{}}}}
        ]}}));
    }
    let args = &body["params"]["arguments"];
    let status = *mock.state.lock().unwrap();
    let remote = |task_id: &str, execution_state: &str| {
        json!({
            "id":task_id, "workspaceId":mock.workspace, "projectId":"p", "title":"audit",
            "stage":if execution_state == "idle" { "todo" } else { "executing" },
            "executionState":execution_state, "promptDeliveryState":"delivered",
            "agentType":"codex", "modelName":"gpt-5.4", "terminalId":"term",
            "lastTerminalStatus": if execution_state == "waiting" { Some("RATE_LIMITED") } else { None },
            "outputTail": if execution_state == "completed" { Some("finished") } else { None }
        })
    };
    let payload = match body["params"]["name"].as_str().unwrap() {
        "workspace_list" => {
            let workspace = json!({"id":mock.workspace,"name":"audit","projectPath":"D:/fleet-flow-test-no-repository","terminalIds":["term"]});
            if mock.workspace == "remote-ws" {
                json!([{"id":"unrelated","name":"unrelated repo","projectPath":"D:/unrelated","terminalIds":[]}, workspace])
            } else {
                json!([workspace])
            }
        }
        "settings_get" => json!({"mcpProfiles":[{"id":"mcp-fleet","name":"Termexo Fleet"}]}),
        "terminal_list" => {
            json!([{"id":"term","workspaceId":mock.workspace,"name":"audit terminal",
            "agentType":"codex","status": match status { "completed" => "COMPLETED", "stop-ok" => "STOPPED", _ => "IDLE" },
            "model":"gpt-5.4","live":true}])
        }
        "terminal_write" => {
            *mock.writes.lock().unwrap() += 1;
            json!({})
        }
        "terminal_read" => {
            json!({"data": if status == "waiting" { "rate limit exceeded" } else { "finished" }})
        }
        "task_list" => {
            let ws = args["workspaceId"].as_str().unwrap();
            mock.board_reads.lock().unwrap().push(ws.to_owned());
            let tasks: Vec<_> = if ws == mock.workspace {
                mock.creates
                    .lock()
                    .unwrap()
                    .iter()
                    .enumerate()
                    .map(|(i, _)| remote(&format!("t{}", i + 1), status))
                    .collect()
            } else {
                vec![]
            };
            json!({"workspaceId":ws,"projects":[{"id":"p","workspaceId":ws,"name":"audit","path":"D:/audit"}],"tasks":tasks})
        }
        "task_create" => {
            tokio::time::sleep(Duration::from_millis(30)).await;
            let mut creates = mock.creates.lock().unwrap();
            creates.push(args.clone());
            remote(&format!("t{}", creates.len()), "idle")
        }
        "task_execute" if status == "idle" => {
            return Json(json!({"jsonrpc":"2.0","id":id,"result":{
                "isError":true,"content":[{"type":"text","text":"The worker did not start the task"}]
            }}));
        }
        "task_execute" => remote(args["taskId"].as_str().unwrap(), "running"),
        "task_stop" => {
            *mock.stops.lock().unwrap() += 1;
            if matches!(status, "stop-ok" | "waiting") {
                return Json(
                    json!({"jsonrpc":"2.0","id":id,"result":{"isError":false,"structuredContent":remote(args["taskId"].as_str().unwrap(),"stopped")}}),
                );
            }
            return Json(json!({"jsonrpc":"2.0","id":id,"result":{
                "isError":true,"content":[{"type":"text","text":"Stop rejected by worker"}]
            }}));
        }
        _ => json!({}),
    };
    Json(json!({"jsonrpc":"2.0","id":id,"result":{"isError":false,"structuredContent":payload}}))
}

fn runtime() -> Arc<FleetRuntime> {
    Arc::new(FleetRuntime {
        db: Arc::new(FleetDatabase::open(":memory:".into()).unwrap()),
        operations: Mutex::new(()),
        snapshots: RwLock::new(HashMap::new()),
        attention: RwLock::new(Vec::new()),
        token_cache: RwLock::new(HashMap::new()),
        mcp_token: "test".into(),
        mcp_url: RwLock::new("http://127.0.0.1:7425/mcp".into()),
        mcp_server: RwLock::new(None),
    })
}

async fn add_node(runtime: &FleetRuntime, node: &str, workspace: &'static str) -> Arc<Mock> {
    let mock = Arc::new(Mock {
        workspace,
        state: StdMutex::new("running"),
        ..Mock::default()
    });
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = Router::new()
        .route("/mcp", post(mcp))
        .with_state(mock.clone());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    runtime
        .token_cache
        .write()
        .await
        .insert(node.into(), "test".into());
    runtime
        .db
        .save_node(&FleetNode {
            id: node.into(),
            name: node.into(),
            transport_type: TransportType::Local,
            endpoint: format!("http://{addr}/mcp"),
            allow_insecure_tls: false,
            status: NodeStatus::Online,
            tags: vec![node.into()],
            max_concurrency: 8,
            last_heartbeat_ms: None,
            last_error: None,
        })
        .unwrap();
    runtime
        .db
        .save_slot(&AccountModelSlot {
            id: format!("slot-{node}"),
            node_id: node.into(),
            label: "audit".into(),
            agent_type: AgentType::Codex,
            account_profile_id: None,
            model_profile_id: None,
            model_family: "openai".into(),
            model_name: "gpt-5.4".into(),
            supported_tiers: vec![
                DifficultyTier::L1,
                DifficultyTier::L2,
                DifficultyTier::L3,
                DifficultyTier::L4,
            ],
            cost_tier: 1,
            quality_tier: 5,
            max_concurrency: 8,
            active_count: 0,
            priority: 10,
            health: SlotHealth::Healthy,
            cooldown_until_ms: None,
        })
        .unwrap();
    runtime.sync_node(node).await.unwrap();
    mock
}

fn input(workflow: WorkflowTemplate, mode: ExecutionMode) -> CreateMissionInput {
    CreateMissionInput {
        title: "Audit flow".into(),
        goal_prompt: "Perform harmless check".into(),
        node_id: "primary".into(),
        workspace_id: "local-ws".into(),
        project_id: String::new(),
        routing_mode: RoutingMode::Balanced,
        workflow_template: workflow,
        required_node_tags: vec![],
        execution_mode: mode,
        preferred_terminal_id: None,
        difficulty: Some(DifficultyTier::L2),
        preferred_agent: None,
        guardrails: GuardrailPolicy {
            isolate_worktree_by_default: false,
            ..Default::default()
        },
        fleet_mcp_profile_id: None,
        auto_dispatch: true,
    }
}

fn task(runtime: &FleetRuntime, id: &str) -> MissionTask {
    runtime
        .db
        .list_tasks()
        .unwrap()
        .into_iter()
        .find(|t| t.id == id)
        .unwrap()
}

fn mission(runtime: &FleetRuntime, id: &str) -> Mission {
    runtime
        .db
        .list_missions()
        .unwrap()
        .into_iter()
        .find(|m| m.id == id)
        .unwrap()
}

#[tokio::test]
async fn cross_node_completed_task_should_be_reconciled() {
    let runtime = runtime();
    add_node(&runtime, "primary", "local-ws").await;
    let remote = add_node(&runtime, "remote", "remote-ws").await;
    let mut request = input(WorkflowTemplate::Direct, ExecutionMode::Task);
    request.required_node_tags = vec!["remote".into()];
    let (_, created) = runtime.create_mission(request).await.unwrap();
    assert_eq!(
        remote.creates.lock().unwrap()[0]["workspaceId"],
        "remote-ws"
    );
    assert_eq!(created.assigned_workspace_id.as_deref(), Some("remote-ws"));
    *remote.state.lock().unwrap() = "completed";
    runtime.sync_all_nodes().await.unwrap();
    let reads = remote.board_reads.lock().unwrap().clone();
    assert_eq!(
        task(&runtime, &created.id).status,
        TaskStatus::Completed,
        "remote board queries: {reads:?}"
    );
    assert_eq!(reads, ["remote-ws", "remote-ws"]);
}

#[tokio::test]
async fn a_failed_launch_reuses_the_created_remote_task() {
    let runtime = runtime();
    let worker = add_node(&runtime, "primary", "local-ws").await;
    *worker.state.lock().unwrap() = "idle";
    let result = runtime
        .create_mission(input(WorkflowTemplate::Direct, ExecutionMode::Task))
        .await;
    assert!(result.is_err());
    let queued = runtime.db.list_tasks().unwrap().pop().unwrap();
    assert_eq!(queued.status, TaskStatus::Queued);
    runtime.sync_node("primary").await.unwrap();
    assert_eq!(task(&runtime, &queued.id).status, TaskStatus::Pending);
    *worker.state.lock().unwrap() = "running";
    runtime.sync_all_nodes().await.unwrap();
    assert_eq!(worker.creates.lock().unwrap().len(), 1);
    let started = task(&runtime, &queued.id);
    assert_eq!(started.termexo_task_id, queued.termexo_task_id);
    assert_eq!(started.status, TaskStatus::Running);
}

#[tokio::test]
async fn completed_terminal_track_should_release_slot() {
    let runtime = runtime();
    let mock = add_node(&runtime, "primary", "local-ws").await;
    let (_, created) = runtime
        .create_mission(input(WorkflowTemplate::Direct, ExecutionMode::Terminal))
        .await
        .unwrap();
    assert_eq!(*mock.writes.lock().unwrap(), 1);
    *mock.state.lock().unwrap() = "completed";
    runtime.sync_all_nodes().await.unwrap();
    assert_eq!(
        (
            task(&runtime, &created.id).status,
            runtime.db.list_slots().unwrap()[0].active_count
        ),
        (TaskStatus::Completed, 0)
    );
}

#[tokio::test]
async fn failed_stop_should_preserve_running_state_and_capacity() {
    let runtime = runtime();
    let mock = add_node(&runtime, "primary", "local-ws").await;
    let (_, created) = runtime
        .create_mission(input(WorkflowTemplate::Direct, ExecutionMode::Task))
        .await
        .unwrap();
    let result = runtime.cancel_task(&created.id).await;
    assert_eq!(*mock.stops.lock().unwrap(), 1);
    assert_eq!(
        (
            result.is_err(),
            task(&runtime, &created.id).status,
            runtime.db.list_slots().unwrap()[0].active_count
        ),
        (true, TaskStatus::Running, 1)
    );
}

#[tokio::test]
async fn one_task_should_create_one_remote_job_when_dispatched_twice() {
    let runtime = runtime();
    let mock = add_node(&runtime, "primary", "local-ws").await;
    let mut request = input(WorkflowTemplate::Direct, ExecutionMode::Task);
    request.auto_dispatch = false;
    let (_, pending) = runtime.create_mission(request).await.unwrap();
    let _ = tokio::join!(
        runtime.dispatch_task(&pending.id),
        runtime.dispatch_task(&pending.id)
    );
    assert_eq!(mock.creates.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn manual_dispatch_should_respect_unfinished_dag_dependencies() {
    let runtime = runtime();
    let mock = add_node(&runtime, "primary", "local-ws").await;
    let (_, root) = runtime
        .create_mission(input(
            WorkflowTemplate::ImplementVerifyReview,
            ExecutionMode::Task,
        ))
        .await
        .unwrap();
    let verify = runtime
        .db
        .list_tasks()
        .unwrap()
        .into_iter()
        .find(|t| t.depends_on == vec![root.id.clone()])
        .unwrap();
    let result = runtime.dispatch_task(&verify.id).await;
    assert_eq!(
        (result.is_err(), mock.creates.lock().unwrap().len()),
        (true, 1)
    );
}

#[tokio::test]
async fn successful_stop_should_resolve_blocked_dag() {
    let runtime = runtime();
    let mock = add_node(&runtime, "primary", "local-ws").await;
    let (flow, root) = runtime
        .create_mission(input(
            WorkflowTemplate::ImplementVerifyReview,
            ExecutionMode::Task,
        ))
        .await
        .unwrap();
    *mock.state.lock().unwrap() = "stop-ok";
    runtime.cancel_task(&root.id).await.unwrap();
    assert_eq!(*mock.stops.lock().unwrap(), 1);
    runtime.sync_all_nodes().await.unwrap();
    assert_ne!(mission(&runtime, &flow.id).status, MissionStatus::Running);
}

#[tokio::test]
async fn a_provider_waiting_on_rate_limit_should_switch_account_or_node() {
    let runtime = runtime();
    let mock = add_node(&runtime, "primary", "local-ws").await;
    let fallback = add_node(&runtime, "backup", "local-ws").await;
    let (_, created) = runtime
        .create_mission(input(WorkflowTemplate::Direct, ExecutionMode::Task))
        .await
        .unwrap();
    assert_eq!(created.assigned_node_id.as_deref(), Some("primary"));
    *mock.state.lock().unwrap() = "waiting";
    runtime.sync_all_nodes().await.unwrap();
    assert_eq!(
        (
            task(&runtime, &created.id).assigned_node_id.as_deref(),
            fallback.creates.lock().unwrap().len()
        ),
        (Some("backup"), 1)
    );
}

#[tokio::test]
async fn planner_submission_should_respect_total_subagent_budget() {
    let runtime = runtime();
    add_node(&runtime, "primary", "local-ws").await;
    let mut request = input(WorkflowTemplate::PlannerDag, ExecutionMode::Task);
    request.guardrails.max_subagents = 1;
    let (_, planner) = runtime.create_mission(request).await.unwrap();
    let outcome = runtime
        .handle_submit_plan(json!({"parent_token":planner.parent_token,"steps":[
            {"key":"a","title":"a","goal":"a","difficulty":"L2"},
            {"key":"b","title":"b","goal":"b","difficulty":"L2"}
        ]}))
        .await;
    assert!(
        outcome.is_err(),
        "plan exceeding max_subagents was accepted: {outcome:?}"
    );
}

#[tokio::test]
async fn l4_reviewer_should_wait_if_only_author_family_is_available() {
    let runtime = runtime();
    let mock = add_node(&runtime, "primary", "local-ws").await;
    let (_, root) = runtime
        .create_mission(input(
            WorkflowTemplate::ImplementVerifyReview,
            ExecutionMode::Task,
        ))
        .await
        .unwrap();
    assert_eq!(root.assigned_model_name.as_deref(), Some("gpt-5.4"));
    *mock.state.lock().unwrap() = "completed";
    runtime.sync_all_nodes().await.unwrap();
    runtime.sync_all_nodes().await.unwrap();
    let reviewer = runtime
        .db
        .list_tasks()
        .unwrap()
        .into_iter()
        .find(|t| t.role == TaskRole::Reviewer)
        .unwrap();
    assert_eq!(reviewer.exclude_model_family.as_deref(), Some("openai"));
    assert_eq!(
        (reviewer.status, reviewer.assigned_model_name),
        (TaskStatus::Pending, None)
    );
}

#[tokio::test]
async fn requested_worktree_isolation_should_not_silently_start_unisolated() {
    let runtime = runtime();
    let mock = add_node(&runtime, "primary", "local-ws").await;
    let (_, parent) = runtime
        .create_mission(input(WorkflowTemplate::Direct, ExecutionMode::Task))
        .await
        .unwrap();
    let result = runtime
        .handle_spawn_subagent(json!({"parent_token":parent.parent_token,
            "title":"Isolated child", "goal":"Check isolation", "isolate_worktree":true
        }))
        .await;
    let creates = mock.creates.lock().unwrap();
    assert!(
        result.is_err() || creates.last().unwrap().get("workingDirectory").is_some(),
        "isolated child started without a worktree"
    );
}

#[tokio::test]
async fn failed_connect_should_not_resolve_as_success() {
    let runtime = runtime();
    let result = runtime
        .upsert_node(UpsertNodeInput {
            id: Some("audit-offline".into()),
            name: "audit offline".into(),
            transport_type: TransportType::Local,
            endpoint: "http://127.0.0.1:1/mcp".into(),
            token: None,
            tags: vec![],
            max_concurrency: 1,
            allow_insecure_tls: false,
        })
        .await;
    assert!(result.is_err());
    assert!(runtime.db.list_nodes().unwrap().is_empty());
}

#[test]
fn remote_mcp_url_is_saved_as_https_without_altering_the_local_address() {
    let runtime = runtime();
    assert!(runtime
        .save_mcp_remote_url("http://example.net/mcp")
        .is_err());
    assert!(runtime
        .save_mcp_remote_url("https://127.0.0.1/mcp")
        .is_err());
    assert!(runtime
        .save_mcp_remote_url("https://user@example.net/mcp")
        .is_err());
    assert_eq!(
        runtime
            .save_mcp_remote_url(" https://fleet.example.net/mcp ")
            .unwrap()
            .as_deref(),
        Some("https://fleet.example.net/mcp")
    );
    assert_eq!(
        runtime
            .db
            .read_setting(FLEET_MCP_REMOTE_URL_KEY)
            .unwrap()
            .as_deref(),
        Some("https://fleet.example.net/mcp")
    );
    assert_eq!(runtime.save_mcp_remote_url("").unwrap(), None);
    assert_eq!(
        *runtime.mcp_url.try_read().unwrap(),
        "http://127.0.0.1:7425/mcp"
    );
}

#[tokio::test]
async fn repeated_plan_submission_reuses_the_plan_and_respects_active_limit() {
    let runtime = runtime();
    let mock = add_node(&runtime, "primary", "local-ws").await;
    let mut request = input(WorkflowTemplate::PlannerDag, ExecutionMode::Task);
    request.guardrails.max_concurrency = 1;
    let (flow, planner) = runtime.create_mission(request).await.unwrap();
    let plan = json!({"parent_token":planner.parent_token,"steps":[
        {"key":"a","title":"a","goal":"a","difficulty":"L2"},
        {"key":"b","title":"b","goal":"b","difficulty":"L2"}
    ]});
    let first = runtime.handle_submit_plan(plan.clone()).await.unwrap();
    let repeated = runtime.handle_submit_plan(plan).await.unwrap();
    assert_eq!(first["createdTaskIds"], repeated["createdTaskIds"]);
    assert_eq!(first["dispatchedTaskIds"].as_array().unwrap().len(), 1);
    assert_eq!(mock.creates.lock().unwrap().len(), 2);
    assert_eq!(runtime.mission_tasks(&flow.id).unwrap().len(), 3);
}
