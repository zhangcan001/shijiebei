use serde_json::Value;

pub(crate) fn review_overfit_guard(
    match_count: usize,
    consecutive_days_with_same_issue: usize,
) -> Value {
    crate::services::review_guard_service::review_overfit_guard(
        match_count,
        consecutive_days_with_same_issue,
    )
}

pub(crate) fn score_diversity_guard(scores: &[Value], draw_prob: f64) -> Value {
    crate::services::review_guard_service::score_diversity_guard(scores, draw_prob)
}

pub fn candidate_adjustment_status() -> &'static str {
    "observation_only"
}
