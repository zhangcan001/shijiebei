use crate::db::{app_dir, cache_get, open_conn};
use crate::services::system_service::project_health_report;
use chrono::Utc;
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::fs;
use tauri::AppHandle;

#[tauri::command]
pub async fn get_project_health_report() -> Result<Value, String> {
    Ok(project_health_report())
}

fn table_exists(conn: &Connection, table: &str) -> bool {
    conn.query_row(
        "select count(*) from sqlite_master where type='table' and name=?1",
        params![table],
        |row| row.get::<_, i64>(0),
    )
    .map(|count| count > 0)
    .unwrap_or(false)
}

fn count_i64(conn: &Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |row| row.get::<_, i64>(0))
        .unwrap_or(0)
}

fn push_check(checks: &mut Vec<Value>, name: &str, ok: bool, severity: &str, message: &str) {
    checks.push(json!({
        "name": name,
        "ok": ok,
        "severity": severity,
        "message": message
    }));
}

pub(crate) fn startup_status_from_checks(checks: &[Value]) -> &'static str {
    if checks.iter().any(|item| {
        !item.get("ok").and_then(Value::as_bool).unwrap_or(false)
            && item.get("severity").and_then(Value::as_str) == Some("critical")
    }) {
        "critical"
    } else if checks
        .iter()
        .any(|item| !item.get("ok").and_then(Value::as_bool).unwrap_or(false))
    {
        "warning"
    } else {
        "ok"
    }
}

#[tauri::command]
pub async fn get_startup_health_check(app: AppHandle) -> Result<Value, String> {
    let mut checks = Vec::new();
    let mut warnings = Vec::new();
    let mut suggested_actions = Vec::new();
    let conn = match open_conn(&app) {
        Ok(conn) => {
            push_check(
                &mut checks,
                "database_readable",
                true,
                "critical",
                "数据库可读。",
            );
            conn
        }
        Err(error) => {
            push_check(
                &mut checks,
                "database_readable",
                false,
                "critical",
                &format!("数据库不可读：{error}"),
            );
            warnings.push("数据库不可读，历史快照和结算可能无法加载。".to_string());
            suggested_actions.push("重启软件；如仍失败，请先备份当前数据目录。".to_string());
            return Ok(json!({
                "status": "critical",
                "checks": checks,
                "warnings": warnings,
                "suggested_actions": suggested_actions
            }));
        }
    };

    let required_tables = [
        "cache",
        "odds_snapshots",
        "pre_match_snapshots",
        "pre_match_snapshot_results",
        "match_results",
        "paper_trading_records",
        "snapshot_audit_logs",
        "data_providers",
        "provider_credentials",
    ];
    let missing_tables = required_tables
        .iter()
        .filter(|table| !table_exists(&conn, table))
        .map(|table| (*table).to_string())
        .collect::<Vec<_>>();
    let tables_ok = missing_tables.is_empty();
    push_check(
        &mut checks,
        "required_tables",
        tables_ok,
        "critical",
        if tables_ok {
            "核心数据表已就绪。"
        } else {
            "存在缺失数据表。"
        },
    );
    if !tables_ok {
        warnings.push(format!("缺失数据表：{}", missing_tables.join(", ")));
        suggested_actions.push("运行一次全局刷新或重启软件，让数据库迁移自动补齐。".to_string());
    }

    let api_football_configured = conn
        .query_row(
            "select count(*) from provider_credentials where provider_id='api_football'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map(|count| count > 0)
        .unwrap_or(false);
    push_check(
        &mut checks,
        "api_football_key_configured",
        api_football_configured,
        "warning",
        if api_football_configured {
            "API-Football Key 已配置，未返回明文。"
        } else {
            "API-Football Key 未配置或该数据源已停用。"
        },
    );

    let match_cache_count = cache_get(&conn, "sporttery")
        .ok()
        .flatten()
        .and_then(|record| record.value.as_array().map(|items| items.len() as i64))
        .unwrap_or(0);
    push_check(
        &mut checks,
        "today_matches_synced",
        match_cache_count > 0,
        "warning",
        if match_cache_count > 0 {
            "今日比赛缓存已存在。"
        } else {
            "今日比赛缓存为空。"
        },
    );
    if match_cache_count == 0 {
        warnings.push("今日比赛尚未同步。".to_string());
        suggested_actions.push("打开数据源页执行一次全局刷新。".to_string());
    }

    let odds_count = if table_exists(&conn, "odds_snapshots") {
        count_i64(&conn, "select count(*) from odds_snapshots")
    } else {
        0
    };
    push_check(
        &mut checks,
        "today_odds_available",
        odds_count > 0,
        "warning",
        if odds_count > 0 {
            "赔率快照已存在。"
        } else {
            "赔率快照为空。"
        },
    );
    if odds_count == 0 {
        warnings.push("当前没有赔率快照，EV 和赔率变化无法可靠判断。".to_string());
        suggested_actions.push("同步赔率后再生成 final snapshot。".to_string());
    }

    let snapshot_count = if table_exists(&conn, "pre_match_snapshots") {
        count_i64(&conn, "select count(*) from pre_match_snapshots")
    } else {
        0
    };
    let final_snapshot_count = if table_exists(&conn, "pre_match_snapshots") {
        count_i64(
            &conn,
            "select count(*) from pre_match_snapshots where is_final_pre_match=1",
        )
    } else {
        0
    };
    push_check(
        &mut checks,
        "pre_match_snapshots_available",
        snapshot_count > 0,
        "warning",
        if snapshot_count > 0 {
            "赛前快照已存在。"
        } else {
            "暂无赛前快照。"
        },
    );
    push_check(
        &mut checks,
        "final_snapshot_available",
        final_snapshot_count > 0,
        "warning",
        if final_snapshot_count > 0 {
            "已存在 final snapshot。"
        } else {
            "暂无 final snapshot。"
        },
    );
    if snapshot_count == 0 {
        warnings.push("暂无赛前快照，今日方案会退回即时预测并标记非冻结。".to_string());
        suggested_actions.push("赛前生成快照，并在临场前标记 final snapshot。".to_string());
    } else if final_snapshot_count == 0 {
        warnings.push("已有快照但没有 final snapshot。".to_string());
        suggested_actions.push("临场确认赔率后标记 final snapshot。".to_string());
    }

    let result_count = if table_exists(&conn, "match_results") {
        count_i64(&conn, "select count(*) from match_results")
    } else {
        0
    };
    push_check(
        &mut checks,
        "results_synced",
        result_count > 0,
        "warning",
        if result_count > 0 {
            "赛果数据已存在。"
        } else {
            "赛果数据为空。"
        },
    );
    if result_count == 0 {
        warnings.push("赛果数据为空，复盘中心无法自动结算。".to_string());
        suggested_actions.push("赛后先同步赛果，再执行自动结算。".to_string());
    }

    let backup_dir_result = app_dir(&app).map(|dir| dir.join("backups"));
    let backup_writable = backup_dir_result
        .as_ref()
        .ok()
        .and_then(|dir| {
            fs::create_dir_all(dir).ok()?;
            let probe = dir.join(".write_probe");
            fs::write(&probe, Utc::now().to_rfc3339()).ok()?;
            let _ = fs::remove_file(probe);
            Some(())
        })
        .is_some();
    push_check(
        &mut checks,
        "backup_dir_writable",
        backup_writable,
        "critical",
        if backup_writable {
            "备份目录可写。"
        } else {
            "备份目录不可写。"
        },
    );
    if !backup_writable {
        warnings.push("备份目录不可写，自动/手动导出可能失败。".to_string());
        suggested_actions.push("检查应用数据目录权限，或更换可写磁盘位置。".to_string());
    }

    let last_backup = cache_get(&conn, "last_backup")
        .ok()
        .flatten()
        .map(|record| record.value)
        .unwrap_or(Value::Null);
    let has_backup_today = last_backup
        .get("created_at")
        .and_then(Value::as_str)
        .map(|created_at| created_at.starts_with(&Utc::now().format("%Y-%m-%d").to_string()))
        .unwrap_or(false);
    push_check(
        &mut checks,
        "backup_today",
        has_backup_today,
        "warning",
        if has_backup_today {
            "今日已备份。"
        } else {
            "今日尚未备份。"
        },
    );
    if !has_backup_today {
        warnings.push("今天还没有备份。".to_string());
        suggested_actions.push("每天结束后导出一次完整 ZIP 备份。".to_string());
    }

    Ok(json!({
        "status": startup_status_from_checks(&checks),
        "checks": checks,
        "warnings": warnings,
        "suggested_actions": suggested_actions,
        "api_football": {
            "configured": api_football_configured,
            "plaintext_returned": false
        },
        "counts": {
            "today_matches": match_cache_count,
            "odds_snapshots": odds_count,
            "pre_match_snapshots": snapshot_count,
            "final_snapshots": final_snapshot_count,
            "match_results": result_count
        },
        "last_backup": last_backup,
        "generated_at": Utc::now().to_rfc3339()
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_check_status_prefers_critical_then_warning() {
        let ok = vec![json!({"ok": true, "severity": "warning"})];
        assert_eq!(startup_status_from_checks(&ok), "ok");
        let warning = vec![json!({"ok": false, "severity": "warning"})];
        assert_eq!(startup_status_from_checks(&warning), "warning");
        let critical = vec![json!({"ok": false, "severity": "critical"})];
        assert_eq!(startup_status_from_checks(&critical), "critical");
    }
}
