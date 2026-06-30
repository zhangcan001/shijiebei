use serde_json::{json, Value};

pub fn frozen_snapshot_fields() -> Vec<&'static str> {
    vec![
        "model_probs_json",
        "calibrated_probs_json",
        "odds_json",
        "ev_json",
        "data_quality_score",
        "risk_tags_json",
        "final_decision",
        "raw_features_json",
    ]
}

pub fn settlement_protection_summary() -> Value {
    json!({
        "service": "snapshot_service",
        "frozen_fields": frozen_snapshot_fields(),
        "rule": "赛后结算只能写结果表，不能覆盖赛前冻结字段。"
    })
}
