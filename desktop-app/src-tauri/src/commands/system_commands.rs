use crate::services::system_service::project_health_report;
use serde_json::Value;

#[tauri::command]
pub async fn get_project_health_report() -> Result<Value, String> {
    Ok(project_health_report())
}
