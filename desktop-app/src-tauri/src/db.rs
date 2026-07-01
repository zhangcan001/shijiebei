use chrono::Utc;
use rusqlite::{params, Connection};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

use crate::models::CacheRecord;
use crate::services::source_service::ensure_provider_registry;

const PRE_MATCH_SNAPSHOT_COLUMNS: &[(&str, &str)] = &[
    ("match_id", "text not null default ''"),
    ("external_fixture_id", "text not null default ''"),
    ("provider_match_id", "text not null default ''"),
    ("snapshot_time", "text not null default ''"),
    ("kickoff_time", "text not null default ''"),
    ("home_team", "text not null default ''"),
    ("away_team", "text not null default ''"),
    ("competition", "text not null default ''"),
    ("season", "text not null default ''"),
    ("stage", "text not null default ''"),
    ("model_version", "text not null default ''"),
    ("model_probs_json", "text not null default '[]'"),
    ("calibrated_probs_json", "text not null default '[]'"),
    ("worldcup_correction_action", "text not null default ''"),
    ("odds_json", "text not null default '[]'"),
    ("market_probs_json", "text not null default '[]'"),
    ("ev_json", "text not null default 'null'"),
    ("data_quality_score", "real not null default 0"),
    ("lineup_status", "text not null default 'unknown'"),
    ("lineup_confidence", "real not null default 0"),
    ("injury_status", "text not null default 'unknown'"),
    ("injury_confidence", "real not null default 0"),
    ("risk_tags_json", "text not null default '[]'"),
    ("final_decision", "text not null default 'observe_only'"),
    ("decision_reason_json", "text not null default '[]'"),
    ("paper_strategy_id", "text not null default ''"),
    ("paper_trade_enabled", "integer not null default 0"),
    ("raw_features_json", "text not null default '{}'"),
    ("created_before_kickoff", "integer not null default 1"),
    ("is_final_pre_match", "integer not null default 0"),
    ("created_at", "text not null default ''"),
    ("updated_at", "text not null default ''"),
];

pub(crate) const PRE_MATCH_SNAPSHOT_SELECT: &str = "s.id, s.match_id, s.external_fixture_id, s.provider_match_id, s.snapshot_time, s.kickoff_time,
                s.home_team, s.away_team, s.competition, s.season, s.stage, s.model_version,
                s.model_probs_json, s.calibrated_probs_json, s.worldcup_correction_action,
                s.odds_json, s.market_probs_json, s.ev_json, s.data_quality_score,
                s.lineup_status, s.lineup_confidence, s.injury_status, s.injury_confidence,
                s.risk_tags_json, s.final_decision, s.decision_reason_json, s.paper_strategy_id,
                s.paper_trade_enabled, s.raw_features_json, s.created_before_kickoff, s.is_final_pre_match, s.created_at, s.updated_at";

pub(crate) const PRE_MATCH_SNAPSHOT_SELECT_COUNT: usize = 33;

pub(crate) fn expected_pre_match_snapshot_columns() -> Vec<&'static str> {
    let mut columns = vec!["id"];
    columns.extend(PRE_MATCH_SNAPSHOT_COLUMNS.iter().map(|(name, _)| *name));
    columns
}

pub(crate) fn pre_match_snapshot_columns(conn: &Connection) -> anyhow::Result<Vec<String>> {
    let mut stmt = conn.prepare("pragma table_info(pre_match_snapshots)")?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub(crate) fn ensure_pre_match_snapshot_schema(conn: &Connection) -> anyhow::Result<Vec<String>> {
    let existing = pre_match_snapshot_columns(conn)?;
    let mut added = Vec::new();
    for (name, definition) in PRE_MATCH_SNAPSHOT_COLUMNS {
        if existing.iter().any(|column| column == name) {
            continue;
        }
        conn.execute(
            &format!("alter table pre_match_snapshots add column {name} {definition}"),
            [],
        )?;
        added.push((*name).to_string());
    }
    Ok(added)
}

pub(crate) fn app_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    Ok(dir)
}

pub(crate) fn db_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app_dir(app)?.join("worldcup-odds.sqlite"))
}

pub(crate) fn open_conn(app: &AppHandle) -> Result<Connection, String> {
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
        create table if not exists worldcup_training_samples (
          id integer primary key autoincrement,
          created_at text not null,
          frozen_at text not null,
          settled_at text not null,
          snapshot_id integer,
          recommendation_id integer not null unique,
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
          result_score text not null,
          actual_outcome text not null,
          hit integer not null,
          profit real not null,
          stage text not null default '',
          raw_payload text not null
        );
        create table if not exists paper_trading_records (
          id integer primary key autoincrement,
          match_id text not null,
          snapshot_id integer,
          strategy_id text not null,
          model_version text not null,
          selection text not null,
          play_type text not null,
          model_prob real not null,
          odds real not null,
          ev real not null,
          advantage_rate real not null,
          data_quality_score real not null,
          risk_tags_json text not null,
          worldcup_correction_action text not null,
          paper_stake real not null,
          result_status text not null default 'pending',
          is_hit integer,
          paper_profit real not null default 0,
          created_at text not null,
          settled_at text
        );
        create table if not exists upset_lab_candidates (
          id integer primary key autoincrement,
          match_id text not null,
          snapshot_id integer,
          source_snapshot_type text not null,
          match_time text not null,
          home_team text not null,
          away_team text not null,
          competition text not null default '',
          stage text not null default '',
          play_pool text not null,
          play_type text not null,
          selection text not null,
          odds real,
          model_prob real not null,
          market_prob real,
          fair_odds real,
          ev real,
          advantage_rate real,
          data_quality_score real not null default 0,
          scan_score real not null default 0,
          upset_score real not null,
          chaos_score real not null,
          risk_level text not null,
          stake_pct real not null,
          stake_advice text not null,
          final_lab_decision text not null,
          trigger_reasons_json text not null,
          block_reasons_json text not null,
          risk_tags_json text not null,
          is_paper_only integer not null default 1,
          paper_record_id integer,
          created_at text not null,
          updated_at text not null
        );
        create index if not exists idx_upset_lab_candidates_match
          on upset_lab_candidates(match_id, snapshot_id, play_pool, final_lab_decision);
        create table if not exists upset_lab_backtest_results (
          id integer primary key autoincrement,
          candidate_id integer,
          match_id text not null,
          play_pool text not null,
          play_type text not null,
          selection text not null,
          odds real not null,
          model_prob real not null,
          ev real not null,
          stake real not null,
          is_hit integer not null,
          profit real not null,
          roi real not null,
          settled_at text not null
        );
        create table if not exists pre_match_snapshots (
          id integer primary key autoincrement,
          match_id text not null,
          external_fixture_id text not null default '',
          provider_match_id text not null default '',
          snapshot_time text not null,
          kickoff_time text not null,
          home_team text not null,
          away_team text not null,
          competition text not null default '',
          season text not null default '',
          stage text not null default '',
          model_version text not null,
          model_probs_json text not null,
          calibrated_probs_json text not null,
          worldcup_correction_action text not null default '',
          odds_json text not null,
          market_probs_json text not null,
          ev_json text not null,
          data_quality_score real not null,
          lineup_status text not null,
          lineup_confidence real not null,
          injury_status text not null,
          injury_confidence real not null,
          risk_tags_json text not null,
          final_decision text not null,
          decision_reason_json text not null,
          paper_strategy_id text not null default '',
          paper_trade_enabled integer not null default 0,
          raw_features_json text not null,
          created_before_kickoff integer not null default 1,
          is_final_pre_match integer not null default 0,
          created_at text not null,
          updated_at text not null
        );
        create index if not exists idx_pre_match_snapshots_match
          on pre_match_snapshots(match_id, id);
        create table if not exists pre_match_snapshot_results (
          id integer primary key autoincrement,
          snapshot_id integer not null,
          match_id text not null,
          home_score integer not null,
          away_score integer not null,
          result_spf text not null,
          total_goals integer not null,
          settled_at text not null,
          is_hit_json text not null,
          paper_profit_json text not null,
          settlement_status text not null
        );
        create table if not exists manual_analysis_notes (
          id integer primary key autoincrement,
          match_id text not null default '',
          snapshot_id integer,
          analysis_source text not null default 'manual',
          analyst_pick text not null default '',
          analyst_reason text not null default '',
          confidence text not null default '',
          risk_level text not null default '',
          raw_prompt text not null default '',
          raw_response text not null default '',
          created_at text not null,
          updated_at text not null
        );
        create index if not exists idx_manual_analysis_notes_match
          on manual_analysis_notes(match_id, id);
        create table if not exists snapshot_audit_logs (
          id integer primary key autoincrement,
          snapshot_id integer,
          match_id text not null,
          audit_type text not null,
          severity text not null,
          message text not null,
          detected_at text not null,
          resolved integer not null default 0,
          resolved_at text
        );
        create index if not exists idx_snapshot_audit_logs_match
          on snapshot_audit_logs(match_id, snapshot_id, audit_type);
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
          provider_id text,
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
        create table if not exists data_providers (
          provider_id text primary key,
          name text not null,
          data_type text not null,
          requires_key integer not null default 0,
          base_confidence real not null,
          enabled integer not null default 1,
          daily_limit integer not null default 0,
          hourly_limit integer not null default 0,
          supported_data_types text not null default '',
          last_success_at text,
          last_error_at text,
          last_error_message text not null default '',
          freshness_score real not null default 0,
          completeness_score real not null default 0,
          confidence_score real not null default 0,
          using_stale_cache integer not null default 0
        );
        create table if not exists provider_credentials (
          provider_id text primary key,
          api_key text not null,
          updated_at text not null
        );
        create table if not exists provider_request_logs (
          id integer primary key autoincrement,
          provider_id text not null,
          data_type text not null,
          requested_at text not null,
          success integer not null,
          error_message text not null default ''
        );
        create table if not exists source_health (
          provider_id text primary key,
          last_success_at text,
          last_error_at text,
          last_error_message text not null default '',
          freshness_score real not null default 0,
          completeness_score real not null default 0,
          confidence_score real not null default 0,
          using_stale_cache integer not null default 0,
          updated_at text not null
        );
        "#,
    )
    .map_err(|error| error.to_string())?;
    let migrations = [
        "alter table predictions add column stake_pct real default 0",
        "alter table predictions add column actual_result text default ''",
        "alter table predictions add column profit real default 0",
        "alter table provider_raw_data add column provider_id text",
        "alter table data_providers add column supported_data_types text not null default ''",
        "alter table paper_trading_records add column source text not null default 'historical_backtest'",
        "alter table paper_trading_records add column created_before_kickoff integer not null default 0",
        "alter table paper_trading_records add column is_final_snapshot integer not null default 0",
        "alter table paper_trading_records add column upset_lab_candidate_id integer",
        "alter table paper_trading_records add column play_pool text not null default ''",
        "alter table paper_trading_records add column risk_level text not null default ''",
        "alter table paper_trading_records add column is_real_bet integer not null default 0",
        "alter table pre_match_snapshots add column created_before_kickoff integer not null default 1",
        "alter table upset_lab_candidates add column data_quality_score real not null default 0",
        "alter table upset_lab_candidates add column scan_score real not null default 0",
    ];
    for sql in migrations {
        let _ = conn.execute(sql, []);
    }
    let _ = ensure_pre_match_snapshot_schema(&conn);
    let _ = ensure_provider_registry(&conn);
    Ok(conn)
}

pub(crate) fn cache_put(conn: &Connection, key: &str, value: &Value) -> anyhow::Result<()> {
    conn.execute(
        "insert into cache(key, updated_at, value) values(?1, ?2, ?3)
         on conflict(key) do update set updated_at=excluded.updated_at, value=excluded.value",
        params![key, Utc::now().to_rfc3339(), value.to_string()],
    )?;
    Ok(())
}

pub(crate) fn cache_get(conn: &Connection, key: &str) -> anyhow::Result<Option<CacheRecord>> {
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
