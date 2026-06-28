use anyhow::{anyhow, Context};
use chrono::Utc;
use rand::Rng;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager};

const SPORTTERY_URL: &str = "https://webapi.sporttery.cn/gateway/uniform/football/getMatchCalculatorV1.qry?channel=1&poolCode=had,hhad,crs,ttg,hafu";
const SPORTTERY_INJURY_URL: &str = "https://webapi.sporttery.cn/gateway/uniform/football/jcInfo/getAllTodayInjurySuspensionV1.qry";
const STATSBOMB_BASE: &str = "https://cdn.jsdelivr.net/gh/statsbomb/open-data@master/data";
const ZGZCW_RESULTS_URL: &str = "https://worldcup.zgzcw.com/zhuanti/worldCupsc";
const DEFAULT_ODDS_API_KEY: &str = "7eb42de8ccd78bbff0ee3dfb7c88a662";

#[derive(Debug, Serialize, Deserialize, Clone)]
struct CacheRecord {
    key: String,
    updated_at: String,
    value: Value,
}

#[derive(Debug, Serialize, Deserialize)]
struct SourceStatus {
    id: String,
    label: String,
    ok: bool,
    updated_at: Option<String>,
    count: usize,
    message: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct MatchRow {
    id: String,
    match_num: String,
    league: String,
    time: String,
    home: String,
    away: String,
    status: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct SimRequest {
    match_id: Option<String>,
    home: String,
    away: String,
    home_lambda: Option<f64>,
    away_lambda: Option<f64>,
    max_goals: Option<u32>,
    simulations: Option<u32>,
    knockout_mode: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ScoreProb {
    score: String,
    probability: f64,
}

#[derive(Debug, Serialize, Deserialize)]
struct SimMarketRow {
    pick: String,
    model_prob: f64,
    ci_low: f64,
    ci_high: f64,
    sporttery_prob: Option<f64>,
    europe_prob: Option<f64>,
    gap_vs_sporttery: Option<f64>,
    gap_vs_europe: Option<f64>,
    fair_odds: f64,
    sporttery_odds: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ProbItem {
    pick: String,
    probability: f64,
    fair_odds: f64,
    sporttery_prob: Option<f64>,
    sporttery_odds: Option<f64>,
    probability_gap: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SimResult {
    home: String,
    away: String,
    lambda_home: f64,
    lambda_away: f64,
    home_win: f64,
    home_win_low: f64,
    home_win_high: f64,
    draw: f64,
    draw_low: f64,
    draw_high: f64,
    away_win: f64,
    away_win_low: f64,
    away_win_high: f64,
    over_25: f64,
    over_25_low: f64,
    over_25_high: f64,
    btts: f64,
    btts_low: f64,
    btts_high: f64,
    total_goals: Vec<ScoreProb>,
    top_scores: Vec<ScoreProb>,
    source_note: String,
    market_rows: Vec<SimMarketRow>,
    adjustment_notes: Vec<String>,
    injury_note: String,
    movement_note: String,
    knockout_note: String,
    simulations: u32,
    simulation_note: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct PredictionRecord {
    id: Option<i64>,
    created_at: Option<String>,
    match_label: String,
    market: String,
    pick: String,
    probability: f64,
    odds: f64,
    safety_margin: f64,
    decision: String,
    stake_pct: Option<f64>,
    actual_result: Option<String>,
    profit: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize)]
struct BankrollSettings {
    bankroll: f64,
    daily_budget_pct: f64,
    max_loss_pct: f64,
    auto_refresh_minutes: i64,
}

#[derive(Debug, Serialize, Deserialize)]
struct ModelDiagnostics {
    total: i64,
    settled: i64,
    hit_rate: f64,
    roi: f64,
    brier_score: f64,
    log_loss: f64,
    calibration: Vec<CalibrationBucket>,
    market_calibration: Vec<MarketCalibration>,
    advice: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct CalibrationBucket {
    bucket: String,
    count: i64,
    avg_probability: f64,
    hit_rate: f64,
}

#[derive(Debug, Serialize, Deserialize)]
struct MarketCalibration {
    market: String,
    count: i64,
    hit_rate: f64,
    avg_probability: f64,
    brier_score: f64,
    roi: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ModelSettings {
    buy_edge: f64,
    buy_gap: f64,
    watch_edge: f64,
    watch_gap: f64,
    max_odds: f64,
    high_odds_limit: f64,
    mode: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct MatchResult {
    home: String,
    away: String,
    score: String,
    half_score: String,
    stage: String,
    status: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct ExternalSourceConfig {
    injury_url: String,
    lineup_url: String,
    stats_url: String,
    notes: String,
}

#[derive(Debug, Clone)]
struct OddsSelection {
    match_id: String,
    match_num: String,
    match_time: String,
    home: String,
    away: String,
    market: String,
    pick: String,
    odds: f64,
    fair_prob: f64,
    goal_line: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Recommendation {
    match_id: String,
    match_num: String,
    match_time: String,
    match_label: String,
    market: String,
    pick: String,
    odds: f64,
    fair_prob: f64,
    model_prob: f64,
    probability_gap: f64,
    expected_return: f64,
    stake_pct: f64,
    europe_prob: Option<f64>,
    europe_gap: Option<f64>,
    europe_odds: Option<f64>,
    decision: String,
    confidence: String,
    tier: String,
    play_style: String,
    combo_group: String,
    data_score: f64,
    data_grade: String,
    quality_action: String,
    support_factors: String,
    risk_factors: String,
    fair_odds: f64,
    advantage_rate: f64,
    action_advice: String,
    play_type_risk_level: String,
    lineup_status: String,
    lineup_confidence: f64,
    anomaly_type: String,
    anomaly_severity: String,
    anomaly_direction: String,
    anomaly_advice: String,
    reason: String,
}

#[derive(Debug, Clone)]
struct EuropeConsensus {
    home_prob: f64,
    draw_prob: f64,
    away_prob: f64,
    home_odds: f64,
    draw_odds: f64,
    away_odds: f64,
    bookmaker_count: usize,
}

#[derive(Debug, Serialize, Deserialize)]
struct MatchAnalysis {
    match_id: String,
    match_num: String,
    match_time: String,
    match_label: String,
    lambda_home: f64,
    lambda_away: f64,
    knockout_note: String,
    had: Vec<ProbItem>,
    hhad: Vec<ProbItem>,
    ttg: Vec<ProbItem>,
    scores: Vec<ProbItem>,
    europe_note: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct OddsMovement {
    id: i64,
    created_at: String,
    match_label: String,
    market: String,
    pick: String,
    initial_odds: f64,
    previous_odds: f64,
    current_odds: f64,
    delta_abs: f64,
    delta_pct: f64,
    direction: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct OddsAnomaly {
    id: i64,
    created_at: String,
    match_id: String,
    match_label: String,
    market: String,
    pick: String,
    anomaly_type: String,
    severity: String,
    impact_direction: String,
    advice: String,
    delta_abs: f64,
    delta_pct: f64,
}

#[derive(Debug, Serialize, Deserialize)]
struct BacktestGroup {
    dimension: String,
    group: String,
    count: i64,
    hit_rate: f64,
    roi: f64,
    total_profit: f64,
    max_drawdown: f64,
    avg_odds: f64,
    avg_advantage_rate: f64,
    brier_score: f64,
    log_loss: f64,
}

#[derive(Debug, Serialize, Deserialize)]
struct BacktestReport {
    groups: Vec<BacktestGroup>,
    most_profitable: String,
    most_loss: String,
    ban_rule_advice: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct TodayBetPlan {
    bankroll: f64,
    daily_budget: f64,
    max_loss: f64,
    singles: Vec<Recommendation>,
    combos: Vec<Recommendation>,
    banned: Vec<Recommendation>,
    watch: Vec<Recommendation>,
    wait_notes: Vec<String>,
    review_hint: String,
}

fn app_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    Ok(dir)
}

fn db_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app_dir(app)?.join("worldcup-odds.sqlite"))
}

fn open_conn(app: &AppHandle) -> Result<Connection, String> {
    let conn = Connection::open(db_path(app)?).map_err(|error| error.to_string())?;
    conn.execute_batch(
        r#"
        create table if not exists cache (
          key text primary key,
          updated_at text not null,
          value text not null
        );
        create table if not exists predictions (
          id integer primary key autoincrement,
          created_at text not null,
          match_label text not null,
          market text not null,
          pick text not null,
          probability real not null,
          odds real not null,
          safety_margin real not null,
          decision text not null
        );
        create table if not exists odds_snapshots (
          id integer primary key autoincrement,
          created_at text not null,
          match_id text not null,
          match_label text not null,
          market text not null,
          pick text not null,
          odds real not null
        );
        create index if not exists idx_odds_snapshots_key
          on odds_snapshots(match_id, market, pick, id);
        create table if not exists odds_movements (
          id integer primary key autoincrement,
          created_at text not null,
          match_id text not null,
          match_label text not null,
          market text not null,
          pick text not null,
          initial_odds real not null,
          previous_odds real not null,
          current_odds real not null,
          delta_abs real not null,
          delta_pct real not null,
          direction text not null
        );
        create table if not exists prediction_snapshots (
          id integer primary key autoincrement,
          created_at text not null,
          match_id text not null,
          match_num text not null,
          match_time text not null,
          match_label text not null,
          model_input text not null,
          probabilities text not null,
          odds_payload text not null,
          data_quality_score real not null,
          data_quality_grade text not null,
          quality_action text not null,
          risk_tags text not null,
          model_version text not null
        );
        create table if not exists bet_recommendations (
          id integer primary key autoincrement,
          snapshot_id integer,
          created_at text not null,
          match_id text not null,
          match_num text not null,
          match_time text not null,
          match_label text not null,
          market text not null,
          pick text not null,
          model_prob real not null,
          sporttery_prob real not null,
          europe_prob real,
          fair_odds real not null,
          current_odds real not null,
          ev real not null,
          advantage_rate real not null,
          recommendation_level text not null,
          action_advice text not null,
          stake_pct real not null,
          data_quality_score real not null,
          data_quality_grade text not null,
          risk_tags text not null,
          play_type_risk_level text not null,
          anomaly_type text not null default '',
          anomaly_severity text not null default '',
          raw_payload text not null
        );
        create table if not exists match_results (
          id integer primary key autoincrement,
          match_id text,
          match_label text not null,
          home text not null,
          away text not null,
          score text not null,
          half_score text not null default '',
          stage text not null default '',
          status text not null default '',
          source text not null default '',
          fetched_at text not null
        );
        create table if not exists bet_results (
          id integer primary key autoincrement,
          recommendation_id integer,
          settled_at text not null,
          match_label text not null,
          market text not null,
          pick text not null,
          hit integer not null,
          stake_pct real not null,
          odds real not null,
          profit real not null,
          result_score text not null
        );
        create table if not exists odds_anomalies (
          id integer primary key autoincrement,
          created_at text not null,
          match_id text not null,
          match_label text not null,
          market text not null,
          pick text not null,
          anomaly_type text not null,
          severity text not null,
          impact_direction text not null,
          advice text not null,
          delta_abs real not null,
          delta_pct real not null
        );
        create table if not exists match_lineup_sources (
          id integer primary key autoincrement,
          match_id text not null,
          provider text not null,
          fetched_at text not null,
          lineup_status text not null,
          confidence real not null,
          raw_payload text not null
        );
        create table if not exists match_lineups (
          id integer primary key autoincrement,
          match_id text not null,
          team text not null,
          player text not null,
          position text not null default '',
          lineup_status text not null,
          confirmed_lineup integer not null default 0,
          confirmed_lineup_confidence real not null default 0,
          start_rate real not null default 0,
          provider text not null default '',
          fetched_at text not null,
          raw_payload text not null default ''
        );
        create table if not exists provider_raw_data (
          id integer primary key autoincrement,
          provider text not null,
          data_type text not null,
          match_id text,
          team text,
          field_name text not null,
          field_value text not null,
          fetched_at text not null,
          confidence real not null,
          raw_payload text not null
        );
        create table if not exists provider_final_values (
          id integer primary key autoincrement,
          data_type text not null,
          match_id text,
          team text,
          field_name text not null,
          final_value text not null,
          confidence real not null,
          provider_count integer not null,
          updated_at text not null
        );
        "#,
    )
    .map_err(|error| error.to_string())?;
    let migrations = [
        "alter table predictions add column stake_pct real default 0",
        "alter table predictions add column actual_result text default ''",
        "alter table predictions add column profit real default 0",
    ];
    for sql in migrations {
        let _ = conn.execute(sql, []);
    }
    Ok(conn)
}

fn cache_put(conn: &Connection, key: &str, value: &Value) -> anyhow::Result<()> {
    conn.execute(
        "insert into cache(key, updated_at, value) values(?1, ?2, ?3)
         on conflict(key) do update set updated_at=excluded.updated_at, value=excluded.value",
        params![key, Utc::now().to_rfc3339(), value.to_string()],
    )?;
    Ok(())
}

fn cache_get(conn: &Connection, key: &str) -> anyhow::Result<Option<CacheRecord>> {
    let mut stmt = conn.prepare("select key, updated_at, value from cache where key=?1")?;
    let mut rows = stmt.query(params![key])?;
    if let Some(row) = rows.next()? {
        let value_text: String = row.get(2)?;
        Ok(Some(CacheRecord {
            key: row.get(0)?,
            updated_at: row.get(1)?,
            value: serde_json::from_str(&value_text)?,
        }))
    } else {
        Ok(None)
    }
}

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

async fn http_json(url: &str) -> anyhow::Result<Value> {
    let client = reqwest::Client::builder()
        .user_agent("worldcup-odds-desktop/0.1")
        .timeout(std::time::Duration::from_secs(35))
        .build()?;
    let response = client.get(url).send().await?;
    if !response.status().is_success() {
        return Err(anyhow!("HTTP {}", response.status()));
    }
    let text = response.text().await?;
    Ok(serde_json::from_str(text.trim_start_matches('\u{feff}'))?)
}

async fn http_sporttery_mobile_json(url: &str) -> anyhow::Result<Value> {
    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (iPhone; CPU iPhone OS 16_0 like Mac OS X) AppleWebKit/605.1.15 Mobile/15E148")
        .timeout(std::time::Duration::from_secs(35))
        .build()?;
    let response = client
        .get(url)
        .header("Referer", "https://m.sporttery.cn/mjc/styl/index.html")
        .header("Origin", "https://m.sporttery.cn")
        .header("Accept", "application/json,text/plain,*/*")
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(anyhow!("HTTP {}", response.status()));
    }
    let text = response.text().await?;
    Ok(serde_json::from_str(text.trim_start_matches('\u{feff}'))?)
}

async fn http_sporttery_browser_json(url: &str) -> anyhow::Result<Value> {
    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/126 Safari/537.36")
        .timeout(std::time::Duration::from_secs(35))
        .build()?;
    let response = client
        .get(url)
        .header("Referer", "https://www.sporttery.cn/jc/jsq/zqspf/")
        .header("Origin", "https://www.sporttery.cn")
        .header("Accept", "application/json,text/plain,*/*")
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(anyhow!("HTTP {}", response.status()));
    }
    let text = response.text().await?;
    Ok(serde_json::from_str(text.trim_start_matches('\u{feff}'))?)
}

async fn http_text(url: &str) -> anyhow::Result<String> {
    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 worldcup-odds-desktop/0.1")
        .timeout(std::time::Duration::from_secs(35))
        .build()?;
    let response = client.get(url).send().await?;
    if !response.status().is_success() {
        return Err(anyhow!("HTTP {}", response.status()));
    }
    Ok(response.text().await?)
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

fn quality_action(score: f64) -> &'static str {
    if score < 55.0 {
        "建议跳过"
    } else if score < 65.0 {
        "只看预测，不建议购买"
    } else if score < 75.0 {
        "观察或极小注"
    } else if score < 85.0 {
        "可小注"
    } else {
        "可进入正式推荐"
    }
}

fn lineup_status_rank(status: &str) -> i32 {
    match status {
        "official" => 5,
        "confirmed" => 4,
        "reported" => 3,
        "probable" => 2,
        "predicted" => 1,
        _ => 0,
    }
}

fn lineup_status_for_match(conn: &Connection, match_id: &str) -> (String, f64) {
    conn.query_row(
        "select lineup_status, max(confidence) from match_lineup_sources where match_id=?1 group by lineup_status order by max(confidence) desc limit 1",
        params![match_id],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?)),
    )
    .unwrap_or_else(|_| ("unknown".to_string(), 0.0))
}

fn play_type_risk_level(market: &str) -> &'static str {
    if market.starts_with("CRS") {
        "极高"
    } else if market.starts_with("TTG") {
        "高"
    } else if market.starts_with("HHAD") {
        "中"
    } else {
        "低"
    }
}

fn apply_quality_and_play_rules(
    market: &str,
    decision: &mut String,
    confidence: &mut String,
    stake: &mut f64,
    reasons: &mut Vec<String>,
    data_score: f64,
    lineup_status: &str,
    lineup_confidence: f64,
) {
    if data_score < 55.0 {
        *decision = "禁止".to_string();
        *confidence = "低".to_string();
        *stake = 0.0;
        reasons.push("数据质量建议：建议跳过".to_string());
    } else if data_score < 65.0 {
        if decision == "可买" {
            *decision = "观察".to_string();
        }
        *stake = stake.min(0.0015);
        reasons.push("数据质量建议：只看预测，不建议购买".to_string());
    } else if data_score < 75.0 {
        if decision == "可买" {
            *decision = "观察".to_string();
        }
        *stake = stake.min(0.0025);
        reasons.push("数据质量建议：观察或极小注".to_string());
    } else if data_score < 85.0 {
        *stake = stake.min(0.005);
        reasons.push("数据质量建议：可小注".to_string());
    } else {
        reasons.push("数据质量建议：可进入正式推荐".to_string());
    }

    if lineup_confidence < 80.0 || lineup_status_rank(lineup_status) < lineup_status_rank("confirmed") {
        if decision == "可买" && *stake > 0.005 {
            *stake = 0.005;
        }
        if confidence == "高" {
            *confidence = "中".to_string();
        }
        reasons.push("首发未确认：推荐等级最高到小注/观察".to_string());
    }

    if market.starts_with("CRS") {
        if decision == "可买" {
            *decision = "观察".to_string();
        }
        *confidence = "低".to_string();
        *stake = stake.min(0.001);
        reasons.push("比分玩法高波动，默认只观察".to_string());
    } else if market.starts_with("TTG") {
        if confidence == "高" {
            *confidence = "中".to_string();
        }
        *stake = stake.min(0.005);
        reasons.push("总进球玩法高波动，默认观察或小注".to_string());
    }
}

fn action_advice(decision: &str, tier: &str, stake: f64, market: &str) -> String {
    if decision == "禁止" || stake <= 0.0 {
        "建议跳过".to_string()
    } else if decision == "观察" {
        "只看预测，等首发/赔率确认".to_string()
    } else if market.starts_with("CRS") {
        "比分默认不下注，可做极小观察".to_string()
    } else if tier == "稳胆" || tier == "让球稳胆" {
        "可单关小注，不建议重仓".to_string()
    } else {
        "可小注单关，谨慎串关".to_string()
    }
}

fn classify_anomaly(market: &str, pick: &str, delta_abs: f64, delta_pct: f64, odds: f64) -> Option<(String, String, String, String)> {
    let abs_pct = delta_pct.abs();
    if abs_pct >= 0.12 || delta_abs.abs() >= 0.60 {
        return Some((
            "临场剧烈波动".to_string(),
            "高".to_string(),
            if delta_abs < 0.0 { "市场加强该方向" } else { "市场削弱该方向" }.to_string(),
            "暂停自动推荐，等待下一次快照确认".to_string(),
        ));
    }
    if delta_abs < -0.08 && odds <= 2.2 {
        return Some(("热门过热".to_string(), "中".to_string(), "低赔方向继续降温".to_string(), "降低仓位，防止追热门".to_string()));
    }
    if delta_abs < -0.04 {
        return Some(("临场降赔".to_string(), "中".to_string(), "市场支持该方向".to_string(), "仅在模型同向时保留".to_string()));
    }
    if delta_abs > 0.08 {
        return Some(("反向升赔".to_string(), "中".to_string(), "市场削弱该方向".to_string(), "推荐降级或跳过".to_string()));
    }
    if market.starts_with("HAD") && pick == "平局" && abs_pct >= 0.04 {
        return Some(("机构分歧".to_string(), "低".to_string(), "平局价格敏感".to_string(), "只作为风险标签".to_string()));
    }
    None
}

#[allow(dead_code)]
fn provider_field_confidence(base: f64, freshness: f64, completeness: f64, consistency: f64) -> f64 {
    (base * freshness * completeness * consistency).clamp(0.0, 100.0)
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
    let fair_odds = if model_prob > 0.0 { 1.0 / model_prob } else { 999.0 };
    let anomaly_type = anomaly.map(|item| item.anomaly_type.clone()).unwrap_or_default();
    let anomaly_severity = anomaly.map(|item| item.severity.clone()).unwrap_or_default();
    let anomaly_direction = anomaly.map(|item| item.impact_direction.clone()).unwrap_or_default();
    let anomaly_advice = anomaly.map(|item| item.advice.clone()).unwrap_or_default();

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
            } else if key == "player_status_data" {
                player_status_count(&record.value)
            } else if key == "historical_results" {
                record.value.as_array().map(|items| items.len()).unwrap_or(0)
            } else if key == "stats_data" {
                record.value.get("teamCount").and_then(Value::as_u64).unwrap_or(0) as usize
            } else {
                record.value.get("teamCount").or_else(|| record.value.get("matchCount")).and_then(Value::as_u64).unwrap_or(0) as usize
            };
            (true, Some(record.updated_at), count, "已缓存".to_string())
        } else {
            (false, None, 0, "未缓存".to_string())
        };
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
            updated_at,
            count,
            message,
        });
    }
    Ok(json!({
        "dbPath": db_path(&app)?,
        "sources": statuses
    }))
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

    let key = odds_api_key
        .as_deref()
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .unwrap_or(DEFAULT_ODDS_API_KEY);
    if !key.trim().is_empty() {
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
    let base_threeway = threeway_from_scores(&matrix);

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
        let scores = score_distribution(lambda_home, lambda_away, 10);
        let europe = europe_value.and_then(|value| europe_consensus(value, &first.home, &first.away));
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
            let (raw_model_prob, ensemble_note) = ensemble_probability(selection, &scores, europe.as_ref(), market_weight);
            let (model_prob, consistency_note) = apply_market_consistency(selection, raw_model_prob, &scores, selections);
            let (model_prob, reflexivity_note) = market_reflexivity_adjustment(selection, model_prob, europe.as_ref(), &movements);
            let combined_note = format!("{}；{}；{}；{}；{}；{}；{}", note, knockout_note, player_note, market_weight_note, ensemble_note, consistency_note, reflexivity_note);
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
        let scores = score_distribution(lambda_home, lambda_away, 10);
        let (home_win, draw, away_win) = threeway_from_scores(&scores);
        let hhad_line = selections
            .iter()
            .find(|selection| selection.market.starts_with("HHAD"))
            .map(|selection| selection.goal_line.clone())
            .unwrap_or_default();
        let (hand_home, hand_draw, hand_away) = handicap_probs(&scores, &hhad_line);
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
        let europe = europe_value.and_then(|value| europe_consensus(value, &first.home, &first.away));
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
            knockout_note: format!("{}；{}；淘汰赛按90分钟赛果建模；平局代表拖入加时/点球区间，不等于最终晋级。", knockout_stage_note, player_status_note),
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

fn odds_bucket(odds: f64) -> String {
    if odds < 1.8 {
        "1.00-1.79".to_string()
    } else if odds < 2.5 {
        "1.80-2.49".to_string()
    } else if odds < 4.0 {
        "2.50-3.99".to_string()
    } else if odds < 6.0 {
        "4.00-5.99".to_string()
    } else {
        "6.00+".to_string()
    }
}

fn probability_bucket(probability: f64) -> String {
    if probability < 0.2 {
        "0%-20%".to_string()
    } else if probability < 0.35 {
        "20%-35%".to_string()
    } else if probability < 0.5 {
        "35%-50%".to_string()
    } else if probability < 0.65 {
        "50%-65%".to_string()
    } else {
        "65%+".to_string()
    }
}

fn data_quality_bucket(score: f64) -> String {
    if score < 55.0 {
        "<55 建议跳过".to_string()
    } else if score < 65.0 {
        "55-65 只看预测".to_string()
    } else if score < 75.0 {
        "65-75 观察".to_string()
    } else if score < 85.0 {
        "75-85 可小注".to_string()
    } else {
        "85+ 正式推荐".to_string()
    }
}

fn max_drawdown_from_profit(items: &[f64]) -> f64 {
    let mut equity = 0.0;
    let mut peak = 0.0;
    let mut max_dd = 0.0;
    for profit in items {
        equity += profit;
        if equity > peak {
            peak = equity;
        }
        let dd = peak - equity;
        if dd > max_dd {
            max_dd = dd;
        }
    }
    max_dd
}

#[tauri::command]
async fn backtest_report(app: AppHandle) -> Result<BacktestReport, String> {
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
            roi: if stake_sum > 0.0 { profit_sum / stake_sum } else { 0.0 },
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
        .map(|item| format!("{}：{}，ROI {}", item.dimension, item.group, item.roi))
        .unwrap_or_else(|| "样本不足".to_string());
    let most_loss = groups
        .iter()
        .min_by(|a, b| a.roi.partial_cmp(&b.roi).unwrap_or(std::cmp::Ordering::Equal))
        .map(|item| format!("{}：{}，ROI {}", item.dimension, item.group, item.roi))
        .unwrap_or_else(|| "样本不足".to_string());
    let ban_rule_advice = groups
        .iter()
        .filter(|item| item.count >= 3 && item.roi < -0.15)
        .map(|item| format!("禁买候选：{}={}（ROI {:.2}%）", item.dimension, item.group, item.roi * 100.0))
        .collect::<Vec<_>>()
        .join("；");
    Ok(BacktestReport {
        groups,
        most_profitable,
        most_loss,
        ban_rule_advice: if ban_rule_advice.is_empty() { "样本不足，暂不生成禁买规则".to_string() } else { ban_rule_advice },
    })
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
        .filter(|item| item.decision == "可买" && item.stake_pct > 0.0 && !item.market.starts_with("CRS"))
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
    let combos = singles
        .iter()
        .filter(|item| item.tier == "稳胆" || item.tier == "让球稳胆")
        .fold(Vec::<Recommendation>::new(), |mut acc, item| {
            if !acc.iter().any(|other| other.match_id == item.match_id) && acc.len() < 4 {
                acc.push(item.clone());
            }
            acc
        });
    let banned = recommendations
        .iter()
        .filter(|item| item.decision == "禁止" || item.quality_action == "建议跳过")
        .take(30)
        .cloned()
        .collect::<Vec<_>>();
    let watch = recommendations
        .iter()
        .filter(|item| item.decision == "观察" || item.quality_action.contains("只看预测"))
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
        combos,
        banned,
        watch,
        wait_notes,
        review_hint: "赛后到复盘中心结算命中/未中，再看历史回测页面更新禁买规则。".to_string(),
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

fn main() {
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
            save_external_source_config,
            probe_external_source,
            refresh_external_sources,
            model_diagnostics,
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
}
