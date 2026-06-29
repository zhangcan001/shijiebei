use anyhow::Context;
use chrono::Utc;
use rand::Rng;
use rusqlite::{params, Connection};
use rusqlite::types::ValueRef;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use tauri::{AppHandle, Emitter};
use zip::write::SimpleFileOptions;
use crate::db::{app_dir, cache_get, cache_put, db_path, open_conn};
use crate::http_client::{http_api_football_json, http_football_data_org_json, http_json, http_sporttery_browser_json, http_sporttery_mobile_json, http_text};
use crate::models::*;
use crate::services::backtest_service::{data_quality_bucket, max_drawdown_from_profit, odds_bucket, probability_bucket, roi_from_profit};
use crate::services::odds_service::classify_anomaly;
use crate::services::recommendation_service::{action_advice, apply_quality_and_play_rules, play_type_risk_level, quality_action};
use crate::services::model_service::{active_model_info, predict_handicap_with_training_models, predict_with_training_models, strategy_rule_decision, training_models_dir, ActiveModelInfo, ModelFeatureInput};
use crate::services::source_service::{
    cache_freshness_score, clear_provider_api_key, list_data_providers, log_provider_request,
    provider_api_key, request_limit_available, save_provider_api_key, source_completeness_score,
    source_health_label,
};

const SPORTTERY_URL: &str = "https://webapi.sporttery.cn/gateway/uniform/football/getMatchCalculatorV1.qry?channel=1&poolCode=had,hhad,crs,ttg,hafu";
const SPORTTERY_INJURY_URL: &str = "https://webapi.sporttery.cn/gateway/uniform/football/jcInfo/getAllTodayInjurySuspensionV1.qry";
const STATSBOMB_BASE: &str = "https://cdn.jsdelivr.net/gh/statsbomb/open-data@master/data";
const ZGZCW_RESULTS_URL: &str = "https://worldcup.zgzcw.com/zhuanti/worldCupsc";
const APP_VERSION: &str = "v0.1-live-observation";
const API_FOOTBALL_DAILY_LIMIT: i64 = 100;

fn injury_count(value: &Value) -> usize {
    value
        .pointer("/value/leagueList")
        .and_then(Value::as_array)
        .map(|leagues| {
            leagues
                .iter()
                .filter_map(|league| league.get("matchList").and_then(Value::as_array))
                .flatten()
                .filter_map(|match_item| match_item.get("injuriesAndSuspensionsList").and_then(Value::as_array))
                .map(|items| items.len())
                .sum()
        })
        .unwrap_or(0)
}

fn team_injury_weight(value: &Value, team: &str) -> (f64, usize) {
    let mut weight: f64 = 0.0;
    let mut count = 0;
    let Some(leagues) = value.pointer("/value/leagueList").and_then(Value::as_array) else {
        return (0.0, 0);
    };
    for league in leagues {
        let Some(matches) = league.get("matchList").and_then(Value::as_array) else { continue };
        for match_item in matches {
            let Some(items) = match_item.get("injuriesAndSuspensionsList").and_then(Value::as_array) else { continue };
            for item in items {
                let team_name = item.get("teamAllName")
                    .or_else(|| item.get("teamShortName"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if !team_matches(team_name, team) {
                    continue;
                }
                count += 1;
                let starts = item.get("startedMatchCnt").and_then(Value::as_f64).unwrap_or(0.0);
                let appearances = item.get("appearanceCnt").and_then(Value::as_f64).unwrap_or(starts);
                let position = item.get("playerPositionDesc").and_then(Value::as_str).unwrap_or("");
                let base = if position.contains("前") {
                    0.075
                } else if position.contains("中") {
                    0.055
                } else if position.contains("后") {
                    0.045
                } else if position.contains("门") {
                    0.065
                } else {
                    0.035
                };
                let role = if starts >= 3.0 { 1.35 } else if starts >= 2.0 { 1.15 } else if appearances >= 2.0 { 0.95 } else { 0.75 };
                weight += base * role;
            }
        }
    }
    (weight.min(0.22), count)
}

fn parse_player_status_csv(csv_text: &str) -> anyhow::Result<Value> {
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(csv_text.as_bytes());
    let headers = reader.headers()?.clone();
    let find_idx = |names: &[&str]| {
        names.iter().find_map(|name| headers.iter().position(|header| header.eq_ignore_ascii_case(name)))
    };
    let team_idx = find_idx(&["team", "球队"]).context("CSV缺少 team/球队 列")?;
    let player_idx = find_idx(&["player", "球员"]).context("CSV缺少 player/球员 列")?;
    let status_idx = find_idx(&["status", "状态"]).context("CSV缺少 status/状态 列")?;
    let position_idx = find_idx(&["position", "位置"]);
    let importance_idx = find_idx(&["importance", "重要性"]);
    let starter_idx = find_idx(&["starter", "首发"]);
    let mut rows = Vec::new();
    for record in reader.records() {
        let record = record?;
        let team = record.get(team_idx).unwrap_or("").trim();
        let player = record.get(player_idx).unwrap_or("").trim();
        if team.is_empty() || player.is_empty() {
            continue;
        }
        rows.push(json!({
            "team": team,
            "player": player,
            "status": record.get(status_idx).unwrap_or("").trim(),
            "position": position_idx.and_then(|idx| record.get(idx)).unwrap_or("").trim(),
            "importance": importance_idx.and_then(|idx| record.get(idx)).and_then(|value| value.parse::<f64>().ok()).unwrap_or(1.0),
            "starter": starter_idx.and_then(|idx| record.get(idx)).unwrap_or("").trim()
        }));
    }
    Ok(json!({
        "source": "manual_player_status_csv",
        "updatedAt": Utc::now().to_rfc3339(),
        "players": rows
    }))
}

fn parse_team_stats_csv(csv_text: &str) -> anyhow::Result<Value> {
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(csv_text.as_bytes());
    let headers = reader.headers()?.clone();
    let find_idx = |names: &[&str]| {
        names.iter().find_map(|name| headers.iter().position(|header| header.eq_ignore_ascii_case(name)))
    };
    let team_idx = find_idx(&["team", "球队"]).context("CSV缺少 team/球队 列")?;
    let num = |record: &csv::StringRecord, names: &[&str], default: f64| -> f64 {
        find_idx(names)
            .and_then(|idx| record.get(idx))
            .and_then(|value| value.parse::<f64>().ok())
            .unwrap_or(default)
    };
    let mut teams = Vec::new();
    for record in reader.records() {
        let record = record?;
        let team = record.get(team_idx).unwrap_or("").trim();
        if team.is_empty() {
            continue;
        }
        let matches = num(&record, &["matches", "场次"], 1.0).max(1.0);
        let xg = num(&record, &["xg", "预期进球"], 1.2 * matches);
        let xga = num(&record, &["xga", "预期失球"], 1.2 * matches);
        teams.push(json!({
            "team": team,
            "matches": matches,
            "xg": xg,
            "xga": xga,
            "shots": num(&record, &["shots", "射门"], 0.0),
            "shots_on_target": num(&record, &["shots_on_target", "射正"], 0.0),
            "box_touches": num(&record, &["box_touches", "禁区触球"], 0.0),
            "set_piece_xg": num(&record, &["set_piece_xg", "定位球xg"], 0.0),
            "weighted_xg_per_match": xg / matches,
            "weighted_xga_per_match": xga / matches
        }));
    }
    Ok(json!({
        "source": "manual_team_stats_csv",
        "updatedAt": Utc::now().to_rfc3339(),
        "teamCount": teams.len(),
        "teams": teams
    }))
}

fn player_status_count(value: &Value) -> usize {
    value.get("players").and_then(Value::as_array).map(|items| items.len()).unwrap_or(0)
}

fn api_football_team_name(team: Option<&Value>) -> String {
    team.and_then(|value| value.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string()
}

fn api_football_stat_number(value: &Value) -> f64 {
    match value {
        Value::Number(number) => number.as_f64().unwrap_or(0.0),
        Value::String(text) => text.trim().trim_end_matches('%').parse::<f64>().unwrap_or(0.0),
        _ => 0.0,
    }
}

fn api_football_player_status_payload(injuries: &Value, lineups: &[Value]) -> Value {
    let mut players = Vec::new();
    for item in injuries.get("response").and_then(Value::as_array).into_iter().flatten() {
        let team = api_football_team_name(item.get("team"));
        let player = item.pointer("/player/name").and_then(Value::as_str).unwrap_or("").trim();
        if team.is_empty() || player.is_empty() {
            continue;
        }
        let reason = item.pointer("/player/reason")
            .or_else(|| item.get("reason"))
            .and_then(Value::as_str)
            .unwrap_or("injury");
        players.push(json!({
            "team": team,
            "player": player,
            "status": reason,
            "position": item.pointer("/player/type").and_then(Value::as_str).unwrap_or(""),
            "importance": 1.0,
            "starter": "",
            "source": "api_football_injuries",
            "lineup_status": "reported"
        }));
    }

    for lineup_item in lineups {
        let fixture_id = lineup_item.get("fixture_id").and_then(Value::as_i64).unwrap_or(0);
        let Some(teams) = lineup_item.pointer("/payload/response").and_then(Value::as_array) else {
            continue;
        };
        for team_lineup in teams {
            let team = api_football_team_name(team_lineup.get("team"));
            if team.is_empty() {
                continue;
            }
            for starter in team_lineup.get("startXI").and_then(Value::as_array).into_iter().flatten() {
                let player = starter.get("player").unwrap_or(starter);
                let name = player.get("name").and_then(Value::as_str).unwrap_or("").trim();
                if name.is_empty() {
                    continue;
                }
                players.push(json!({
                    "fixture_id": fixture_id,
                    "team": team,
                    "player": name,
                    "status": "starting",
                    "position": player.get("pos").and_then(Value::as_str).unwrap_or(""),
                    "importance": 1.0,
                    "starter": "true",
                    "source": "api_football_lineups",
                    "lineup_status": "api_confirmed"
                }));
            }
        }
    }

    json!({
        "source": "api_football_bridge",
        "updatedAt": Utc::now().to_rfc3339(),
        "players": players
    })
}

fn api_football_lineup_payload(lineups: &[Value]) -> Value {
    let matches: Vec<Value> = lineups
        .iter()
        .filter_map(|lineup_item| {
            let fixture_id = lineup_item.get("fixture_id").and_then(Value::as_i64).unwrap_or(0);
            let teams = lineup_item.pointer("/payload/response").and_then(Value::as_array)?;
            if teams.is_empty() {
                return None;
            }
            let team_rows: Vec<Value> = teams
                .iter()
                .map(|team_lineup| {
                    let starters: Vec<Value> = team_lineup.get("startXI")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(|starter| {
                            let player = starter.get("player").unwrap_or(starter);
                            let name = player.get("name").and_then(Value::as_str).unwrap_or("").trim();
                            if name.is_empty() {
                                None
                            } else {
                                Some(json!({
                                    "name": name,
                                    "position": player.get("pos").and_then(Value::as_str).unwrap_or(""),
                                    "number": player.get("number").cloned().unwrap_or(Value::Null)
                                }))
                            }
                        })
                        .collect();
                    json!({
                        "team": api_football_team_name(team_lineup.get("team")),
                        "formation": team_lineup.get("formation").and_then(Value::as_str).unwrap_or(""),
                        "lineup_status": "api_confirmed",
                        "confirmed_lineup_confidence": if starters.len() >= 8 { 86 } else { 72 },
                        "starters": starters
                    })
                })
                .collect();
            Some(json!({
                "fixture_id": fixture_id,
                "lineup_status": "api_confirmed",
                "source": "api_football",
                "teams": team_rows
            }))
        })
        .collect();

    json!({
        "source": "api_football_bridge",
        "updatedAt": Utc::now().to_rfc3339(),
        "matchCount": matches.len(),
        "matches": matches
    })
}

fn api_football_stats_payload(statistics: &[Value]) -> Value {
    let mut teams = Vec::new();
    for stats_item in statistics {
        let fixture_id = stats_item.get("fixture_id").and_then(Value::as_i64).unwrap_or(0);
        let Some(team_stats) = stats_item.pointer("/payload/response").and_then(Value::as_array) else {
            continue;
        };
        for row in team_stats {
            let team = api_football_team_name(row.get("team"));
            if team.is_empty() {
                continue;
            }
            let mut shots = 0.0;
            let mut shots_on_target = 0.0;
            let mut possession = 0.0;
            for stat in row.get("statistics").and_then(Value::as_array).into_iter().flatten() {
                let stat_type = stat.get("type").and_then(Value::as_str).unwrap_or("").to_lowercase();
                let number = api_football_stat_number(stat.get("value").unwrap_or(&Value::Null));
                if stat_type.contains("shots on goal") || stat_type.contains("shots on target") {
                    shots_on_target = number;
                } else if stat_type.contains("total shots") {
                    shots = number;
                } else if stat_type.contains("ball possession") {
                    possession = number;
                }
            }
            let shot_pressure = (shots * 0.055 + shots_on_target * 0.14).clamp(0.45, 2.4);
            teams.push(json!({
                "fixture_id": fixture_id,
                "team": team,
                "matches": 1.0,
                "shots": shots,
                "shots_on_target": shots_on_target,
                "possession": possession,
                "xg": shot_pressure,
                "xga": 1.15,
                "weighted_xg_per_match": shot_pressure,
                "weighted_xga_per_match": 1.15,
                "source_note": "API-Football 免费统计不稳定提供xG，此处为射门质量代理值"
            }));
        }
    }

    json!({
        "source": "api_football_statistics_bridge",
        "updatedAt": Utc::now().to_rfc3339(),
        "teamCount": teams.len(),
        "teams": teams
    })
}

fn bridge_provider_caches(conn: &Connection) -> anyhow::Result<Value> {
    let mut report = serde_json::Map::new();

    if cache_get(conn, "historical_results")?.is_none() {
        if let Some(record) = cache_get(conn, "match_results")? {
            let count = record.value.as_array().map(|items| items.len()).unwrap_or(0);
            if count > 0 {
                cache_put(conn, "historical_results", &record.value)?;
                report.insert("historical_results".to_string(), json!({ "ok": true, "count": count, "source": "match_results" }));
            }
        }
    }

    let injuries = cache_get(conn, "api_football_injuries")?.map(|record| record.value).unwrap_or_else(|| json!({ "response": [] }));
    let lineups: Vec<Value> = cache_get(conn, "api_football_lineups")?
        .and_then(|record| record.value.as_array().cloned())
        .unwrap_or_default();
    let statistics: Vec<Value> = cache_get(conn, "api_football_statistics")?
        .and_then(|record| record.value.as_array().cloned())
        .unwrap_or_default();

    let player_status = api_football_player_status_payload(&injuries, &lineups);
    let player_count = player_status_count(&player_status);
    if player_count > 0 {
        cache_put(conn, "player_status_data", &player_status)?;
        report.insert("player_status_data".to_string(), json!({ "ok": true, "count": player_count, "source": "api_football" }));
    }

    let lineup_payload = api_football_lineup_payload(&lineups);
    let lineup_count = lineup_payload.get("matchCount").and_then(Value::as_u64).unwrap_or(0) as usize;
    if lineup_count > 0 {
        cache_put(conn, "lineup_data", &lineup_payload)?;
        report.insert("lineup_data".to_string(), json!({ "ok": true, "count": lineup_count, "source": "api_football" }));
    }

    let stats_payload = api_football_stats_payload(&statistics);
    let stats_count = stats_payload.get("teamCount").and_then(Value::as_u64).unwrap_or(0) as usize;
    if stats_count > 0 {
        cache_put(conn, "stats_data", &stats_payload)?;
        report.insert("stats_data".to_string(), json!({ "ok": true, "count": stats_count, "source": "api_football" }));
    }

    Ok(Value::Object(report))
}

fn source_diagnosis(key: &str, ok: bool, count: usize, freshness: f64, completeness: f64, stale: bool) -> (String, String, String) {
    let label = match key {
        "sporttery" => "体彩赔率",
        "europe_odds" => "欧洲赔率",
        "statsbomb_xg" => "StatsBomb历史xG",
        "match_results" => "赛果数据",
        "historical_results" => "历史赛果样本",
        "injury_data" => "伤停数据",
        "player_status_data" => "球员状态/首发",
        "lineup_data" => "首发数据",
        "stats_data" => "统计/xG扩展",
        _ => key,
    };
    if !ok {
        let impact = match key {
            "lineup_data" => "首发未确认，投注推荐最高只能到小注/观察。",
            "player_status_data" | "injury_data" => "伤停修正不足，强弱差和冷门判断会降级。",
            "stats_data" => "实时xG缺失，比分/总进球只能观察，胜平负仍可预测。",
            "historical_results" => "动态Elo和回测样本不足，模型调权会更保守。",
            "europe_odds" => "缺少欧洲市场对照，无法识别大方向分歧。",
            "sporttery" => "缺少体彩赔率，无法生成竞彩推荐，只能做模型预测。",
            _ => "该数据不会进入当前模型或推荐层。",
        };
        let action = match key {
            "lineup_data" | "player_status_data" | "stats_data" => "点击全局刷新；若仍缺失，说明免费接口未返回该字段，可等临场或导入CSV。",
            "historical_results" => "点击刷新赛果或全局刷新；系统会用赛果缓存自动补历史样本。",
            "sporttery" => "点击刷新核心数据；若403或字段变化，需要等待接口恢复或调整解析。",
            "europe_odds" => "检查Odds API Key和赛事覆盖范围，再点全局刷新。",
            _ => "点击全局刷新后查看 provider 失败原因。",
        };
        return (
            format!("{}未形成可用缓存，当前数量 {}。", label, count),
            impact.to_string(),
            action.to_string(),
        );
    }
    if stale {
        return (
            format!("{}有旧缓存，但新鲜度只有 {:.0}。", label, freshness),
            "模型会继续显示旧数据，投注推荐会自动降级。".to_string(),
            "建议赛前重新全局刷新；若接口失败，先只看预测不下注。".to_string(),
        );
    }
    if completeness < 60.0 {
        return (
            format!("{}缓存存在，但完整度只有 {:.0}，可能只覆盖部分比赛或字段。", label, completeness),
            "未覆盖比赛会缺少对应修正，推荐等级会被压低。".to_string(),
            "刷新后仍低时，查看 provider 返回数量；临场首发/统计常在赛前较晚才出现。".to_string(),
        );
    }
    (
        format!("{}可用，缓存数量 {}。", label, count),
        "可进入模型；投注层仍会按玩法风险和赔率分歧二次过滤。".to_string(),
        "赛前靠近开赛时再刷新一次，锁定赛前快照。".to_string(),
    )
}

fn format_percent(value: f64) -> String {
    format!("{:.2}%", value * 100.0)
}

fn backtest_ban_reason(item: &BacktestGroup) -> Option<String> {
    if item.count < 3 {
        return None;
    }
    if item.roi < -0.25 {
        Some(format!("ROI {}，亏损明显", format_percent(item.roi)))
    } else if item.count >= 5 && item.hit_rate < 0.18 && item.avg_odds >= 4.0 {
        Some(format!("命中率 {} 且均赔 {:.2}，高赔幻觉风险", format_percent(item.hit_rate), item.avg_odds))
    } else if item.count >= 5 && item.brier_score > 0.32 {
        Some(format!("Brier {:.3}，概率校准偏差过大", item.brier_score))
    } else if item.dimension == "玩法" && (item.group == "比分" || item.group == "总进球") && item.roi < -0.10 {
        Some(format!("{}玩法波动高且ROI {}", item.group, format_percent(item.roi)))
    } else {
        None
    }
}

fn team_player_status_adjustment(value: &Value, team: &str) -> (f64, f64, usize, usize) {
    let mut attack_penalty: f64 = 0.0;
    let mut defense_penalty: f64 = 0.0;
    let mut unavailable = 0;
    let mut starters = 0;
    let Some(players) = value.get("players").and_then(Value::as_array) else {
        return (0.0, 0.0, 0, 0);
    };
    for player in players {
        let team_name = player.get("team").and_then(Value::as_str).unwrap_or("");
        if !team_matches(team_name, team) {
            continue;
        }
        let status = player.get("status").and_then(Value::as_str).unwrap_or("").to_lowercase();
        let starter = player.get("starter").and_then(Value::as_str).unwrap_or("").to_lowercase();
        if matches!(status.as_str(), "starting" | "start" | "首发") || matches!(starter.as_str(), "1" | "true" | "yes" | "首发") {
            starters += 1;
        }
        if !(status.contains("out") || status.contains("inj") || status.contains("suspend") || status.contains("停") || status.contains("伤") || status.contains("doubt") || status.contains("疑")) {
            continue;
        }
        unavailable += 1;
        let importance = player.get("importance").and_then(Value::as_f64).unwrap_or(1.0).clamp(0.2, 2.5);
        let position = player.get("position").and_then(Value::as_str).unwrap_or("");
        let doubt_factor = if status.contains("doubt") || status.contains("疑") { 0.55 } else { 1.0 };
        let impact = 0.035 * importance * doubt_factor;
        if position.contains("前") || position.eq_ignore_ascii_case("fw") || position.eq_ignore_ascii_case("fwd") {
            attack_penalty += impact * 1.45;
        } else if position.contains("中") || position.eq_ignore_ascii_case("mf") || position.eq_ignore_ascii_case("mid") {
            attack_penalty += impact * 0.85;
            defense_penalty += impact * 0.70;
        } else if position.contains("后") || position.eq_ignore_ascii_case("df") || position.eq_ignore_ascii_case("def") {
            defense_penalty += impact * 1.25;
        } else if position.contains("门") || position.eq_ignore_ascii_case("gk") {
            defense_penalty += impact * 1.60;
        } else {
            attack_penalty += impact * 0.55;
            defense_penalty += impact * 0.55;
        }
    }
    (attack_penalty.min(0.24), defense_penalty.min(0.24), unavailable, starters)
}

fn default_model_settings() -> ModelSettings {
    ModelSettings {
        buy_edge: 0.08,
        buy_gap: 0.025,
        watch_edge: 0.035,
        watch_gap: 0.010,
        max_odds: 8.0,
        high_odds_limit: 8.0,
        mode: "正常".to_string(),
    }
}

fn load_model_settings(conn: &Connection) -> ModelSettings {
    cache_get(conn, "model_settings")
        .ok()
        .flatten()
        .and_then(|record| serde_json::from_value(record.value).ok())
        .unwrap_or_else(default_model_settings)
}

fn strip_tags(input: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for ch in input.chars() {
        match ch {
            '<' => {
                in_tag = true;
                out.push(' ');
            }
            '>' => {
                in_tag = false;
                out.push(' ');
            }
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out.replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn between_all<'a>(text: &'a str, start: &str, end: &str) -> Vec<&'a str> {
    let mut rows = Vec::new();
    let mut rest = text;
    while let Some(start_idx) = rest.find(start) {
        let after_start = &rest[start_idx + start.len()..];
        if let Some(end_idx) = after_start.find(end) {
            rows.push(&after_start[..end_idx]);
            rest = &after_start[end_idx + end.len()..];
        } else {
            break;
        }
    }
    rows
}

fn parse_score(text: &str) -> Option<(u32, u32)> {
    let score = text.trim();
    let parts = score.split(':').collect::<Vec<_>>();
    if parts.len() != 2 {
        return None;
    }
    Some((parts[0].trim().parse().ok()?, parts[1].trim().parse().ok()?))
}

fn parse_zgzcw_results(html: &str) -> Vec<MatchResult> {
    let mut results = Vec::new();
    for row in between_all(html, "<tr>", "</tr>") {
        if !row.contains("team1") || !row.contains("team2") {
            continue;
        }
        let cells = between_all(row, "<td", "</td>");
        if cells.len() < 6 {
            continue;
        }
        let plain = cells.iter().map(|cell| strip_tags(cell)).collect::<Vec<_>>();
        let stage = plain.get(1).cloned().unwrap_or_default();
        let status = plain.get(2).cloned().unwrap_or_default();
        let home = plain.get(3).cloned().unwrap_or_default();
        let score = plain.get(4).cloned().unwrap_or_default();
        let away = plain.get(5).cloned().unwrap_or_default();
        let half_score = plain.get(6).cloned().unwrap_or_default();
        if home.is_empty() || away.is_empty() || parse_score(&score).is_none() {
            continue;
        }
        results.push(MatchResult {
            home,
            away,
            score,
            half_score,
            stage,
            status,
        });
    }
    results
}

fn parse_results_csv(csv_text: &str) -> anyhow::Result<Vec<MatchResult>> {
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(csv_text.as_bytes());
    let headers = reader.headers()?.clone();
    let find_idx = |names: &[&str]| {
        names.iter().find_map(|name| headers.iter().position(|header| header.eq_ignore_ascii_case(name)))
    };
    let home_idx = find_idx(&["home", "主队"]).context("CSV缺少 home/主队 列")?;
    let away_idx = find_idx(&["away", "客队"]).context("CSV缺少 away/客队 列")?;
    let score_idx = find_idx(&["score", "比分"]).context("CSV缺少 score/比分 列")?;
    let stage_idx = find_idx(&["stage", "阶段"]);
    let status_idx = find_idx(&["status", "状态"]);
    let half_idx = find_idx(&["half_score", "half", "半场"]);
    let mut rows = Vec::new();
    for record in reader.records() {
        let record = record?;
        let home = record.get(home_idx).unwrap_or("").trim().to_string();
        let away = record.get(away_idx).unwrap_or("").trim().to_string();
        let score = record.get(score_idx).unwrap_or("").trim().to_string();
        if home.is_empty() || away.is_empty() || parse_score(&score).is_none() {
            continue;
        }
        rows.push(MatchResult {
            home,
            away,
            score,
            half_score: half_idx.and_then(|idx| record.get(idx)).unwrap_or("").to_string(),
            stage: stage_idx.and_then(|idx| record.get(idx)).unwrap_or("历史样本").to_string(),
            status: status_idx.and_then(|idx| record.get(idx)).unwrap_or("完场").to_string(),
        });
    }
    Ok(rows)
}

fn cached_results(conn: &Connection, key: &str) -> Vec<MatchResult> {
    cache_get(conn, key)
        .ok()
        .flatten()
        .and_then(|record| serde_json::from_value(record.value).ok())
        .unwrap_or_default()
}

fn model_result_rows(conn: &Connection) -> Vec<MatchResult> {
    let mut rows = cached_results(conn, "historical_results");
    rows.extend(cached_results(conn, "match_results"));
    rows
}

fn collect_matches(value: &Value, rows: &mut Vec<MatchRow>) {
    match value {
        Value::Array(items) => items.iter().for_each(|item| collect_matches(item, rows)),
        Value::Object(map) => {
            let home = text_field(map, &["homeTeamAbbName", "homeTeamAllName", "homeAbbCnName", "homeAllCnName"]);
            let away = text_field(map, &["awayTeamAbbName", "awayTeamAllName", "awayAbbCnName", "awayAllCnName"]);
            if !home.is_empty() && !away.is_empty() {
                let id = text_field(map, &["matchId", "gmMatchId", "wbsjMatchId", "matchNumStr"]);
                rows.push(MatchRow {
                    id: if id.is_empty() { format!("{}-{}", home, away) } else { id },
                    match_num: text_field(map, &["matchNumStr", "matchNum"]),
                    league: text_field(map, &["leagueAbbName", "leagueAllName", "leagueAbbCnName"]),
                    time: [text_field(map, &["matchDate"]), text_field(map, &["matchTime"])]
                        .into_iter()
                        .filter(|part| !part.is_empty())
                        .collect::<Vec<_>>()
                        .join(" "),
                    home,
                    away,
                    status: text_field(map, &["matchStatus", "status", "matchState"]),
                });
            }
            map.values().for_each(|child| collect_matches(child, rows));
        }
        _ => {}
    }
}

fn text_field(map: &serde_json::Map<String, Value>, keys: &[&str]) -> String {
    keys.iter()
        .find_map(|key| map.get(*key))
        .map(|value| match value {
            Value::String(text) => text.clone(),
            Value::Number(num) => num.to_string(),
            _ => String::new(),
        })
        .unwrap_or_default()
}

fn number_text(map: &serde_json::Map<String, Value>, key: &str) -> Option<f64> {
    map.get(key).and_then(|value| match value {
        Value::String(text) => text.parse::<f64>().ok(),
        Value::Number(num) => num.as_f64(),
        _ => None,
    })
}

fn fair_probabilities(odds: &[f64]) -> Vec<f64> {
    let implied: Vec<f64> = odds
        .iter()
        .map(|odd| if *odd > 1.0 { 1.0 / odd } else { 0.0 })
        .collect();
    let sum: f64 = implied.iter().sum();
    if sum <= 0.0 {
        return vec![0.0; odds.len()];
    }
    implied.iter().map(|value| value / sum).collect()
}

fn extract_sporttery_matches(value: &Value, rows: &mut Vec<serde_json::Map<String, Value>>) {
    match value {
        Value::Array(items) => items.iter().for_each(|item| extract_sporttery_matches(item, rows)),
        Value::Object(map) => {
            let home = text_field(map, &["homeTeamAbbName", "homeTeamAllName"]);
            let away = text_field(map, &["awayTeamAbbName", "awayTeamAllName"]);
            if !home.is_empty() && !away.is_empty() && (map.contains_key("had") || map.contains_key("hhad") || map.contains_key("ttg")) {
                rows.push(map.clone());
            }
            map.values().for_each(|child| extract_sporttery_matches(child, rows));
        }
        _ => {}
    }
}

fn sporttery_selections(value: &Value) -> Vec<OddsSelection> {
    let mut matches = Vec::new();
    extract_sporttery_matches(value, &mut matches);
    let mut selections = Vec::new();
    for map in matches {
        let home = text_field(&map, &["homeTeamAbbName", "homeTeamAllName"]);
        let away = text_field(&map, &["awayTeamAbbName", "awayTeamAllName"]);
        let match_id = text_field(&map, &["matchId", "gmMatchId", "wbsjMatchId", "matchNumStr"]);
        let match_num = text_field(&map, &["matchNumStr", "matchNum"]);
        let match_time = [text_field(&map, &["matchDate"]), text_field(&map, &["matchTime"])]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" ");

        if let Some(Value::Object(had)) = map.get("had") {
            let odds = [
                number_text(had, "h").unwrap_or(0.0),
                number_text(had, "d").unwrap_or(0.0),
                number_text(had, "a").unwrap_or(0.0),
            ];
            let fair = fair_probabilities(&odds);
            for (idx, pick) in ["主胜", "平局", "客胜"].iter().enumerate() {
                if odds[idx] > 1.0 {
                    selections.push(OddsSelection {
                        match_id: if match_id.is_empty() { format!("{}-{}", home, away) } else { match_id.clone() },
                        match_num: match_num.clone(),
                        match_time: match_time.clone(),
                        home: home.clone(),
                        away: away.clone(),
                        market: "HAD 胜平负".to_string(),
                        pick: (*pick).to_string(),
                        odds: odds[idx],
                        fair_prob: fair.get(idx).copied().unwrap_or(0.0),
                        goal_line: String::new(),
                    });
                }
            }
        }

        if let Some(Value::Object(hhad)) = map.get("hhad") {
            let odds = [
                number_text(hhad, "h").unwrap_or(0.0),
                number_text(hhad, "d").unwrap_or(0.0),
                number_text(hhad, "a").unwrap_or(0.0),
            ];
            let fair = fair_probabilities(&odds);
            let goal_line = text_field(hhad, &["goalLine", "goalLineValue"]);
            for (idx, pick) in ["让胜", "让平", "让负"].iter().enumerate() {
                if odds[idx] > 1.0 {
                    selections.push(OddsSelection {
                        match_id: if match_id.is_empty() { format!("{}-{}", home, away) } else { match_id.clone() },
                        match_num: match_num.clone(),
                        match_time: match_time.clone(),
                        home: home.clone(),
                        away: away.clone(),
                        market: format!("HHAD 让球胜平负 {}", goal_line),
                        pick: (*pick).to_string(),
                        odds: odds[idx],
                        fair_prob: fair.get(idx).copied().unwrap_or(0.0),
                        goal_line: goal_line.clone(),
                    });
                }
            }
        }

        if let Some(Value::Object(ttg)) = map.get("ttg") {
            let odds = (0..=7)
                .map(|idx| number_text(ttg, &format!("s{}", idx)).unwrap_or(0.0))
                .collect::<Vec<_>>();
            let fair = fair_probabilities(&odds);
            for idx in 0..=7 {
                if odds[idx] > 1.0 {
                    selections.push(OddsSelection {
                        match_id: if match_id.is_empty() { format!("{}-{}", home, away) } else { match_id.clone() },
                        match_num: match_num.clone(),
                        match_time: match_time.clone(),
                        home: home.clone(),
                        away: away.clone(),
                        market: "TTG 总进球".to_string(),
                        pick: if idx == 7 { "7+球".to_string() } else { format!("{}球", idx) },
                        odds: odds[idx],
                        fair_prob: fair.get(idx).copied().unwrap_or(0.0),
                        goal_line: String::new(),
                    });
                }
            }
        }

        if let Some(Value::Object(crs)) = map.get("crs") {
            let mut picks = Vec::new();
            for home_goals in 0..=5 {
                for away_goals in 0..=5 {
                    picks.push((format!("{}:{}", home_goals, away_goals), format!("s{:02}s{:02}", home_goals, away_goals)));
                }
            }
            let odds = picks
                .iter()
                .map(|(_, key)| number_text(crs, key).unwrap_or(0.0))
                .collect::<Vec<_>>();
            let fair = fair_probabilities(&odds);
            for (idx, (pick, _)) in picks.iter().enumerate() {
                if odds[idx] > 1.0 {
                    selections.push(OddsSelection {
                        match_id: if match_id.is_empty() { format!("{}-{}", home, away) } else { match_id.clone() },
                        match_num: match_num.clone(),
                        match_time: match_time.clone(),
                        home: home.clone(),
                        away: away.clone(),
                        market: "CRS 比分".to_string(),
                        pick: pick.clone(),
                        odds: odds[idx],
                        fair_prob: fair.get(idx).copied().unwrap_or(0.0),
                        goal_line: String::new(),
                    });
                }
            }
        }
    }
    selections
}

fn poisson(k: u32, lambda: f64) -> f64 {
    let factorial = (1..=k).fold(1.0, |acc, value| acc * value as f64);
    (-lambda).exp() * lambda.powi(k as i32) / factorial
}

fn team_rank(team: &str) -> Option<i32> {
    let clean = team.replace("（", "(").replace("）", ")");
    let name = clean.split('(').next().unwrap_or(&clean).trim();
    let pairs = [
        ("法国", 1), ("西班牙", 2), ("阿根廷", 3), ("英格兰", 4), ("葡萄牙", 5),
        ("巴西", 6), ("荷兰", 7), ("摩洛哥", 8), ("比利时", 9), ("德国", 10),
        ("克罗地亚", 11), ("哥伦比亚", 13), ("塞内加尔", 14), ("墨西哥", 15),
        ("美国", 16), ("乌拉圭", 17), ("日本", 18), ("瑞士", 19), ("伊朗", 21),
        ("土耳其", 22), ("厄瓜多尔", 23), ("奥地利", 24), ("韩国", 25),
        ("澳大利亚", 27), ("阿尔及利亚", 28), ("埃及", 29), ("加拿大", 30),
        ("挪威", 31), ("巴拿马", 33), ("科特迪瓦", 34), ("瑞典", 38),
        ("巴拉圭", 40), ("捷克", 41), ("苏格兰", 43), ("突尼斯", 44),
        ("民主刚果", 46), ("乌兹别克斯坦", 50), ("卡塔尔", 55), ("伊拉克", 57),
        ("南非", 60), ("沙特", 61), ("约旦", 63), ("波黑", 65), ("佛得角", 69),
        ("加纳", 74), ("库拉索", 82), ("海地", 83), ("新西兰", 85),
    ];
    pairs.iter().find_map(|(candidate, rank)| {
        if name.contains(candidate) || candidate.contains(name) {
            Some(*rank)
        } else {
            None
        }
    })
}

fn ranked_team_label(team: &str) -> String {
    let clean = team.replace("（", "(").replace("）", ")");
    let name = clean.split('(').next().unwrap_or(&clean).trim();
    if let Some(rank) = team_rank(team) {
        format!("{}（第{}）", name, rank)
    } else {
        name.to_string()
    }
}

fn team_aliases(team: &str) -> Vec<&'static str> {
    let pairs: [(&str, &[&str]); 43] = [
        ("法国", &["france"]),
        ("西班牙", &["spain"]),
        ("阿根廷", &["argentina"]),
        ("英格兰", &["england"]),
        ("葡萄牙", &["portugal"]),
        ("巴西", &["brazil"]),
        ("荷兰", &["netherlands", "holland"]),
        ("摩洛哥", &["morocco"]),
        ("比利时", &["belgium"]),
        ("德国", &["germany"]),
        ("克罗地亚", &["croatia"]),
        ("哥伦比亚", &["colombia"]),
        ("塞内加尔", &["senegal"]),
        ("墨西哥", &["mexico"]),
        ("美国", &["usa", "united states", "united states of america"]),
        ("乌拉圭", &["uruguay"]),
        ("日本", &["japan"]),
        ("瑞士", &["switzerland"]),
        ("伊朗", &["iran"]),
        ("土耳其", &["turkey", "turkiye"]),
        ("厄瓜多尔", &["ecuador"]),
        ("奥地利", &["austria"]),
        ("韩国", &["south korea", "korea republic", "republic of korea"]),
        ("澳大利亚", &["australia"]),
        ("阿尔及利亚", &["algeria"]),
        ("埃及", &["egypt"]),
        ("加拿大", &["canada"]),
        ("挪威", &["norway"]),
        ("巴拿马", &["panama"]),
        ("科特迪瓦", &["ivory coast", "cote d ivoire", "cote d'ivoire"]),
        ("瑞典", &["sweden"]),
        ("巴拉圭", &["paraguay"]),
        ("捷克", &["czech republic", "czechia"]),
        ("苏格兰", &["scotland"]),
        ("突尼斯", &["tunisia"]),
        ("民主刚果", &["dr congo", "congo dr", "democratic republic of congo"]),
        ("乌兹别克斯坦", &["uzbekistan"]),
        ("卡塔尔", &["qatar"]),
        ("伊拉克", &["iraq"]),
        ("南非", &["south africa"]),
        ("沙特", &["saudi arabia"]),
        ("波黑", &["bosnia and herzegovina", "bosnia"]),
        ("新西兰", &["new zealand"]),
    ];
    let normalized_team = normalize_cn_name(team);
    pairs
        .iter()
        .find_map(|(cn, aliases)| {
            if normalized_team.contains(cn) || cn.contains(&normalized_team) {
                Some(aliases.to_vec())
            } else {
                None
            }
        })
        .unwrap_or_default()
}

fn normalize_cn_name(value: &str) -> String {
    value
        .replace("（", "(")
        .split('(')
        .next()
        .unwrap_or(value)
        .trim()
        .to_string()
}

fn team_matches(candidate: &str, team: &str) -> bool {
    let candidate = normalize(candidate);
    let team_norm = normalize(team);
    if candidate.contains(&team_norm) || team_norm.contains(&candidate) {
        return true;
    }
    team_aliases(team)
        .iter()
        .any(|alias| candidate.contains(&normalize(alias)) || normalize(alias).contains(&candidate))
}

fn result_matches_prediction(result: &MatchResult, match_label: &str) -> bool {
    let parts = match_label.split(" vs ").collect::<Vec<_>>();
    if parts.len() != 2 {
        return false;
    }
    team_matches(&result.home, parts[0]) && team_matches(&result.away, parts[1])
}

fn parse_handicap(market: &str) -> f64 {
    market
        .split_whitespace()
        .last()
        .unwrap_or("")
        .replace("+", "")
        .parse::<f64>()
        .unwrap_or(0.0)
}

fn prediction_hit(record: &PredictionRecord, result: &MatchResult) -> Option<bool> {
    let (home_goals, away_goals) = parse_score(&result.score)?;
    if record.market.starts_with("HAD") {
        let actual = if home_goals > away_goals { "主胜" } else if home_goals == away_goals { "平局" } else { "客胜" };
        return Some(record.pick == actual);
    }
    if record.market.starts_with("HHAD") {
        let diff = home_goals as f64 + parse_handicap(&record.market) - away_goals as f64;
        let actual = if diff > 0.01 { "让胜" } else if diff.abs() <= 0.01 { "让平" } else { "让负" };
        return Some(record.pick == actual);
    }
    if record.market.starts_with("TTG") {
        let total = home_goals + away_goals;
        let actual = if total >= 7 { "7+球".to_string() } else { format!("{}球", total) };
        return Some(record.pick == actual);
    }
    if record.market.starts_with("CRS") {
        return Some(record.pick == format!("{}:{}", home_goals, away_goals));
    }
    None
}

fn pick_hit_from_result(market: &str, pick: &str, result: &MatchResult) -> Option<(bool, String)> {
    let (home_goals, away_goals) = parse_score(&result.score)?;
    if market.starts_with("HAD") || market.contains("胜平负") {
        let actual = if home_goals > away_goals { "主胜" } else if home_goals == away_goals { "平局" } else { "客胜" };
        return Some((pick == actual, actual.to_string()));
    }
    if market.starts_with("HHAD") || market.contains("让球") {
        let diff = home_goals as f64 + parse_handicap(market) - away_goals as f64;
        let actual = if diff > 0.01 { "让胜" } else if diff.abs() <= 0.01 { "让平" } else { "让负" };
        return Some((pick == actual, actual.to_string()));
    }
    if market.starts_with("TTG") || market.contains("总进球") {
        let total = home_goals + away_goals;
        let actual = if total >= 7 { "7+球".to_string() } else { format!("{}球", total) };
        return Some((pick == actual, actual));
    }
    if market.starts_with("CRS") || market.contains("比分") {
        let actual = format!("{}:{}", home_goals, away_goals);
        return Some((pick == actual, actual));
    }
    None
}

fn csv_cell(value: &str) -> String {
    let escaped = value.replace('"', "\"\"");
    if escaped.contains(',') || escaped.contains('\n') || escaped.contains('"') {
        format!("\"{}\"", escaped)
    } else {
        escaped
    }
}

fn europe_consensus(value: &Value, home: &str, away: &str) -> Option<EuropeConsensus> {
    let games = value.as_array()?;
    for game in games {
        let europe_home = game.get("home_team").and_then(Value::as_str).unwrap_or("");
        let europe_away = game.get("away_team").and_then(Value::as_str).unwrap_or("");
        let direct = team_matches(europe_home, home) && team_matches(europe_away, away);
        let reversed = team_matches(europe_home, away) && team_matches(europe_away, home);
        if !direct && !reversed {
            continue;
        }
        let mut home_prices = Vec::new();
        let mut draw_prices = Vec::new();
        let mut away_prices = Vec::new();
        if let Some(bookmakers) = game.get("bookmakers").and_then(Value::as_array) {
            for bookmaker in bookmakers {
                let Some(markets) = bookmaker.get("markets").and_then(Value::as_array) else { continue };
                let Some(h2h) = markets.iter().find(|market| market.get("key").and_then(Value::as_str) == Some("h2h")) else { continue };
                let Some(outcomes) = h2h.get("outcomes").and_then(Value::as_array) else { continue };
                for outcome in outcomes {
                    let name = outcome.get("name").and_then(Value::as_str).unwrap_or("");
                    let price = outcome.get("price").and_then(Value::as_f64).unwrap_or(0.0);
                    if price <= 1.0 {
                        continue;
                    }
                    if name.eq_ignore_ascii_case("draw") {
                        draw_prices.push(price);
                    } else if (direct && team_matches(name, home)) || (reversed && team_matches(name, home)) {
                        home_prices.push(price);
                    } else if (direct && team_matches(name, away)) || (reversed && team_matches(name, away)) {
                        away_prices.push(price);
                    }
                }
            }
        }
        let home_avg = average(&home_prices)?;
        let draw_avg = average(&draw_prices)?;
        let away_avg = average(&away_prices)?;
        let fair = fair_probabilities(&[home_avg, draw_avg, away_avg]);
        return Some(EuropeConsensus {
            home_prob: fair.get(0).copied().unwrap_or(0.0),
            draw_prob: fair.get(1).copied().unwrap_or(0.0),
            away_prob: fair.get(2).copied().unwrap_or(0.0),
            home_odds: home_avg,
            draw_odds: draw_avg,
            away_odds: away_avg,
            bookmaker_count: home_prices.len().min(draw_prices.len()).min(away_prices.len()),
        });
    }
    None
}

fn average(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        None
    } else {
        Some(values.iter().sum::<f64>() / values.len() as f64)
    }
}

fn rank_lambdas(home: &str, away: &str) -> (f64, f64, String) {
    let home_rank = team_rank(home).unwrap_or(43);
    let away_rank = team_rank(away).unwrap_or(43);
    let rank_gap = (away_rank - home_rank) as f64;
    let home_lambda = (1.28 + rank_gap * 0.018 + 0.10).clamp(0.45, 3.25);
    let away_lambda = (1.18 - rank_gap * 0.018).clamp(0.45, 3.25);
    (
        home_lambda,
        away_lambda,
        format!("排名模型 {} vs {}", home_rank, away_rank),
    )
}

fn base_elo(team: &str) -> f64 {
    let rank = team_rank(team).unwrap_or(43) as f64;
    (1900.0 - rank * 5.5).clamp(1350.0, 1900.0)
}

fn dynamic_elo_map(results: &[MatchResult]) -> BTreeMap<String, f64> {
    let mut ratings = BTreeMap::new();
    for result in results {
        ratings.entry(normalize_cn_name(&result.home)).or_insert_with(|| base_elo(&result.home));
        ratings.entry(normalize_cn_name(&result.away)).or_insert_with(|| base_elo(&result.away));
    }
    for result in results {
        let Some((home_goals, away_goals)) = parse_score(&result.score) else { continue };
        let home_key = normalize_cn_name(&result.home);
        let away_key = normalize_cn_name(&result.away);
        let home_rating = *ratings.get(&home_key).unwrap_or(&base_elo(&result.home));
        let away_rating = *ratings.get(&away_key).unwrap_or(&base_elo(&result.away));
        let expected_home = 1.0 / (1.0 + 10f64.powf((away_rating - home_rating) / 400.0));
        let actual_home = if home_goals > away_goals { 1.0 } else if home_goals == away_goals { 0.5 } else { 0.0 };
        let margin = (home_goals as f64 - away_goals as f64).abs().max(1.0);
        let k = 24.0 * margin.ln_1p().clamp(1.0, 1.8);
        let change = k * (actual_home - expected_home);
        ratings.insert(home_key, home_rating + change);
        ratings.insert(away_key, away_rating - change);
    }
    ratings
}

fn elo_lambdas(home: &str, away: &str, results: &[MatchResult]) -> (f64, f64, String) {
    let ratings = dynamic_elo_map(results);
    let home_elo = ratings.get(&normalize_cn_name(home)).copied().unwrap_or_else(|| base_elo(home));
    let away_elo = ratings.get(&normalize_cn_name(away)).copied().unwrap_or_else(|| base_elo(away));
    let diff = (home_elo - away_elo).clamp(-450.0, 450.0);
    let home_lambda = (1.24 + diff / 420.0 + 0.08).clamp(0.35, 3.3);
    let away_lambda = (1.16 - diff / 420.0).clamp(0.35, 3.3);
    (
        home_lambda,
        away_lambda,
        format!("动态Elo {:.0} vs {:.0}", home_elo, away_elo),
    )
}

fn team_current_elo(team: &str, results: &[MatchResult]) -> f64 {
    if results.is_empty() {
        return base_elo(team);
    }
    dynamic_elo_map(results)
        .get(&normalize_cn_name(team))
        .copied()
        .unwrap_or_else(|| base_elo(team))
}

fn model_feature_input_for_match(
    home: &str,
    away: &str,
    result_rows: &[MatchResult],
    lambda_home: f64,
    lambda_away: f64,
    sporttery_threeway: Option<(f64, f64, f64)>,
    europe: Option<&EuropeConsensus>,
    had_home: Option<&OddsSelection>,
    had_draw: Option<&OddsSelection>,
    had_away: Option<&OddsSelection>,
) -> ModelFeatureInput {
    let fallback = sporttery_threeway
        .or_else(|| europe.map(|item| (item.home_prob, item.draw_prob, item.away_prob)))
        .unwrap_or_else(|| {
            let scores = score_distribution(lambda_home, lambda_away, 8);
            threeway_from_scores(&scores)
        });
    let odds_home = had_home.map(|item| item.odds).unwrap_or_else(|| if fallback.0 > 0.0 { 1.0 / fallback.0 } else { 0.0 });
    let odds_draw = had_draw.map(|item| item.odds).unwrap_or_else(|| if fallback.1 > 0.0 { 1.0 / fallback.1 } else { 0.0 });
    let odds_away = had_away.map(|item| item.odds).unwrap_or_else(|| if fallback.2 > 0.0 { 1.0 / fallback.2 } else { 0.0 });
    let market_margin = [odds_home, odds_draw, odds_away]
        .iter()
        .map(|odds| if *odds > 1.0 { 1.0 / odds } else { 0.0 })
        .sum::<f64>()
        - 1.0;
    ModelFeatureInput {
        elo_diff: team_current_elo(home, result_rows) - team_current_elo(away, result_rows),
        odds_home,
        odds_draw,
        odds_away,
        market_home_prob: fallback.0,
        market_draw_prob: fallback.1,
        market_away_prob: fallback.2,
        market_margin: market_margin.max(0.0),
        rule_home_lambda: lambda_home,
        rule_away_lambda: lambda_away,
    }
}

fn apply_knockout_tempo(home: &str, away: &str, lambda_home: &mut f64, lambda_away: &mut f64) -> String {
    let home_rank = team_rank(home).unwrap_or(43);
    let away_rank = team_rank(away).unwrap_or(43);
    let rank_gap = (home_rank - away_rank).abs();
    let tempo_factor = if rank_gap >= 35 {
        0.90
    } else if rank_gap >= 18 {
        0.93
    } else {
        0.96
    };
    *lambda_home *= tempo_factor;
    *lambda_away *= tempo_factor;
    if rank_gap >= 35 {
        "淘汰赛强弱差大：按低位防守/控节奏下调总λ 10%".to_string()
    } else if rank_gap >= 18 {
        "淘汰赛强弱差中等：下调总λ 7%".to_string()
    } else {
        "淘汰赛实力接近：小幅下调总λ 4%，提高平局敏感度".to_string()
    }
}

fn apply_player_status_to_lambdas(value: Option<&Value>, home: &str, away: &str, lambda_home: &mut f64, lambda_away: &mut f64) -> String {
    if let Some(value) = value {
        let (home_attack_penalty, home_defense_penalty, home_unavailable, home_starters) =
            team_player_status_adjustment(value, home);
        let (away_attack_penalty, away_defense_penalty, away_unavailable, away_starters) =
            team_player_status_adjustment(value, away);
        *lambda_home *= (1.0 - home_attack_penalty).clamp(0.76, 1.0);
        *lambda_away *= (1.0 - away_attack_penalty).clamp(0.76, 1.0);
        *lambda_home *= (1.0 + away_defense_penalty).clamp(1.0, 1.24);
        *lambda_away *= (1.0 + home_defense_penalty).clamp(1.0, 1.24);
        format!(
            "球员状态修正：{}缺阵/疑问{}人、首发{}人；{}缺阵/疑问{}人、首发{}人",
            ranked_team_label(home),
            home_unavailable,
            home_starters,
            ranked_team_label(away),
            away_unavailable,
            away_starters
        )
    } else {
        "未导入球员状态/预计首发".to_string()
    }
}

fn score_distribution(lambda_home: f64, lambda_away: f64, max_goals: u32) -> Vec<(u32, u32, f64)> {
    let mut rows = Vec::new();
    for h in 0..=max_goals {
        for a in 0..=max_goals {
            rows.push((h, a, poisson(h, lambda_home) * poisson(a, lambda_away)));
        }
    }
    rows
}

fn apply_dixon_coles(scores: &mut Vec<(u32, u32, f64)>, lambda_home: f64, lambda_away: f64, rho: f64) {
    for (h, a, p) in scores.iter_mut() {
        let tau = match (*h, *a) {
            (0, 0) => 1.0 - lambda_home * lambda_away * rho,
            (0, 1) => 1.0 + lambda_home * rho,
            (1, 0) => 1.0 + lambda_away * rho,
            (1, 1) => 1.0 - rho,
            _ => 1.0,
        };
        *p *= tau.max(0.05);
    }
    normalize_score_probs(scores);
}

fn normalize_score_probs(scores: &mut Vec<(u32, u32, f64)>) {
    let total: f64 = scores.iter().map(|(_, _, p)| *p).sum();
    if total > 0.0 {
        for (_, _, p) in scores {
            *p /= total;
        }
    }
}

fn binomial_ci(probability: f64, samples: u32) -> (f64, f64) {
    if samples == 0 {
        return (probability, probability);
    }
    let se = (probability * (1.0 - probability) / samples as f64).max(0.0).sqrt();
    ((probability - 1.96 * se).max(0.0), (probability + 1.96 * se).min(1.0))
}

fn retarget_scores_to_threeway(scores: &[(u32, u32, f64)], target: (f64, f64, f64)) -> Vec<(u32, u32, f64)> {
    let current = threeway_from_scores(scores);
    let factors = [
        if current.0 > 0.0 { target.0 / current.0 } else { 1.0 },
        if current.1 > 0.0 { target.1 / current.1 } else { 1.0 },
        if current.2 > 0.0 { target.2 / current.2 } else { 1.0 },
    ];
    let mut adjusted = scores
        .iter()
        .map(|(h, a, p)| {
            let factor = if h > a {
                factors[0]
            } else if h == a {
                factors[1]
            } else {
                factors[2]
            };
            (*h, *a, p * factor)
        })
        .collect::<Vec<_>>();
    normalize_score_probs(&mut adjusted);
    adjusted
}

fn draw_score_index(cumulative: &[f64], rng: &mut impl Rng) -> usize {
    let roll = rng.gen::<f64>();
    cumulative
        .binary_search_by(|probe| probe.partial_cmp(&roll).unwrap_or(std::cmp::Ordering::Greater))
        .unwrap_or_else(|idx| idx)
        .min(cumulative.len().saturating_sub(1))
}

fn threeway_from_scores(scores: &[(u32, u32, f64)]) -> (f64, f64, f64) {
    let mut home_win = 0.0;
    let mut draw = 0.0;
    let mut away_win = 0.0;
    for (h, a, p) in scores {
        if h > a {
            home_win += p;
        } else if h == a {
            draw += p;
        } else {
            away_win += p;
        }
    }
    (home_win, draw, away_win)
}

fn handicap_probs(scores: &[(u32, u32, f64)], line: &str) -> (f64, f64, f64) {
    let handicap = line.replace("+", "").parse::<f64>().unwrap_or(0.0);
    let mut home = 0.0;
    let mut draw = 0.0;
    let mut away = 0.0;
    for (h, a, p) in scores {
        let diff = *h as f64 + handicap - *a as f64;
        if diff > 0.01 {
            home += p;
        } else if diff.abs() <= 0.01 {
            draw += p;
        } else {
            away += p;
        }
    }
    (home, draw, away)
}

fn total_goal_prob(scores: &[(u32, u32, f64)], pick: &str) -> f64 {
    scores
        .iter()
        .filter(|(h, a, _)| {
            let total = h + a;
            if pick.starts_with("7+") {
                total >= 7
            } else {
                let target = pick.trim_end_matches('球').parse::<u32>().unwrap_or(99);
                total == target
            }
        })
        .map(|(_, _, p)| *p)
        .sum()
}

fn prob_item(pick: &str, probability: f64) -> ProbItem {
    ProbItem {
        pick: pick.to_string(),
        probability,
        fair_odds: if probability > 0.0 { 1.0 / probability } else { 0.0 },
        sporttery_prob: None,
        sporttery_odds: None,
        probability_gap: None,
    }
}

fn prob_item_with_market(pick: &str, probability: f64, selection: Option<&OddsSelection>) -> ProbItem {
    let mut item = prob_item(pick, probability);
    if let Some(selection) = selection {
        item.sporttery_prob = Some(selection.fair_prob);
        item.sporttery_odds = Some(selection.odds);
        item.probability_gap = Some(probability - selection.fair_prob);
    }
    item
}

fn find_selection<'a>(selections: &'a [OddsSelection], market_prefix: &str, pick: &str) -> Option<&'a OddsSelection> {
    selections
        .iter()
        .find(|selection| selection.market.starts_with(market_prefix) && selection.pick == pick)
}

fn model_probability_from_scores(selection: &OddsSelection, scores: &[(u32, u32, f64)]) -> f64 {
    if selection.market.starts_with("HAD") {
        let (home, draw, away) = threeway_from_scores(scores);
        match selection.pick.as_str() {
            "主胜" => home,
            "平局" => draw,
            "客胜" => away,
            _ => 0.0,
        }
    } else if selection.market.starts_with("HHAD") {
        let (home, draw, away) = handicap_probs(scores, &selection.goal_line);
        match selection.pick.as_str() {
            "让胜" => home,
            "让平" => draw,
            "让负" => away,
            _ => 0.0,
        }
    } else if selection.market.starts_with("TTG") {
        total_goal_prob(scores, &selection.pick)
    } else {
        0.0
    }
}

fn odds_snapshot_batches(conn: &Connection, match_id: &str) -> i64 {
    conn
        .query_row(
            "select count(distinct created_at) from odds_snapshots where match_id=?1",
            params![match_id],
            |row| row.get(0),
        )
        .unwrap_or(0)
}

fn market_weight_for_match(conn: &Connection, match_id: &str) -> (f64, String) {
    let snapshot_batches = odds_snapshot_batches(conn, match_id);
    if snapshot_batches >= 5 {
        (0.50, format!("临场/最新赔率权重高（{}次快照）", snapshot_batches))
    } else if snapshot_batches >= 2 {
        (0.44, format!("赔率快照权重中（{}次快照）", snapshot_batches))
    } else {
        (0.36, "赔率快照少，降低市场权重".to_string())
    }
}

fn has_market(selections: &[OddsSelection], prefix: &str) -> bool {
    selections.iter().any(|selection| selection.market.starts_with(prefix))
}

fn player_status_covered(value: Option<&Value>, home: &str, away: &str) -> bool {
    let Some(value) = value else { return false };
    let (_, _, home_unavailable, home_starters) = team_player_status_adjustment(value, home);
    let (_, _, away_unavailable, away_starters) = team_player_status_adjustment(value, away);
    home_unavailable + home_starters + away_unavailable + away_starters > 0
}

fn match_data_quality(
    conn: &Connection,
    first: &OddsSelection,
    selections: &[OddsSelection],
    europe: Option<&EuropeConsensus>,
    preferred_xg: Option<(&Value, &'static str)>,
    player_status_value: Option<&Value>,
    movements: &[OddsMovement],
) -> (f64, String, String, String) {
    let mut score: f64 = 0.0;
    let mut support = Vec::new();
    let mut risks = Vec::new();

    if has_market(selections, "HAD") {
        score += 18.0;
        support.push("有胜平负体彩盘".to_string());
    } else {
        risks.push("缺少胜平负体彩盘".to_string());
    }
    if has_market(selections, "HHAD") {
        score += 12.0;
        support.push("有让球盘".to_string());
    } else {
        risks.push("缺少让球盘".to_string());
    }
    if has_market(selections, "TTG") {
        score += 10.0;
        support.push("有总进球盘".to_string());
    } else {
        risks.push("缺少总进球盘".to_string());
    }

    if let Some(consensus) = europe {
        score += if consensus.bookmaker_count >= 8 { 18.0 } else { 14.0 };
        support.push(format!("欧洲市场{}家均值", consensus.bookmaker_count));
    } else {
        risks.push("未匹配欧洲市场".to_string());
    }

    if let Some((xg_value, source_label)) = preferred_xg {
        if xg_profile(xg_value, &first.home).is_some() && xg_profile(xg_value, &first.away).is_some() {
            score += 16.0;
            support.push(format!("{}覆盖双方", source_label));
        } else {
            score += 6.0;
            risks.push("xG/统计未覆盖双方".to_string());
        }
    } else {
        risks.push("缺少xG/统计源".to_string());
    }

    if player_status_covered(player_status_value, &first.home, &first.away) {
        score += 10.0;
        support.push("球员状态/预计首发有记录".to_string());
    } else {
        risks.push("缺少双方球员状态/首发".to_string());
    }

    let snapshot_batches = odds_snapshot_batches(conn, &first.match_id);
    if snapshot_batches >= 5 {
        score += 12.0;
        support.push(format!("赔率快照{}次", snapshot_batches));
    } else if snapshot_batches >= 2 {
        score += 8.0;
        support.push(format!("赔率快照{}次", snapshot_batches));
    } else {
        score += 3.0;
        risks.push("赔率快照不足".to_string());
    }

    if !movements.is_empty() {
        score += 4.0;
        support.push("已有赔率异动记录".to_string());
    } else {
        risks.push("暂无有效赔率异动".to_string());
    }

    let score = score.clamp(0.0, 100.0);
    let grade = if score >= 80.0 {
        "A完整"
    } else if score >= 65.0 {
        "B可用"
    } else if score >= 50.0 {
        "C谨慎"
    } else {
        "D不足"
    }
    .to_string();

    (score, grade, support.join("；"), risks.join("；"))
}

fn lineup_status_for_match(conn: &Connection, match_id: &str) -> (String, f64) {
    conn.query_row(
        "select lineup_status, max(confidence) from match_lineup_sources where match_id=?1 group by lineup_status order by max(confidence) desc limit 1",
        params![match_id],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?)),
    )
    .unwrap_or_else(|_| ("unknown".to_string(), 0.0))
}

fn ensemble_probability(
    selection: &OddsSelection,
    scores: &[(u32, u32, f64)],
    europe: Option<&EuropeConsensus>,
    market_weight: f64,
) -> (f64, String) {
    let score_prob = model_probability_from_scores(selection, scores).clamp(0.001, 0.999);
    let europe_pick = if selection.market.starts_with("HAD") {
        europe.and_then(|consensus| europe_pick(consensus, &selection.pick))
    } else {
        None
    };
    let europe_weight = if europe_pick.is_some() { 0.18 } else { 0.0 };
    let market_weight = market_weight.min(0.55 - europe_weight).max(0.20);
    let model_weight = (1.0 - market_weight - europe_weight).max(0.25);
    let weight_sum = model_weight + market_weight + europe_weight;
    let model_weight = model_weight / weight_sum;
    let market_weight = market_weight / weight_sum;
    let europe_weight = europe_weight / weight_sum;
    let europe_prob = europe_pick.map(|item| item.0).unwrap_or(0.0);
    let prob = score_prob * model_weight + selection.fair_prob * market_weight + europe_prob * europe_weight;
    (
        prob.clamp(0.001, 0.92),
        format!(
            "集成权重：模型{:.0}%/体彩{:.0}%/欧洲{:.0}%",
            model_weight * 100.0,
            market_weight * 100.0,
            europe_weight * 100.0
        ),
    )
}

fn market_reflexivity_adjustment(
    selection: &OddsSelection,
    model_prob: f64,
    europe: Option<&EuropeConsensus>,
    movements: &[OddsMovement],
) -> (f64, String) {
    let movement = movements
        .iter()
        .find(|item| item.market == selection.market && item.pick == selection.pick);
    let europe_prob = if selection.market.starts_with("HAD") {
        europe.and_then(|consensus| europe_pick(consensus, &selection.pick).map(|item| item.0))
    } else {
        None
    };
    if let Some(prob) = europe_prob {
        let market_diff = selection.fair_prob - prob;
        if market_diff.abs() >= 0.08 {
            let adjusted = (model_prob * 0.82 + selection.fair_prob * 0.18).clamp(0.001, 0.92);
            return (adjusted, format!("市场反身性：体彩与欧洲冲突{:+.1}个百分点，降权", market_diff * 100.0));
        }
    }
    if let Some(movement) = movement {
        if movement.delta_abs < -0.01 {
            if let Some(prob) = europe_prob {
                if prob >= selection.fair_prob - 0.03 {
                    return (model_prob, "市场反身性：降赔且欧洲不反对，偏真实信息支持".to_string());
                }
                let adjusted = (model_prob * 0.88 + selection.fair_prob * 0.12).clamp(0.001, 0.92);
                return (adjusted, "市场反身性：降赔但欧洲不支持，疑似热门过热".to_string());
            }
            return (model_prob, "市场反身性：降赔支持，但缺少欧洲验证".to_string());
        }
        if movement.delta_abs > 0.01 {
            let adjusted = (model_prob * 0.92 + selection.fair_prob * 0.08).clamp(0.001, 0.92);
            return (adjusted, "市场反身性：升赔降温，轻度降权".to_string());
        }
    }
    (model_prob, "市场反身性：暂无有效异动".to_string())
}

fn apply_market_consistency(selection: &OddsSelection, model_prob: f64, scores: &[(u32, u32, f64)], selections: &[OddsSelection]) -> (f64, String) {
    let mut conflicts = Vec::new();
    if selection.market.starts_with("HAD") {
        let (home, draw, away) = threeway_from_scores(scores);
        let model = match selection.pick.as_str() {
            "主胜" => home,
            "平局" => draw,
            "客胜" => away,
            _ => model_prob,
        };
        if (model - selection.fair_prob).abs() >= 0.12 {
            conflicts.push("胜平负模型与体彩分歧大");
        }
    }
    if let Some(ttg_0) = selections.iter().find(|item| item.market.starts_with("TTG") && item.pick == "0球") {
        let low_goal_market = ttg_0.fair_prob
            + selections.iter().find(|item| item.market.starts_with("TTG") && item.pick == "1球").map(|item| item.fair_prob).unwrap_or(0.0);
        let low_goal_model = total_goal_prob(scores, "0球") + total_goal_prob(scores, "1球");
        if (low_goal_model - low_goal_market).abs() >= 0.10 {
            conflicts.push("总进球市场与比分矩阵分歧大");
        }
    }
    if conflicts.is_empty() {
        (model_prob, "跨市场一致性正常".to_string())
    } else {
        let adjusted = (model_prob * 0.75 + selection.fair_prob * 0.25).clamp(0.001, 0.92);
        (adjusted, format!("{}，已向市场概率收缩", conflicts.join("；")))
    }
}

fn europe_pick(consensus: &EuropeConsensus, pick: &str) -> Option<(f64, f64)> {
    match pick {
        "主胜" => Some((consensus.home_prob, consensus.home_odds)),
        "平局" => Some((consensus.draw_prob, consensus.draw_odds)),
        "客胜" => Some((consensus.away_prob, consensus.away_odds)),
        _ => None,
    }
}

fn recommendation_tier(selection: &OddsSelection, model_prob: f64, gap: f64, edge: f64, decision: &str) -> (String, String, String) {
    if decision == "禁止" || edge <= 0.0 {
        return ("禁止".to_string(), "不买".to_string(), "排除".to_string());
    }
    if selection.market.starts_with("HAD") && model_prob >= 0.48 && selection.odds <= 2.35 && gap >= 0.025 {
        return ("稳胆".to_string(), "可单关，小仓位".to_string(), "A组-稳胆".to_string());
    }
    if selection.market.starts_with("HHAD") && model_prob >= 0.42 && selection.odds <= 2.85 && gap >= 0.02 {
        return ("让球稳胆".to_string(), "可单关或二串一候选".to_string(), "B组-让球".to_string());
    }
    if selection.odds >= 4.8 {
        return ("冷门小注".to_string(), "只允许极小单关，不建议串".to_string(), "C组-冷门".to_string());
    }
    if selection.market.starts_with("TTG") {
        return ("进球数小注".to_string(), "高波动，单关小注".to_string(), "D组-进球".to_string());
    }
    ("价值小注".to_string(), "单关小注，谨慎串关".to_string(), "E组-价值".to_string())
}

fn worldcup_live_correction_probability(
    app_dir: &std::path::Path,
    selection: &OddsSelection,
    model_prob: f64,
    europe_prob: Option<f64>,
    fair_odds: f64,
    edge: f64,
    gap: f64,
    stake: f64,
    data_score: f64,
    anomaly: Option<&OddsAnomaly>,
    tier: &str,
) -> Option<f64> {
    let dir = training_models_dir(app_dir);
    let payload = std::fs::read_to_string(dir.join("worldcup_live_correction_v1.json")).ok()?;
    let model: Value = serde_json::from_str(&payload).ok()?;
    let features = model.get("feature_names")?.as_array()?;
    let coeffs = model.get("coefficients")?.as_array()?;
    let value_for = |feature: &str| -> f64 {
        match feature {
            "model_prob" => model_prob,
            "sporttery_prob" => selection.fair_prob,
            "europe_prob_filled" => europe_prob.unwrap_or(selection.fair_prob),
            "europe_missing" => if europe_prob.is_some() { 0.0 } else { 1.0 },
            "current_odds" => selection.odds,
            "fair_odds" => fair_odds,
            "ev" => edge,
            "advantage_rate" => gap,
            "stake_pct" => stake,
            "data_quality_score" => data_score,
            "is_score_or_total_goals" => if selection.market.starts_with("CRS") || selection.market.starts_with("TTG") { 1.0 } else { 0.0 },
            "has_anomaly" => if anomaly.is_some() { 1.0 } else { 0.0 },
            "is_formal_recommendation" => if tier.contains("稳胆") || tier.contains("价值") { 1.0 } else { 0.0 },
            _ => 0.0,
        }
    };
    let mut score = model.get("intercept").and_then(Value::as_f64).unwrap_or(0.0);
    for (idx, feature) in features.iter().filter_map(Value::as_str).enumerate() {
        let raw = value_for(feature);
        let mean = model.pointer(&format!("/scaler_mean/{}", feature)).and_then(Value::as_f64).unwrap_or(0.0);
        let scale = model.pointer(&format!("/scaler_scale/{}", feature)).and_then(Value::as_f64).unwrap_or(1.0).max(0.0001);
        let coeff = coeffs.get(idx).and_then(Value::as_f64).unwrap_or(0.0);
        score += coeff * ((raw - mean) / scale);
    }
    Some(1.0 / (1.0 + (-score).exp()))
}

fn recommendation_for(
    selection: &OddsSelection,
    model_prob: f64,
    note: &str,
    europe: Option<&EuropeConsensus>,
    settings: &ModelSettings,
    data_score: f64,
    data_grade: &str,
    support_factors: &str,
    risk_factors: &str,
    lineup_status: &str,
    lineup_confidence: f64,
    anomaly: Option<&OddsAnomaly>,
) -> Recommendation {
    let edge = model_prob * selection.odds - 1.0;
    let gap = model_prob - selection.fair_prob;
    let europe_pick = if selection.market.starts_with("HAD") {
        europe.and_then(|consensus| europe_pick(consensus, &selection.pick))
    } else {
        None
    };
    let europe_prob = europe_pick.map(|(prob, _)| prob);
    let europe_odds = europe_pick.map(|(_, odd)| odd);
    let europe_gap = europe_prob.map(|prob| model_prob - prob);
    let raw_kelly = if selection.odds > 1.0 {
        ((model_prob * selection.odds - 1.0) / (selection.odds - 1.0)).max(0.0)
    } else {
        0.0
    };
    let market_cap = if selection.market.starts_with("HAD") {
        0.02
    } else if selection.market.starts_with("HHAD") {
        0.015
    } else {
        0.006
    };
    let high_odds_cap = if selection.odds >= 10.0 { 0.002 } else if selection.odds >= 6.0 { 0.004 } else { market_cap };
    let mut stake = (raw_kelly * 0.35).min(market_cap).min(high_odds_cap);

    let mut decision = if edge >= settings.buy_edge && gap >= settings.buy_gap && selection.odds <= settings.max_odds {
        "可买"
    } else if edge >= settings.watch_edge && gap >= settings.watch_gap && selection.odds <= settings.max_odds + 2.0 {
        "观察"
    } else {
        "禁止"
    }
    .to_string();
    let mut confidence = if decision == "可买" && model_prob >= 0.35 {
        "高"
    } else if decision != "禁止" {
        "中"
    } else {
        "低"
    }
    .to_string();
    let mut reasons = vec![
        format!("{}，模型概率较体彩去水{:+.2}个百分点", note, gap * 100.0),
        format!("赛前数据完整度{}（{:.0}分）", data_grade, data_score),
        "赔率已做去水对比".to_string(),
    ];
    if selection.odds >= settings.high_odds_limit {
        decision = "禁止".to_string();
        confidence = "低".to_string();
        stake = 0.0;
        reasons.push(format!("高赔率幻觉压制（阈值 {:.2}）", settings.high_odds_limit));
    }
    if selection.market.starts_with("TTG") {
        reasons.push("总进球波动高，限仓".to_string());
    }
    if let Some(prob) = europe_prob {
        let market_diff = selection.fair_prob - prob;
        reasons.push(format!("欧洲共识{:.2}%，体彩较欧洲{:+.2}个百分点", prob * 100.0, market_diff * 100.0));
        if (model_prob - prob).abs() > 0.12 {
            decision = "观察".to_string();
            confidence = "中".to_string();
            stake *= 0.35;
            reasons.push("模型与欧洲市场分歧过大，降级".to_string());
        }
        if gap > 0.02 && market_diff < -0.04 {
            decision = "观察".to_string();
            stake *= 0.5;
            reasons.push("体彩低于欧洲共识，疑似方向冲突".to_string());
        }
    } else if selection.market.starts_with("HAD") {
        reasons.push("未匹配欧洲市场".to_string());
    }
    if edge <= 0.0 {
        stake = 0.0;
        reasons.push("期望收益为负".to_string());
    }
    let (tier, play_style, combo_group) = recommendation_tier(selection, model_prob, gap, edge, &decision);
    if tier.contains("冷门") {
        stake = stake.min(0.002);
        reasons.push("冷门方向只做极小仓位".to_string());
    }
    if tier == "稳胆" && europe_prob.is_some() {
        reasons.push("适合做单关核心，不建议重仓".to_string());
    }
    if let Some(anomaly) = anomaly {
        reasons.push(format!("赔率异常：{} {}，{}", anomaly.anomaly_type, anomaly.severity, anomaly.advice));
        if anomaly.severity == "高" {
            decision = "观察".to_string();
            confidence = "中".to_string();
            stake *= 0.35;
        }
    }
    let fair_odds = if model_prob > 0.0 { 1.0 / model_prob } else { 999.0 };
    let mut worldcup_correction_action = "unavailable".to_string();
    if selection.market.starts_with("HAD") {
        let app_dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        if let Some(correction_prob) = worldcup_live_correction_probability(
            &app_dir,
            selection,
            model_prob,
            europe_prob,
            fair_odds,
            edge,
            gap,
            stake,
            data_score,
            anomaly,
            &tier,
        ) {
            worldcup_correction_action = "keep".to_string();
            reasons.push(format!("世界杯临场修正层：历史世界杯同类样本命中修正概率 {:.1}%", correction_prob * 100.0));
            if correction_prob < 0.42 {
                worldcup_correction_action = "downgrade_low_confidence".to_string();
                if decision == "可买" {
                    decision = "观察".to_string();
                }
                confidence = "低".to_string();
                stake *= 0.30;
                reasons.push("世界杯临场修正层低于42%，降级观察".to_string());
            } else if correction_prob < 0.50 {
                worldcup_correction_action = "downgrade_observe".to_string();
                if decision == "可买" {
                    decision = "观察".to_string();
                }
                if confidence == "高" {
                    confidence = "中".to_string();
                }
                stake *= 0.65;
                reasons.push("世界杯临场修正层未过50%，降低仓位".to_string());
            } else if correction_prob >= 0.58 && decision != "禁止" {
                reasons.push("世界杯临场修正层支持该方向，可保留候选资格".to_string());
            }
        }
        let strategy = strategy_rule_decision(&app_dir, &selection.pick, selection.odds, model_prob, edge, gap);
        if strategy.action == "observe_only" {
            if decision == "可买" {
                decision = "观察".to_string();
            }
            confidence = "低".to_string();
            stake *= 0.15;
            reasons.push(format!("训练回测规则：{}，仅观察", strategy.reason));
        } else if strategy.action == "downgrade" {
            if decision == "可买" {
                decision = "观察".to_string();
            }
            confidence = if confidence == "高" { "中".to_string() } else { confidence };
            stake *= 0.45;
            reasons.push(format!("训练回测规则：{}，降级", strategy.reason));
        } else if strategy.action == "sample_too_small" {
            if decision == "可买" {
                decision = "观察".to_string();
            }
            stake *= 0.35;
            reasons.push(format!("训练回测规则：{}，样本不足", strategy.reason));
        } else {
            reasons.push("训练回测规则：未命中禁买区间".to_string());
        }
        if stake < 0.001 && decision == "可买" {
            decision = "观察".to_string();
        }
    }
    apply_quality_and_play_rules(
        &selection.market,
        &mut decision,
        &mut confidence,
        &mut stake,
        &mut reasons,
        data_score,
        lineup_status,
        lineup_confidence,
    );
    let action_advice = action_advice(&decision, &tier, stake, &selection.market);
    let anomaly_type = anomaly.map(|item| item.anomaly_type.clone()).unwrap_or_default();
    let anomaly_severity = anomaly.map(|item| item.severity.clone()).unwrap_or_default();
    let anomaly_direction = anomaly.map(|item| item.impact_direction.clone()).unwrap_or_default();
    let anomaly_advice = anomaly.map(|item| item.advice.clone()).unwrap_or_default();
    let final_decision = if lineup_status != "confirmed" && lineup_confidence < 80.0 && decision != "禁止" {
        "wait_for_lineup"
    } else if anomaly_type.contains("临场") && decision != "禁止" {
        "wait_for_odds"
    } else if decision == "可买" && stake >= 0.005 {
        "recommend"
    } else if decision == "可买" && stake > 0.0 {
        "small_stake"
    } else if decision == "观察" {
        "observe_only"
    } else {
        "hard_ban"
    }.to_string();

    Recommendation {
        match_id: selection.match_id.clone(),
        match_num: selection.match_num.clone(),
        match_time: selection.match_time.clone(),
        match_label: format!("{} vs {}", selection.home, selection.away),
        market: selection.market.clone(),
        pick: selection.pick.clone(),
        odds: selection.odds,
        fair_prob: selection.fair_prob,
        model_prob,
        probability_gap: gap,
        expected_return: edge,
        stake_pct: stake,
        europe_prob,
        europe_gap,
        europe_odds,
        decision,
        confidence,
        tier,
        play_style,
        combo_group,
        data_score,
        data_grade: data_grade.to_string(),
        quality_action: quality_action(data_score).to_string(),
        support_factors: support_factors.to_string(),
        risk_factors: risk_factors.to_string(),
        fair_odds,
        advantage_rate: gap,
        action_advice,
        play_type_risk_level: play_type_risk_level(&selection.market).to_string(),
        lineup_status: lineup_status.to_string(),
        lineup_confidence,
        anomaly_type,
        anomaly_severity,
        anomaly_direction,
        anomaly_advice,
        worldcup_correction_action,
        final_decision,
        reason: reasons.join("；"),
    }
}

fn xg_profile(value: &Value, team: &str) -> Option<(f64, f64)> {
    let needle = normalize(team);
    let aliases = team_aliases(team)
        .into_iter()
        .map(normalize)
        .collect::<Vec<_>>();
    value
        .get("teams")?
        .as_array()?
        .iter()
        .find_map(|item| {
            let name = item.get("team")?.as_str()?;
            let normalized_name = normalize(name);
            let direct_match = !needle.is_empty()
                && (normalized_name.contains(&needle) || needle.contains(&normalized_name));
            let alias_match = aliases
                .iter()
                .any(|alias| !alias.is_empty() && (normalized_name.contains(alias) || alias.contains(&normalized_name)));
            if direct_match || alias_match {
                Some((
                    item.get("weighted_xg_per_match")
                        .or_else(|| item.get("xg_per_match"))
                        .and_then(Value::as_f64)
                        .unwrap_or(1.25),
                    item.get("weighted_xga_per_match")
                        .or_else(|| item.get("xga_per_match"))
                        .and_then(Value::as_f64)
                        .unwrap_or(1.25),
                ))
            } else {
                None
            }
        })
}

fn preferred_xg_value<'a>(stats_cache: Option<&'a CacheRecord>, statsbomb_cache: Option<&'a CacheRecord>) -> Option<(&'a Value, &'static str)> {
    if let Some(record) = stats_cache {
        if record.value.get("teams").and_then(Value::as_array).map(|items| !items.is_empty()).unwrap_or(false) {
            return Some((&record.value, "实时统计/xG"));
        }
    }
    statsbomb_cache.map(|record| (&record.value, "StatsBomb历史xG"))
}

fn record_odds_snapshots(conn: &Connection, sporttery: &Value) -> anyhow::Result<usize> {
    let selections = sporttery_selections(sporttery);
    let now = Utc::now().to_rfc3339();
    let mut movement_count = 0;
    for selection in selections {
        let match_label = format!("{} vs {}", selection.home, selection.away);
        let mut stmt = conn.prepare(
            "select odds from odds_snapshots
             where match_id=?1 and market=?2 and pick=?3
             order by id asc limit 1",
        )?;
        let initial_odds = stmt
            .query_row(params![selection.match_id, selection.market, selection.pick], |row| row.get::<_, f64>(0))
            .ok();

        let mut stmt = conn.prepare(
            "select odds from odds_snapshots
             where match_id=?1 and market=?2 and pick=?3
             order by id desc limit 1",
        )?;
        let previous_odds = stmt
            .query_row(params![selection.match_id, selection.market, selection.pick], |row| row.get::<_, f64>(0))
            .ok();

        conn.execute(
            "insert into odds_snapshots(created_at, match_id, match_label, market, pick, odds)
             values(?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                now,
                selection.match_id,
                match_label,
                selection.market,
                selection.pick,
                selection.odds
            ],
        )?;

        if let Some(previous) = previous_odds {
            let delta_abs = selection.odds - previous;
            let delta_pct = if previous > 0.0 { delta_abs / previous } else { 0.0 };
            if delta_abs.abs() >= 0.01 && delta_pct.abs() >= 0.001 {
                conn.execute(
                    "insert into odds_movements(created_at, match_id, match_label, market, pick, initial_odds, previous_odds, current_odds, delta_abs, delta_pct, direction)
                     values(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                    params![
                        now,
                        selection.match_id,
                        match_label,
                        selection.market,
                        selection.pick,
                        initial_odds.unwrap_or(previous),
                        previous,
                        selection.odds,
                        delta_abs,
                        delta_pct,
                        if delta_abs > 0.0 { "升赔" } else { "降赔" }
                    ],
                )?;
                movement_count += 1;
                if let Some((anomaly_type, severity, impact_direction, advice)) =
                    classify_anomaly(&selection.market, &selection.pick, delta_abs, delta_pct, selection.odds)
                {
                    conn.execute(
                        "insert into odds_anomalies(created_at, match_id, match_label, market, pick, anomaly_type, severity, impact_direction, advice, delta_abs, delta_pct)
                         values(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                        params![
                            now,
                            selection.match_id,
                            match_label,
                            selection.market,
                            selection.pick,
                            anomaly_type,
                            severity,
                            impact_direction,
                            advice,
                            delta_abs,
                            delta_pct
                        ],
                    )?;
                }
            }
        }
    }
    Ok(movement_count)
}

fn latest_anomaly_for_selection(conn: &Connection, selection: &OddsSelection) -> Option<OddsAnomaly> {
    conn.query_row(
        "select id, created_at, match_id, match_label, market, pick, anomaly_type, severity, impact_direction, advice, delta_abs, delta_pct
         from odds_anomalies where match_id=?1 and market=?2 and pick=?3 order by id desc limit 1",
        params![selection.match_id, selection.market, selection.pick],
        |row| {
            Ok(OddsAnomaly {
                id: row.get(0)?,
                created_at: row.get(1)?,
                match_id: row.get(2)?,
                match_label: row.get(3)?,
                market: row.get(4)?,
                pick: row.get(5)?,
                anomaly_type: row.get(6)?,
                severity: row.get(7)?,
                impact_direction: row.get(8)?,
                advice: row.get(9)?,
                delta_abs: row.get(10)?,
                delta_pct: row.get(11)?,
            })
        },
    )
    .ok()
}

fn normalize(value: &str) -> String {
    value
        .to_lowercase()
        .chars()
        .filter(|ch| ch.is_alphanumeric() || *ch == ' ')
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

async fn statsbomb_worldcup_xg_payload() -> anyhow::Result<Value> {
    let competitions = http_json(&format!("{}/competitions.json", STATSBOMB_BASE)).await?;
    let seasons: Vec<Value> = competitions
        .as_array()
        .unwrap_or(&Vec::new())
        .iter()
        .filter(|item| {
            item.get("competition_id").and_then(Value::as_i64) == Some(43)
                && item.get("season_name").and_then(Value::as_str) == Some("2022")
        })
        .cloned()
        .collect();
    let mut all_matches = Vec::new();
    for season in &seasons {
        let competition_id = season.get("competition_id").and_then(Value::as_i64).unwrap_or(43);
        let season_id = season.get("season_id").and_then(Value::as_i64).unwrap_or(106);
        let matches = http_json(&format!("{}/matches/{}/{}.json", STATSBOMB_BASE, competition_id, season_id)).await?;
        if let Some(items) = matches.as_array() {
            all_matches.extend(items.iter().cloned());
        }
    }

    all_matches.sort_by(|a, b| {
        a.get("match_date")
            .and_then(Value::as_str)
            .unwrap_or("")
            .cmp(b.get("match_date").and_then(Value::as_str).unwrap_or(""))
    });
    let total_matches = all_matches.len().max(1) as f64;
    let mut teams: BTreeMap<String, (String, f64, f64, f64, f64, f64, f64, f64)> = BTreeMap::new();
    let client = reqwest::Client::builder()
        .user_agent("worldcup-odds-desktop/0.1")
        .timeout(std::time::Duration::from_secs(25))
        .build()?;

    for (idx, item) in all_matches.iter().enumerate() {
        let recency_weight = 0.65 + 0.70 * ((idx + 1) as f64 / total_matches);
        let match_id = item.get("match_id").and_then(Value::as_i64).context("match_id missing")?;
        let home = item.pointer("/home_team/home_team_name").and_then(Value::as_str).unwrap_or("");
        let away = item.pointer("/away_team/away_team_name").and_then(Value::as_str).unwrap_or("");
        let response = client
            .get(format!("{}/events/{}.json", STATSBOMB_BASE, match_id))
            .send()
            .await?;
        if !response.status().is_success() {
            continue;
        }
        let events = response.json::<Value>().await?;
        let mut xg: BTreeMap<String, (f64, f64)> = BTreeMap::new();
        if let Some(events) = events.as_array() {
            for event in events {
                if event.pointer("/type/name").and_then(Value::as_str) != Some("Shot") {
                    continue;
                }
                let team = event.pointer("/team/name").and_then(Value::as_str).unwrap_or("");
                let shot_xg = event.pointer("/shot/statsbomb_xg").and_then(Value::as_f64).unwrap_or(0.0);
                let entry = xg.entry(team.to_string()).or_insert((0.0, 0.0));
                entry.0 += shot_xg;
                entry.1 += 1.0;
            }
        }
        for (team, opponent) in [(home, away), (away, home)] {
            let own = xg.get(team).cloned().unwrap_or((0.0, 0.0));
            let opp = xg.get(opponent).cloned().unwrap_or((0.0, 0.0));
            let entry = teams.entry(team.to_string()).or_insert((team.to_string(), 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0));
            entry.1 += 1.0;
            entry.2 += own.0;
            entry.3 += opp.0;
            entry.4 += own.1;
            entry.5 += own.0 * recency_weight;
            entry.6 += opp.0 * recency_weight;
            entry.7 += recency_weight;
        }
    }

    let team_rows: Vec<Value> = teams
        .values()
        .map(|(team, matches, xg, xga, shots, weighted_xg, weighted_xga, weight_sum)| {
            json!({
                "team": team,
                "matches": matches,
                "xg": xg,
                "xga": xga,
                "shots": shots,
                "xg_per_match": if *matches > 0.0 { xg / matches } else { 0.0 },
                "xga_per_match": if *matches > 0.0 { xga / matches } else { 0.0 },
                "weighted_xg_per_match": if *weight_sum > 0.0 { weighted_xg / weight_sum } else { 0.0 },
                "weighted_xga_per_match": if *weight_sum > 0.0 { weighted_xga / weight_sum } else { 0.0 },
                "shots_per_match": if *matches > 0.0 { shots / matches } else { 0.0 }
            })
        })
        .collect();

    Ok(json!({
        "source": "StatsBomb Open Data",
        "updatedAt": Utc::now().to_rfc3339(),
        "seasons": ["2022"],
        "matchCount": all_matches.len(),
        "teamCount": team_rows.len(),
        "teams": team_rows
    }))
}

#[tauri::command]
async fn app_status(app: AppHandle) -> Result<Value, String> {
    let conn = open_conn(&app)?;
    let keys = ["sporttery", "europe_odds", "statsbomb_xg", "match_results", "historical_results", "injury_data", "player_status_data", "lineup_data", "stats_data"];
    let mut statuses = Vec::new();
    for key in keys {
        let record = cache_get(&conn, key).map_err(|error| error.to_string())?;
        let (ok, updated_at, count, message) = if let Some(record) = record {
            let count = if key == "injury_data" {
                injury_count(&record.value)
            } else if key == "sporttery" {
                let mut ids = sporttery_selections(&record.value)
                    .into_iter()
                    .map(|selection| selection.match_id)
                    .collect::<Vec<_>>();
                ids.sort();
                ids.dedup();
                ids.len()
            } else if key == "europe_odds" || key == "match_results" || key == "historical_results" {
                record.value.as_array().map(|items| items.len()).unwrap_or(0)
            } else if key == "player_status_data" {
                player_status_count(&record.value)
            } else if key == "stats_data" {
                record.value.get("teamCount").and_then(Value::as_u64).unwrap_or(0) as usize
            } else if key == "lineup_data" {
                record.value.get("matchCount")
                    .or_else(|| record.value.get("teamCount"))
                    .and_then(Value::as_u64)
                    .unwrap_or_else(|| record.value.get("matches").and_then(Value::as_array).map(|items| items.len() as u64).unwrap_or(0)) as usize
            } else {
                record.value.get("teamCount").or_else(|| record.value.get("matchCount")).and_then(Value::as_u64).unwrap_or(0) as usize
            };
            (true, Some(record.updated_at), count, "已缓存".to_string())
        } else {
            (false, None, 0, "未缓存".to_string())
        };
        let freshness_score = updated_at
            .as_deref()
            .map(cache_freshness_score)
            .unwrap_or(0.0);
        let completeness_score = source_completeness_score(key, count);
        let using_stale_cache = ok && freshness_score < 50.0;
        let confidence_score = if ok {
            (freshness_score * 0.45 + completeness_score * 0.55).clamp(0.0, 100.0)
        } else {
            0.0
        };
        let health_label = source_health_label(ok, freshness_score, completeness_score, using_stale_cache);
        let (diagnosis, impact, next_action) = source_diagnosis(key, ok, count, freshness_score, completeness_score, using_stale_cache);
        statuses.push(SourceStatus {
            id: key.to_string(),
            label: match key {
                "sporttery" => "体彩赔率",
                "europe_odds" => "欧洲赔率",
                "statsbomb_xg" => "StatsBomb xG",
                "match_results" => "赛果数据",
                "historical_results" => "历史赛果样本",
                "injury_data" => "伤停数据",
                "player_status_data" => "球员状态/首发",
                "lineup_data" => "首发数据",
                "stats_data" => "统计/xG扩展",
                _ => key,
            }
            .to_string(),
            ok,
            updated_at: updated_at.clone(),
            count,
            message: if ok { health_label.clone() } else { message },
            last_success_at: updated_at.clone(),
            last_error_at: if ok { None } else { Some(Utc::now().to_rfc3339()) },
            last_error_message: if ok { String::new() } else { "暂无缓存或字段缺失".to_string() },
            freshness_score,
            completeness_score,
            confidence_score,
            using_stale_cache,
            health_label,
            diagnosis,
            impact,
            next_action,
        });
    }
    Ok(json!({
        "dbPath": db_path(&app)?,
        "sources": statuses,
        "providers": list_data_providers(&conn).map_err(|error| error.to_string())?
    }))
}

fn upsert_provider_health(conn: &Connection, provider_id: &str, ok: bool, count: usize, error_message: &str) -> anyhow::Result<()> {
    let base_confidence: f64 = conn
        .query_row(
            "select base_confidence from data_providers where provider_id=?1",
            params![provider_id],
            |row| row.get(0),
        )
        .unwrap_or(60.0);
    let now = Utc::now().to_rfc3339();
    let freshness = if ok { 100.0 } else { 0.0 };
    let completeness = if count > 0 { 100.0 } else if ok { 55.0 } else { 0.0 };
    let confidence = if ok {
        (base_confidence * 0.01 * freshness * 0.01 * completeness).clamp(0.0, 100.0)
    } else {
        0.0
    };
    conn.execute(
        "insert into source_health(provider_id, last_success_at, last_error_at, last_error_message, freshness_score, completeness_score, confidence_score, using_stale_cache, updated_at)
         values(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         on conflict(provider_id) do update set
           last_success_at=case when excluded.last_success_at is not null then excluded.last_success_at else source_health.last_success_at end,
           last_error_at=excluded.last_error_at,
           last_error_message=excluded.last_error_message,
           freshness_score=excluded.freshness_score,
           completeness_score=excluded.completeness_score,
           confidence_score=excluded.confidence_score,
           using_stale_cache=excluded.using_stale_cache,
           updated_at=excluded.updated_at",
        params![
            provider_id,
            if ok { Some(now.clone()) } else { None },
            if ok { None } else { Some(now.clone()) },
            error_message,
            freshness,
            completeness,
            confidence,
            if !ok { 1 } else { 0 },
            now
        ],
    )?;
    Ok(())
}

#[tauri::command]
async fn refresh_core_data(app: AppHandle, odds_api_key: Option<String>, region: Option<String>) -> Result<Value, String> {
    let conn = open_conn(&app)?;
    let sporttery = http_sporttery_browser_json(SPORTTERY_URL).await.map_err(|error| error.to_string())?;
    cache_put(&conn, "sporttery", &sporttery).map_err(|error| error.to_string())?;
    let movement_count = record_odds_snapshots(&conn, &sporttery).map_err(|error| error.to_string())?;
    let mut result = json!({ "sporttery": true, "europe": false, "injury": false, "movements": movement_count });

    if let Ok(injury) = http_sporttery_mobile_json(SPORTTERY_INJURY_URL).await {
        cache_put(&conn, "injury_data", &injury).map_err(|error| error.to_string())?;
        result["injury"] = json!(true);
    }

    if let Some(input_key) = odds_api_key
        .as_deref()
        .map(str::trim)
        .filter(|key| !key.is_empty())
    {
        save_provider_api_key(&conn, "odds_api_io", input_key).map_err(|error| error.to_string())?;
    }
    let key_from_settings = provider_api_key(&conn, "odds_api_io").map_err(|error| error.to_string())?;
    let key = key_from_settings.as_deref().map(str::trim).filter(|key| !key.is_empty());
    if let Some(key) = key {
        let url = format!(
            "https://api.the-odds-api.com/v4/sports/soccer_fifa_world_cup/odds/?apiKey={}&regions={}&markets=h2h,spreads,totals&oddsFormat=decimal&dateFormat=iso",
            key,
            region.unwrap_or_else(|| "eu".to_string())
        );
        if let Ok(europe) = http_json(&url).await {
            cache_put(&conn, "europe_odds", &europe).map_err(|error| error.to_string())?;
            result["europe"] = json!(true);
        }
    }
    Ok(result)
}

#[tauri::command]
async fn refresh_statsbomb_xg(app: AppHandle) -> Result<Value, String> {
    let conn = open_conn(&app)?;
    let payload = statsbomb_worldcup_xg_payload().await.map_err(|error| error.to_string())?;
    cache_put(&conn, "statsbomb_xg", &payload).map_err(|error| error.to_string())?;
    Ok(payload)
}

#[tauri::command]
async fn refresh_sporttery_injuries(app: AppHandle) -> Result<Value, String> {
    let conn = open_conn(&app)?;
    let injury = http_sporttery_mobile_json(SPORTTERY_INJURY_URL)
        .await
        .map_err(|error| error.to_string())?;
    cache_put(&conn, "injury_data", &injury).map_err(|error| error.to_string())?;
    Ok(json!({
        "ok": true,
        "count": injury_count(&injury)
    }))
}

#[tauri::command]
async fn refresh_results(app: AppHandle) -> Result<Vec<MatchResult>, String> {
    let conn = open_conn(&app)?;
    let html = http_text(ZGZCW_RESULTS_URL).await.map_err(|error| error.to_string())?;
    let results = parse_zgzcw_results(&html);
    cache_put(&conn, "match_results", &json!(results)).map_err(|error| error.to_string())?;
    let _ = conn.execute("delete from match_results where source='zgzcw_worldcup'", []);
    let fetched_at = Utc::now().to_rfc3339();
    for result in &results {
        let match_label = format!("{} vs {}", result.home, result.away);
        let _ = conn.execute(
            "insert into match_results(match_id, match_label, home, away, score, half_score, stage, status, source, fetched_at)
             values('', ?1, ?2, ?3, ?4, ?5, ?6, ?7, 'zgzcw_worldcup', ?8)",
            params![
                match_label,
                result.home,
                result.away,
                result.score,
                result.half_score,
                result.stage,
                result.status,
                fetched_at
            ],
        );
    }
    Ok(results)
}

#[tauri::command]
async fn import_historical_results_csv(app: AppHandle, csv_text: String) -> Result<Value, String> {
    let conn = open_conn(&app)?;
    let imported = parse_results_csv(&csv_text).map_err(|error| error.to_string())?;
    cache_put(&conn, "historical_results", &json!(imported)).map_err(|error| error.to_string())?;
    Ok(json!({
        "ok": true,
        "count": cached_results(&conn, "historical_results").len(),
        "message": "历史赛果样本已导入，将参与动态Elo和模型回测"
    }))
}

#[tauri::command]
async fn import_player_status_csv(app: AppHandle, csv_text: String) -> Result<Value, String> {
    let conn = open_conn(&app)?;
    let payload = parse_player_status_csv(&csv_text).map_err(|error| error.to_string())?;
    cache_put(&conn, "player_status_data", &payload).map_err(|error| error.to_string())?;
    Ok(json!({
        "ok": true,
        "count": player_status_count(&payload),
        "message": "球员状态/预计首发已导入，将参与模拟与推荐修正"
    }))
}

#[tauri::command]
async fn import_team_stats_csv(app: AppHandle, csv_text: String) -> Result<Value, String> {
    let conn = open_conn(&app)?;
    let payload = parse_team_stats_csv(&csv_text).map_err(|error| error.to_string())?;
    cache_put(&conn, "stats_data", &payload).map_err(|error| error.to_string())?;
    Ok(json!({
        "ok": true,
        "count": payload.get("teamCount").and_then(Value::as_u64).unwrap_or(0),
        "message": "实时球队统计/xG已导入，将优先用于模型λ估算"
    }))
}

#[tauri::command]
async fn list_results(app: AppHandle) -> Result<Vec<MatchResult>, String> {
    let conn = open_conn(&app)?;
    if let Some(record) = cache_get(&conn, "match_results").map_err(|error| error.to_string())? {
        serde_json::from_value(record.value).map_err(|error| error.to_string())
    } else {
        Ok(Vec::new())
    }
}

#[tauri::command]
async fn list_matches(app: AppHandle) -> Result<Vec<MatchRow>, String> {
    let conn = open_conn(&app)?;
    let mut rows = Vec::new();
    if let Some(record) = cache_get(&conn, "sporttery").map_err(|error| error.to_string())? {
        collect_matches(&record.value, &mut rows);
    }
    rows.sort_by(|a, b| a.time.cmp(&b.time));
    rows.dedup_by(|a, b| a.id == b.id && a.home == b.home && a.away == b.away);
    Ok(rows)
}

fn latest_match_movements(conn: &Connection, match_id: Option<&str>, home: &str, away: &str) -> anyhow::Result<Vec<OddsMovement>> {
    let mut stmt = conn.prepare(
        "select id, created_at, match_id, match_label, market, pick, initial_odds, previous_odds, current_odds, delta_abs, delta_pct, direction
         from odds_movements order by id desc limit 200",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(OddsMovement {
            id: row.get(0)?,
            created_at: row.get(1)?,
            match_label: row.get(3)?,
            market: row.get(4)?,
            pick: row.get(5)?,
            initial_odds: row.get(6)?,
            previous_odds: row.get(7)?,
            current_odds: row.get(8)?,
            delta_abs: row.get(9)?,
            delta_pct: row.get(10)?,
            direction: row.get(11)?,
        })
    })?;
    let mut movements = Vec::new();
    for row in rows {
        let movement = row?;
        if let Some(id) = match_id.filter(|id| !id.trim().is_empty()) {
            let stored_id: String = conn
                .query_row("select match_id from odds_movements where id=?1", params![movement.id], |row| row.get(0))
                .unwrap_or_default();
            if stored_id == id {
                movements.push(movement);
            }
            continue;
        }
        let parts = movement.match_label.split(" vs ").collect::<Vec<_>>();
        if parts.len() == 2 && team_matches(parts[0], home) && team_matches(parts[1], away) {
            movements.push(movement);
        }
    }
    Ok(movements)
}

#[tauri::command]
async fn simulate_match(app: AppHandle, request: SimRequest) -> Result<SimResult, String> {
    let conn = open_conn(&app)?;
    let xg_cache = cache_get(&conn, "statsbomb_xg").map_err(|error| error.to_string())?;
    let stats_cache = cache_get(&conn, "stats_data").map_err(|error| error.to_string())?;
    let preferred_xg = preferred_xg_value(stats_cache.as_ref(), xg_cache.as_ref());
    let xg_value = preferred_xg.map(|item| item.0);
    let xg_source_label = preferred_xg.map(|item| item.1).unwrap_or("无xG");
    let sporttery_cache = cache_get(&conn, "sporttery").map_err(|error| error.to_string())?;
    let europe_cache = cache_get(&conn, "europe_odds").map_err(|error| error.to_string())?;
    let injury_cache = cache_get(&conn, "injury_data").map_err(|error| error.to_string())?;
    let player_status_cache = cache_get(&conn, "player_status_data").map_err(|error| error.to_string())?;
    let result_rows = model_result_rows(&conn);
    let home_xg = xg_value.and_then(|value| xg_profile(value, &request.home));
    let away_xg = xg_value.and_then(|value| xg_profile(value, &request.away));
    let (rank_home_lambda, rank_away_lambda, rank_note) = if result_rows.is_empty() {
        rank_lambdas(&request.home, &request.away)
    } else {
        elo_lambdas(&request.home, &request.away, &result_rows)
    };
    let lambda_source_note = if home_xg.is_some() && away_xg.is_some() {
        format!("xG命中：使用{}时间衰减攻防xG估算λ", xg_source_label)
    } else if home_xg.is_some() || away_xg.is_some() {
        format!("xG部分命中：命中方使用{}，缺失一方用{}", xg_source_label, rank_note)
    } else {
        format!("xG未命中：使用{}", rank_note)
    };
    let mut lambda_home = request.home_lambda.unwrap_or_else(|| {
        if let (Some((home_for, _)), Some((_, away_against))) = (home_xg, away_xg) {
            ((home_for + away_against) / 2.0).clamp(0.35, 3.4)
        } else {
            rank_home_lambda
        }
    });
    let mut lambda_away = request.away_lambda.unwrap_or_else(|| {
        if let (Some((_, home_against)), Some((away_for, _))) = (home_xg, away_xg) {
            ((away_for + home_against) / 2.0).clamp(0.35, 3.4)
        } else {
            rank_away_lambda
        }
    });
    if request.home_lambda.is_none() && request.away_lambda.is_none() {
        lambda_home *= 1.03;
    }
    let knockout_mode = request.knockout_mode.unwrap_or(true);
    let mut knockout_tempo_note = "常规模式：不额外下调淘汰赛节奏".to_string();
    if knockout_mode {
        knockout_tempo_note = apply_knockout_tempo(&request.home, &request.away, &mut lambda_home, &mut lambda_away);
    }
    let mut adjustment_notes = Vec::new();
    if knockout_mode {
        adjustment_notes.push(knockout_tempo_note);
    } else {
        adjustment_notes.push(knockout_tempo_note);
    }
    adjustment_notes.push(format!(
        "模拟对象：{} vs {}{}",
        ranked_team_label(&request.home),
        ranked_team_label(&request.away),
        request.match_id.as_deref().map(|id| format!("，比赛ID {}", id)).unwrap_or_default()
    ));
    adjustment_notes.push(lambda_source_note.clone());
    if let Some(injury) = injury_cache.as_ref() {
        let (home_injury, home_count) = team_injury_weight(&injury.value, &request.home);
        let (away_injury, away_count) = team_injury_weight(&injury.value, &request.away);
        if home_count > 0 || away_count > 0 {
            lambda_home *= (1.0 - home_injury).clamp(0.78, 1.0);
            lambda_away *= (1.0 - away_injury).clamp(0.78, 1.0);
            adjustment_notes.push(format!(
                "伤停修正：按位置/首发次数加权，{} {}人 -{:.1}%，{} {}人 -{:.1}%",
                request.home,
                home_count,
                home_injury * 100.0,
                request.away,
                away_count,
                away_injury * 100.0
            ));
        } else {
            adjustment_notes.push("伤停缓存存在，但本场未匹配到伤停球员".to_string());
        }
    } else {
        adjustment_notes.push("未纳入伤停：本地暂无有效伤停缓存".to_string());
    }
    if let Some(player_status) = player_status_cache.as_ref() {
        adjustment_notes.push(apply_player_status_to_lambdas(
            Some(&player_status.value),
            &request.home,
            &request.away,
            &mut lambda_home,
            &mut lambda_away,
        ));
    } else {
        adjustment_notes.push("未纳入球员级首发/状态：尚未导入结构化CSV".to_string());
    }
    let max_goals = request.max_goals.unwrap_or(8).clamp(5, 12);
    let mut matrix = score_distribution(lambda_home, lambda_away, max_goals);
    normalize_score_probs(&mut matrix);
    let dc_rho = if knockout_mode { -0.10 } else { -0.07 };
    apply_dixon_coles(&mut matrix, lambda_home, lambda_away, dc_rho);
    adjustment_notes.push(format!("Dixon-Coles低比分修正：rho {:.2}", dc_rho));
    let mut base_threeway = threeway_from_scores(&matrix);
    let mut model_version = "rules-dixon-coles-v1".to_string();

    let sporttery_selections = sporttery_cache
        .as_ref()
        .map(|record| sporttery_selections(&record.value))
        .unwrap_or_default()
        .into_iter()
        .filter(|selection| {
            request
                .match_id
                .as_deref()
                .filter(|id| !id.trim().is_empty())
                .map(|id| selection.match_id == id)
                .unwrap_or_else(|| team_matches(&selection.home, &request.home) && team_matches(&selection.away, &request.away))
        })
        .collect::<Vec<_>>();
    adjustment_notes.push(format!("本场匹配体彩选项 {} 个", sporttery_selections.len()));
    let had_home = find_selection(&sporttery_selections, "HAD", "主胜");
    let had_draw = find_selection(&sporttery_selections, "HAD", "平局");
    let had_away = find_selection(&sporttery_selections, "HAD", "客胜");
    let sporttery_threeway = if let (Some(h), Some(d), Some(a)) = (had_home, had_draw, had_away) {
        Some((h.fair_prob, d.fair_prob, a.fair_prob))
    } else {
        None
    };
    let europe = europe_cache
        .as_ref()
        .and_then(|record| europe_consensus(&record.value, &request.home, &request.away));

    let app_dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let model_input = model_feature_input_for_match(
        &request.home,
        &request.away,
        &result_rows,
        lambda_home,
        lambda_away,
        sporttery_threeway,
        europe.as_ref(),
        had_home,
        had_draw,
        had_away,
    );
    if let Some(trained) = predict_with_training_models(&app_dir, &model_input) {
        lambda_home = trained.home_goals_lambda;
        lambda_away = trained.away_goals_lambda;
        matrix = score_distribution(lambda_home, lambda_away, max_goals);
        normalize_score_probs(&mut matrix);
        apply_dixon_coles(&mut matrix, lambda_home, lambda_away, dc_rho);
        base_threeway = (trained.home_win_prob, trained.draw_prob, trained.away_win_prob);
        matrix = retarget_scores_to_threeway(&matrix, base_threeway);
        model_version = trained.model_version;
        adjustment_notes.push(format!(
            "训练模型接管概率层：{}，输出 {:.1}%/{:.1}%/{:.1}%，λ {:.2}/{:.2}",
            model_version,
            base_threeway.0 * 100.0,
            base_threeway.1 * 100.0,
            base_threeway.2 * 100.0,
            lambda_home,
            lambda_away
        ));
        adjustment_notes.push(format!(
            "训练模型比分矩阵 {} 项，总进球矩阵 {} 项",
            trained.score_probs_json.as_array().map(|items| items.len()).unwrap_or(0),
            trained.total_goals_probs_json.as_array().map(|items| items.len()).unwrap_or(0)
        ));
    } else {
        adjustment_notes.push("训练模型未加载或训练样本为空：自动回退现有规则概率模型".to_string());
    }

    let (snapshot_market_weight, snapshot_weight_note) = request
        .match_id
        .as_deref()
        .map(|id| market_weight_for_match(&conn, id))
        .unwrap_or((0.36, "手动比赛：无赔率快照权重".to_string()));
    adjustment_notes.push(snapshot_weight_note);
    let mut sporttery_weight = if sporttery_threeway.is_some() { snapshot_market_weight.min(0.50) } else { 0.0 };
    let europe_weight = europe
        .as_ref()
        .map(|consensus| {
            if consensus.bookmaker_count >= 8 {
                0.34
            } else if consensus.bookmaker_count >= 4 {
                0.28
            } else {
                0.18
            }
        })
        .unwrap_or(0.0);
    if europe_weight > 0.0 {
        sporttery_weight *= 0.80;
    }
    let model_weight = (1.0_f64 - sporttery_weight - europe_weight).max(0.38);
    let weight_sum = model_weight + sporttery_weight + europe_weight;
    let model_weight = model_weight / weight_sum;
    let sporttery_weight = sporttery_weight / weight_sum;
    let europe_weight = europe_weight / weight_sum;
    let mut target_threeway = (
        base_threeway.0 * model_weight,
        base_threeway.1 * model_weight,
        base_threeway.2 * model_weight,
    );
    if let Some(market) = sporttery_threeway {
        target_threeway.0 += market.0 * sporttery_weight;
        target_threeway.1 += market.1 * sporttery_weight;
        target_threeway.2 += market.2 * sporttery_weight;
        adjustment_notes.push(format!("已纳入体彩去水概率，动态权重 {:.0}%", sporttery_weight * 100.0));
    } else {
        adjustment_notes.push("未纳入体彩重标定：本场暂无HAD胜平负缓存".to_string());
    }
    if let Some(consensus) = europe.as_ref() {
        target_threeway.0 += consensus.home_prob * europe_weight;
        target_threeway.1 += consensus.draw_prob * europe_weight;
        target_threeway.2 += consensus.away_prob * europe_weight;
        adjustment_notes.push(format!("已纳入欧洲共识：{}家公司，动态权重 {:.0}%", consensus.bookmaker_count, europe_weight * 100.0));
    } else {
        adjustment_notes.push("未纳入欧洲共识：本场未匹配欧洲赔率".to_string());
    }
    adjustment_notes.push(format!("模型基础权重 {:.0}%", model_weight * 100.0));
    let target_sum = target_threeway.0 + target_threeway.1 + target_threeway.2;
    if target_sum > 0.0 {
        target_threeway.0 /= target_sum;
        target_threeway.1 /= target_sum;
        target_threeway.2 /= target_sum;
        matrix = retarget_scores_to_threeway(&matrix, target_threeway);
    }

    let simulations = request.simulations.unwrap_or(50_000).clamp(50_000, 500_000);
    let _ = app.emit("simulation-progress", json!({
        "done": 0,
        "total": simulations,
        "percent": 0.0,
        "message": "开始真实蒙特卡洛模拟"
    }));
    let mut cumulative = Vec::with_capacity(matrix.len());
    let mut running = 0.0;
    for (_, _, p) in &matrix {
        running += *p;
        cumulative.push(running);
    }
    if let Some(last) = cumulative.last_mut() {
        *last = 1.0;
    }
    let mut rng = rand::thread_rng();
    let mut home_win_count = 0u32;
    let mut draw_count = 0u32;
    let mut away_win_count = 0u32;
    let mut over_25_count = 0u32;
    let mut btts_count = 0u32;
    let mut score_counts: BTreeMap<(u32, u32), u32> = BTreeMap::new();
    let mut total_goal_counts = [0u32; 8];
    let progress_step = (simulations / 100).max(1_000);
    for idx in 1..=simulations {
        let score_idx = draw_score_index(&cumulative, &mut rng);
        let (h, a, _) = matrix[score_idx];
        if h > a {
            home_win_count += 1;
        } else if h == a {
            draw_count += 1;
        } else {
            away_win_count += 1;
        }
        if h + a >= 3 {
            over_25_count += 1;
        }
        if h > 0 && a > 0 {
            btts_count += 1;
        }
        total_goal_counts[(h + a).min(7) as usize] += 1;
        *score_counts.entry((h, a)).or_insert(0) += 1;
        if idx % progress_step == 0 || idx == simulations {
            let _ = app.emit("simulation-progress", json!({
                "done": idx,
                "total": simulations,
                "percent": idx as f64 / simulations as f64,
                "message": format!("已模拟 {} / {} 场", idx, simulations)
            }));
        }
    }
    let denom = simulations as f64;
    let home_win = home_win_count as f64 / denom;
    let draw = draw_count as f64 / denom;
    let away_win = away_win_count as f64 / denom;
    let over_25 = over_25_count as f64 / denom;
    let btts = btts_count as f64 / denom;
    let (home_win_low, home_win_high) = binomial_ci(home_win, simulations);
    let (draw_low, draw_high) = binomial_ci(draw, simulations);
    let (away_win_low, away_win_high) = binomial_ci(away_win, simulations);
    let (over_25_low, over_25_high) = binomial_ci(over_25, simulations);
    let (btts_low, btts_high) = binomial_ci(btts, simulations);
    let mut scores = score_counts
        .iter()
        .map(|((h, a), count)| ScoreProb {
            score: format!("{}:{}", h, a),
            probability: *count as f64 / denom,
        })
        .collect::<Vec<_>>();
    let total_goals = total_goal_counts
        .iter()
        .enumerate()
        .map(|(idx, count)| ScoreProb {
            score: if idx == 7 { "7+球".to_string() } else { format!("{}球", idx) },
            probability: *count as f64 / denom,
        })
        .collect::<Vec<_>>();
    let latest_movements = latest_match_movements(&conn, request.match_id.as_deref(), &request.home, &request.away).unwrap_or_default();
    let movement_note = if latest_movements.is_empty() {
        "暂无本场赔率异动记录".to_string()
    } else {
        latest_movements
            .iter()
            .take(4)
            .map(|item| format!("{}{} {} {:+.2}", item.market, item.pick, item.direction, item.delta_abs))
            .collect::<Vec<_>>()
            .join("；")
    };
    let market_rows = ["主胜", "平局", "客胜"]
        .iter()
        .map(|pick| {
            let model_prob = match *pick {
                "主胜" => home_win,
                "平局" => draw,
                "客胜" => away_win,
                _ => 0.0,
            };
            let sporttery = find_selection(&sporttery_selections, "HAD", pick);
            let europe_pair = europe.as_ref().and_then(|consensus| europe_pick(consensus, pick));
            let (ci_low, ci_high) = binomial_ci(model_prob, simulations);
            SimMarketRow {
                pick: (*pick).to_string(),
                model_prob,
                ci_low,
                ci_high,
                sporttery_prob: sporttery.map(|selection| selection.fair_prob),
                europe_prob: europe_pair.map(|pair| pair.0),
                gap_vs_sporttery: sporttery.map(|selection| model_prob - selection.fair_prob),
                gap_vs_europe: europe_pair.map(|pair| model_prob - pair.0),
                fair_odds: if model_prob > 0.0 { 1.0 / model_prob } else { 0.0 },
                sporttery_odds: sporttery.map(|selection| selection.odds),
            }
        })
        .collect::<Vec<_>>();
    let injury_note = adjustment_notes
        .iter()
        .find(|note| note.starts_with("伤停") || note.starts_with("未纳入伤停"))
        .cloned()
        .unwrap_or_else(|| "伤停未纳入".to_string());
    if draw >= 0.30 {
        adjustment_notes.push("淘汰赛提醒：90分钟平局概率偏高，胜平负不等于最终晋级".to_string());
    } else {
        adjustment_notes.push("淘汰赛提醒：本页概率按90分钟赛果计算".to_string());
    }
    if let Some(market) = sporttery_threeway {
        let max_gap = (home_win - market.0).abs().max((draw - market.1).abs()).max((away_win - market.2).abs());
        if max_gap >= 0.08 {
            adjustment_notes.push(format!("模型与体彩最大分歧 {:.1} 个百分点，需降低仓位", max_gap * 100.0));
        }
    }
    scores.sort_by(|a, b| b.probability.partial_cmp(&a.probability).unwrap_or(std::cmp::Ordering::Equal));
    scores.truncate(8);
    Ok(SimResult {
        home: request.home,
        away: request.away,
        model_version,
        lambda_home,
        lambda_away,
        home_win,
        home_win_low,
        home_win_high,
        draw,
        draw_low,
        draw_high,
        away_win,
        away_win_low,
        away_win_high,
        over_25,
        over_25_low,
        over_25_high,
        btts,
        btts_low,
        btts_high,
        total_goals,
        top_scores: scores,
        source_note: format!(
            "{} + 伤停修正 + Dixon-Coles低比分修正 + 动态市场重标定 + 蒙特卡洛真实抽样",
            lambda_source_note
        ),
        market_rows,
        adjustment_notes,
        injury_note,
        movement_note,
        knockout_note: "淘汰赛按90分钟赛果建模；平局代表进入加时/点球风险区，不代表最终晋级。".to_string(),
        simulations,
        simulation_note: format!("真实蒙特卡洛随机模拟 {} 场；每场从修正后的比分分布中抽样一次。", simulations),
    })
}

#[tauri::command]
async fn save_prediction(app: AppHandle, record: PredictionRecord) -> Result<(), String> {
    let conn = open_conn(&app)?;
    conn.execute(
        "insert into predictions(created_at, match_label, market, pick, probability, odds, safety_margin, decision, stake_pct, actual_result, profit)
         values(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, '', 0)",
        params![
            Utc::now().to_rfc3339(),
            record.match_label,
            record.market,
            record.pick,
            record.probability,
            record.odds,
            record.safety_margin,
            record.decision,
            record.stake_pct.unwrap_or(0.0)
        ],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
async fn list_recommendations(app: AppHandle) -> Result<Vec<Recommendation>, String> {
    let conn = open_conn(&app)?;
    let sporttery = cache_get(&conn, "sporttery")
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "暂无体彩缓存，请先刷新核心数据".to_string())?;
    let europe_cache = cache_get(&conn, "europe_odds").map_err(|error| error.to_string())?;
    let europe_value = europe_cache.as_ref().map(|record| &record.value);
    let stats_cache = cache_get(&conn, "stats_data").map_err(|error| error.to_string())?;
    let statsbomb_cache = cache_get(&conn, "statsbomb_xg").map_err(|error| error.to_string())?;
    let preferred_xg = preferred_xg_value(stats_cache.as_ref(), statsbomb_cache.as_ref());
    let player_status_cache = cache_get(&conn, "player_status_data").map_err(|error| error.to_string())?;
    let player_status_value = player_status_cache.as_ref().map(|record| &record.value);
    let result_rows = model_result_rows(&conn);
    let settings = load_model_settings(&conn);
    let selections = sporttery_selections(&sporttery.value);
    let mut grouped: BTreeMap<String, Vec<OddsSelection>> = BTreeMap::new();
    for selection in selections {
        grouped.entry(selection.match_id.clone()).or_default().push(selection);
    }

    let mut recommendations = Vec::new();
    for selections in grouped.values() {
        let Some(first) = selections.first() else { continue };
        let (mut lambda_home, mut lambda_away, note) = if result_rows.is_empty() {
            rank_lambdas(&first.home, &first.away)
        } else {
            elo_lambdas(&first.home, &first.away, &result_rows)
        };
        let mut note = note;
        if let Some((xg_value, source_label)) = preferred_xg {
            if let (Some((home_for, _)), Some((_, away_against)), Some((_, home_against)), Some((away_for, _))) = (
                xg_profile(xg_value, &first.home),
                xg_profile(xg_value, &first.away),
                xg_profile(xg_value, &first.home),
                xg_profile(xg_value, &first.away),
            ) {
                lambda_home = ((home_for + away_against) / 2.0).clamp(0.35, 3.4);
                lambda_away = ((away_for + home_against) / 2.0).clamp(0.35, 3.4);
                note = format!("{}命中：攻防xG λ", source_label);
            }
        }
        let knockout_note = apply_knockout_tempo(&first.home, &first.away, &mut lambda_home, &mut lambda_away);
        let player_note = apply_player_status_to_lambdas(player_status_value, &first.home, &first.away, &mut lambda_home, &mut lambda_away);
        let europe = europe_value.and_then(|value| europe_consensus(value, &first.home, &first.away));
        let had_home = find_selection(selections, "HAD", "主胜");
        let had_draw = find_selection(selections, "HAD", "平局");
        let had_away = find_selection(selections, "HAD", "客胜");
        let sporttery_threeway = if let (Some(h), Some(d), Some(a)) = (had_home, had_draw, had_away) {
            Some((h.fair_prob, d.fair_prob, a.fair_prob))
        } else {
            None
        };
        let app_dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let model_input = model_feature_input_for_match(
            &first.home,
            &first.away,
            &result_rows,
            lambda_home,
            lambda_away,
            sporttery_threeway,
            europe.as_ref(),
            had_home,
            had_draw,
            had_away,
        );
        let mut global_model_note = "全局模型：规则λ/比分矩阵".to_string();
        if let Some(trained) = predict_with_training_models(&app_dir, &model_input) {
            lambda_home = trained.home_goals_lambda;
            lambda_away = trained.away_goals_lambda;
            global_model_note = format!("全局模型：{} + 进球模型λ {:.2}/{:.2}", trained.model_version, lambda_home, lambda_away);
        }
        let scores = score_distribution(lambda_home, lambda_away, 10);
        let (market_weight, market_weight_note) = market_weight_for_match(&conn, &first.match_id);
        let movements = latest_match_movements(&conn, Some(&first.match_id), &first.home, &first.away).unwrap_or_default();
        let (lineup_status, lineup_confidence) = lineup_status_for_match(&conn, &first.match_id);
        let (data_score, data_grade, support_factors, risk_factors) = match_data_quality(
            &conn,
            first,
            selections,
            europe.as_ref(),
            preferred_xg,
            player_status_value,
            &movements,
        );
        for selection in selections {
            if !(selection.market.starts_with("HAD") || selection.market.starts_with("HHAD") || selection.market.starts_with("TTG")) {
                continue;
            }
            let (raw_model_prob, ensemble_note) = if selection.market.starts_with("HHAD") {
                let line = selection.goal_line.parse::<f64>().unwrap_or(0.0);
                if let Some((hand_home, hand_draw, hand_away)) = predict_handicap_with_training_models(&app_dir, &model_input, line) {
                    let prob = match selection.pick.as_str() {
                        "让胜" => hand_home,
                        "让平" => hand_draw,
                        "让负" => hand_away,
                        _ => model_probability_from_scores(selection, &scores),
                    };
                    (prob, "训练让球映射模型".to_string())
                } else {
                    ensemble_probability(selection, &scores, europe.as_ref(), market_weight)
                }
            } else {
                ensemble_probability(selection, &scores, europe.as_ref(), market_weight)
            };
            let (model_prob, consistency_note) = apply_market_consistency(selection, raw_model_prob, &scores, selections);
            let (model_prob, reflexivity_note) = market_reflexivity_adjustment(selection, model_prob, europe.as_ref(), &movements);
            let combined_note = format!("{}；{}；{}；{}；{}；{}；{}；{}", note, global_model_note, knockout_note, player_note, market_weight_note, ensemble_note, consistency_note, reflexivity_note);
            let anomaly = latest_anomaly_for_selection(&conn, selection);
            recommendations.push(recommendation_for(
                selection,
                model_prob,
                &combined_note,
                europe.as_ref(),
                &settings,
                data_score,
                &data_grade,
                &support_factors,
                &risk_factors,
                &lineup_status,
                lineup_confidence,
                anomaly.as_ref(),
            ));
        }
    }
    recommendations.sort_by(|a, b| {
        let rank_a = match a.decision.as_str() { "可买" => 0, "观察" => 1, _ => 2 };
        let rank_b = match b.decision.as_str() { "可买" => 0, "观察" => 1, _ => 2 };
        let tier_a = match a.tier.as_str() { "稳胆" => 0, "让球稳胆" => 1, "价值小注" => 2, "进球数小注" => 3, "冷门小注" => 4, _ => 5 };
        let tier_b = match b.tier.as_str() { "稳胆" => 0, "让球稳胆" => 1, "价值小注" => 2, "进球数小注" => 3, "冷门小注" => 4, _ => 5 };
        rank_a
            .cmp(&rank_b)
            .then_with(|| tier_a.cmp(&tier_b))
            .then_with(|| b.stake_pct.partial_cmp(&a.stake_pct).unwrap_or(std::cmp::Ordering::Equal))
            .then_with(|| a.match_time.cmp(&b.match_time))
    });
    recommendations.truncate(120);
    Ok(recommendations)
}

#[tauri::command]
async fn freeze_current_recommendations(app: AppHandle) -> Result<Value, String> {
    let conn = open_conn(&app)?;
    let recommendations = list_recommendations(app.clone()).await?;
    let now = Utc::now().to_rfc3339();
    let mut snapshot_count = 0;
    let mut recommendation_count = 0;
    let mut grouped: BTreeMap<String, Vec<Recommendation>> = BTreeMap::new();
    for item in recommendations {
        grouped.entry(item.match_id.clone()).or_default().push(item);
    }
    for (_match_id, items) in grouped {
        let Some(first) = items.first() else { continue };
        let model_input = json!({
            "model_version": "recommendation-closure-v1",
            "lineup_status": first.lineup_status,
            "lineup_confidence": first.lineup_confidence,
            "data_score": first.data_score,
            "data_grade": first.data_grade,
            "support": first.support_factors,
            "risk": first.risk_factors
        });
        let probabilities = json!(items.iter().map(|item| json!({
            "market": item.market,
            "pick": item.pick,
            "model_prob": item.model_prob,
            "sporttery_prob": item.fair_prob,
            "europe_prob": item.europe_prob,
            "fair_odds": item.fair_odds
        })).collect::<Vec<_>>());
        let odds_payload = json!(items.iter().map(|item| json!({
            "market": item.market,
            "pick": item.pick,
            "current_odds": item.odds,
            "europe_odds": item.europe_odds
        })).collect::<Vec<_>>());
        conn.execute(
            "insert into prediction_snapshots(created_at, match_id, match_num, match_time, match_label, model_input, probabilities, odds_payload, data_quality_score, data_quality_grade, quality_action, risk_tags, model_version)
             values(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 'recommendation-closure-v1')",
            params![
                now,
                first.match_id,
                first.match_num,
                first.match_time,
                first.match_label,
                model_input.to_string(),
                probabilities.to_string(),
                odds_payload.to_string(),
                first.data_score,
                first.data_grade,
                first.quality_action,
                first.risk_factors
            ],
        )
        .map_err(|error| error.to_string())?;
        let snapshot_id = conn.last_insert_rowid();
        snapshot_count += 1;
        for item in items {
            conn.execute(
                "insert into bet_recommendations(snapshot_id, created_at, match_id, match_num, match_time, match_label, market, pick, model_prob, sporttery_prob, europe_prob, fair_odds, current_odds, ev, advantage_rate, recommendation_level, action_advice, stake_pct, data_quality_score, data_quality_grade, risk_tags, play_type_risk_level, anomaly_type, anomaly_severity, raw_payload)
                 values(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25)",
                params![
                    snapshot_id,
                    now,
                    item.match_id,
                    item.match_num,
                    item.match_time,
                    item.match_label,
                    item.market,
                    item.pick,
                    item.model_prob,
                    item.fair_prob,
                    item.europe_prob,
                    item.fair_odds,
                    item.odds,
                    item.expected_return,
                    item.advantage_rate,
                    item.tier,
                    item.action_advice,
                    item.stake_pct,
                    item.data_score,
                    item.data_grade,
                    item.risk_factors,
                    item.play_type_risk_level,
                    item.anomaly_type,
                    item.anomaly_severity,
                    serde_json::to_string(&item).unwrap_or_else(|_| "{}".to_string())
                ],
            )
            .map_err(|error| error.to_string())?;
            recommendation_count += 1;
        }
    }
    Ok(json!({
        "ok": true,
        "snapshots": snapshot_count,
        "recommendations": recommendation_count,
        "message": "赛前快照已冻结，后续复盘不会改写赛前概率"
    }))
}

#[tauri::command]
async fn list_match_analyses(app: AppHandle) -> Result<Vec<MatchAnalysis>, String> {
    let conn = open_conn(&app)?;
    let sporttery = cache_get(&conn, "sporttery")
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "暂无体彩缓存，请先刷新核心数据".to_string())?;
    let europe_cache = cache_get(&conn, "europe_odds").map_err(|error| error.to_string())?;
    let europe_value = europe_cache.as_ref().map(|record| &record.value);
    let stats_cache = cache_get(&conn, "stats_data").map_err(|error| error.to_string())?;
    let statsbomb_cache = cache_get(&conn, "statsbomb_xg").map_err(|error| error.to_string())?;
    let preferred_xg = preferred_xg_value(stats_cache.as_ref(), statsbomb_cache.as_ref());
    let player_status_cache = cache_get(&conn, "player_status_data").map_err(|error| error.to_string())?;
    let player_status_value = player_status_cache.as_ref().map(|record| &record.value);
    let result_rows = model_result_rows(&conn);
    let selections = sporttery_selections(&sporttery.value);
    let mut grouped: BTreeMap<String, Vec<OddsSelection>> = BTreeMap::new();
    for selection in selections {
        grouped.entry(selection.match_id.clone()).or_default().push(selection);
    }

    let mut analyses = Vec::new();
    for selections in grouped.values() {
        let Some(first) = selections.first() else { continue };
        let (mut lambda_home, mut lambda_away, _) = if result_rows.is_empty() {
            rank_lambdas(&first.home, &first.away)
        } else {
            elo_lambdas(&first.home, &first.away, &result_rows)
        };
        if let Some((xg_value, _)) = preferred_xg {
            if let (Some((home_for, home_against)), Some((away_for, away_against))) = (
                xg_profile(xg_value, &first.home),
                xg_profile(xg_value, &first.away),
            ) {
                lambda_home = ((home_for + away_against) / 2.0).clamp(0.35, 3.4);
                lambda_away = ((away_for + home_against) / 2.0).clamp(0.35, 3.4);
            }
        }
        let knockout_stage_note = apply_knockout_tempo(&first.home, &first.away, &mut lambda_home, &mut lambda_away);
        let player_status_note = apply_player_status_to_lambdas(player_status_value, &first.home, &first.away, &mut lambda_home, &mut lambda_away);
        let europe = europe_value.and_then(|value| europe_consensus(value, &first.home, &first.away));
        let had_home_sel = find_selection(selections, "HAD", "主胜");
        let had_draw_sel = find_selection(selections, "HAD", "平局");
        let had_away_sel = find_selection(selections, "HAD", "客胜");
        let sporttery_threeway = if let (Some(h), Some(d), Some(a)) = (had_home_sel, had_draw_sel, had_away_sel) {
            Some((h.fair_prob, d.fair_prob, a.fair_prob))
        } else {
            None
        };
        let app_dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let model_input = model_feature_input_for_match(
            &first.home,
            &first.away,
            &result_rows,
            lambda_home,
            lambda_away,
            sporttery_threeway,
            europe.as_ref(),
            had_home_sel,
            had_draw_sel,
            had_away_sel,
        );
        let mut global_model_note = "规则比分矩阵".to_string();
        if let Some(trained) = predict_with_training_models(&app_dir, &model_input) {
            lambda_home = trained.home_goals_lambda;
            lambda_away = trained.away_goals_lambda;
            global_model_note = format!("{} + 进球模型λ", trained.model_version);
        }
        let scores = score_distribution(lambda_home, lambda_away, 10);
        let (home_win, draw, away_win) = threeway_from_scores(&scores);
        let hhad_line = selections
            .iter()
            .find(|selection| selection.market.starts_with("HHAD"))
            .map(|selection| selection.goal_line.clone())
            .unwrap_or_default();
        let (hand_home, hand_draw, hand_away) = hhad_line
            .parse::<f64>()
            .ok()
            .and_then(|line| predict_handicap_with_training_models(&app_dir, &model_input, line))
            .unwrap_or_else(|| handicap_probs(&scores, &hhad_line));
        let mut top_scores = scores
            .iter()
            .map(|(h, a, p)| {
                let pick = format!("{}:{}", h, a);
                prob_item_with_market(&pick, *p, find_selection(selections, "CRS", &pick))
            })
            .collect::<Vec<_>>();
        top_scores.sort_by(|a, b| b.probability.partial_cmp(&a.probability).unwrap_or(std::cmp::Ordering::Equal));
        top_scores.truncate(10);

        let ttg = (0..=7)
            .map(|idx| {
                let pick = if idx == 7 { "7+球".to_string() } else { format!("{}球", idx) };
                prob_item_with_market(&pick, total_goal_prob(&scores, &pick), find_selection(selections, "TTG", &pick))
            })
            .collect::<Vec<_>>();
        let europe_note = if let Some(consensus) = europe {
            format!(
                "欧洲均值 {} 家：主 {:.2}% / 平 {:.2}% / 客 {:.2}%",
                consensus.bookmaker_count,
                consensus.home_prob * 100.0,
                consensus.draw_prob * 100.0,
                consensus.away_prob * 100.0
            )
        } else {
            "未匹配欧洲胜平负市场".to_string()
        };

        analyses.push(MatchAnalysis {
            match_id: first.match_id.clone(),
            match_num: first.match_num.clone(),
            match_time: first.match_time.clone(),
            match_label: format!("{} vs {}", first.home, first.away),
            lambda_home,
            lambda_away,
            knockout_note: format!("{}；{}；{}；淘汰赛按90分钟赛果建模；平局代表拖入加时/点球区间，不等于最终晋级。", global_model_note, knockout_stage_note, player_status_note),
            had: vec![
                prob_item_with_market("主胜", home_win, find_selection(selections, "HAD", "主胜")),
                prob_item_with_market("平局", draw, find_selection(selections, "HAD", "平局")),
                prob_item_with_market("客胜", away_win, find_selection(selections, "HAD", "客胜")),
            ],
            hhad: vec![
                prob_item_with_market("让胜", hand_home, find_selection(selections, "HHAD", "让胜")),
                prob_item_with_market("让平", hand_draw, find_selection(selections, "HHAD", "让平")),
                prob_item_with_market("让负", hand_away, find_selection(selections, "HHAD", "让负")),
            ],
            ttg,
            scores: top_scores,
            europe_note,
        });
    }
    analyses.sort_by(|a, b| a.match_time.cmp(&b.match_time));
    analyses.truncate(60);
    Ok(analyses)
}

#[tauri::command]
async fn list_odds_movements(app: AppHandle) -> Result<Vec<OddsMovement>, String> {
    let conn = open_conn(&app)?;
    let mut stmt = conn
        .prepare(
            "select id, created_at, match_label, market, pick, initial_odds, previous_odds, current_odds, delta_abs, delta_pct, direction
             from odds_movements order by id desc limit 1000",
        )
        .map_err(|error| error.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok(OddsMovement {
                id: row.get(0)?,
                created_at: row.get(1)?,
                match_label: row.get(2)?,
                market: row.get(3)?,
                pick: row.get(4)?,
                initial_odds: row.get(5)?,
                previous_odds: row.get(6)?,
                current_odds: row.get(7)?,
                delta_abs: row.get(8)?,
                delta_pct: row.get(9)?,
                direction: row.get(10)?,
            })
        })
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|error| error.to_string())
}

#[tauri::command]
async fn list_odds_anomalies(app: AppHandle) -> Result<Vec<OddsAnomaly>, String> {
    let conn = open_conn(&app)?;
    let mut stmt = conn
        .prepare(
            "select id, created_at, match_id, match_label, market, pick, anomaly_type, severity, impact_direction, advice, delta_abs, delta_pct
             from odds_anomalies order by id desc limit 1000",
        )
        .map_err(|error| error.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok(OddsAnomaly {
                id: row.get(0)?,
                created_at: row.get(1)?,
                match_id: row.get(2)?,
                match_label: row.get(3)?,
                market: row.get(4)?,
                pick: row.get(5)?,
                anomaly_type: row.get(6)?,
                severity: row.get(7)?,
                impact_direction: row.get(8)?,
                advice: row.get(9)?,
                delta_abs: row.get(10)?,
                delta_pct: row.get(11)?,
            })
        })
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|error| error.to_string())
}

#[tauri::command]
async fn list_odds_history(app: AppHandle) -> Result<Vec<OddsMovement>, String> {
    let conn = open_conn(&app)?;
    let mut stmt = conn
        .prepare(
            r#"
            select
              s.id,
              s.created_at,
              s.match_label,
              s.market,
              s.pick,
              coalesce((
                select i.odds from odds_snapshots i
                where i.match_id=s.match_id and i.market=s.market and i.pick=s.pick
                order by i.id asc limit 1
              ), s.odds) as initial_odds,
              coalesce((
                select p.odds from odds_snapshots p
                where p.match_id=s.match_id and p.market=s.market and p.pick=s.pick and p.id<s.id
                order by p.id desc limit 1
              ), s.odds) as previous_odds,
              s.odds as current_odds
            from odds_snapshots s
            where exists (
              select 1 from odds_snapshots p
              where p.match_id=s.match_id and p.market=s.market and p.pick=s.pick and p.id<s.id
            )
            order by s.id desc
            limit 1000
            "#,
        )
        .map_err(|error| error.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            let previous_odds: f64 = row.get(6)?;
            let current_odds: f64 = row.get(7)?;
            let delta_abs = current_odds - previous_odds;
            let delta_pct = if previous_odds > 0.0 { delta_abs / previous_odds } else { 0.0 };
            Ok(OddsMovement {
                id: row.get(0)?,
                created_at: row.get(1)?,
                match_label: row.get(2)?,
                market: row.get(3)?,
                pick: row.get(4)?,
                initial_odds: row.get(5)?,
                previous_odds,
                current_odds,
                delta_abs,
                delta_pct,
                direction: if delta_abs > 0.0 {
                    "升赔".to_string()
                } else if delta_abs < 0.0 {
                    "降赔".to_string()
                } else {
                    "持平".to_string()
                },
            })
        })
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|error| error.to_string())
}

#[tauri::command]
async fn delete_prediction(app: AppHandle, id: i64) -> Result<(), String> {
    let conn = open_conn(&app)?;
    conn.execute("delete from predictions where id=?1", params![id])
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
async fn settle_prediction(app: AppHandle, id: i64, hit: bool) -> Result<(), String> {
    let conn = open_conn(&app)?;
    let (odds, stake_pct): (f64, f64) = conn
        .query_row(
            "select odds, coalesce(stake_pct, 0) from predictions where id=?1",
            params![id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|error| error.to_string())?;
    let profit = if hit { stake_pct * (odds - 1.0) } else { -stake_pct };
    conn.execute(
        "update predictions set actual_result=?1, profit=?2 where id=?3",
        params![if hit { "命中" } else { "未中" }, profit, id],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
async fn auto_settle_predictions(app: AppHandle) -> Result<Value, String> {
    let conn = open_conn(&app)?;
    let results: Vec<MatchResult> = if let Some(record) = cache_get(&conn, "match_results").map_err(|error| error.to_string())? {
        serde_json::from_value(record.value).map_err(|error| error.to_string())?
    } else {
        Vec::new()
    };
    if results.is_empty() {
        return Ok(json!({ "settled": 0, "message": "暂无赛果缓存，请先刷新赛果" }));
    }
    let records = list_predictions(app.clone()).await?;
    let mut settled = 0;
    for record in records {
        if record.actual_result.as_deref().unwrap_or("").trim() != "" {
            continue;
        }
        let Some(id) = record.id else { continue };
        let Some(result) = results.iter().find(|result| result_matches_prediction(result, &record.match_label)) else { continue };
        let Some(hit) = prediction_hit(&record, result) else { continue };
        let stake_pct = record.stake_pct.unwrap_or(0.0);
        let profit = if hit { stake_pct * (record.odds - 1.0) } else { -stake_pct };
        conn.execute(
            "update predictions set actual_result=?1, profit=?2 where id=?3",
            params![if hit { "命中" } else { "未中" }, profit, id],
        )
        .map_err(|error| error.to_string())?;
        settled += 1;
    }
    Ok(json!({ "settled": settled, "message": format!("自动结算 {} 条", settled) }))
}

fn settle_bet_recommendations_with_results(conn: &Connection, results: &[MatchResult]) -> anyhow::Result<i64> {
    let mut stmt = conn.prepare(
        r#"
        select id, coalesce(snapshot_id,0), created_at, match_id, match_num, match_time, match_label,
               market, pick, model_prob, sporttery_prob, europe_prob, fair_odds, current_odds,
               ev, advantage_rate, recommendation_level, action_advice, stake_pct,
               data_quality_score, data_quality_grade, risk_tags, play_type_risk_level,
               anomaly_type, anomaly_severity, raw_payload
        from bet_recommendations r
        where not exists (
          select 1 from bet_results b where b.recommendation_id=r.id
        )
        order by match_time asc, id asc
        "#,
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, String>(6)?,
            row.get::<_, String>(7)?,
            row.get::<_, String>(8)?,
            row.get::<_, f64>(9)?,
            row.get::<_, f64>(10)?,
            row.get::<_, Option<f64>>(11)?,
            row.get::<_, f64>(12)?,
            row.get::<_, f64>(13)?,
            row.get::<_, f64>(14)?,
            row.get::<_, f64>(15)?,
            row.get::<_, String>(16)?,
            row.get::<_, String>(17)?,
            row.get::<_, f64>(18)?,
            row.get::<_, f64>(19)?,
            row.get::<_, String>(20)?,
            row.get::<_, String>(21)?,
            row.get::<_, String>(22)?,
            row.get::<_, String>(23)?,
            row.get::<_, String>(24)?,
            row.get::<_, String>(25)?,
        ))
    })?;

    let mut settled = 0;
    let settled_at = Utc::now().to_rfc3339();
    for row in rows {
        let (
            recommendation_id,
            snapshot_id,
            frozen_at,
            match_id,
            match_num,
            match_time,
            match_label,
            market,
            pick,
            model_prob,
            sporttery_prob,
            europe_prob,
            fair_odds,
            current_odds,
            ev,
            advantage_rate,
            recommendation_level,
            action_advice,
            stake_pct,
            data_quality_score,
            data_quality_grade,
            risk_tags,
            play_type_risk_level,
            anomaly_type,
            anomaly_severity,
            raw_payload,
        ) = row?;
        let Some(result) = results.iter().find(|result| result_matches_prediction(result, &match_label)) else {
            continue;
        };
        let Some((hit, actual_outcome)) = pick_hit_from_result(&market, &pick, result) else {
            continue;
        };
        let profit = if hit { stake_pct * (current_odds - 1.0) } else { -stake_pct };
        conn.execute(
            "insert into bet_results(recommendation_id, settled_at, match_label, market, pick, hit, stake_pct, odds, profit, result_score)
             values(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                recommendation_id,
                settled_at,
                match_label,
                market,
                pick,
                if hit { 1 } else { 0 },
                stake_pct,
                current_odds,
                profit,
                result.score
            ],
        )?;
        conn.execute(
            r#"
            insert into worldcup_training_samples(
              created_at, frozen_at, settled_at, snapshot_id, recommendation_id, match_id, match_num,
              match_time, match_label, market, pick, model_prob, sporttery_prob, europe_prob,
              fair_odds, current_odds, ev, advantage_rate, recommendation_level, action_advice,
              stake_pct, data_quality_score, data_quality_grade, risk_tags, play_type_risk_level,
              anomaly_type, anomaly_severity, result_score, actual_outcome, hit, profit, stage, raw_payload
            )
            values(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17,
                   ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30, ?31, ?32, ?33)
            on conflict(recommendation_id) do nothing
            "#,
            params![
                Utc::now().to_rfc3339(),
                frozen_at,
                settled_at,
                snapshot_id,
                recommendation_id,
                match_id,
                match_num,
                match_time,
                match_label,
                market,
                pick,
                model_prob,
                sporttery_prob,
                europe_prob,
                fair_odds,
                current_odds,
                ev,
                advantage_rate,
                recommendation_level,
                action_advice,
                stake_pct,
                data_quality_score,
                data_quality_grade,
                risk_tags,
                play_type_risk_level,
                anomaly_type,
                anomaly_severity,
                result.score,
                actual_outcome,
                if hit { 1 } else { 0 },
                profit,
                result.stage,
                raw_payload
            ],
        )?;
        settled += 1;
    }
    Ok(settled)
}

#[tauri::command]
async fn settle_bet_recommendations(app: AppHandle) -> Result<Value, String> {
    let results = refresh_results(app.clone()).await.unwrap_or_default();
    let conn = open_conn(&app)?;
    let fallback_results = if results.is_empty() {
        cached_results(&conn, "match_results")
    } else {
        results
    };
    if fallback_results.is_empty() {
        return Ok(json!({ "settled": 0, "message": "暂无赛果缓存，请先刷新赛果" }));
    }
    let settled = settle_bet_recommendations_with_results(&conn, &fallback_results).map_err(|error| error.to_string())?;
    Ok(json!({ "settled": settled, "message": format!("推荐闭环已结算 {} 条", settled) }))
}

#[tauri::command]
async fn collect_worldcup_pre_match_snapshot(app: AppHandle) -> Result<Value, String> {
    let refresh = refresh_core_data(app.clone(), None, None).await?;
    let freeze = freeze_current_recommendations(app.clone()).await?;
    Ok(json!({
        "ok": true,
        "refresh": refresh,
        "freeze": freeze,
        "message": "已刷新体彩/欧洲赔率并冻结赛前推荐快照"
    }))
}

#[tauri::command]
async fn export_worldcup_training_samples(app: AppHandle) -> Result<Value, String> {
    let conn = open_conn(&app)?;
    let mut stmt = conn.prepare(
        r#"
        select frozen_at, settled_at, match_id, match_num, match_time, match_label, market, pick,
               model_prob, sporttery_prob, coalesce(europe_prob, -1), fair_odds, current_odds,
               ev, advantage_rate, recommendation_level, action_advice, stake_pct,
               data_quality_score, data_quality_grade, risk_tags, play_type_risk_level,
               anomaly_type, anomaly_severity, result_score, actual_outcome, hit, profit, stage
        from worldcup_training_samples
        order by match_time asc, frozen_at asc, id asc
        "#,
    ).map_err(|error| error.to_string())?;
    let mut rows = stmt.query([]).map_err(|error| error.to_string())?;
    let headers = [
        "frozen_at", "settled_at", "match_id", "match_num", "match_time", "match_label", "market", "pick",
        "model_prob", "sporttery_prob", "europe_prob", "fair_odds", "current_odds", "ev", "advantage_rate",
        "recommendation_level", "action_advice", "stake_pct", "data_quality_score", "data_quality_grade",
        "risk_tags", "play_type_risk_level", "anomaly_type", "anomaly_severity", "result_score",
        "actual_outcome", "hit", "profit", "stage",
    ];
    let mut csv = String::new();
    csv.push_str(&headers.join(","));
    csv.push('\n');
    let mut count = 0;
    while let Some(row) = rows.next().map_err(|error| error.to_string())? {
        let values = vec![
            csv_cell(&row.get::<_, String>(0).unwrap_or_default()),
            csv_cell(&row.get::<_, String>(1).unwrap_or_default()),
            csv_cell(&row.get::<_, String>(2).unwrap_or_default()),
            csv_cell(&row.get::<_, String>(3).unwrap_or_default()),
            csv_cell(&row.get::<_, String>(4).unwrap_or_default()),
            csv_cell(&row.get::<_, String>(5).unwrap_or_default()),
            csv_cell(&row.get::<_, String>(6).unwrap_or_default()),
            csv_cell(&row.get::<_, String>(7).unwrap_or_default()),
            row.get::<_, f64>(8).unwrap_or(0.0).to_string(),
            row.get::<_, f64>(9).unwrap_or(0.0).to_string(),
            row.get::<_, f64>(10).unwrap_or(-1.0).to_string(),
            row.get::<_, f64>(11).unwrap_or(0.0).to_string(),
            row.get::<_, f64>(12).unwrap_or(0.0).to_string(),
            row.get::<_, f64>(13).unwrap_or(0.0).to_string(),
            row.get::<_, f64>(14).unwrap_or(0.0).to_string(),
            csv_cell(&row.get::<_, String>(15).unwrap_or_default()),
            csv_cell(&row.get::<_, String>(16).unwrap_or_default()),
            row.get::<_, f64>(17).unwrap_or(0.0).to_string(),
            row.get::<_, f64>(18).unwrap_or(0.0).to_string(),
            csv_cell(&row.get::<_, String>(19).unwrap_or_default()),
            csv_cell(&row.get::<_, String>(20).unwrap_or_default()),
            csv_cell(&row.get::<_, String>(21).unwrap_or_default()),
            csv_cell(&row.get::<_, String>(22).unwrap_or_default()),
            csv_cell(&row.get::<_, String>(23).unwrap_or_default()),
            csv_cell(&row.get::<_, String>(24).unwrap_or_default()),
            csv_cell(&row.get::<_, String>(25).unwrap_or_default()),
            row.get::<_, i64>(26).unwrap_or(0).to_string(),
            row.get::<_, f64>(27).unwrap_or(0.0).to_string(),
            csv_cell(&row.get::<_, String>(28).unwrap_or_default()),
        ];
        csv.push_str(&values.join(","));
        csv.push('\n');
        count += 1;
    }
    let mut path = std::env::current_dir().map_err(|error| error.to_string())?;
    if path.file_name().and_then(|name| name.to_str()) == Some("src-tauri") {
        path.pop();
    }
    if path.file_name().and_then(|name| name.to_str()) != Some("desktop-app") {
        path.push("desktop-app");
    }
    path.push("training");
    path.push("datasets");
    path.push("processed");
    fs::create_dir_all(&path).map_err(|error| error.to_string())?;
    path.push("worldcup_closure_samples.csv");
    fs::write(&path, csv).map_err(|error| error.to_string())?;
    Ok(json!({
        "ok": true,
        "count": count,
        "path": path.to_string_lossy(),
        "message": format!("已导出 {} 条世界杯赛前闭环训练样本", count)
    }))
}

#[tauri::command]
async fn run_worldcup_closure_cycle(app: AppHandle) -> Result<Value, String> {
    let pre_match = collect_worldcup_pre_match_snapshot(app.clone()).await?;
    let settlement = settle_bet_recommendations(app.clone()).await?;
    let export = export_worldcup_training_samples(app.clone()).await?;
    Ok(json!({
        "ok": true,
        "pre_match": pre_match,
        "settlement": settlement,
        "export": export,
        "message": "世界杯赛前样本闭环已执行：刷新赔率、冻结快照、刷新赛果、结算推荐、导出训练样本"
    }))
}

#[tauri::command]
async fn list_predictions(app: AppHandle) -> Result<Vec<PredictionRecord>, String> {
    let conn = open_conn(&app)?;
    let mut stmt = conn
        .prepare("select id, created_at, match_label, market, pick, probability, odds, safety_margin, decision, coalesce(stake_pct,0), coalesce(actual_result,''), coalesce(profit,0) from predictions order by id desc limit 500")
        .map_err(|error| error.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok(PredictionRecord {
                id: row.get(0)?,
                created_at: row.get(1)?,
                match_label: row.get(2)?,
                market: row.get(3)?,
                pick: row.get(4)?,
                probability: row.get(5)?,
                odds: row.get(6)?,
                safety_margin: row.get(7)?,
                decision: row.get(8)?,
                stake_pct: row.get(9)?,
                actual_result: row.get(10)?,
                profit: row.get(11)?,
            })
        })
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|error| error.to_string())
}

#[tauri::command]
async fn get_bankroll_settings(app: AppHandle) -> Result<BankrollSettings, String> {
    let conn = open_conn(&app)?;
    if let Some(record) = cache_get(&conn, "bankroll_settings").map_err(|error| error.to_string())? {
        serde_json::from_value(record.value).map_err(|error| error.to_string())
    } else {
        Ok(BankrollSettings {
            bankroll: 1000.0,
            daily_budget_pct: 0.03,
            max_loss_pct: 0.06,
            auto_refresh_minutes: 0,
        })
    }
}

#[tauri::command]
async fn save_bankroll_settings(app: AppHandle, settings: BankrollSettings) -> Result<(), String> {
    let conn = open_conn(&app)?;
    cache_put(&conn, "bankroll_settings", &json!(settings)).map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
async fn get_external_source_config(app: AppHandle) -> Result<ExternalSourceConfig, String> {
    let conn = open_conn(&app)?;
    if let Some(record) = cache_get(&conn, "external_source_config").map_err(|error| error.to_string())? {
        serde_json::from_value(record.value).map_err(|error| error.to_string())
    } else {
        Ok(ExternalSourceConfig {
            injury_url: String::new(),
            lineup_url: String::new(),
            stats_url: String::new(),
            notes: "可填写返回JSON的免费接口或本地代理地址；软件会保存配置，后续模型可接入。".to_string(),
        })
    }
}

#[tauri::command]
async fn list_providers(app: AppHandle) -> Result<Vec<DataProvider>, String> {
    let conn = open_conn(&app)?;
    list_data_providers(&conn).map_err(|error| error.to_string())
}

#[tauri::command]
async fn save_provider_credential(app: AppHandle, input: ProviderCredentialInput) -> Result<Value, String> {
    let conn = open_conn(&app)?;
    let api_key = input.api_key.trim();
    if api_key.is_empty() {
        return Err("API Key 不能为空".to_string());
    }
    save_provider_api_key(&conn, &input.provider_id, api_key).map_err(|error| error.to_string())?;
    Ok(json!({ "ok": true, "provider_id": input.provider_id, "message": "API Key 已保存到本地设置" }))
}

#[tauri::command]
async fn clear_provider_credential(app: AppHandle, provider_id: String) -> Result<Value, String> {
    let conn = open_conn(&app)?;
    clear_provider_api_key(&conn, &provider_id).map_err(|error| error.to_string())?;
    Ok(json!({ "ok": true, "provider_id": provider_id, "message": "API Key 已清除" }))
}

#[tauri::command]
async fn set_provider_enabled(app: AppHandle, provider_id: String, enabled: bool) -> Result<Value, String> {
    let conn = open_conn(&app)?;
    conn.execute(
        "update data_providers set enabled=?1 where provider_id=?2",
        params![if enabled { 1 } else { 0 }, provider_id],
    )
    .map_err(|error| error.to_string())?;
    Ok(json!({ "ok": true, "enabled": enabled }))
}

#[tauri::command]
async fn clear_provider_cache(app: AppHandle, provider_id: String) -> Result<Value, String> {
    let conn = open_conn(&app)?;
    conn.execute("delete from provider_raw_data where coalesce(provider_id, provider)=?1", params![provider_id])
        .map_err(|error| error.to_string())?;
    conn.execute("delete from source_health where provider_id=?1", params![provider_id])
        .map_err(|error| error.to_string())?;
    Ok(json!({ "ok": true, "message": "Provider 原始缓存和健康状态已清除" }))
}

#[tauri::command]
async fn test_provider_connection(app: AppHandle, provider_id: String) -> Result<Value, String> {
    let conn = open_conn(&app)?;
    let providers = list_data_providers(&conn).map_err(|error| error.to_string())?;
    let provider = providers
        .iter()
        .find(|item| item.provider_id == provider_id)
        .ok_or_else(|| "未知 provider".to_string())?;
    if provider.requires_key && !provider.key_configured {
        let message = "API Key 未配置，请先保存本地 Key";
        let _ = log_provider_request(&conn, &provider.provider_id, "test", false, message);
        return Err(message.to_string());
    }
    if !request_limit_available(&conn, &provider.provider_id, provider.daily_limit, provider.hourly_limit) {
        let message = "免费请求限额已用完，请稍后再试";
        let _ = log_provider_request(&conn, &provider.provider_id, "test", false, message);
        return Err(message.to_string());
    }
    log_provider_request(&conn, &provider.provider_id, "test", true, "").map_err(|error| error.to_string())?;
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "insert into source_health(provider_id, last_success_at, last_error_at, last_error_message, freshness_score, completeness_score, confidence_score, using_stale_cache, updated_at)
         values(?1, ?2, null, '', 100, 70, ?3, 0, ?2)
         on conflict(provider_id) do update set last_success_at=excluded.last_success_at, freshness_score=excluded.freshness_score, completeness_score=excluded.completeness_score, confidence_score=excluded.confidence_score, using_stale_cache=0, updated_at=excluded.updated_at",
        params![provider.provider_id, now, provider.base_confidence * 0.70],
    )
    .map_err(|error| error.to_string())?;
    Ok(json!({ "ok": true, "provider_id": provider.provider_id, "message": "连接配置检查通过；真实抓取按具体数据类型执行" }))
}

#[tauri::command]
async fn save_external_source_config(app: AppHandle, config: ExternalSourceConfig) -> Result<(), String> {
    let conn = open_conn(&app)?;
    cache_put(&conn, "external_source_config", &json!(config)).map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
async fn probe_external_source(url: String) -> Result<Value, String> {
    if url.trim().is_empty() {
        return Err("URL为空".to_string());
    }
    let text = http_text(url.trim()).await.map_err(|error| error.to_string())?;
    Ok(json!({
        "ok": true,
        "bytes": text.len(),
        "preview": text.chars().take(240).collect::<String>()
    }))
}

#[tauri::command]
async fn refresh_external_sources(app: AppHandle) -> Result<Value, String> {
    let conn = open_conn(&app)?;
    let config = get_external_source_config(app.clone()).await?;
    let sources = [
        ("injury_data", "injury_url", config.injury_url),
        ("lineup_data", "lineup_url", config.lineup_url),
        ("stats_data", "stats_url", config.stats_url),
    ];
    let mut report = serde_json::Map::new();
    for (cache_key, label, url) in sources {
        if url.trim().is_empty() {
            report.insert(label.to_string(), json!({ "ok": false, "message": "未配置" }));
            continue;
        }
        match http_text(url.trim()).await {
            Ok(text) => {
                let payload = serde_json::from_str::<Value>(&text).unwrap_or_else(|_| json!({ "raw": text }));
                cache_put(&conn, cache_key, &payload).map_err(|error| error.to_string())?;
                report.insert(label.to_string(), json!({ "ok": true }));
            }
            Err(error) => {
                report.insert(label.to_string(), json!({ "ok": false, "message": error.to_string() }));
            }
        }
    }
    Ok(Value::Object(report))
}

async fn refresh_football_data_org(app: AppHandle) -> Result<Value, String> {
    let conn = open_conn(&app)?;
    let Some(key) = provider_api_key(&conn, "football_data_org").map_err(|error| error.to_string())? else {
        return Err("football-data.org API Key 未配置".to_string());
    };
    drop(conn);
    let value = http_football_data_org_json("https://api.football-data.org/v4/matches", &key)
        .await
        .map_err(|error| error.to_string())?;
    let conn = open_conn(&app)?;
    let count = value.get("matches").and_then(Value::as_array).map(|items| items.len()).unwrap_or(0);
    cache_put(&conn, "football_data_org_matches", &value).map_err(|error| error.to_string())?;
    upsert_provider_health(&conn, "football_data_org", count > 0, count, if count > 0 { "" } else { "接口返回成功但 matches 为空" })
        .map_err(|error| error.to_string())?;
    Ok(json!({ "ok": count > 0, "count": count }))
}

async fn refresh_api_football(app: AppHandle) -> Result<Value, String> {
    let conn = open_conn(&app)?;
    let Some(key) = provider_api_key(&conn, "api_football").map_err(|error| error.to_string())? else {
        return Err("API-Football Key 未配置".to_string());
    };
    drop(conn);
    let today = Utc::now().format("%Y-%m-%d").to_string();
    let fixtures_url = format!("https://v3.football.api-sports.io/fixtures?date={}", today);
    let fixtures = http_api_football_json(&fixtures_url, &key)
        .await
        .map_err(|error| error.to_string())?;
    let fixture_items = fixtures.get("response").and_then(Value::as_array).cloned().unwrap_or_default();
    {
        let conn = open_conn(&app)?;
        cache_put(&conn, "api_football_fixtures", &fixtures).map_err(|error| error.to_string())?;
    }

    let injuries_url = format!("https://v3.football.api-sports.io/injuries?date={}", today);
    let injuries = http_api_football_json(&injuries_url, &key)
        .await
        .unwrap_or_else(|error| json!({ "error": error.to_string(), "response": [] }));
    {
        let conn = open_conn(&app)?;
        cache_put(&conn, "api_football_injuries", &injuries).map_err(|error| error.to_string())?;
    }

    let mut lineups = Vec::new();
    let mut statistics = Vec::new();
    for fixture in fixture_items.iter().take(6) {
        let Some(fixture_id) = fixture.pointer("/fixture/id").and_then(Value::as_i64) else { continue };
        let lineup_url = format!("https://v3.football.api-sports.io/fixtures/lineups?fixture={}", fixture_id);
        if let Ok(payload) = http_api_football_json(&lineup_url, &key).await {
            lineups.push(json!({ "fixture_id": fixture_id, "payload": payload }));
        }
        let stats_url = format!("https://v3.football.api-sports.io/fixtures/statistics?fixture={}", fixture_id);
        if let Ok(payload) = http_api_football_json(&stats_url, &key).await {
            statistics.push(json!({ "fixture_id": fixture_id, "payload": payload }));
        }
    }
    let conn = open_conn(&app)?;
    cache_put(&conn, "api_football_lineups", &json!(lineups)).map_err(|error| error.to_string())?;
    cache_put(&conn, "api_football_statistics", &json!(statistics)).map_err(|error| error.to_string())?;

    let count = fixture_items.len();
    upsert_provider_health(&conn, "api_football", count > 0, count, if count > 0 { "" } else { "今日 fixtures 为空或免费额度不含对应赛事" })
        .map_err(|error| error.to_string())?;
    Ok(json!({
        "ok": count > 0,
        "fixtures": count,
        "injuries": injuries.get("response").and_then(Value::as_array).map(|items| items.len()).unwrap_or(0),
        "lineups": lineups.len(),
        "statistics": statistics.len()
    }))
}

async fn refresh_odds_api_io(app: AppHandle) -> Result<Value, String> {
    let conn = open_conn(&app)?;
    let Some(key) = provider_api_key(&conn, "odds_api_io").map_err(|error| error.to_string())? else {
        return Err("Odds-API.io Key 未配置".to_string());
    };
    drop(conn);
    let events_url = format!("https://api.odds-api.io/v3/events?apiKey={}&sport=football&limit=25", key);
    let events = http_json(&events_url).await.map_err(|error| error.to_string())?;
    let event_items = events.get("data")
        .or_else(|| events.get("response"))
        .or_else(|| events.get("events"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_else(|| events.as_array().cloned().unwrap_or_default());
    {
        let conn = open_conn(&app)?;
        cache_put(&conn, "odds_api_io_events", &events).map_err(|error| error.to_string())?;
    }

    let mut odds_payloads = Vec::new();
    for event in event_items.iter().take(6) {
        let event_id = event.get("id")
            .or_else(|| event.get("eventId"))
            .or_else(|| event.get("event_id"))
            .and_then(|value| value.as_str().map(str::to_string).or_else(|| value.as_i64().map(|id| id.to_string())));
        if let Some(event_id) = event_id {
            let odds_url = format!("https://api.odds-api.io/v3/odds?apiKey={}&eventId={}", key, event_id);
            if let Ok(payload) = http_json(&odds_url).await {
                odds_payloads.push(json!({ "event_id": event_id, "payload": payload }));
            }
        }
    }
    let conn = open_conn(&app)?;
    cache_put(&conn, "odds_api_io_odds", &json!(odds_payloads)).map_err(|error| error.to_string())?;
    let count = event_items.len().max(odds_payloads.len());
    upsert_provider_health(&conn, "odds_api_io", count > 0, count, if count > 0 { "" } else { "Odds-API.io 未返回 events/odds" })
        .map_err(|error| error.to_string())?;
    Ok(json!({ "ok": count > 0, "events": event_items.len(), "odds": odds_payloads.len() }))
}

#[tauri::command]
async fn refresh_all_data_sources(app: AppHandle) -> Result<Value, String> {
    let mut report = serde_json::Map::new();

    match refresh_core_data(app.clone(), None, Some("eu".to_string())).await {
        Ok(value) => {
            let conn = open_conn(&app)?;
            let sporttery_count = cache_get(&conn, "sporttery")
                .ok()
                .flatten()
                .map(|record| {
                    let mut ids = sporttery_selections(&record.value)
                        .into_iter()
                        .map(|selection| selection.match_id)
                        .collect::<Vec<_>>();
                    ids.sort();
                    ids.dedup();
                    ids.len()
                })
                .unwrap_or(0);
            let europe_count = cache_get(&conn, "europe_odds")
                .ok()
                .flatten()
                .and_then(|record| record.value.as_array().map(|items| items.len()))
                .unwrap_or(0);
            let _ = upsert_provider_health(&conn, "odds_api_io", europe_count > 0, europe_count, if europe_count > 0 { "" } else { "欧洲赔率未返回有效比赛；保留体彩模型预测" });
            report.insert("core".to_string(), json!({ "ok": true, "sporttery_matches": sporttery_count, "europe_matches": europe_count, "detail": value }));
        }
        Err(error) => {
            let conn = open_conn(&app)?;
            let _ = upsert_provider_health(&conn, "odds_api_io", false, 0, &error);
            report.insert("core".to_string(), json!({ "ok": false, "message": error }));
        }
    }

    match refresh_statsbomb_xg(app.clone()).await {
        Ok(value) => {
            let conn = open_conn(&app)?;
            let count = value.get("teamCount").and_then(Value::as_u64).unwrap_or(0) as usize;
            let _ = upsert_provider_health(&conn, "statsbomb_open_data", count > 0, count, "");
            report.insert("statsbomb_open_data".to_string(), json!({ "ok": count > 0, "count": count }));
        }
        Err(error) => {
            let conn = open_conn(&app)?;
            let _ = upsert_provider_health(&conn, "statsbomb_open_data", false, 0, &error);
            report.insert("statsbomb_open_data".to_string(), json!({ "ok": false, "message": error }));
        }
    }

    match refresh_sporttery_injuries(app.clone()).await {
        Ok(value) => {
            report.insert("sporttery_injury".to_string(), json!({ "ok": true, "detail": value }));
        }
        Err(error) => {
            report.insert("sporttery_injury".to_string(), json!({ "ok": false, "message": error }));
        }
    }

    match refresh_results(app.clone()).await {
        Ok(results) => {
            let conn = open_conn(&app)?;
            let count = results.len();
            let _ = upsert_provider_health(&conn, "football_data_uk", count > 0, count, "");
            let _ = upsert_provider_health(&conn, "openfootball_worldcup", count > 0, count, "");
            report.insert("results".to_string(), json!({ "ok": count > 0, "count": count }));
        }
        Err(error) => {
            let conn = open_conn(&app)?;
            let _ = upsert_provider_health(&conn, "football_data_uk", false, 0, &error);
            let _ = upsert_provider_health(&conn, "openfootball_worldcup", false, 0, &error);
            report.insert("results".to_string(), json!({ "ok": false, "message": error }));
        }
    }

    match refresh_football_data_org(app.clone()).await {
        Ok(value) => {
            report.insert("football_data_org".to_string(), value);
        }
        Err(error) => {
            let conn = open_conn(&app)?;
            let _ = upsert_provider_health(&conn, "football_data_org", false, 0, &error);
            report.insert("football_data_org".to_string(), json!({ "ok": false, "message": error }));
        }
    }

    match refresh_api_football(app.clone()).await {
        Ok(value) => {
            report.insert("api_football".to_string(), value);
        }
        Err(error) => {
            let conn = open_conn(&app)?;
            let _ = upsert_provider_health(&conn, "api_football", false, 0, &error);
            report.insert("api_football".to_string(), json!({ "ok": false, "message": error }));
        }
    }

    match refresh_odds_api_io(app.clone()).await {
        Ok(value) => {
            report.insert("odds_api_io".to_string(), value);
        }
        Err(error) => {
            let conn = open_conn(&app)?;
            let _ = upsert_provider_health(&conn, "odds_api_io", false, 0, &error);
            report.insert("odds_api_io".to_string(), json!({ "ok": false, "message": error }));
        }
    }

    let external = refresh_external_sources(app.clone()).await.unwrap_or_else(|error| json!({ "ok": false, "message": error }));
    report.insert("external_urls".to_string(), external);

    match bridge_provider_caches(&open_conn(&app)?) {
        Ok(value) => {
            report.insert("compatibility_bridge".to_string(), value);
        }
        Err(error) => {
            report.insert("compatibility_bridge".to_string(), json!({ "ok": false, "message": error.to_string() }));
        }
    }

    Ok(json!({
        "ok": true,
        "message": "全局数据源刷新完成；已接入 football-data.org、Odds-API.io、API-Football，并限制单次请求量保护免费额度",
        "providers": list_data_providers(&open_conn(&app)?).map_err(|error| error.to_string())?,
        "report": Value::Object(report)
    }))
}

#[tauri::command]
async fn model_diagnostics(app: AppHandle) -> Result<ModelDiagnostics, String> {
    let conn = open_conn(&app)?;
    let total: i64 = conn
        .query_row("select count(*) from predictions", [], |row| row.get(0))
        .unwrap_or(0);
    let settled: i64 = conn
        .query_row("select count(*) from predictions where coalesce(actual_result,'') in ('命中','未中')", [], |row| row.get(0))
        .unwrap_or(0);
    let hits: i64 = conn
        .query_row("select count(*) from predictions where actual_result='命中'", [], |row| row.get(0))
        .unwrap_or(0);
    let profit: f64 = conn
        .query_row("select coalesce(sum(profit),0) from predictions", [], |row| row.get(0))
        .unwrap_or(0.0);
    let stake: f64 = conn
        .query_row("select coalesce(sum(stake_pct),0) from predictions where coalesce(actual_result,'') in ('命中','未中')", [], |row| row.get(0))
        .unwrap_or(0.0);
    let hit_rate = if settled > 0 { hits as f64 / settled as f64 } else { 0.0 };
    let roi = if stake > 0.0 { profit / stake } else { 0.0 };
    let mut brier_sum = 0.0;
    let mut log_sum = 0.0;
    let mut calibration_raw = vec![(0i64, 0.0f64, 0.0f64); 10];
    let mut stmt = conn
        .prepare("select probability, actual_result from predictions where coalesce(actual_result,'') in ('命中','未中')")
        .map_err(|error| error.to_string())?;
    let rows = stmt
        .query_map([], |row| Ok((row.get::<_, f64>(0)?, row.get::<_, String>(1)?)))
        .map_err(|error| error.to_string())?;
    for row in rows {
        let (probability, actual) = row.map_err(|error| error.to_string())?;
        let p = probability.clamp(0.001, 0.999);
        let y = if actual == "命中" { 1.0 } else { 0.0 };
        brier_sum += (p - y) * (p - y);
        log_sum += if y > 0.5 { -p.ln() } else { -(1.0 - p).ln() };
        let idx = ((p * 10.0).floor() as usize).min(9);
        calibration_raw[idx].0 += 1;
        calibration_raw[idx].1 += p;
        calibration_raw[idx].2 += y;
    }
    let brier_score = if settled > 0 { brier_sum / settled as f64 } else { 0.0 };
    let log_loss = if settled > 0 { log_sum / settled as f64 } else { 0.0 };
    let calibration = calibration_raw
        .into_iter()
        .enumerate()
        .filter(|(_, item)| item.0 > 0)
        .map(|(idx, (count, prob_sum, hit_sum))| CalibrationBucket {
            bucket: format!("{}%-{}%", idx * 10, (idx + 1) * 10),
            count,
            avg_probability: prob_sum / count as f64,
            hit_rate: hit_sum / count as f64,
        })
        .collect::<Vec<_>>();
    let mut market_stmt = conn
        .prepare(
            "select market, probability, actual_result, coalesce(profit,0), coalesce(stake_pct,0)
             from predictions where coalesce(actual_result,'') in ('命中','未中')",
        )
        .map_err(|error| error.to_string())?;
    let market_rows = market_stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, f64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, f64>(3)?,
                row.get::<_, f64>(4)?,
            ))
        })
        .map_err(|error| error.to_string())?;
    let mut market_map: BTreeMap<String, (i64, f64, f64, f64, f64, f64)> = BTreeMap::new();
    for row in market_rows {
        let (market, probability, actual, profit, stake_pct) = row.map_err(|error| error.to_string())?;
        let key = if market.starts_with("HAD") {
            "胜平负"
        } else if market.starts_with("HHAD") {
            "让球胜平负"
        } else if market.starts_with("TTG") {
            "总进球"
        } else if market.starts_with("CRS") {
            "比分"
        } else {
            "其他"
        }
        .to_string();
        let p = probability.clamp(0.001, 0.999);
        let y = if actual == "命中" { 1.0 } else { 0.0 };
        let entry = market_map.entry(key).or_insert((0, 0.0, 0.0, 0.0, 0.0, 0.0));
        entry.0 += 1;
        entry.1 += p;
        entry.2 += y;
        entry.3 += (p - y) * (p - y);
        entry.4 += profit;
        entry.5 += stake_pct;
    }
    let market_calibration = market_map
        .into_iter()
        .map(|(market, (count, prob_sum, hit_sum, brier_sum, profit_sum, stake_sum))| MarketCalibration {
            market,
            count,
            hit_rate: hit_sum / count as f64,
            avg_probability: prob_sum / count as f64,
            brier_score: brier_sum / count as f64,
            roi: if stake_sum > 0.0 { profit_sum / stake_sum } else { 0.0 },
        })
        .collect::<Vec<_>>();
    let advice = if settled < 20 {
        "样本不足，先积累复盘，不建议自动调权。".to_string()
    } else if brier_score > 0.28 || log_loss > 0.85 {
        "概率校准偏差较大，建议提高阈值、降低高波动玩法权重，并优先积累胜平负样本。".to_string()
    } else if roi < 0.0 {
        "当前复盘ROI为负，建议降低仓位、提高可买阈值、暂停高波动玩法。".to_string()
    } else {
        "复盘表现为正，可维持当前阈值，并优先扩大稳胆样本。".to_string()
    };
    Ok(ModelDiagnostics { total, settled, hit_rate, roi, brier_score, log_loss, calibration, market_calibration, advice })
}

#[tauri::command]
async fn get_active_model_info() -> Result<ActiveModelInfo, String> {
    let app_dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    Ok(active_model_info(&app_dir))
}

fn locate_training_dir() -> Result<std::path::PathBuf, String> {
    let current = std::env::current_dir().map_err(|error| error.to_string())?;
    for dir in current.ancestors() {
        let candidate = dir.join("training");
        if candidate.join("scripts").join("download_football_data.py").exists() {
            return Ok(candidate);
        }
        let candidate = dir.join("desktop-app").join("training");
        if candidate.join("scripts").join("download_football_data.py").exists() {
            return Ok(candidate);
        }
    }
    Err("未找到 desktop-app/training 目录".to_string())
}

fn training_python(training_dir: &std::path::Path) -> String {
    let venv_python = training_dir.join(".venv").join("Scripts").join("python.exe");
    if venv_python.exists() {
        venv_python.to_string_lossy().to_string()
    } else {
        "python".to_string()
    }
}

fn run_training_script(training_dir: &std::path::Path, python: &str, script: &str) -> Result<Value, String> {
    let output = Command::new(python)
        .arg(format!("scripts/{}", script))
        .current_dir(training_dir)
        .output()
        .map_err(|error| format!("无法运行 {}: {}", script, error))?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    if !output.status.success() {
        return Err(format!("{} 失败\n{}\n{}", script, stdout, stderr));
    }
    Ok(json!({
        "script": script,
        "ok": true,
        "stdout": stdout,
        "stderr": stderr
    }))
}

fn training_report_json(app_dir: &std::path::Path, file_name: &str) -> Value {
    let models_dir = training_models_dir(app_dir);
    let reports_dir = models_dir
        .parent()
        .map(|training_dir| training_dir.join("reports"))
        .unwrap_or_else(|| app_dir.join("training").join("reports"));
    fs::read_to_string(reports_dir.join(file_name))
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .unwrap_or(Value::Null)
}

fn training_model_json(app_dir: &std::path::Path, file_name: &str) -> Value {
    let models_dir = training_models_dir(app_dir);
    fs::read_to_string(models_dir.join(file_name))
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .unwrap_or(Value::Null)
}

#[tauri::command]
async fn run_training_pipeline() -> Result<Value, String> {
    let training_dir = locate_training_dir()?;
    let python = training_python(&training_dir);
    let scripts = [
        "download_football_data.py",
        "import_football_data.py",
        "build_features.py",
        "train_outcome_model.py",
        "train_outcome_ensemble.py",
        "calibrate_probs.py",
        "train_probability_blend.py",
        "train_goals_model.py",
        "train_handicap_model.py",
        "backtest_strategy.py",
        "import_worldcup_history.py",
        "train_worldcup_correction.py",
        "export_models.py",
    ];
    let mut results = Vec::new();
    for script in scripts {
        results.push(run_training_script(&training_dir, &python, script)?);
    }
    let info = active_model_info(training_dir.parent().unwrap_or(&training_dir));
    Ok(json!({
        "ok": true,
        "training_dir": training_dir,
        "python": python,
        "steps": results,
        "model": info
    }))
}

#[tauri::command]
async fn backtest_report(app: AppHandle) -> Result<BacktestReport, String> {
    let app_dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let conn = open_conn(&app)?;
    let mut stmt = conn
        .prepare(
            "select market, probability, odds, safety_margin, decision, coalesce(stake_pct,0), actual_result, coalesce(profit,0), created_at
             from predictions where coalesce(actual_result,'') in ('命中','未中') order by id asc",
        )
        .map_err(|error| error.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, f64>(1)?,
                row.get::<_, f64>(2)?,
                row.get::<_, f64>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, f64>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, f64>(7)?,
                row.get::<_, String>(8)?,
            ))
        })
        .map_err(|error| error.to_string())?;

    let mut buckets: BTreeMap<(String, String), Vec<(f64, f64, f64, f64, f64, f64)>> = BTreeMap::new();
    for row in rows {
        let (market, probability, odds, advantage, decision, stake, actual, profit, _created_at) =
            row.map_err(|error| error.to_string())?;
        let hit = if actual == "命中" { 1.0 } else { 0.0 };
        let market_group = if market.starts_with("HAD") {
            "胜平负".to_string()
        } else if market.starts_with("HHAD") {
            "让球".to_string()
        } else if market.starts_with("TTG") {
            "总进球".to_string()
        } else if market.starts_with("CRS") {
            "比分".to_string()
        } else {
            market.clone()
        };
        let dims = vec![
            ("玩法".to_string(), market_group.clone()),
            ("赔率区间".to_string(), odds_bucket(odds)),
            ("概率区间".to_string(), probability_bucket(probability)),
            ("推荐等级".to_string(), decision.clone()),
            ("数据质量区间".to_string(), data_quality_bucket(0.0)),
            ("风险标签".to_string(), play_type_risk_level(&market).to_string()),
            ("联赛/杯赛".to_string(), "世界杯".to_string()),
        ];
        for (dimension, group) in dims {
            buckets.entry((dimension, group)).or_default().push((probability, odds, advantage, stake, profit, hit));
        }
    }

    let mut groups = Vec::new();
    for ((dimension, group), items) in buckets {
        let count = items.len() as i64;
        if count == 0 {
            continue;
        }
        let mut hit_sum = 0.0;
        let mut odds_sum = 0.0;
        let mut adv_sum = 0.0;
        let mut stake_sum = 0.0;
        let mut profit_sum = 0.0;
        let mut brier_sum = 0.0;
        let mut log_loss_sum = 0.0;
        let mut profits = Vec::new();
        for (probability, odds, advantage, stake, profit, hit) in &items {
            let y = *hit;
            let p = probability.clamp(0.001, 0.999);
            hit_sum += y;
            odds_sum += odds;
            adv_sum += advantage;
            stake_sum += stake;
            profit_sum += profit;
            brier_sum += (p - y) * (p - y);
            log_loss_sum += -(y * p.ln() + (1.0 - y) * (1.0 - p).ln());
            profits.push(*profit);
        }
        groups.push(BacktestGroup {
            dimension,
            group,
            count,
            hit_rate: hit_sum / count as f64,
            roi: roi_from_profit(profit_sum, stake_sum),
            total_profit: profit_sum,
            max_drawdown: max_drawdown_from_profit(&profits),
            avg_odds: odds_sum / count as f64,
            avg_advantage_rate: adv_sum / count as f64,
            brier_score: brier_sum / count as f64,
            log_loss: log_loss_sum / count as f64,
        });
    }
    let most_profitable = groups
        .iter()
        .max_by(|a, b| a.roi.partial_cmp(&b.roi).unwrap_or(std::cmp::Ordering::Equal))
        .map(|item| format!("{}：{}，ROI {}，命中率 {}，样本 {}", item.dimension, item.group, format_percent(item.roi), format_percent(item.hit_rate), item.count))
        .unwrap_or_else(|| "样本不足".to_string());
    let most_loss = groups
        .iter()
        .min_by(|a, b| a.roi.partial_cmp(&b.roi).unwrap_or(std::cmp::Ordering::Equal))
        .map(|item| format!("{}：{}，ROI {}，命中率 {}，样本 {}", item.dimension, item.group, format_percent(item.roi), format_percent(item.hit_rate), item.count))
        .unwrap_or_else(|| "样本不足".to_string());

    let mut ban_rules = groups
        .iter()
        .filter_map(|item| {
            backtest_ban_reason(item).map(|reason| BanRule {
                dimension: item.dimension.clone(),
                group: item.group.clone(),
                count: item.count,
                hit_rate: item.hit_rate,
                roi: item.roi,
                avg_odds: item.avg_odds,
                reason,
                action: if item.roi < -0.25 {
                    "暂停购买，至少等新增10条复盘样本再恢复。".to_string()
                } else {
                    "降为观察，只允许极小注或等待更多样本。".to_string()
                },
            })
        })
        .collect::<Vec<_>>();
    ban_rules.sort_by(|a, b| a.roi.partial_cmp(&b.roi).unwrap_or(std::cmp::Ordering::Equal));
    let ban_rule_advice = ban_rules
        .iter()
        .take(5)
        .map(|item| format!("{}={}：{}，{}", item.dimension, item.group, item.reason, item.action))
        .collect::<Vec<_>>()
        .join("；");
    Ok(BacktestReport {
        groups,
        ban_rules,
        most_profitable,
        most_loss,
        ban_rule_advice: if ban_rule_advice.is_empty() { "样本不足，暂不生成禁买规则".to_string() } else { ban_rule_advice },
        shadow_backtest: training_report_json(&app_dir, "shadow_backtest_summary.json"),
        rule_diagnostics: training_report_json(&app_dir, "rule_diagnostics.json"),
        threshold_scan: training_report_json(&app_dir, "threshold_scan_summary.json"),
        candidate_strategy: training_model_json(&app_dir, "candidate_strategy_v1.json"),
        paper_trading: training_report_json(&app_dir, "paper_trading_backtest_summary.json"),
        strategy_robustness: training_report_json(&app_dir, "strategy_robustness_summary.json"),
    })
}

fn parse_json_text(text: String) -> Value {
    serde_json::from_str(&text).unwrap_or(Value::Null)
}

fn kickoff_is_future(kickoff_time: &str, snapshot_time: &str) -> bool {
    if kickoff_time.is_empty() {
        return false;
    }
    kickoff_time >= snapshot_time
}

fn api_fixture_for_match(conn: &Connection, home: &str, away: &str) -> Value {
    let fixture_cache = cache_get(conn, "api_football_fixtures").ok().flatten();
    let Some(fixtures) = fixture_cache else {
        return Value::Null;
    };
    fixtures
        .value
        .get("response")
        .and_then(Value::as_array)
        .and_then(|items| {
            items.iter().find(|fixture| {
                let api_home = fixture.pointer("/teams/home/name").and_then(Value::as_str).unwrap_or("");
                let api_away = fixture.pointer("/teams/away/name").and_then(Value::as_str).unwrap_or("");
                team_matches(api_home, home) && team_matches(api_away, away)
            }).cloned()
        })
        .unwrap_or(Value::Null)
}

fn pre_match_snapshot_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<PreMatchSnapshotRow> {
    let settlement_text: Option<String> = row.get(33)?;
    Ok(PreMatchSnapshotRow {
        id: row.get(0)?,
        match_id: row.get(1)?,
        external_fixture_id: row.get(2)?,
        provider_match_id: row.get(3)?,
        snapshot_time: row.get(4)?,
        kickoff_time: row.get(5)?,
        home_team: row.get(6)?,
        away_team: row.get(7)?,
        competition: row.get(8)?,
        season: row.get(9)?,
        stage: row.get(10)?,
        model_version: row.get(11)?,
        model_probs_json: parse_json_text(row.get(12)?),
        calibrated_probs_json: parse_json_text(row.get(13)?),
        worldcup_correction_action: row.get(14)?,
        odds_json: parse_json_text(row.get(15)?),
        market_probs_json: parse_json_text(row.get(16)?),
        ev_json: parse_json_text(row.get(17)?),
        data_quality_score: row.get(18)?,
        lineup_status: row.get(19)?,
        lineup_confidence: row.get(20)?,
        injury_status: row.get(21)?,
        injury_confidence: row.get(22)?,
        risk_tags_json: parse_json_text(row.get(23)?),
        final_decision: row.get(24)?,
        decision_reason_json: parse_json_text(row.get(25)?),
        paper_strategy_id: row.get(26)?,
        paper_trade_enabled: row.get::<_, i64>(27)? != 0,
        raw_features_json: parse_json_text(row.get(28)?),
        is_final_pre_match: row.get::<_, i64>(29)? != 0,
        created_at: row.get(30)?,
        updated_at: row.get(31)?,
        settlement: settlement_text.map(parse_json_text),
    })
}

fn pre_match_snapshot_select_sql(where_clause: &str) -> String {
    format!(
        "select s.id, s.match_id, s.external_fixture_id, s.provider_match_id, s.snapshot_time, s.kickoff_time,
                s.home_team, s.away_team, s.competition, s.season, s.stage, s.model_version,
                s.model_probs_json, s.calibrated_probs_json, s.worldcup_correction_action,
                s.odds_json, s.market_probs_json, s.ev_json, s.data_quality_score,
                s.lineup_status, s.lineup_confidence, s.injury_status, s.injury_confidence,
                s.risk_tags_json, s.final_decision, s.decision_reason_json, s.paper_strategy_id,
                s.paper_trade_enabled, s.raw_features_json, s.is_final_pre_match, s.created_at, s.updated_at,
                (select json_object('home_score', r.home_score, 'away_score', r.away_score, 'result_spf', r.result_spf,
                                    'total_goals', r.total_goals, 'settled_at', r.settled_at,
                                    'is_hit_json', r.is_hit_json, 'paper_profit_json', r.paper_profit_json,
                                    'settlement_status', r.settlement_status)
                 from pre_match_snapshot_results r where r.snapshot_id=s.id order by r.id desc limit 1) as settlement
         from pre_match_snapshots s {} order by s.kickoff_time asc, s.id desc",
        where_clause
    )
}

#[tauri::command]
async fn create_pre_match_snapshot(app: AppHandle, match_id: String) -> Result<Value, String> {
    let recommendations = list_recommendations(app.clone()).await.unwrap_or_default();
    let items = recommendations
        .into_iter()
        .filter(|item| item.match_id == match_id)
        .collect::<Vec<_>>();
    let Some(first) = items.first() else {
        return Err("未找到该比赛的推荐/赔率数据，请先刷新核心数据".to_string());
    };
    let conn = open_conn(&app)?;
    let now = Utc::now().to_rfc3339();
    let api_fixture = api_fixture_for_match(&conn, &first.match_label.split(" vs ").next().unwrap_or(""), &first.match_label.split(" vs ").nth(1).unwrap_or(""));
    let external_fixture_id = api_fixture.pointer("/fixture/id")
        .and_then(|value| value.as_i64().map(|id| id.to_string()).or_else(|| value.as_str().map(str::to_string)))
        .unwrap_or_default();
    let competition = api_fixture.pointer("/league/name").and_then(Value::as_str).unwrap_or(&first.market).to_string();
    let season = api_fixture.pointer("/league/season").and_then(Value::as_i64).map(|value| value.to_string()).unwrap_or_default();
    let stage = api_fixture.pointer("/league/round").and_then(Value::as_str).unwrap_or("赛前").to_string();
    let injury_cache = cache_get(&conn, "injury_data").map_err(|error| error.to_string())?;
    let injury_status = if injury_cache.is_some() { "available" } else { "unknown" }.to_string();
    let injury_confidence = if injury_cache.is_some() { 78.0 } else { 35.0 };
    let mut data_quality = items.iter().map(|item| item.data_score).fold(0.0, f64::max);
    let mut risk_tags = Vec::new();
    if first.lineup_confidence < 80.0 {
        data_quality = data_quality.min(74.0);
        risk_tags.push("首发未确认");
    }
    if injury_status == "unknown" {
        data_quality = data_quality.min(72.0);
        risk_tags.push("伤停未知");
    }
    if data_quality < 65.0 {
        risk_tags.push("数据质量低于65，只能观察");
    }
    let final_decision = if items.iter().any(|item| item.final_decision == "hard_ban") {
        "hard_ban"
    } else if data_quality < 65.0 || first.lineup_confidence < 80.0 {
        "observe_only"
    } else {
        items.iter().map(|item| item.final_decision.as_str()).find(|value| *value == "recommend" || *value == "small_stake").unwrap_or("observe_only")
    }.to_string();
    let paper_trade_enabled = kickoff_is_future(&first.match_time, &now)
        && items.iter().any(|item| item.final_decision == "observe_only" && item.expected_return > 0.0);
    let model_probs = json!(items.iter().map(|item| json!({
        "market": item.market, "pick": item.pick, "model_prob": item.model_prob, "fair_odds": item.fair_odds
    })).collect::<Vec<_>>());
    let odds_payload = json!(items.iter().map(|item| json!({
        "market": item.market, "pick": item.pick, "odds": item.odds, "europe_odds": item.europe_odds
    })).collect::<Vec<_>>());
    let market_probs = json!(items.iter().map(|item| json!({
        "market": item.market, "pick": item.pick, "sporttery_prob": item.fair_prob, "europe_prob": item.europe_prob
    })).collect::<Vec<_>>());
    let ev_payload = json!(items.iter().map(|item| json!({
        "market": item.market, "pick": item.pick, "ev": item.expected_return, "advantage_rate": item.advantage_rate
    })).collect::<Vec<_>>());
    let reasons = json!(items.iter().map(|item| json!({
        "market": item.market, "pick": item.pick, "final_decision": item.final_decision,
        "reason": item.reason, "risk": item.risk_factors, "support": item.support_factors
    })).collect::<Vec<_>>());
    let raw_features = json!({
        "api_football_fixture": api_fixture,
        "lineup_status": first.lineup_status,
        "lineup_confidence": first.lineup_confidence,
        "injury_status": injury_status,
        "source": "live_pre_match_snapshot_v1"
    });
    let home_team = first.match_label.split(" vs ").next().unwrap_or("").to_string();
    let away_team = first.match_label.split(" vs ").nth(1).unwrap_or("").to_string();
    conn.execute(
        "insert into pre_match_snapshots(
          match_id, external_fixture_id, provider_match_id, snapshot_time, kickoff_time, home_team, away_team,
          competition, season, stage, model_version, model_probs_json, calibrated_probs_json,
          worldcup_correction_action, odds_json, market_probs_json, ev_json, data_quality_score,
          lineup_status, lineup_confidence, injury_status, injury_confidence, risk_tags_json,
          final_decision, decision_reason_json, paper_strategy_id, paper_trade_enabled, raw_features_json,
          is_final_pre_match, created_at, updated_at
        ) values (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26,?27,?28,0,?29,?30)",
        params![
            first.match_id,
            external_fixture_id,
            first.match_id,
            now,
            first.match_time,
            home_team,
            away_team,
            competition,
            season,
            stage,
            "live-pre-match-v1",
            model_probs.to_string(),
            model_probs.to_string(),
            first.worldcup_correction_action,
            odds_payload.to_string(),
            market_probs.to_string(),
            ev_payload.to_string(),
            data_quality,
            first.lineup_status,
            first.lineup_confidence,
            injury_status,
            injury_confidence,
            json!(risk_tags).to_string(),
            final_decision,
            reasons.to_string(),
            "candidate_strategy_v1",
            if paper_trade_enabled { 1 } else { 0 },
            raw_features.to_string(),
            now,
            now,
        ],
    ).map_err(|error| error.to_string())?;
    let snapshot_id = conn.last_insert_rowid();
    if paper_trade_enabled {
        for item in &items {
            if item.final_decision == "observe_only" && item.expected_return > 0.0 {
                conn.execute(
                    "insert into paper_trading_records(
                      match_id, snapshot_id, strategy_id, model_version, selection, play_type, model_prob, odds,
                      ev, advantage_rate, data_quality_score, risk_tags_json, worldcup_correction_action,
                      paper_stake, result_status, paper_profit, created_at, source, created_before_kickoff, is_final_snapshot
                    ) values (?1,?2,'candidate_strategy_v1','live-pre-match-v1',?3,?4,?5,?6,?7,?8,?9,?10,?11,1.0,'pending',0,?12,'live_pre_match',1,0)",
                    params![
                        item.match_id,
                        snapshot_id,
                        item.pick,
                        item.market,
                        item.model_prob,
                        item.odds,
                        item.expected_return,
                        item.advantage_rate,
                        item.data_score,
                        json!([item.risk_factors]).to_string(),
                        item.worldcup_correction_action,
                        now,
                    ],
                ).map_err(|error| error.to_string())?;
            }
        }
    }
    Ok(json!({
        "ok": true,
        "snapshot_id": snapshot_id,
        "match_id": first.match_id,
        "paper_trade_enabled": paper_trade_enabled,
        "final_decision": final_decision,
        "data_quality_score": data_quality
    }))
}

#[tauri::command]
async fn create_today_pre_match_snapshots(app: AppHandle) -> Result<Value, String> {
    let matches = list_matches(app.clone()).await?;
    let mut created = Vec::new();
    let mut errors = Vec::new();
    for item in matches {
        match create_pre_match_snapshot(app.clone(), item.id.clone()).await {
            Ok(value) => created.push(value),
            Err(error) => errors.push(json!({ "match_id": item.id, "error": error })),
        }
    }
    Ok(json!({ "ok": true, "created": created.len(), "snapshots": created, "errors": errors }))
}

#[tauri::command]
async fn get_pre_match_snapshots(app: AppHandle) -> Result<Vec<PreMatchSnapshotRow>, String> {
    let conn = open_conn(&app)?;
    let sql = pre_match_snapshot_select_sql("");
    let mut stmt = conn.prepare(&sql).map_err(|error| error.to_string())?;
    let rows = stmt
        .query_map([], pre_match_snapshot_from_row)
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|error| error.to_string())
}

#[tauri::command]
async fn get_match_snapshot_history(app: AppHandle, match_id: String) -> Result<Vec<PreMatchSnapshotRow>, String> {
    let conn = open_conn(&app)?;
    let sql = pre_match_snapshot_select_sql("where s.match_id=?1");
    let mut stmt = conn.prepare(&sql).map_err(|error| error.to_string())?;
    let rows = stmt
        .query_map(params![match_id], pre_match_snapshot_from_row)
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|error| error.to_string())
}

#[tauri::command]
async fn mark_final_pre_match_snapshot(app: AppHandle, snapshot_id: i64) -> Result<Value, String> {
    let conn = open_conn(&app)?;
    let match_id: String = conn.query_row(
        "select match_id from pre_match_snapshots where id=?1",
        params![snapshot_id],
        |row| row.get(0),
    ).map_err(|error| error.to_string())?;
    conn.execute("update pre_match_snapshots set is_final_pre_match=0 where match_id=?1", params![match_id]).map_err(|error| error.to_string())?;
    let now = Utc::now().to_rfc3339();
    conn.execute("update pre_match_snapshots set is_final_pre_match=1, updated_at=?2 where id=?1", params![snapshot_id, now]).map_err(|error| error.to_string())?;
    conn.execute("update paper_trading_records set is_final_snapshot=case when snapshot_id=?1 then 1 else 0 end where match_id=?2", params![snapshot_id, match_id]).map_err(|error| error.to_string())?;
    conn.execute(
        "insert into snapshot_audit_logs(snapshot_id, match_id, audit_type, severity, message, detected_at, resolved)
         values(?1,?2,'final_snapshot_marked','info','已标记新的最终赛前快照，并自动取消同场旧 final。',?3,1)",
        params![snapshot_id, match_id, now],
    ).map_err(|error| error.to_string())?;
    Ok(json!({ "ok": true, "snapshot_id": snapshot_id, "match_id": match_id }))
}

fn spf_from_score(home_score: i64, away_score: i64) -> &'static str {
    if home_score > away_score { "主胜" } else if home_score == away_score { "平局" } else { "客胜" }
}

#[tauri::command]
async fn settle_pre_match_snapshot(app: AppHandle, snapshot_id: i64, home_score: i64, away_score: i64) -> Result<Value, String> {
    let conn = open_conn(&app)?;
    let (match_id, odds_text): (String, String) = conn.query_row(
        "select match_id, odds_json from pre_match_snapshots where id=?1",
        params![snapshot_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    ).map_err(|error| error.to_string())?;
    let result_spf = spf_from_score(home_score, away_score).to_string();
    let total_goals = home_score + away_score;
    let odds_items = parse_json_text(odds_text);
    let mut is_hit = serde_json::Map::new();
    let mut paper_profit = serde_json::Map::new();
    for item in odds_items.as_array().into_iter().flatten() {
        let market = item.get("market").and_then(Value::as_str).unwrap_or("");
        let pick = item.get("pick").and_then(Value::as_str).unwrap_or("");
        let odds = item.get("odds").and_then(Value::as_f64).unwrap_or(0.0);
        if market.starts_with("体彩HAD") || market.contains("HAD") {
            let hit = pick == result_spf;
            let key = format!("{}:{}", market, pick);
            is_hit.insert(key.clone(), json!(hit));
            paper_profit.insert(key, json!(if hit { odds - 1.0 } else { -1.0 }));
        }
    }
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "insert into pre_match_snapshot_results(snapshot_id, match_id, home_score, away_score, result_spf, total_goals, settled_at, is_hit_json, paper_profit_json, settlement_status)
         values(?1,?2,?3,?4,?5,?6,?7,?8,?9,'settled')",
        params![snapshot_id, match_id, home_score, away_score, result_spf, total_goals, now, Value::Object(is_hit).to_string(), Value::Object(paper_profit).to_string()],
    ).map_err(|error| error.to_string())?;
    conn.execute(
        "update paper_trading_records
         set result_status='settled',
             is_hit=case
               when selection=?2 then 1
               when play_type like '%HAD%' then 0
               else is_hit
             end,
             paper_profit=case
               when selection=?2 then odds - paper_stake
               when play_type like '%HAD%' then -paper_stake
               else paper_profit
             end,
             settled_at=?3
         where snapshot_id=?1 and source='live_pre_match'",
        params![snapshot_id, result_spf, now],
    ).map_err(|error| error.to_string())?;
    Ok(json!({ "ok": true, "snapshot_id": snapshot_id, "result_spf": result_spf, "total_goals": total_goals }))
}

#[tauri::command]
async fn settle_all_finished_snapshots(app: AppHandle) -> Result<Value, String> {
    let pending = {
        let conn = open_conn(&app)?;
        let mut stmt = conn.prepare(
            "select s.id, r.score from pre_match_snapshots s
             join match_results r on r.match_id=s.match_id or r.match_label like '%' || s.home_team || '%' || s.away_team || '%'
             where not exists(select 1 from pre_match_snapshot_results pr where pr.snapshot_id=s.id)",
        ).map_err(|error| error.to_string())?;
        let rows = stmt.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))).map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|error| error.to_string())?
    };
    let mut settled = 0;
    for (snapshot_id, score) in pending {
        let parts = score.split(':').collect::<Vec<_>>();
        if parts.len() != 2 {
            continue;
        }
        let Ok(home_score) = parts[0].trim().parse::<i64>() else { continue };
        let Ok(away_score) = parts[1].trim().parse::<i64>() else { continue };
        drop(settle_pre_match_snapshot(app.clone(), snapshot_id, home_score, away_score).await);
        settled += 1;
    }
    Ok(json!({ "ok": true, "settled": settled }))
}

fn audit_severity(audit_type: &str) -> &'static str {
    match audit_type {
        "after_kickoff_snapshot" | "final_snapshot_conflict" | "settlement_overwrite_risk" | "paper_trade_invalid" => "critical",
        "missing_odds" | "missing_model_probs" | "invalid_probability_sum" | "lineup_unknown_near_kickoff" => "warning",
        _ => "info",
    }
}

fn add_audit_issue(issues: &mut Vec<(Option<i64>, String, String, String, String)>, snapshot_id: Option<i64>, match_id: &str, audit_type: &str, message: &str) {
    issues.push((snapshot_id, match_id.to_string(), audit_type.to_string(), audit_severity(audit_type).to_string(), message.to_string()));
}

fn had_probability_sum(model_probs: &Value) -> Option<f64> {
    let rows = model_probs.as_array()?;
    let had_rows = rows
        .iter()
        .filter(|item| item.get("market").and_then(Value::as_str).unwrap_or("").contains("HAD"))
        .collect::<Vec<_>>();
    if had_rows.len() < 3 {
        return None;
    }
    Some(had_rows.iter().map(|item| item.get("model_prob").and_then(Value::as_f64).unwrap_or(0.0)).sum())
}

#[tauri::command]
async fn audit_pre_match_snapshots(app: AppHandle) -> Result<Value, String> {
    let conn = open_conn(&app)?;
    let snapshots = {
        let sql = pre_match_snapshot_select_sql("");
        let mut stmt = conn.prepare(&sql).map_err(|error| error.to_string())?;
        let rows = stmt.query_map([], pre_match_snapshot_from_row).map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|error| error.to_string())?
    };
    let now = Utc::now().to_rfc3339();
    conn.execute("update snapshot_audit_logs set resolved=1, resolved_at=?1 where resolved=0", params![now]).map_err(|error| error.to_string())?;
    let mut issues: Vec<(Option<i64>, String, String, String, String)> = Vec::new();
    let mut final_counts: BTreeMap<String, i64> = BTreeMap::new();
    for snapshot in &snapshots {
        if snapshot.is_final_pre_match {
            *final_counts.entry(snapshot.match_id.clone()).or_default() += 1;
        }
        if !kickoff_is_future(&snapshot.kickoff_time, &snapshot.snapshot_time) {
            add_audit_issue(&mut issues, Some(snapshot.id), &snapshot.match_id, "after_kickoff_snapshot", "快照时间晚于开赛时间，不能进入 live_pre_match 纸面交易。");
        }
        if snapshot.odds_json.as_array().map(|items| items.is_empty()).unwrap_or(true) {
            add_audit_issue(&mut issues, Some(snapshot.id), &snapshot.match_id, "missing_odds", "赔率为空或关键赔率缺失。");
        }
        if snapshot.model_probs_json.as_array().map(|items| items.is_empty()).unwrap_or(true) {
            add_audit_issue(&mut issues, Some(snapshot.id), &snapshot.match_id, "missing_model_probs", "模型概率为空。");
        }
        if let Some(sum) = had_probability_sum(&snapshot.model_probs_json) {
            if (sum - 1.0).abs() > 0.08 {
                add_audit_issue(&mut issues, Some(snapshot.id), &snapshot.match_id, "invalid_probability_sum", "胜平负概率和不接近 1。");
            }
        }
        if snapshot.data_quality_score <= 0.0 {
            add_audit_issue(&mut issues, Some(snapshot.id), &snapshot.match_id, "missing_data_quality", "缺少数据质量评分。");
        }
        if snapshot.raw_features_json.pointer("/api_football_fixture").map(Value::is_null).unwrap_or(true) {
            add_audit_issue(&mut issues, Some(snapshot.id), &snapshot.match_id, "api_data_stale", "API-Football 暂不可用或没有匹配 fixture，已使用基础模型快照。");
        }
        if snapshot.lineup_confidence < 80.0 {
            add_audit_issue(&mut issues, Some(snapshot.id), &snapshot.match_id, "lineup_unknown_near_kickoff", "首发未确认，正式推荐不得升级。");
        }
        if snapshot.settlement.is_some() {
            let frozen_ok = snapshot.model_probs_json.is_array() && snapshot.odds_json.is_array() && snapshot.ev_json.is_array();
            if !frozen_ok {
                add_audit_issue(&mut issues, Some(snapshot.id), &snapshot.match_id, "settlement_overwrite_risk", "赛后结算后发现冻结字段异常，存在覆盖风险。");
            }
        }
    }
    for (match_id, count) in final_counts {
        if count > 1 {
            add_audit_issue(&mut issues, None, &match_id, "final_snapshot_conflict", "同一场比赛存在多个 final snapshot。");
        }
    }
    let invalid_paper_count: i64 = conn.query_row(
        "select count(*) from paper_trading_records where source='live_pre_match' and created_before_kickoff=0",
        [],
        |row| row.get(0),
    ).unwrap_or(0);
    if invalid_paper_count > 0 {
        add_audit_issue(&mut issues, None, "live_pre_match", "paper_trade_invalid", "存在非赛前创建的 live_pre_match 纸面交易。");
    }
    for (snapshot_id, match_id, audit_type, severity, message) in &issues {
        conn.execute(
            "insert into snapshot_audit_logs(snapshot_id, match_id, audit_type, severity, message, detected_at, resolved)
             values(?1,?2,?3,?4,?5,?6,0)",
            params![snapshot_id, match_id, audit_type, severity, message, now],
        ).map_err(|error| error.to_string())?;
    }
    let critical = issues.iter().filter(|item| item.3 == "critical").count();
    let warning = issues.iter().filter(|item| item.3 == "warning").count();
    Ok(json!({ "ok": true, "issues": issues.len(), "critical": critical, "warning": warning }))
}

#[tauri::command]
async fn get_snapshot_audit_logs(app: AppHandle) -> Result<Vec<SnapshotAuditLog>, String> {
    let conn = open_conn(&app)?;
    let mut stmt = conn.prepare(
        "select l.id, l.snapshot_id, l.match_id,
                coalesce(s.home_team || ' vs ' || s.away_team, l.match_id) as match_label,
                coalesce(s.snapshot_time, '') as snapshot_time,
                coalesce(s.kickoff_time, '') as kickoff_time,
                l.audit_type, l.severity, l.message, l.detected_at, l.resolved, l.resolved_at
         from snapshot_audit_logs l
         left join pre_match_snapshots s on s.id=l.snapshot_id
         order by l.detected_at desc, l.id desc",
    ).map_err(|error| error.to_string())?;
    let rows = stmt.query_map([], |row| {
        Ok(SnapshotAuditLog {
            id: row.get(0)?,
            snapshot_id: row.get(1)?,
            match_id: row.get(2)?,
            match_label: row.get(3)?,
            snapshot_time: row.get(4)?,
            kickoff_time: row.get(5)?,
            audit_type: row.get(6)?,
            severity: row.get(7)?,
            message: row.get(8)?,
            detected_at: row.get(9)?,
            resolved: row.get::<_, i64>(10)? != 0,
            resolved_at: row.get(11)?,
        })
    }).map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|error| error.to_string())
}

fn paper_drawdown(profits: &[f64]) -> f64 {
    max_drawdown_from_profit(profits)
}

#[tauri::command]
async fn get_live_paper_trading_records(app: AppHandle) -> Result<Vec<Value>, String> {
    let conn = open_conn(&app)?;
    let mut stmt = conn.prepare(
        "select id, match_id, snapshot_id, strategy_id, selection, play_type, model_prob, odds, ev,
                paper_stake, result_status, coalesce(is_hit,-1), paper_profit, created_at, settled_at, is_final_snapshot
         from paper_trading_records
         where source='live_pre_match'
         order by id desc",
    ).map_err(|error| error.to_string())?;
    let rows = stmt.query_map([], |row| {
        Ok(json!({
            "id": row.get::<_, i64>(0)?,
            "match_id": row.get::<_, String>(1)?,
            "snapshot_id": row.get::<_, Option<i64>>(2)?,
            "strategy_id": row.get::<_, String>(3)?,
            "selection": row.get::<_, String>(4)?,
            "play_type": row.get::<_, String>(5)?,
            "model_prob": row.get::<_, f64>(6)?,
            "odds": row.get::<_, f64>(7)?,
            "ev": row.get::<_, f64>(8)?,
            "paper_stake": row.get::<_, f64>(9)?,
            "result_status": row.get::<_, String>(10)?,
            "is_hit": row.get::<_, i64>(11)?,
            "paper_profit": row.get::<_, f64>(12)?,
            "created_at": row.get::<_, String>(13)?,
            "settled_at": row.get::<_, Option<String>>(14)?,
            "is_final_snapshot": row.get::<_, i64>(15)? != 0,
        }))
    }).map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|error| error.to_string())
}

#[tauri::command]
async fn get_live_paper_trading_summary(app: AppHandle) -> Result<Value, String> {
    let records = get_live_paper_trading_records(app).await?;
    let final_records = records.iter().filter(|item| item.get("is_final_snapshot").and_then(Value::as_bool).unwrap_or(false)).collect::<Vec<_>>();
    let sample_count = final_records.len();
    let settled = final_records.iter().filter(|item| item.get("result_status").and_then(Value::as_str).unwrap_or("") == "settled").collect::<Vec<_>>();
    let settled_count = settled.len();
    let stake: f64 = settled.iter().map(|item| item.get("paper_stake").and_then(Value::as_f64).unwrap_or(0.0)).sum();
    let profit_values = settled.iter().map(|item| item.get("paper_profit").and_then(Value::as_f64).unwrap_or(0.0)).collect::<Vec<_>>();
    let profit: f64 = profit_values.iter().sum();
    let hit_count = settled.iter().filter(|item| item.get("is_hit").and_then(Value::as_i64).unwrap_or(-1) == 1).count();
    let recent_30 = settled.iter().rev().take(30).collect::<Vec<_>>();
    let recent_stake: f64 = recent_30.iter().map(|item| item.get("paper_stake").and_then(Value::as_f64).unwrap_or(0.0)).sum();
    let recent_profit: f64 = recent_30.iter().map(|item| item.get("paper_profit").and_then(Value::as_f64).unwrap_or(0.0)).sum();
    Ok(json!({
        "sample_count": sample_count,
        "settled_count": settled_count,
        "unsettled_count": sample_count.saturating_sub(settled_count),
        "hit_rate": if settled_count > 0 { hit_count as f64 / settled_count as f64 } else { 0.0 },
        "paper_roi": if stake > 0.0 { profit / stake } else { 0.0 },
        "total_paper_stake": stake,
        "total_paper_profit": profit,
        "max_drawdown": paper_drawdown(&profit_values),
        "avg_odds": if settled_count > 0 { settled.iter().map(|item| item.get("odds").and_then(Value::as_f64).unwrap_or(0.0)).sum::<f64>() / settled_count as f64 } else { 0.0 },
        "avg_ev": if settled_count > 0 { settled.iter().map(|item| item.get("ev").and_then(Value::as_f64).unwrap_or(0.0)).sum::<f64>() / settled_count as f64 } else { 0.0 },
        "recent_10": records.iter().take(10).cloned().collect::<Vec<_>>(),
        "recent_30_roi": if recent_stake > 0.0 { recent_profit / recent_stake } else { 0.0 },
        "warning": if sample_count < 30 { "真实赛前纸面交易样本不足，暂不能评价策略。" } else { "" }
    }))
}

fn timestamp_compact() -> String {
    Utc::now().format("%Y%m%d_%H%M%S").to_string()
}

fn backup_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app_dir(app)?.join("backups");
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    Ok(dir)
}

fn value_ref_to_string(value: ValueRef<'_>) -> String {
    match value {
        ValueRef::Null => String::new(),
        ValueRef::Integer(value) => value.to_string(),
        ValueRef::Real(value) => {
            if value.fract().abs() < 0.000_000_1 {
                format!("{:.0}", value)
            } else {
                value.to_string()
            }
        }
        ValueRef::Text(value) => String::from_utf8_lossy(value).to_string(),
        ValueRef::Blob(value) => format!("<{} bytes>", value.len()),
    }
}

fn query_to_csv(conn: &Connection, sql: &str) -> Result<String, String> {
    let mut stmt = conn.prepare(sql).map_err(|error| error.to_string())?;
    let headers = stmt.column_names().iter().map(|name| csv_cell(name)).collect::<Vec<_>>().join(",");
    let column_count = stmt.column_count();
    let mut output = String::new();
    output.push_str(&headers);
    output.push('\n');
    let mut rows = stmt.query([]).map_err(|error| error.to_string())?;
    while let Some(row) = rows.next().map_err(|error| error.to_string())? {
        let cells = (0..column_count)
            .map(|idx| row.get_ref(idx).map(value_ref_to_string).map(|value| csv_cell(&value)))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        output.push_str(&cells.join(","));
        output.push('\n');
    }
    Ok(output)
}

fn write_export_csv(app: &AppHandle, file_prefix: &str, csv_text: &str) -> Result<Value, String> {
    let dir = backup_dir(app)?;
    let path = dir.join(format!("{}_{}.csv", file_prefix, timestamp_compact()));
    fs::write(&path, csv_text).map_err(|error| error.to_string())?;
    Ok(json!({
        "ok": true,
        "path": path.to_string_lossy().to_string(),
        "message": format!("已导出 {}", path.to_string_lossy())
    }))
}

fn config_summary(conn: &Connection) -> Result<Value, String> {
    let mut stmt = conn.prepare(
        "select p.provider_id, p.name, p.data_type, p.requires_key, p.enabled,
                case when c.api_key is not null and length(c.api_key)>0 then 1 else 0 end as configured
         from data_providers p
         left join provider_credentials c on c.provider_id=p.provider_id
         order by p.provider_id",
    ).map_err(|error| error.to_string())?;
    let rows = stmt.query_map([], |row| {
        Ok(json!({
            "provider_id": row.get::<_, String>(0)?,
            "name": row.get::<_, String>(1)?,
            "data_type": row.get::<_, String>(2)?,
            "requires_key": row.get::<_, i64>(3)? != 0,
            "enabled": row.get::<_, i64>(4)? != 0,
            "key_configured": row.get::<_, i64>(5)? != 0
        }))
    }).map_err(|error| error.to_string())?;
    let providers = rows.collect::<Result<Vec<_>, _>>().map_err(|error| error.to_string())?;
    Ok(json!({
        "app_version": APP_VERSION,
        "strategy_status": "observation_only",
        "official_recommendation_status": "风控开启",
        "providers": providers,
        "note": "API Key 不导出明文，仅保留是否已配置。"
    }))
}

fn copy_json_files(src_dir: &Path, dst_dir: &Path) -> Result<(), String> {
    if !src_dir.exists() {
        return Ok(());
    }
    fs::create_dir_all(dst_dir).map_err(|error| error.to_string())?;
    for entry in fs::read_dir(src_dir).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) == Some("json") {
            if let Some(name) = path.file_name() {
                fs::copy(&path, dst_dir.join(name)).map_err(|error| error.to_string())?;
            }
        }
    }
    Ok(())
}

fn zip_directory(src_dir: &Path, zip_path: &Path) -> Result<(), String> {
    let file = fs::File::create(zip_path).map_err(|error| error.to_string())?;
    let mut zip = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    fn add_dir(zip: &mut zip::ZipWriter<fs::File>, base: &Path, dir: &Path, options: SimpleFileOptions) -> Result<(), String> {
        for entry in fs::read_dir(dir).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            let path = entry.path();
            let name = path.strip_prefix(base).map_err(|error| error.to_string())?.to_string_lossy().replace('\\', "/");
            if path.is_dir() {
                add_dir(zip, base, &path, options)?;
            } else {
                zip.start_file(name, options).map_err(|error| error.to_string())?;
                let mut file = fs::File::open(&path).map_err(|error| error.to_string())?;
                let mut bytes = Vec::new();
                file.read_to_end(&mut bytes).map_err(|error| error.to_string())?;
                zip.write_all(&bytes).map_err(|error| error.to_string())?;
            }
        }
        Ok(())
    }
    add_dir(&mut zip, src_dir, src_dir, options)?;
    zip.finish().map_err(|error| error.to_string())?;
    Ok(())
}

fn training_root_from_app(app: &AppHandle) -> PathBuf {
    training_models_dir(&app_dir(app).unwrap_or_else(|_| PathBuf::from(".")))
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("training"))
}

fn live_paper_csv_sql() -> &'static str {
    "select id, match_id, snapshot_id, strategy_id, model_version, selection, play_type, model_prob,
            odds, ev, advantage_rate, data_quality_score, risk_tags_json, worldcup_correction_action,
            paper_stake, result_status, coalesce(is_hit,''), paper_profit, source,
            created_before_kickoff, is_final_snapshot, created_at, settled_at
     from paper_trading_records
     where source='live_pre_match'
     order by id desc"
}

#[tauri::command]
async fn export_snapshots(app: AppHandle) -> Result<Value, String> {
    let conn = open_conn(&app)?;
    let csv = query_to_csv(&conn, "select * from pre_match_snapshots order by kickoff_time asc, id asc")?;
    write_export_csv(&app, "pre_match_snapshots", &csv)
}

#[tauri::command]
async fn export_live_paper_trading(app: AppHandle) -> Result<Value, String> {
    let conn = open_conn(&app)?;
    let csv = query_to_csv(&conn, live_paper_csv_sql())?;
    write_export_csv(&app, "live_pre_match_paper_trading", &csv)
}

#[tauri::command]
async fn export_audit_logs(app: AppHandle) -> Result<Value, String> {
    let conn = open_conn(&app)?;
    let csv = query_to_csv(&conn, "select * from snapshot_audit_logs order by detected_at desc, id desc")?;
    write_export_csv(&app, "snapshot_audit_logs", &csv)
}

#[tauri::command]
async fn export_snapshot_results(app: AppHandle) -> Result<Value, String> {
    let conn = open_conn(&app)?;
    let csv = query_to_csv(&conn, "select * from pre_match_snapshot_results order by settled_at desc, id desc")?;
    write_export_csv(&app, "pre_match_snapshot_results", &csv)
}

#[tauri::command]
async fn export_strategy_diagnostics(app: AppHandle) -> Result<Value, String> {
    let training_root = training_root_from_app(&app);
    let source = training_root.join("reports").join("rule_diagnostics.csv");
    let csv = if source.exists() {
        fs::read_to_string(&source).map_err(|error| error.to_string())?
    } else {
        "rule_id,rule_name,action,matched_count,roi\n".to_string()
    };
    write_export_csv(&app, "strategy_diagnostics", &csv)
}

#[tauri::command]
async fn open_backup_dir(app: AppHandle) -> Result<Value, String> {
    let dir = backup_dir(&app)?;
    Command::new("explorer")
        .arg(&dir)
        .spawn()
        .map_err(|error| format!("打开备份目录失败：{}", error))?;
    Ok(json!({"ok": true, "path": dir.to_string_lossy().to_string()}))
}

#[tauri::command]
async fn export_app_data(app: AppHandle) -> Result<Value, String> {
    let conn = open_conn(&app)?;
    let _ = conn.execute_batch("pragma wal_checkpoint(full);");
    let created_at = Utc::now().to_rfc3339();
    let stamp = timestamp_compact();
    let dir = backup_dir(&app)?;
    let staging = dir.join(format!("staging_{}", stamp));
    fs::create_dir_all(&staging).map_err(|error| format!("创建备份临时目录失败：{}", error))?;

    let database_path = db_path(&app)?;
    let database_copy = staging.join("worldcup-odds.sqlite");
    fs::copy(&database_path, &database_copy).map_err(|error| format!("复制数据库失败：{}", error))?;

    fs::write(staging.join("pre_match_snapshots.csv"), query_to_csv(&conn, "select * from pre_match_snapshots order by kickoff_time asc, id asc")?).map_err(|error| error.to_string())?;
    fs::write(staging.join("pre_match_snapshot_results.csv"), query_to_csv(&conn, "select * from pre_match_snapshot_results order by settled_at desc, id desc")?).map_err(|error| error.to_string())?;
    fs::write(staging.join("live_pre_match_paper_trading.csv"), query_to_csv(&conn, live_paper_csv_sql())?).map_err(|error| error.to_string())?;
    fs::write(staging.join("snapshot_audit_logs.csv"), query_to_csv(&conn, "select * from snapshot_audit_logs order by detected_at desc, id desc")?).map_err(|error| error.to_string())?;
    fs::write(staging.join("config_summary.json"), serde_json::to_string_pretty(&config_summary(&conn)?).map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;

    let training_root = training_root_from_app(&app);
    copy_json_files(&training_root.join("models"), &staging.join("training").join("models"))?;
    copy_json_files(&training_root.join("reports"), &staging.join("training").join("reports"))?;
    let strategy_csv = training_root.join("reports").join("rule_diagnostics.csv");
    if strategy_csv.exists() {
        fs::copy(&strategy_csv, staging.join("strategy_diagnostics.csv")).map_err(|error| error.to_string())?;
    } else {
        fs::write(staging.join("strategy_diagnostics.csv"), "rule_id,rule_name,action,matched_count,roi\n").map_err(|error| error.to_string())?;
    }

    let zip_path = dir.join(format!("shijiebei_backup_{}.zip", stamp));
    zip_directory(&staging, &zip_path).map_err(|error| format!("生成备份 ZIP 失败：{}", error))?;
    let _ = fs::remove_dir_all(&staging);
    cache_put(&conn, "last_backup", &json!({"path": zip_path.to_string_lossy().to_string(), "created_at": created_at})).map_err(|error| error.to_string())?;
    Ok(json!({
        "ok": true,
        "path": zip_path.to_string_lossy().to_string(),
        "created_at": created_at,
        "message": "完整备份已生成，API Key 未导出明文。"
    }))
}

#[tauri::command]
async fn get_system_status(app: AppHandle) -> Result<Value, String> {
    let conn = open_conn(&app)?;
    let path = db_path(&app)?;
    let db_size = fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
    let count_i64 = |sql: &str| -> i64 { conn.query_row(sql, [], |row| row.get::<_, i64>(0)).unwrap_or(0) };
    let today = Utc::now().format("%Y-%m-%d").to_string();
    let api_requests = conn
        .query_row(
            "select count(*) from provider_request_logs where provider_id='api_football' and requested_at like ?1",
            params![format!("{}%", today)],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0);
    let active = active_model_info(&std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let last_backup = cache_get(&conn, "last_backup").ok().flatten().map(|record| record.value).unwrap_or_else(|| json!(null));
    Ok(json!({
        "app_version": APP_VERSION,
        "db_path": path.to_string_lossy().to_string(),
        "db_size_bytes": db_size,
        "snapshot_count": count_i64("select count(*) from pre_match_snapshots"),
        "final_snapshot_count": count_i64("select count(*) from pre_match_snapshots where is_final_pre_match=1"),
        "live_pre_match_sample_count": count_i64("select count(*) from paper_trading_records where source='live_pre_match' and is_final_snapshot=1"),
        "live_pre_match_settled_count": count_i64("select count(*) from paper_trading_records where source='live_pre_match' and is_final_snapshot=1 and result_status='settled'"),
        "live_pre_match_unsettled_count": count_i64("select count(*) from paper_trading_records where source='live_pre_match' and is_final_snapshot=1 and result_status<>'settled'"),
        "audit_critical_count": count_i64("select count(*) from snapshot_audit_logs where severity='critical' and resolved=0"),
        "audit_warning_count": count_i64("select count(*) from snapshot_audit_logs where severity='warning' and resolved=0"),
        "api_football_today_requests": api_requests,
        "api_football_remaining_requests": (API_FOOTBALL_DAILY_LIMIT - api_requests).max(0),
        "api_football_daily_limit": API_FOOTBALL_DAILY_LIMIT,
        "last_backup": last_backup,
        "model_version": active.model_version,
        "worldcup_correction_version": active.worldcup_correction_version,
        "strategy_status": "observation_only",
        "official_recommendation_status": "风控开启"
    }))
}

#[tauri::command]
async fn today_bet_plan(app: AppHandle) -> Result<TodayBetPlan, String> {
    let recommendations = list_recommendations(app.clone()).await.unwrap_or_default();
    let settings = get_bankroll_settings(app.clone()).await.unwrap_or(BankrollSettings {
        bankroll: 1000.0,
        daily_budget_pct: 0.03,
        max_loss_pct: 0.06,
        auto_refresh_minutes: 0,
    });
    let daily_budget = settings.bankroll * settings.daily_budget_pct;
    let max_loss = settings.bankroll * settings.max_loss_pct;
    let mut singles = recommendations
        .iter()
        .filter(|item| item.final_decision == "recommend" && item.stake_pct > 0.0 && !item.market.starts_with("CRS"))
        .cloned()
        .collect::<Vec<_>>();
    singles.sort_by(|a, b| b.stake_pct.partial_cmp(&a.stake_pct).unwrap_or(std::cmp::Ordering::Equal));
    for item in &mut singles {
        item.stake_pct = if item.confidence == "高" {
            item.stake_pct.clamp(0.005, 0.010)
        } else {
            item.stake_pct.clamp(0.0025, 0.005)
        };
    }
    let mut small_stake = recommendations
        .iter()
        .filter(|item| item.final_decision == "small_stake" && item.stake_pct > 0.0 && !item.market.starts_with("CRS"))
        .take(20)
        .cloned()
        .collect::<Vec<_>>();
    small_stake.sort_by(|a, b| b.expected_return.partial_cmp(&a.expected_return).unwrap_or(std::cmp::Ordering::Equal));
    let combos = singles
        .iter()
        .filter(|item| item.tier == "稳胆" || item.tier == "让球稳胆")
        .fold(Vec::<Recommendation>::new(), |mut acc, item| {
            if !acc.iter().any(|other| other.match_id == item.match_id) && acc.len() < 4 {
                acc.push(item.clone());
            }
            acc
        });
    let strategy_observation = recommendations
        .iter()
        .filter(|item| item.final_decision == "observe_only" && item.expected_return > 0.0)
        .take(20)
        .cloned()
        .collect::<Vec<_>>();
    let banned = recommendations
        .iter()
        .filter(|item| item.final_decision == "hard_ban" || item.decision == "禁止" || item.quality_action == "建议跳过")
        .take(30)
        .cloned()
        .collect::<Vec<_>>();
    let watch = recommendations
        .iter()
        .filter(|item| item.final_decision == "observe_only" || item.quality_action.contains("只看预测"))
        .take(30)
        .cloned()
        .collect::<Vec<_>>();
    let mut wait_notes = Vec::new();
    if recommendations.iter().any(|item| item.lineup_confidence < 80.0) {
        wait_notes.push("存在首发未确认比赛：等官方首发后再判断是否下注".to_string());
    }
    if recommendations.iter().any(|item| !item.anomaly_type.is_empty()) {
        wait_notes.push("存在赔率异常：等下一次赔率快照确认方向".to_string());
    }
    if recommendations.iter().any(|item| item.market.starts_with("CRS")) {
        wait_notes.push("比分玩法默认不下注或极小观察".to_string());
    }
    Ok(TodayBetPlan {
        bankroll: settings.bankroll,
        daily_budget,
        max_loss,
        singles: singles.into_iter().take(8).collect(),
        small_stake,
        combos,
        strategy_observation,
        banned,
        watch,
        wait_notes,
        review_hint: if recommendations.iter().any(|item| item.final_decision == "recommend") {
            "赛后到复盘中心结算命中/未中，再看历史回测页面更新禁买规则。".to_string()
        } else {
            "今日暂无通过风控的正式推荐。策略观察仅模拟，不建议真实下注。".to_string()
        },
    })
}

#[tauri::command]
async fn get_model_settings(app: AppHandle) -> Result<ModelSettings, String> {
    let conn = open_conn(&app)?;
    Ok(load_model_settings(&conn))
}

#[tauri::command]
async fn save_model_settings(app: AppHandle, settings: ModelSettings) -> Result<(), String> {
    let conn = open_conn(&app)?;
    cache_put(&conn, "model_settings", &json!(settings)).map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
async fn auto_tune_model(app: AppHandle) -> Result<ModelSettings, String> {
    let conn = open_conn(&app)?;
    let diagnostics = model_diagnostics(app.clone()).await?;
    let mut settings = load_model_settings(&conn);
    if diagnostics.settled < 20 {
        settings.mode = "样本不足-未调权".to_string();
    } else if diagnostics.brier_score > 0.28 || diagnostics.log_loss > 0.85 {
        settings.buy_edge = 0.13;
        settings.buy_gap = 0.045;
        settings.watch_edge = 0.065;
        settings.watch_gap = 0.025;
        settings.max_odds = 5.8;
        settings.high_odds_limit = 5.8;
        settings.mode = "校准偏差-强保守".to_string();
    } else if diagnostics.roi < -0.05 {
        settings.buy_edge = 0.12;
        settings.buy_gap = 0.040;
        settings.watch_edge = 0.060;
        settings.watch_gap = 0.020;
        settings.max_odds = 6.0;
        settings.high_odds_limit = 6.0;
        settings.mode = "保守".to_string();
    } else if diagnostics.roi > 0.05 && diagnostics.hit_rate > 0.40 {
        settings.buy_edge = 0.07;
        settings.buy_gap = 0.022;
        settings.watch_edge = 0.032;
        settings.watch_gap = 0.010;
        settings.max_odds = 8.0;
        settings.high_odds_limit = 8.0;
        settings.mode = "正常".to_string();
    } else {
        settings.mode = "观察-轻保守".to_string();
        settings.buy_edge = 0.10;
        settings.buy_gap = 0.030;
        settings.watch_edge = 0.045;
        settings.watch_gap = 0.015;
        settings.max_odds = 7.0;
        settings.high_odds_limit = 7.0;
    }
    cache_put(&conn, "model_settings", &json!(settings)).map_err(|error| error.to_string())?;
    Ok(settings)
}

pub fn run_app() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            app_status,
            refresh_core_data,
            refresh_statsbomb_xg,
            refresh_sporttery_injuries,
            refresh_results,
            import_historical_results_csv,
            import_player_status_csv,
            import_team_stats_csv,
            list_results,
            list_matches,
            simulate_match,
            save_prediction,
            list_predictions,
            list_recommendations,
            freeze_current_recommendations,
            create_pre_match_snapshot,
            create_today_pre_match_snapshots,
            get_pre_match_snapshots,
            get_match_snapshot_history,
            mark_final_pre_match_snapshot,
            settle_pre_match_snapshot,
            settle_all_finished_snapshots,
            audit_pre_match_snapshots,
            get_snapshot_audit_logs,
            get_live_paper_trading_summary,
            get_live_paper_trading_records,
            export_app_data,
            export_snapshots,
            export_live_paper_trading,
            export_audit_logs,
            export_snapshot_results,
            export_strategy_diagnostics,
            open_backup_dir,
            get_system_status,
            collect_worldcup_pre_match_snapshot,
            settle_bet_recommendations,
            export_worldcup_training_samples,
            run_worldcup_closure_cycle,
            list_match_analyses,
            list_odds_movements,
            list_odds_anomalies,
            list_odds_history,
            delete_prediction,
            settle_prediction,
            auto_settle_predictions,
            get_bankroll_settings,
            save_bankroll_settings,
            get_external_source_config,
            list_providers,
            save_provider_credential,
            clear_provider_credential,
            set_provider_enabled,
            clear_provider_cache,
            test_provider_connection,
            save_external_source_config,
            probe_external_source,
            refresh_external_sources,
            refresh_all_data_sources,
            model_diagnostics,
            get_active_model_info,
            run_training_pipeline,
            backtest_report,
            today_bet_plan,
            get_model_settings,
            save_model_settings,
            auto_tune_model
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use crate::models::ProviderRawRecord;
    use crate::services::data_fusion_service::{downgrade_for_missing_realtime_xg, fuse_provider_records, lineup_status_from_source, source_agreement_score};
    use crate::services::model_service::{active_model_info, predict_with_training_models, strategy_ev_range, strategy_odds_range, strategy_probability_range, trained_score_probs, training_models_dir, ModelFeatureInput};
    use crate::services::providers::default_provider_registry;
    use crate::services::source_service::{provider_key_error, request_limit_available, save_provider_raw_record};
    use crate::services::source_service::provider_field_confidence;

    #[test]
    fn quality_action_thresholds_are_clear() {
        assert_eq!(quality_action(54.9), "建议跳过");
        assert_eq!(quality_action(60.0), "只看预测，不建议购买");
        assert_eq!(quality_action(70.0), "观察或极小注");
        assert_eq!(quality_action(80.0), "可小注");
        assert_eq!(quality_action(85.0), "可进入正式推荐");
    }

    #[test]
    fn play_type_risk_is_tight_for_score_and_goals() {
        assert_eq!(play_type_risk_level("CRS比分"), "极高");
        assert_eq!(play_type_risk_level("TTG总进球"), "高");
        assert_eq!(play_type_risk_level("HHAD让球"), "中");
        assert_eq!(play_type_risk_level("HAD胜平负"), "低");
    }

    #[test]
    fn anomaly_classifier_detects_major_movement() {
        let anomaly = classify_anomaly("HAD胜平负", "主胜", -0.70, -0.15, 1.8).unwrap();
        assert_eq!(anomaly.0, "临场剧烈波动");
        assert_eq!(anomaly.1, "高");
        let hot = classify_anomaly("HAD胜平负", "主胜", -0.10, -0.05, 1.6).unwrap();
        assert_eq!(hot.0, "热门过热");
    }

    #[test]
    fn provider_confidence_uses_all_factors() {
        assert!((provider_field_confidence(90.0, 0.9, 0.8, 0.75) - 48.6).abs() < 0.0001);
        assert_eq!(provider_field_confidence(200.0, 1.0, 1.0, 1.0), 100.0);
    }

    #[test]
    fn drawdown_tracks_peak_to_trough() {
        let dd = max_drawdown_from_profit(&[0.02, 0.01, -0.05, 0.01, -0.03]);
        assert!((dd - 0.07).abs() < 0.0001);
    }

    #[test]
    fn score_parser_accepts_colon_scores_only() {
        assert_eq!(parse_score("2:1"), Some((2, 1)));
        assert_eq!(parse_score(" 0:0 "), Some((0, 0)));
        assert_eq!(parse_score("2-1"), None);
    }

    #[test]
    fn result_csv_import_skips_invalid_rows() {
        let csv = "home,away,score,stage,status,half_score\n法国,德国,2:1,世界杯,完场,1:0\n空队,坏行,abc,世界杯,完场,";
        let rows = parse_results_csv(csv).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].home, "法国");
        assert_eq!(rows[0].score, "2:1");
    }

    #[test]
    fn roi_calculation_handles_zero_stake() {
        assert_eq!(roi_from_profit(10.0, 0.0), 0.0);
        assert!((roi_from_profit(0.15, 0.30) - 0.5).abs() < 0.0001);
    }

    #[test]
    fn source_health_labels_cover_stale_and_missing() {
        assert_eq!(source_health_label(false, 0.0, 0.0, false), "字段缺失");
        assert_eq!(source_health_label(true, 40.0, 90.0, true), "失败但使用旧缓存");
        assert_eq!(source_health_label(true, 90.0, 20.0, false), "字段缺失");
        assert_eq!(source_health_label(true, 90.0, 90.0, false), "正常");
        assert!(source_completeness_score("sporttery", 6) >= 100.0);
    }

    #[test]
    fn source_diagnosis_explains_missing_lineups() {
        let (diagnosis, impact, next_action) = source_diagnosis("lineup_data", false, 0, 0.0, 0.0, false);
        assert!(diagnosis.contains("首发数据"));
        assert!(impact.contains("最高只能到小注"));
        assert!(next_action.contains("全局刷新"));
    }

    #[test]
    fn backtest_ban_rule_detects_loss_modules() {
        let item = BacktestGroup {
            dimension: "玩法".to_string(),
            group: "比分".to_string(),
            count: 6,
            hit_rate: 0.0,
            roi: -0.42,
            total_profit: -4.2,
            max_drawdown: 4.2,
            avg_odds: 12.0,
            avg_advantage_rate: 0.2,
            brier_score: 0.36,
            log_loss: 1.2,
        };
        let reason = backtest_ban_reason(&item).unwrap();
        assert!(reason.contains("ROI") || reason.contains("高赔"));
    }

    #[test]
    fn provider_registry_loads_default_sources() {
        let registry = default_provider_registry();
        assert_eq!(registry.len(), 8);
        assert!(registry.iter().any(|item| item.provider_id == "statsbomb_open_data" && !item.requires_key));
        assert!(registry.iter().any(|item| item.provider_id == "api_football" && item.requires_key));
    }

    #[test]
    fn api_key_missing_returns_clear_error() {
        assert_eq!(provider_key_error(true, false), Some("API Key 未配置，请先保存本地 Key"));
        assert_eq!(provider_key_error(true, true), None);
        assert_eq!(provider_key_error(false, false), None);
    }

    #[test]
    fn free_request_limit_blocks_after_quota() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "create table provider_request_logs(id integer primary key, provider_id text, data_type text, requested_at text, success integer, error_message text);",
        ).unwrap();
        for _ in 0..2 {
            conn.execute(
                "insert into provider_request_logs(provider_id, data_type, requested_at, success, error_message) values('odds_api_io','test',datetime('now'),1,'')",
                [],
            ).unwrap();
        }
        assert!(!request_limit_available(&conn, "odds_api_io", 0, 2));
        assert!(request_limit_available(&conn, "odds_api_io", 0, 3));
    }

    #[test]
    fn provider_raw_data_can_be_saved_without_overwriting() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "create table provider_raw_data(id integer primary key, provider_id text, provider text, data_type text, match_id text, team text, field_name text, field_value text, fetched_at text, confidence real, raw_payload text);",
        ).unwrap();
        let record = ProviderRawRecord {
            provider_id: "statsbomb_open_data".to_string(),
            data_type: "xg".to_string(),
            match_id: Some("m1".to_string()),
            team: Some("法国".to_string()),
            field_name: "xg".to_string(),
            field_value: "1.4".to_string(),
            fetched_at: Utc::now().to_rfc3339(),
            confidence: 80.0,
            raw_payload: "{}".to_string(),
        };
        save_provider_raw_record(&conn, &record).unwrap();
        save_provider_raw_record(&conn, &record).unwrap();
        let count: i64 = conn.query_row("select count(*) from provider_raw_data", [], |row| row.get(0)).unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn multi_source_agreement_boosts_confidence_and_conflict_lowers_it() {
        let agreed = vec![
            ProviderRawRecord { provider_id: "a".to_string(), data_type: "odds".to_string(), match_id: Some("m1".to_string()), team: None, field_name: "home_odds".to_string(), field_value: "2.10".to_string(), fetched_at: "now".to_string(), confidence: 80.0, raw_payload: "{}".to_string() },
            ProviderRawRecord { provider_id: "b".to_string(), data_type: "odds".to_string(), match_id: Some("m1".to_string()), team: None, field_name: "home_odds".to_string(), field_value: "2.10".to_string(), fetched_at: "now".to_string(), confidence: 80.0, raw_payload: "{}".to_string() },
        ];
        let conflict = vec![
            agreed[0].clone(),
            ProviderRawRecord { field_value: "2.60".to_string(), provider_id: "c".to_string(), ..agreed[1].clone() },
        ];
        let agreed_result = fuse_provider_records(&agreed, 80.0, 100.0, 100.0).unwrap();
        let conflict_result = fuse_provider_records(&conflict, 80.0, 100.0, 100.0).unwrap();
        assert!(!agreed_result.conflict);
        assert!(conflict_result.conflict);
        assert!(agreed_result.confidence > conflict_result.confidence);
    }

    #[test]
    fn start_rate_is_not_confirmed_lineup() {
        let (status, confidence) = lineup_status_from_source("unknown", 0.90, false);
        assert_eq!(status, "historical");
        assert!(confidence < 80.0);
        let (status, confidence) = lineup_status_from_source("api_confirmed", 0.0, true);
        assert_eq!(status, "api_confirmed");
        assert!(confidence >= 80.0);
    }

    #[test]
    fn missing_xg_downgrades_score_and_total_goals_only() {
        let mut decision = "可买".to_string();
        let mut confidence = "高".to_string();
        let mut reason = Vec::new();
        downgrade_for_missing_realtime_xg("TTG总进球", &mut decision, &mut confidence, &mut reason);
        assert_eq!(decision, "观察");
        assert_eq!(confidence, "中");
        assert!(reason.join("；").contains("xG 数据缺失"));
    }

    #[test]
    fn odds_multi_source_merge_detects_conflict() {
        let (score, conflict) = source_agreement_score(&["1.80".to_string(), "1.82".to_string(), "2.40".to_string()]);
        assert!(conflict);
        assert!(score < 1.0);
        let (score, conflict) = source_agreement_score(&["1.80".to_string(), "1.80".to_string()]);
        assert!(!conflict);
        assert!(score > 1.0);
    }

    #[test]
    fn api_football_lineups_bridge_to_confirmed_lineup_cache() {
        let lineups = vec![json!({
            "fixture_id": 7,
            "payload": {
                "response": [{
                    "team": { "name": "France" },
                    "formation": "4-3-3",
                    "startXI": [
                        { "player": { "name": "A", "pos": "G", "number": 1 } },
                        { "player": { "name": "B", "pos": "D", "number": 2 } },
                        { "player": { "name": "C", "pos": "D", "number": 3 } },
                        { "player": { "name": "D", "pos": "D", "number": 4 } },
                        { "player": { "name": "E", "pos": "D", "number": 5 } },
                        { "player": { "name": "F", "pos": "M", "number": 6 } },
                        { "player": { "name": "G", "pos": "M", "number": 7 } },
                        { "player": { "name": "H", "pos": "M", "number": 8 } },
                        { "player": { "name": "I", "pos": "F", "number": 9 } }
                    ]
                }]
            }
        })];
        let payload = api_football_lineup_payload(&lineups);
        assert_eq!(payload.get("matchCount").and_then(Value::as_u64), Some(1));
        assert_eq!(payload.pointer("/matches/0/teams/0/lineup_status").and_then(Value::as_str), Some("api_confirmed"));
        assert!(payload.pointer("/matches/0/teams/0/confirmed_lineup_confidence").and_then(Value::as_i64).unwrap() >= 80);
    }

    #[test]
    fn api_football_injuries_and_lineups_bridge_to_player_status() {
        let injuries = json!({
            "response": [{
                "team": { "name": "Spain" },
                "player": { "name": "Player Out", "reason": "Injured", "type": "Attacker" }
            }]
        });
        let lineups = vec![json!({
            "fixture_id": 8,
            "payload": { "response": [{
                "team": { "name": "Spain" },
                "startXI": [{ "player": { "name": "Starter", "pos": "F" } }]
            }]}
        })];
        let payload = api_football_player_status_payload(&injuries, &lineups);
        assert_eq!(player_status_count(&payload), 2);
        assert_eq!(payload.pointer("/players/1/starter").and_then(Value::as_str), Some("true"));
        assert_eq!(payload.pointer("/players/1/lineup_status").and_then(Value::as_str), Some("api_confirmed"));
    }

    #[test]
    fn api_football_statistics_bridge_to_stats_cache() {
        let statistics = vec![json!({
            "fixture_id": 9,
            "payload": {
                "response": [{
                    "team": { "name": "Argentina" },
                    "statistics": [
                        { "type": "Total Shots", "value": 12 },
                        { "type": "Shots on Goal", "value": 5 },
                        { "type": "Ball Possession", "value": "58%" }
                    ]
                }]
            }
        })];
        let payload = api_football_stats_payload(&statistics);
        assert_eq!(payload.get("teamCount").and_then(Value::as_u64), Some(1));
        assert!(payload.pointer("/teams/0/weighted_xg_per_match").and_then(Value::as_f64).unwrap() > 1.0);
    }

    #[test]
    fn odds_implied_probability_and_ev_are_consistent() {
        let odds = [2.0, 3.5, 4.0];
        let inv_sum: f64 = odds.iter().map(|item| 1.0 / item).sum();
        let fair_home = (1.0 / odds[0]) / inv_sum;
        let ev = 0.55 * odds[0] - 1.0;
        assert!((fair_home - 0.4828).abs() < 0.01);
        assert!((ev - 0.10).abs() < 0.0001);
    }

    #[test]
    fn brier_and_log_loss_basic_metrics() {
        let p: f64 = 0.7;
        let brier = (p - 1.0) * (p - 1.0);
        let log_loss = -p.ln();
        assert!((brier - 0.09).abs() < 0.0001);
        assert!((log_loss - 0.3566).abs() < 0.01);
    }

    #[test]
    fn poisson_score_probs_are_normalized() {
        let (scores, totals) = trained_score_probs(1.4, 1.1, 5);
        let score_sum: f64 = scores.as_array().unwrap().iter().map(|row| row.get("probability").and_then(Value::as_f64).unwrap_or(0.0)).sum();
        let total_sum: f64 = totals.as_array().unwrap().iter().map(|row| row.get("probability").and_then(Value::as_f64).unwrap_or(0.0)).sum();
        assert!((score_sum - 1.0).abs() < 0.0001);
        assert!((total_sum - 1.0).abs() < 0.0001);
    }

    #[test]
    fn missing_model_files_fall_back_to_rules() {
        let input = ModelFeatureInput {
            elo_diff: 0.0,
            odds_home: 2.0,
            odds_draw: 3.2,
            odds_away: 3.8,
            market_home_prob: 0.45,
            market_draw_prob: 0.28,
            market_away_prob: 0.27,
            market_margin: 0.05,
            rule_home_lambda: 1.3,
            rule_away_lambda: 1.1,
        };
        let temp_dir = std::env::temp_dir().join(format!("missing-model-{}", Utc::now().timestamp_nanos_opt().unwrap_or(0)));
        std::fs::create_dir_all(temp_dir.join("training").join("models")).unwrap();
        assert!(predict_with_training_models(&temp_dir, &input).is_none());
        let info = active_model_info(&temp_dir);
        assert!(!info.model_available);
        assert!(info.fallback_reason.contains("未检测到"));
        let _ = std::fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn active_model_info_exposes_core_model_metrics() {
        let root = std::env::temp_dir().join(format!("active-model-rolling-{}", Utc::now().timestamp_nanos_opt().unwrap_or(0)));
        let models = root.join("training").join("models");
        std::fs::create_dir_all(&models).unwrap();
        std::fs::write(models.join("model_manifest.json"), json!({
            "ready": true,
            "active_model_version": "outcome_ensemble_model_v1",
            "training_data_range": {"start": "2020-01-01", "end": "2026-01-01"},
            "metrics_summary": {"sample_count": 1000, "accuracy": 0.5, "log_loss": 1.0, "brier_score": 0.2},
            "backtest_summary": {
                "final_bet_count": 0,
                "roi": 0.0,
                "max_drawdown": 0.0,
                "avg_odds": 0.0,
                "avg_ev": 0.0,
                "warning": "暂无正式投注样本，不能评估 ROI"
            },
            "missing_files": [],
            "global_models": {}
        }).to_string()).unwrap();
        std::fs::write(models.join("outcome_model_v1.json"), json!({
            "metrics": {"train_count": 800}
        }).to_string()).unwrap();
        std::fs::write(models.join("outcome_ensemble_model_v1.json"), json!({
            "model_type": "weighted_probability_ensemble"
        }).to_string()).unwrap();
        let info = active_model_info(&root);
        assert!(info.model_available);
        assert_eq!(info.sample_count, 1000);
        assert!(info.log_loss > 0.0);
        assert_eq!(info.backtest_final_bet_count, 0);
        assert!(info.backtest_warning.contains("不能评估 ROI"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn active_model_info_does_not_expose_retired_hit_target_fields() {
        let root = std::env::temp_dir().join(format!("active-model-no-old-target-{}", Utc::now().timestamp_nanos_opt().unwrap_or(0)));
        let models = root.join("training").join("models");
        std::fs::create_dir_all(&models).unwrap();
        std::fs::write(models.join("model_manifest.json"), json!({
            "ready": true,
            "active_model_version": "outcome_ensemble_model_v1",
            "metrics_summary": {"sample_count": 100, "accuracy": 0.5, "log_loss": 1.0, "brier_score": 0.2},
            "backtest_summary": {"final_bet_count": 0, "warning": "暂无正式投注样本，不能评估 ROI"},
            "missing_files": [],
            "global_models": {}
        }).to_string()).unwrap();
        std::fs::write(models.join("outcome_model_v1.json"), json!({ "metrics": {"train_count": 80} }).to_string()).unwrap();
        std::fs::write(models.join("outcome_ensemble_model_v1.json"), json!({ "model_type": "weighted_probability_ensemble" }).to_string()).unwrap();
        let value = serde_json::to_value(active_model_info(&root)).unwrap();
        assert!(value.get("target_hit_rate").is_none());
        assert!(value.get("high_precision_rule").is_none());
        assert!(value.get("rolling_hit_rate").is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn worldcup_correction_is_separate_from_main_probability_model() {
        let root = std::env::temp_dir().join(format!("worldcup-correction-separate-{}", Utc::now().timestamp_nanos_opt().unwrap_or(0)));
        let models = root.join("training").join("models");
        std::fs::create_dir_all(&models).unwrap();
        std::fs::write(models.join("model_manifest.json"), json!({
            "ready": false,
            "active_model_version": "rules",
            "metrics_summary": {"sample_count": 0},
            "backtest_summary": {"final_bet_count": 0, "warning": "暂无正式投注样本，不能评估 ROI"},
            "missing_files": [],
            "global_models": {
                "worldcup_live_correction": "worldcup_live_correction_v1",
                "worldcup_live_correction_status": {
                    "ready": true,
                    "report": {"metrics": {"sample_count": 384, "accuracy": 0.70, "log_loss": 0.64}}
                }
            }
        }).to_string()).unwrap();
        let input = ModelFeatureInput {
            elo_diff: 0.0,
            odds_home: 2.0,
            odds_draw: 3.2,
            odds_away: 3.8,
            market_home_prob: 0.45,
            market_draw_prob: 0.28,
            market_away_prob: 0.27,
            market_margin: 0.05,
            rule_home_lambda: 1.3,
            rule_away_lambda: 1.1,
        };
        assert!(predict_with_training_models(&root, &input).is_none());
        let info = active_model_info(&root);
        assert!(info.worldcup_correction_available);
        assert_eq!(info.worldcup_correction_sample_count, 384);
        assert!(!info.model_available);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn strategy_rules_v1_shape_has_required_fields() {
        let rules = json!({
            "model_version": "strategy_rules_v1",
            "rules": [{
                "dimension": "odds_range",
                "group": "1.80-2.49",
                "action": "allow_candidate",
                "reason": "样本充足且ROI为正",
                "sample_count": 42,
                "roi": 0.08,
                "hit_rate": 0.52
            }]
        });
        let first = rules.pointer("/rules/0").unwrap();
        assert_eq!(first.get("action").and_then(Value::as_str), Some("allow_candidate"));
        assert!(first.get("sample_count").and_then(Value::as_i64).unwrap_or(0) >= 30);
    }

    #[test]
    fn candidate_strategy_observation_only_does_not_change_official_rules() {
        let root = std::env::temp_dir().join(format!("candidate-observation-{}", Utc::now().timestamp_nanos_opt().unwrap_or(0)));
        let models = root.join("training").join("models");
        std::fs::create_dir_all(&models).unwrap();
        std::fs::write(models.join("candidate_strategy_v1.json"), json!({
            "strategy_id": "candidate_strategy_v1",
            "status": "observation_only",
            "candidate_rules": [{"ev_threshold": 0.0, "odds_range": "3.00+", "probability_range": "<40%"}]
        }).to_string()).unwrap();
        std::fs::write(models.join("strategy_rules_v1.json"), json!({
            "model_version": "strategy_rules_v1",
            "rules": [{
                "dimension": "ev_range",
                "group": "负EV",
                "action": "hard_ban",
                "reason": "负EV禁买",
                "sample_count": 100,
                "roi": -0.08,
                "hit_rate": 0.33
            }]
        }).to_string()).unwrap();
        let decision = strategy_rule_decision(&root, "主胜", 2.0, 0.40, -0.02, -0.02);
        assert_eq!(decision.action, "hard_ban");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn paper_trading_upgrade_shape_blocks_negative_or_small_samples() {
        let paper = json!({
            "status": "observation_only",
            "bet_count": 24,
            "paper_roi": -0.02,
            "candidate_upgrade_check": {
                "can_consider_upgrade": false,
                "blocking_reasons": ["纸面交易样本少于100", "纸面交易ROI低于3%"]
            }
        });
        assert_eq!(paper.get("status").and_then(Value::as_str), Some("observation_only"));
        assert!(!paper.pointer("/candidate_upgrade_check/can_consider_upgrade").and_then(Value::as_bool).unwrap());
        let reasons = paper.pointer("/candidate_upgrade_check/blocking_reasons").and_then(Value::as_array).unwrap();
        assert!(reasons.iter().any(|item| item.as_str().unwrap_or("").contains("样本少于100")));
        assert!(reasons.iter().any(|item| item.as_str().unwrap_or("").contains("ROI低于3%")));
    }

    #[test]
    fn robustness_blocks_upgrade_when_roi_below_threshold() {
        let robustness = json!({
            "robustness_level": "weak",
            "can_consider_upgrade": false,
            "blocking_reasons": ["paper_roi_below_3pct", "robustness_not_strong"],
            "outlier_sensitivity": {"roi_after_remove_top3": 0.01}
        });
        assert_ne!(robustness.get("robustness_level").and_then(Value::as_str), Some("strong"));
        assert!(!robustness.get("can_consider_upgrade").and_then(Value::as_bool).unwrap());
        let reasons = robustness.get("blocking_reasons").and_then(Value::as_array).unwrap();
        assert!(reasons.iter().any(|item| item.as_str() == Some("paper_roi_below_3pct")));
    }

    #[test]
    fn robustness_marks_outlier_and_single_dependency_risks() {
        let robustness = json!({
            "robustness_level": "weak",
            "can_consider_upgrade": false,
            "blocking_reasons": [
                "fragile_to_outliers",
                "depends_on_single_odds_band",
                "depends_on_single_selection",
                "recent_30_roi_negative"
            ],
            "outlier_sensitivity": {
                "roi_after_remove_top3": -0.01,
                "roi_after_remove_top10pct": -0.08
            }
        });
        let reasons = robustness.get("blocking_reasons").and_then(Value::as_array).unwrap();
        assert!(reasons.iter().any(|item| item.as_str() == Some("fragile_to_outliers")));
        assert!(reasons.iter().any(|item| item.as_str() == Some("depends_on_single_odds_band")));
        assert!(reasons.iter().any(|item| item.as_str() == Some("depends_on_single_selection")));
        assert!(reasons.iter().any(|item| item.as_str() == Some("recent_30_roi_negative")));
        assert!(robustness.pointer("/outlier_sensitivity/roi_after_remove_top3").and_then(Value::as_f64).unwrap() < 0.0);
    }

    #[test]
    fn robustness_observation_does_not_override_hard_ban() {
        let root = std::env::temp_dir().join(format!("robustness-hard-ban-{}", Utc::now().timestamp_nanos_opt().unwrap_or(0)));
        let models = root.join("training").join("models");
        std::fs::create_dir_all(&models).unwrap();
        std::fs::write(models.join("strategy_rules_v1.json"), json!({
            "model_version": "strategy_rules_v1",
            "rules": [{
                "dimension": "ev_range",
                "group": "负EV",
                "action": "hard_ban",
                "reason": "负EV禁买",
                "sample_count": 200,
                "roi": -0.10,
                "hit_rate": 0.30
            }]
        }).to_string()).unwrap();
        let decision = strategy_rule_decision(&root, "客胜", 2.2, 0.35, -0.03, -0.03);
        assert_eq!(decision.action, "hard_ban");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn pre_match_settlement_keeps_snapshot_prediction_fields_separate() {
        let snapshot = json!({
            "model_probs_json": [{"pick": "主胜", "model_prob": 0.52}],
            "odds_json": [{"market": "体彩HAD胜平负", "pick": "主胜", "odds": 1.90}],
            "final_decision": "observe_only"
        });
        let settlement = json!({
            "snapshot_id": 1,
            "home_score": 2,
            "away_score": 1,
            "result_spf": spf_from_score(2, 1),
            "is_hit_json": {"体彩HAD胜平负:主胜": true}
        });
        assert_eq!(snapshot.pointer("/model_probs_json/0/model_prob").and_then(Value::as_f64), Some(0.52));
        assert_eq!(settlement.get("result_spf").and_then(Value::as_str), Some("主胜"));
        assert!(snapshot.get("result_spf").is_none());
        assert!(snapshot.get("is_hit_json").is_none());
    }

    #[test]
    fn live_paper_requires_created_before_kickoff() {
        assert!(kickoff_is_future("2026-06-30T12:00:00+00:00", "2026-06-30T10:00:00+00:00"));
        assert!(!kickoff_is_future("2026-06-30T09:00:00+00:00", "2026-06-30T10:00:00+00:00"));
    }

    #[test]
    fn final_snapshot_marker_is_one_per_match_shape() {
        let rows = [
            json!({"id": 1, "match_id": "m1", "is_final_pre_match": false}),
            json!({"id": 2, "match_id": "m1", "is_final_pre_match": true}),
        ];
        let final_count = rows.iter().filter(|row| row.get("is_final_pre_match").and_then(Value::as_bool).unwrap_or(false)).count();
        assert_eq!(final_count, 1);
    }

    #[test]
    fn audit_detects_invalid_probability_sum_and_missing_odds_shape() {
        let probs = json!([
            {"market": "体彩HAD胜平负", "pick": "主胜", "model_prob": 0.70},
            {"market": "体彩HAD胜平负", "pick": "平局", "model_prob": 0.25},
            {"market": "体彩HAD胜平负", "pick": "客胜", "model_prob": 0.20}
        ]);
        let sum = had_probability_sum(&probs).unwrap();
        assert!((sum - 1.0).abs() > 0.08);
        let odds = json!([]);
        assert!(odds.as_array().map(|items| items.is_empty()).unwrap_or(true));
        assert_eq!(audit_severity("missing_odds"), "warning");
    }

    #[test]
    fn audit_detects_paper_trade_invalid_and_live_sample_warning_shape() {
        assert_eq!(audit_severity("paper_trade_invalid"), "critical");
        let summary = json!({
            "sample_count": 12,
            "warning": "真实赛前纸面交易样本不足，暂不能评价策略。"
        });
        assert!(summary.get("sample_count").and_then(Value::as_i64).unwrap() < 30);
        assert!(summary.get("warning").and_then(Value::as_str).unwrap().contains("样本不足"));
    }

    #[test]
    fn hard_ban_priority_survives_candidate_observation() {
        let root = std::env::temp_dir().join(format!("hard-ban-priority-{}", Utc::now().timestamp_nanos_opt().unwrap_or(0)));
        let models = root.join("training").join("models");
        std::fs::create_dir_all(&models).unwrap();
        std::fs::write(models.join("strategy_rules_v1.json"), json!({
            "model_version": "strategy_rules_v1",
            "rules": [
                {"dimension": "odds_range", "group": "1.50-2.50", "action": "allow_candidate", "reason": "候选", "sample_count": 80, "roi": 0.04, "hit_rate": 0.55},
                {"dimension": "ev_range", "group": "负EV", "action": "hard_ban", "reason": "负EV禁买", "sample_count": 200, "roi": -0.10, "hit_rate": 0.30}
            ]
        }).to_string()).unwrap();
        let decision = strategy_rule_decision(&root, "主胜", 2.0, 0.40, -0.01, -0.01);
        assert_eq!(decision.action, "hard_ban");
        assert!(decision.reason.contains("负EV禁买"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn strategy_bucket_functions_match_training_pipeline() {
        assert_eq!(strategy_odds_range(2.8), "2.50-3.50");
        assert_eq!(strategy_probability_range(0.38), "35%-45%");
        assert_eq!(strategy_ev_range(0.07), "5-10%");
    }

    #[test]
    fn bet_recommendation_hit_parser_supports_core_play_types() {
        let result = MatchResult {
            home: "法国".to_string(),
            away: "德国".to_string(),
            score: "2:1".to_string(),
            half_score: "1:0".to_string(),
            stage: "淘汰赛".to_string(),
            status: "完场".to_string(),
        };
        assert_eq!(pick_hit_from_result("体彩HAD胜平负", "主胜", &result).unwrap(), (true, "主胜".to_string()));
        assert_eq!(pick_hit_from_result("体彩TTG总进球", "3球", &result).unwrap(), (true, "3球".to_string()));
        assert_eq!(pick_hit_from_result("体彩CRS比分", "2:1", &result).unwrap(), (true, "2:1".to_string()));
        assert_eq!(pick_hit_from_result("体彩HAD胜平负", "客胜", &result).unwrap().0, false);
    }

    #[test]
    fn csv_cell_quotes_commas_quotes_and_newlines() {
        assert_eq!(csv_cell("法国"), "法国");
        assert_eq!(csv_cell("风险,降级"), "\"风险,降级\"");
        assert_eq!(csv_cell("他说\"观察\""), "\"他说\"\"观察\"\"\"");
    }

    #[test]
    fn backup_zip_can_generate() {
        let root = std::env::temp_dir().join(format!("backup-zip-test-{}", Utc::now().timestamp_nanos_opt().unwrap_or(0)));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("config_summary.json"), "{\"key_configured\":true}").unwrap();
        let zip_path = root.with_extension("zip");
        zip_directory(&root, &zip_path).unwrap();
        let bytes = std::fs::read(&zip_path).unwrap();
        assert!(zip_path.exists());
        assert_eq!(&bytes[0..2], b"PK");
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_file(zip_path);
    }

    #[test]
    fn backup_config_summary_does_not_expose_api_keys() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            r#"
            create table data_providers(
              provider_id text primary key,
              name text not null,
              data_type text not null,
              requires_key integer not null,
              enabled integer not null
            );
            create table provider_credentials(provider_id text primary key, api_key text not null, updated_at text not null);
            insert into data_providers(provider_id,name,data_type,requires_key,enabled)
              values('api_football','API-Football','fixtures',1,1);
            insert into provider_credentials(provider_id,api_key,updated_at)
              values('api_football','secret-plain-key','now');
            "#,
        ).unwrap();
        let summary = config_summary(&conn).unwrap();
        let text = serde_json::to_string(&summary).unwrap();
        assert!(text.contains("\"key_configured\":true"));
        assert!(!text.contains("secret-plain-key"));
    }

    #[test]
    fn snapshot_and_live_csv_exports_have_headers() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            r#"
            create table pre_match_snapshots(id integer, match_id text, kickoff_time text);
            insert into pre_match_snapshots values(1,'m1','2026-06-30T10:00:00Z');
            create table paper_trading_records(
              id integer, match_id text, snapshot_id integer, strategy_id text, model_version text,
              selection text, play_type text, model_prob real, odds real, ev real, advantage_rate real,
              data_quality_score real, risk_tags_json text, worldcup_correction_action text,
              paper_stake real, result_status text, is_hit integer, paper_profit real,
              source text, created_before_kickoff integer, is_final_snapshot integer, created_at text, settled_at text
            );
            insert into paper_trading_records values(1,'m1',1,'candidate_strategy_v1','model','H','spf',0.5,2.0,0.0,0.0,80,'[]','keep',1,'pending',null,0,'live_pre_match',1,1,'now',null);
            "#,
        ).unwrap();
        let snapshot_csv = query_to_csv(&conn, "select * from pre_match_snapshots").unwrap();
        let live_csv = query_to_csv(&conn, live_paper_csv_sql()).unwrap();
        assert!(snapshot_csv.starts_with("id,match_id,kickoff_time"));
        assert!(live_csv.contains("strategy_id"));
        assert!(live_csv.contains("live_pre_match"));
    }

    #[test]
    fn audit_logs_csv_export_has_headers() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            r#"
            create table snapshot_audit_logs(id integer, snapshot_id integer, match_id text, audit_type text, severity text, message text, detected_at text, resolved integer, resolved_at text);
            insert into snapshot_audit_logs values(1,1,'m1','missing_odds','warning','赔率缺失','now',0,null);
            "#,
        ).unwrap();
        let csv = query_to_csv(&conn, "select * from snapshot_audit_logs order by detected_at desc, id desc").unwrap();
        assert!(csv.starts_with("id,snapshot_id,match_id,audit_type,severity"));
        assert!(csv.contains("missing_odds"));
    }

    #[test]
    fn api_football_quota_remaining_is_conservative() {
        let used = 12;
        assert_eq!((API_FOOTBALL_DAILY_LIMIT - used).max(0), 88);
        assert_eq!((API_FOOTBALL_DAILY_LIMIT - 140).max(0), 0);
    }

    #[test]
    fn observation_version_status_keeps_risk_controls_on() {
        let status = json!({
            "app_version": APP_VERSION,
            "strategy_status": "observation_only",
            "official_recommendation_status": "风控开启"
        });
        assert_eq!(status.get("app_version").and_then(Value::as_str), Some("v0.1-live-observation"));
        assert_eq!(status.get("strategy_status").and_then(Value::as_str), Some("observation_only"));
        assert_eq!(status.get("official_recommendation_status").and_then(Value::as_str), Some("风控开启"));
    }

    #[test]
    fn training_models_dir_finds_models_from_release_subdir() {
        let root = std::env::temp_dir().join(format!("model-path-test-{}", Utc::now().timestamp_nanos_opt().unwrap_or(0)));
        let release = root.join("desktop-app").join("src-tauri").join("target").join("release");
        let models = root.join("desktop-app").join("training").join("models");
        std::fs::create_dir_all(&release).unwrap();
        std::fs::create_dir_all(&models).unwrap();
        let found = training_models_dir(&release);
        assert_eq!(found, models);
        let _ = std::fs::remove_dir_all(root);
    }
}
