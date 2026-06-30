use serde_json::{json, Value};

pub const UPSET_LAB_ALLOWED_DECISIONS: [&str; 5] = [
    "no_odds_scan",
    "scan_only",
    "paper_candidate",
    "tiny_stake_candidate",
    "forbidden",
];

pub fn normalize_upset_lab_decision(decision: &str, hard_ban: bool) -> &'static str {
    if hard_ban {
        return "forbidden";
    }
    UPSET_LAB_ALLOWED_DECISIONS
        .iter()
        .copied()
        .find(|item| *item == decision)
        .unwrap_or("scan_only")
}

pub fn can_write_settleable_paper_trade(decision: &str, odds: Option<f64>) -> bool {
    matches!(decision, "paper_candidate" | "tiny_stake_candidate")
        && odds.map(|value| value > 1.0).unwrap_or(false)
}

pub fn default_upset_lab_empty_funnel() -> Value {
    json!({
        "today_matches_count": 0,
        "pre_match_snapshot_count": 0,
        "odds_match_count": 0,
        "generated_no_odds_scan_count": 0,
        "empty_reason": "冷门实验室仅做高风险扫描观察，候选不会进入正式推荐。"
    })
}
