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
    let commands_rs = {
        let mod_path = desktop
            .join("src-tauri")
            .join("src")
            .join("commands")
            .join("mod.rs");
        if mod_path.exists() {
            mod_path
        } else {
            desktop.join("src-tauri").join("src").join("commands.rs")
        }
    };
    let docs_arch = desktop.join("docs").join("ARCHITECTURE.md");
    let main_js_size = count_lines(&main_js);
    let commands_rs_size = count_lines(&commands_rs);
    let commands_text = read_rs_tree(&desktop.join("src-tauri").join("src").join("commands"));
    let command_count = commands_text.matches("#[tauri::command]").count() as i64;
    let test_count = commands_text.matches("#[test]").count() as i64;
    let services_dir = desktop.join("src-tauri").join("src").join("services");
    let views_dir = desktop.join("src").join("views");
    let db_text =
        fs::read_to_string(desktop.join("src-tauri").join("src").join("db.rs")).unwrap_or_default();
    let table_count = db_text.matches("create table if not exists").count() as i64;
    let mut risk_flags = Vec::new();
    if main_js_size > 800 {
        risk_flags.push("giant_frontend_file");
    }
    if commands_rs_size > 2000 {
        risk_flags.push("giant_commands_file");
    }
    if !docs_arch.exists() {
        risk_flags.push("missing_architecture_doc");
    }
    if commands_text.contains("upset_lab_candidates") && commands_text.contains("today_bet_plan") {
        risk_flags.push("upset_lab_not_fully_extracted");
    }
    risk_flags.push("snapshot_flow_needs_facade_split");
    if main_js_size > 0 && commands_rs_size > 0 {
        risk_flags.push("clean_core_refactor_in_progress");
    }
    json!({
        "main_js_size": main_js_size,
        "commands_rs_size": commands_rs_size,
        "command_count": command_count,
        "service_count": count_files_with_ext(&services_dir, "rs"),
        "view_count": count_files_with_ext(&views_dir, "js"),
        "table_count": table_count,
        "test_count": test_count,
        "current_version": "v0.2-clean-core",
        "risk_flags": risk_flags,
        "notes": [
            "正式推荐和冷门实验室通过运行guard隔离，但commands.rs仍需继续拆分。",
            "复盘只生成review_note和observation_only候选，不自动改正式规则。",
            "下一步应把main.js渲染函数迁移到views/components，把commands.rs迁移到commands/* facade。"
        ]
    })
}
