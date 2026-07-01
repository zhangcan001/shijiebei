use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct CacheRecord {
    pub(crate) key: String,
    pub(crate) updated_at: String,
    pub(crate) value: Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct SourceStatus {
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) ok: bool,
    pub(crate) updated_at: Option<String>,
    pub(crate) count: usize,
    pub(crate) message: String,
    pub(crate) last_success_at: Option<String>,
    pub(crate) last_error_at: Option<String>,
    pub(crate) last_error_message: String,
    pub(crate) freshness_score: f64,
    pub(crate) completeness_score: f64,
    pub(crate) confidence_score: f64,
    pub(crate) using_stale_cache: bool,
    pub(crate) health_label: String,
    pub(crate) diagnosis: String,
    pub(crate) impact: String,
    pub(crate) next_action: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct MatchRow {
    pub(crate) id: String,
    pub(crate) match_num: String,
    pub(crate) league: String,
    pub(crate) time: String,
    pub(crate) home: String,
    pub(crate) away: String,
    pub(crate) status: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct SimRequest {
    pub(crate) match_id: Option<String>,
    pub(crate) home: String,
    pub(crate) away: String,
    pub(crate) home_lambda: Option<f64>,
    pub(crate) away_lambda: Option<f64>,
    pub(crate) max_goals: Option<u32>,
    pub(crate) simulations: Option<u32>,
    pub(crate) knockout_mode: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct ScoreProb {
    pub(crate) score: String,
    pub(crate) probability: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct SimMarketRow {
    pub(crate) pick: String,
    pub(crate) model_prob: f64,
    pub(crate) ci_low: f64,
    pub(crate) ci_high: f64,
    pub(crate) sporttery_prob: Option<f64>,
    pub(crate) europe_prob: Option<f64>,
    pub(crate) gap_vs_sporttery: Option<f64>,
    pub(crate) gap_vs_europe: Option<f64>,
    pub(crate) fair_odds: f64,
    pub(crate) sporttery_odds: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct ProbItem {
    pub(crate) pick: String,
    pub(crate) probability: f64,
    pub(crate) fair_odds: f64,
    pub(crate) sporttery_prob: Option<f64>,
    pub(crate) sporttery_odds: Option<f64>,
    pub(crate) probability_gap: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct SimResult {
    pub(crate) home: String,
    pub(crate) away: String,
    pub(crate) model_version: String,
    pub(crate) lambda_home: f64,
    pub(crate) lambda_away: f64,
    pub(crate) home_win: f64,
    pub(crate) home_win_low: f64,
    pub(crate) home_win_high: f64,
    pub(crate) draw: f64,
    pub(crate) draw_low: f64,
    pub(crate) draw_high: f64,
    pub(crate) away_win: f64,
    pub(crate) away_win_low: f64,
    pub(crate) away_win_high: f64,
    pub(crate) over_25: f64,
    pub(crate) over_25_low: f64,
    pub(crate) over_25_high: f64,
    pub(crate) btts: f64,
    pub(crate) btts_low: f64,
    pub(crate) btts_high: f64,
    pub(crate) total_goals: Vec<ScoreProb>,
    pub(crate) top_scores: Vec<ScoreProb>,
    pub(crate) source_note: String,
    pub(crate) market_rows: Vec<SimMarketRow>,
    pub(crate) adjustment_notes: Vec<String>,
    pub(crate) injury_note: String,
    pub(crate) movement_note: String,
    pub(crate) knockout_note: String,
    pub(crate) simulations: u32,
    pub(crate) simulation_note: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct PredictionRecord {
    pub(crate) id: Option<i64>,
    pub(crate) created_at: Option<String>,
    pub(crate) match_label: String,
    pub(crate) market: String,
    pub(crate) pick: String,
    pub(crate) probability: f64,
    pub(crate) odds: f64,
    pub(crate) safety_margin: f64,
    pub(crate) decision: String,
    pub(crate) stake_pct: Option<f64>,
    pub(crate) actual_result: Option<String>,
    pub(crate) profit: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct BankrollSettings {
    pub(crate) bankroll: f64,
    pub(crate) daily_budget_pct: f64,
    pub(crate) max_loss_pct: f64,
    pub(crate) auto_refresh_minutes: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct ModelDiagnostics {
    pub(crate) total: i64,
    pub(crate) settled: i64,
    pub(crate) hit_rate: f64,
    pub(crate) roi: f64,
    pub(crate) brier_score: f64,
    pub(crate) log_loss: f64,
    pub(crate) calibration: Vec<CalibrationBucket>,
    pub(crate) market_calibration: Vec<MarketCalibration>,
    pub(crate) advice: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct CalibrationBucket {
    pub(crate) bucket: String,
    pub(crate) count: i64,
    pub(crate) avg_probability: f64,
    pub(crate) hit_rate: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct MarketCalibration {
    pub(crate) market: String,
    pub(crate) count: i64,
    pub(crate) hit_rate: f64,
    pub(crate) avg_probability: f64,
    pub(crate) brier_score: f64,
    pub(crate) roi: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct ModelSettings {
    pub(crate) buy_edge: f64,
    pub(crate) buy_gap: f64,
    pub(crate) watch_edge: f64,
    pub(crate) watch_gap: f64,
    pub(crate) max_odds: f64,
    pub(crate) high_odds_limit: f64,
    pub(crate) mode: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct MatchResult {
    pub(crate) home: String,
    pub(crate) away: String,
    pub(crate) score: String,
    pub(crate) half_score: String,
    pub(crate) stage: String,
    pub(crate) status: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct ExternalSourceConfig {
    pub(crate) injury_url: String,
    pub(crate) lineup_url: String,
    pub(crate) stats_url: String,
    pub(crate) notes: String,
}

#[derive(Debug, Clone)]
pub(crate) struct OddsSelection {
    pub(crate) match_id: String,
    pub(crate) match_num: String,
    pub(crate) match_time: String,
    pub(crate) home: String,
    pub(crate) away: String,
    pub(crate) market: String,
    pub(crate) pick: String,
    pub(crate) odds: f64,
    pub(crate) fair_prob: f64,
    pub(crate) goal_line: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct Recommendation {
    pub(crate) match_id: String,
    pub(crate) match_num: String,
    pub(crate) match_time: String,
    pub(crate) match_label: String,
    pub(crate) market: String,
    pub(crate) pick: String,
    pub(crate) odds: f64,
    pub(crate) fair_prob: f64,
    pub(crate) model_prob: f64,
    pub(crate) probability_gap: f64,
    pub(crate) expected_return: f64,
    pub(crate) stake_pct: f64,
    pub(crate) europe_prob: Option<f64>,
    pub(crate) europe_gap: Option<f64>,
    pub(crate) europe_odds: Option<f64>,
    pub(crate) decision: String,
    pub(crate) confidence: String,
    pub(crate) tier: String,
    pub(crate) play_style: String,
    pub(crate) combo_group: String,
    pub(crate) data_score: f64,
    pub(crate) data_grade: String,
    pub(crate) quality_action: String,
    pub(crate) support_factors: String,
    pub(crate) risk_factors: String,
    pub(crate) fair_odds: f64,
    pub(crate) advantage_rate: f64,
    pub(crate) action_advice: String,
    pub(crate) play_type_risk_level: String,
    pub(crate) lineup_status: String,
    pub(crate) lineup_confidence: f64,
    pub(crate) anomaly_type: String,
    pub(crate) anomaly_severity: String,
    pub(crate) anomaly_direction: String,
    pub(crate) anomaly_advice: String,
    pub(crate) worldcup_correction_action: String,
    pub(crate) final_decision: String,
    pub(crate) reason: String,
}

#[derive(Debug, Clone)]
pub(crate) struct EuropeConsensus {
    pub(crate) home_prob: f64,
    pub(crate) draw_prob: f64,
    pub(crate) away_prob: f64,
    pub(crate) home_odds: f64,
    pub(crate) draw_odds: f64,
    pub(crate) away_odds: f64,
    pub(crate) bookmaker_count: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct MatchAnalysis {
    pub(crate) match_id: String,
    pub(crate) match_num: String,
    pub(crate) match_time: String,
    pub(crate) match_label: String,
    pub(crate) lambda_home: f64,
    pub(crate) lambda_away: f64,
    pub(crate) knockout_note: String,
    pub(crate) had: Vec<ProbItem>,
    pub(crate) hhad: Vec<ProbItem>,
    pub(crate) hhad_line: String,
    pub(crate) ttg: Vec<ProbItem>,
    pub(crate) scores: Vec<ProbItem>,
    pub(crate) europe_note: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct OddsMovement {
    pub(crate) id: i64,
    pub(crate) created_at: String,
    pub(crate) match_label: String,
    pub(crate) market: String,
    pub(crate) pick: String,
    pub(crate) initial_odds: f64,
    pub(crate) previous_odds: f64,
    pub(crate) current_odds: f64,
    pub(crate) delta_abs: f64,
    pub(crate) delta_pct: f64,
    pub(crate) direction: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct OddsAnomaly {
    pub(crate) id: i64,
    pub(crate) created_at: String,
    pub(crate) match_id: String,
    pub(crate) match_label: String,
    pub(crate) market: String,
    pub(crate) pick: String,
    pub(crate) anomaly_type: String,
    pub(crate) severity: String,
    pub(crate) impact_direction: String,
    pub(crate) advice: String,
    pub(crate) delta_abs: f64,
    pub(crate) delta_pct: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct BacktestGroup {
    pub(crate) dimension: String,
    pub(crate) group: String,
    pub(crate) count: i64,
    pub(crate) hit_rate: f64,
    pub(crate) roi: f64,
    pub(crate) total_profit: f64,
    pub(crate) max_drawdown: f64,
    pub(crate) avg_odds: f64,
    pub(crate) avg_advantage_rate: f64,
    pub(crate) brier_score: f64,
    pub(crate) log_loss: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct BanRule {
    pub(crate) dimension: String,
    pub(crate) group: String,
    pub(crate) count: i64,
    pub(crate) hit_rate: f64,
    pub(crate) roi: f64,
    pub(crate) avg_odds: f64,
    pub(crate) reason: String,
    pub(crate) action: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct BacktestReport {
    pub(crate) groups: Vec<BacktestGroup>,
    pub(crate) ban_rules: Vec<BanRule>,
    pub(crate) most_profitable: String,
    pub(crate) most_loss: String,
    pub(crate) ban_rule_advice: String,
    pub(crate) shadow_backtest: Value,
    pub(crate) rule_diagnostics: Value,
    pub(crate) threshold_scan: Value,
    pub(crate) candidate_strategy: Value,
    pub(crate) paper_trading: Value,
    pub(crate) strategy_robustness: Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct TodayBetPlan {
    pub(crate) bankroll: f64,
    pub(crate) daily_budget: f64,
    pub(crate) max_loss: f64,
    pub(crate) singles: Vec<Recommendation>,
    pub(crate) small_stake: Vec<Recommendation>,
    pub(crate) combos: Vec<Recommendation>,
    pub(crate) strategy_observation: Vec<Recommendation>,
    pub(crate) banned: Vec<Recommendation>,
    pub(crate) watch: Vec<Recommendation>,
    pub(crate) wait_notes: Vec<String>,
    pub(crate) review_hint: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct PreMatchSnapshotRow {
    pub(crate) id: i64,
    pub(crate) match_id: String,
    pub(crate) external_fixture_id: String,
    pub(crate) provider_match_id: String,
    pub(crate) snapshot_time: String,
    pub(crate) kickoff_time: String,
    pub(crate) home_team: String,
    pub(crate) away_team: String,
    pub(crate) competition: String,
    pub(crate) season: String,
    pub(crate) stage: String,
    pub(crate) model_version: String,
    pub(crate) model_probs_json: Value,
    pub(crate) calibrated_probs_json: Value,
    pub(crate) worldcup_correction_action: String,
    pub(crate) odds_json: Value,
    pub(crate) market_probs_json: Value,
    pub(crate) ev_json: Value,
    pub(crate) data_quality_score: f64,
    pub(crate) lineup_status: String,
    pub(crate) lineup_confidence: f64,
    pub(crate) injury_status: String,
    pub(crate) injury_confidence: f64,
    pub(crate) risk_tags_json: Value,
    pub(crate) final_decision: String,
    pub(crate) decision_reason_json: Value,
    pub(crate) paper_strategy_id: String,
    pub(crate) paper_trade_enabled: bool,
    pub(crate) raw_features_json: Value,
    pub(crate) created_before_kickoff: bool,
    pub(crate) is_final_pre_match: bool,
    pub(crate) is_final_snapshot: bool,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
    pub(crate) settlement: Option<Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct SnapshotAuditLog {
    pub(crate) id: i64,
    pub(crate) snapshot_id: Option<i64>,
    pub(crate) match_id: String,
    pub(crate) match_label: String,
    pub(crate) snapshot_time: String,
    pub(crate) kickoff_time: String,
    pub(crate) audit_type: String,
    pub(crate) severity: String,
    pub(crate) message: String,
    pub(crate) detected_at: String,
    pub(crate) resolved: bool,
    pub(crate) resolved_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct DataProvider {
    pub(crate) provider_id: String,
    pub(crate) name: String,
    pub(crate) data_type: String,
    pub(crate) requires_key: bool,
    pub(crate) base_confidence: f64,
    pub(crate) enabled: bool,
    pub(crate) daily_limit: i64,
    pub(crate) hourly_limit: i64,
    pub(crate) last_success_at: Option<String>,
    pub(crate) last_error_at: Option<String>,
    pub(crate) last_error_message: String,
    pub(crate) freshness_score: f64,
    pub(crate) completeness_score: f64,
    pub(crate) confidence_score: f64,
    pub(crate) using_stale_cache: bool,
    pub(crate) supported_data_types: Vec<String>,
    pub(crate) key_configured: bool,
    pub(crate) today_requests: i64,
    pub(crate) hour_requests: i64,
    pub(crate) health_label: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct ProviderCredentialInput {
    pub(crate) provider_id: String,
    pub(crate) api_key: String,
}

#[derive(Debug, Clone)]
pub(crate) struct ProviderRegistryItem {
    pub(crate) provider_id: &'static str,
    pub(crate) name: &'static str,
    pub(crate) supported_data_types: &'static [&'static str],
    pub(crate) requires_key: bool,
    pub(crate) base_confidence: f64,
    pub(crate) daily_limit: i64,
    pub(crate) hourly_limit: i64,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct ProviderRawRecord {
    pub(crate) provider_id: String,
    pub(crate) data_type: String,
    pub(crate) match_id: Option<String>,
    pub(crate) team: Option<String>,
    pub(crate) field_name: String,
    pub(crate) field_value: String,
    pub(crate) fetched_at: String,
    pub(crate) confidence: f64,
    pub(crate) raw_payload: String,
}

#[allow(dead_code)]
#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct FusionResult {
    pub(crate) data_type: String,
    pub(crate) match_id: Option<String>,
    pub(crate) team: Option<String>,
    pub(crate) field_name: String,
    pub(crate) final_value: String,
    pub(crate) confidence: f64,
    pub(crate) provider_count: i64,
    pub(crate) conflict: bool,
}
