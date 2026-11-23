use std::sync::Arc;

use fleet_protocol::{
    AccountModelSlot, FleetNode, Mission, MissionTask, NodeProbeResult, NotificationSettings,
};
use tauri::State;

use crate::runtime::{CreateMissionInput, FleetRuntime, FleetStateOverview, UpsertNodeInput};

#[tauri::command]
pub async fn get_fleet_overview(
    runtime: State<'_, Arc<FleetRuntime>>,
) -> Result<FleetStateOverview, String> {
    runtime.overview().await
}

#[tauri::command]
pub async fn probe_fleet_node(
    endpoint: String,
    token: String,
    allow_insecure_tls: bool,
    runtime: State<'_, Arc<FleetRuntime>>,
) -> Result<NodeProbeResult, String> {
    Ok(runtime.probe_node(&endpoint, &token, allow_insecure_tls).await)
}

#[tauri::command]
pub async fn save_notification_settings(
    settings: NotificationSettings,
    runtime: State<'_, Arc<FleetRuntime>>,
) -> Result<NotificationSettings, String> {
    runtime.save_notification_settings(settings).await
}

#[tauri::command]
pub async fn send_test_notification(
    runtime: State<'_, Arc<FleetRuntime>>,
) -> Result<String, String> {
    runtime.send_test_notification().await
}

#[tauri::command]
pub async fn upsert_fleet_node(
    input: UpsertNodeInput,
    runtime: State<'_, Arc<FleetRuntime>>,
) -> Result<FleetNode, String> {
    runtime.upsert_node(input).await
}

#[tauri::command]
pub async fn remove_fleet_node(
    node_id: String,
    runtime: State<'_, Arc<FleetRuntime>>,
) -> Result<(), String> {
    runtime.remove_node(&node_id).await
}

#[tauri::command]
pub async fn sync_fleet_node(
    node_id: String,
    runtime: State<'_, Arc<FleetRuntime>>,
) -> Result<FleetNode, String> {
    runtime.sync_node(&node_id).await
}

#[tauri::command]
pub async fn sync_all_fleet_nodes(runtime: State<'_, Arc<FleetRuntime>>) -> Result<(), String> {
    runtime.sync_all_nodes().await
}

#[tauri::command]
pub async fn save_account_slot(
    slot: AccountModelSlot,
    runtime: State<'_, Arc<FleetRuntime>>,
) -> Result<AccountModelSlot, String> {
    runtime.save_slot(slot).await
}

#[tauri::command]
pub async fn reset_account_slot_health(
    slot_id: String,
    runtime: State<'_, Arc<FleetRuntime>>,
) -> Result<AccountModelSlot, String> {
    runtime.reset_slot_health(&slot_id).await
}

#[tauri::command]
pub async fn create_mission(
    input: CreateMissionInput,
    runtime: State<'_, Arc<FleetRuntime>>,
) -> Result<(Mission, MissionTask), String> {
    runtime.create_mission(input).await
}

#[tauri::command]
pub async fn dispatch_mission_task(
    task_id: String,
    runtime: State<'_, Arc<FleetRuntime>>,
) -> Result<MissionTask, String> {
    runtime.dispatch_task(&task_id).await
}

#[tauri::command]
pub async fn retry_task_with_escalation(
    task_id: String,
    runtime: State<'_, Arc<FleetRuntime>>,
) -> Result<MissionTask, String> {
    runtime.retry_task_with_escalation(&task_id).await
}

#[tauri::command]
pub async fn cancel_mission_task(
    task_id: String,
    runtime: State<'_, Arc<FleetRuntime>>,
) -> Result<MissionTask, String> {
    runtime.cancel_task(&task_id).await
}

#[tauri::command]
pub async fn advance_mission_dag(
    mission_id: String,
    runtime: State<'_, Arc<FleetRuntime>>,
) -> Result<Vec<String>, String> {
    runtime.advance_mission_dag(&mission_id).await
}

#[tauri::command]
pub async fn reply_node_terminal(
    node_id: String,
    terminal_id: String,
    message: String,
    submit: bool,
    runtime: State<'_, Arc<FleetRuntime>>,
) -> Result<(), String> {
    runtime
        .reply_terminal(&node_id, &terminal_id, &message, submit)
        .await
}

#[tauri::command]
pub async fn read_node_terminal_tail(
    node_id: String,
    terminal_id: String,
    runtime: State<'_, Arc<FleetRuntime>>,
) -> Result<String, String> {
    runtime.read_terminal_tail(&node_id, &terminal_id).await
}
