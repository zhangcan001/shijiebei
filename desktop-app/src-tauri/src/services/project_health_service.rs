use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

fn count_lines(path: &Path) -> i64 {
    fs::read_to_string(path)
        .map(|text| text.lines().count() as i64)
        .unwrap_or(0)
}

fn count_files_with_ext(path: &Path, ext: &str) -> i64 {
    fs::read_dir(path)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().and_then(|value| value.to_str()) == Some(ext))
        .count() as i64
}

fn read_rs_tree(path: &Path) -> String {
    if path.is_file() {
        return fs::read_to_string(path).unwrap_or_default();
    }
    let mut text = String::new();
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let child = entry.path();
            if child.is_dir() {
                text.push_str(&read_rs_tree(&child));
            } else if child.extension().and_then(|value| value.to_str()) == Some("rs") {
                text.push_str(&fs::read_to_string(child).unwrap_or_default());
            }
        }
    }
    text
}

fn desktop_app_root() -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    if cwd.join("src-tauri").exists() {
        cwd
    } else if cwd.join("desktop-app").exists() {
        cwd.join("desktop-app")
    } else {
        cwd.ancestors()
            .find_map(|ancestor| {
                let candidate = ancestor.join("desktop-app");
                if candidate.exists() {
                    Some(candidate)
                } else if ancestor.join("src-tauri").exists() {
                    Some(ancestor.to_path_buf())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| PathBuf::from("."))
    }
}

pub fn project_health_report() -> Value {
    let desktop = desktop_app_root();
    let main_js = desktop.join("src").join("main.js");
    let commands_mod = desktop
        .join("src-tauri")
        .join("src")
        .join("commands")
        .join("mod.rs");
    let legacy_commands = desktop
        .join("src-tauri")
        .join("src")
        .join("commands")
        .join("legacy_commands.rs");
    let docs_arch = desktop.join("docs").join("ARCHITECTURE.md");
    let upset_lab_view = desktop.join("src").join("views").join("UpsetLabView.js");
    let snapshot_view = desktop.join("src").join("views").join("SnapshotView.js");
    let project_health_view = desktop
        .join("src")
        .join("views")
        .join("ProjectHealthView.js");
    let service_path = |name: &str| {
        desktop
            .join("src-tauri")
            .join("src")
            .join("services")
            .join(name)
    };
    let upset_lab_service = service_path("upset_lab_service.rs");
    let snapshot_service = service_path("snapshot_service.rs");
    let review_service = service_path("review_service.rs");
    let export_service = service_path("export_service.rs");
    let project_health_service = service_path("project_health_service.rs");
    let review_guard_service = service_path("review_guard_service.rs");
    let main_js_lines = count_lines(&main_js);
    let commands_mod_lines = count_lines(&commands_mod);
    let legacy_commands_lines = count_lines(&legacy_commands);
    let commands_text = read_rs_tree(&desktop.join("src-tauri").join("src").join("commands"));
    let services_text = read_rs_tree(&desktop.join("src-tauri").join("src").join("services"));
    let development_text = fs::read_to_string(desktop.join("DEVELOPMENT.md")).unwrap_or_default();
    let services_dir = desktop.join("src-tauri").join("src").join("services");
    let views_dir = desktop.join("src").join("views");
    let db_text =
        fs::read_to_string(desktop.join("src-tauri").join("src").join("db.rs")).unwrap_or_default();
    let mut risk_flags = Vec::new();
    let stable_personal_mode = true;
    if !docs_arch.exists() {
        risk_flags.push("missing_architecture_doc");
    }
    if !upset_lab_view.exists() {
        risk_flags.push("missing_upset_lab_view");
    }
    if !upset_lab_service.exists() {
        risk_flags.push("missing_upset_lab_service");
    }
    if !snapshot_service.exists() {
        risk_flags.push("missing_snapshot_service");
    }
    if !review_service.exists() {
        risk_flags.push("missing_review_service");
    }
    if !development_text.contains("v0.2-stable-personal") {
        risk_flags.push("development_version_outdated");
    }
    if !commands_text.contains("score_diversity_guard")
        && !services_text.contains("score_diversity_guard")
    {
        risk_flags.push("score_prior_may_overfit");
    }
    if !commands_text.contains("review_overfit_guard")
        && !services_text.contains("review_overfit_guard")
    {
        risk_flags.push("review_can_auto_change_rules");
    }
    if commands_text.contains("upset_lab_candidates") && commands_text.contains("today_bet_plan") {
        risk_flags.push("upset_lab_not_fully_extracted");
    }
    json!({
        "main_js_size": main_js_lines,
        "main_js_lines": main_js_lines,
        "commands_rs_size": commands_mod_lines,
        "commands_mod_lines": commands_mod_lines,
        "legacy_commands_lines": legacy_commands_lines,
        "has_architecture_doc": docs_arch.exists(),
        "has_upset_lab_view": upset_lab_view.exists(),
        "has_snapshot_view": snapshot_view.exists(),
        "has_project_health_view": project_health_view.exists(),
        "has_upset_lab_service": upset_lab_service.exists(),
        "has_snapshot_service": snapshot_service.exists(),
        "has_review_service": review_service.exists(),
        "has_export_service": export_service.exists(),
        "has_project_health_service": project_health_service.exists(),
        "has_review_guard_service": review_guard_service.exists(),
        "clean_core_version": "v0.2-stable-personal",
        "current_version": "v0.2-stable-personal",
        "stable_personal_mode": stable_personal_mode,
        "legacy_commands_active": legacy_commands_lines > 0,
        "legacy_main_active": main_js_lines <= 5,
        "clean_core_split_paused": true,
        "command_count": commands_text.matches("#[tauri::command]").count() as i64,
        "service_count": count_files_with_ext(&services_dir, "rs"),
        "view_count": count_files_with_ext(&views_dir, "js"),
        "table_count": db_text.matches("create table if not exists").count() as i64,
        "test_count": commands_text.matches("#[test]").count() as i64,
        "risk_flags": risk_flags,
        "notes": [
            "当前版本以稳定运行为优先，legacy 文件暂不继续大拆。",
            "正式推荐和冷门实验室保持隔离；hard_ban 永远最高优先级。",
            "复盘只生成review_note和observation_only候选，不自动改正式规则。",
            "v0.2-stable-personal 阶段只做小范围修 bug、测试和数据校验。"
        ]
    })
}
