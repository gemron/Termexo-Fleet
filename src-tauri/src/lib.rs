mod commands;
mod runtime;
mod storage;

use std::sync::Arc;
use std::time::Duration;

use tauri::{Emitter, Manager};

use crate::runtime::FleetRuntime;
use crate::storage::FleetDatabase;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "termexo_fleet=info,fleet_=info".into()),
        )
        .init();

    tauri::Builder::default()
        .setup(|app| {
            let data_dir = app
                .path()
                .app_data_dir()
                .map_err(|e| format!("Cannot locate app data directory: {e}"))?;
            let db = Arc::new(FleetDatabase::open(data_dir.join("termexo-fleet.db"))?);
            let runtime = FleetRuntime::new(db)?;
            app.manage(runtime.clone());

            let app_handle = app.handle().clone();
            let runtime_bg = runtime.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(error) = runtime_bg.start_mcp_server().await {
                    tracing::error!(%error, "Failed to start reverse Fleet MCP server");
                }
                loop {
                    let _ = runtime_bg.sync_all_nodes().await;
                    let _ = app_handle.emit("fleet-state-changed", ());
                    tokio::time::sleep(Duration::from_secs(3)).await;
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_fleet_overview,
            commands::probe_fleet_node,
            commands::save_notification_settings,
            commands::send_test_notification,
            commands::upsert_fleet_node,
            commands::remove_fleet_node,
            commands::sync_fleet_node,
            commands::sync_all_fleet_nodes,
            commands::save_account_slot,
            commands::reset_account_slot_health,
            commands::create_mission,
            commands::dispatch_mission_task,
            commands::retry_task_with_escalation,
            commands::cancel_mission_task,
            commands::advance_mission_dag,
            commands::reply_node_terminal,
            commands::read_node_terminal_tail,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Termexo Fleet");
}
