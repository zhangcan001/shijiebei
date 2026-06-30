use serde_json::{json, Value};

pub(crate) fn review_overfit_guard(
    match_count: usize,
    consecutive_days_with_same_issue: usize,
) -> Value {
    let review_note_only = match_count < 10;
    let candidate_adjustment_allowed = match_count >= 30 && consecutive_days_with_same_issue >= 3;
    let mut blocking_reasons = Vec::new();
    if match_count < 10 {
        blocking_reasons.push("单日样本少于10场，只能生成review_note");
    }
    if match_count < 30 {
        blocking_reasons.push("单日样本少于30场，不能生成candidate_adjustment");
    }
    if consecutive_days_with_same_issue < 3 {
        blocking_reasons.push("同类问题未连续出现3个比赛日，不能形成候选调整");
    }
    json!({
        "review_note_only": review_note_only,
        "candidate_adjustment_allowed": candidate_adjustment_allowed,
        "candidate_adjustment_status": "observation_only",
        "blocking_reasons": blocking_reasons,
        "forbidden_outputs": [
            "score_rule",
            "正式推荐规则",
            "比分权重",
            "总进球权重",
            "冷门实验室入池规则",
            "hard_ban规则",
            "observe_only规则"
        ],
        "message": if review_note_only {
            "当前复盘样本较少，仅作为观察记录，不会自动修改模型或推荐规则。"
        } else if !candidate_adjustment_allowed {
            "复盘样本仍不足以形成候选调整，保持observation_only。"
        } else {
            "满足候选调整观察条件，但仍需人工确认。"
        }
    })
}

pub(crate) fn score_diversity_guard(scores: &[Value], draw_prob: f64) -> Value {
    let top1_is_11 = scores
        .first()
        .and_then(|item| item.get("score"))
        .and_then(Value::as_str)
        == Some("1:1");
    let draw_scores = scores
        .iter()
        .take(5)
        .filter(|item| {
            item.get("score")
                .and_then(Value::as_str)
                .and_then(|score| {
                    let parts = score.split(':').collect::<Vec<_>>();
                    if parts.len() == 2 {
                        Some(parts[0] == parts[1])
                    } else {
                        None
                    }
                })
                .unwrap_or(false)
        })
        .count();
    json!({
        "top1_is_1_1": top1_is_11,
        "draw_score_count_top5": draw_scores,
        "warning": if top1_is_11 && draw_prob < 0.25 {
            "1:1 只能作为候选比分之一，当前平局概率不足，不应仅因先验置顶。"
        } else if draw_scores > 2 && draw_prob < 0.34 {
            "比分参考过度集中于平局，请结合胜平负和总进球判断。"
        } else {
            ""
        }
    })
}
