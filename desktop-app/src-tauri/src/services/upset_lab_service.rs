use serde_json::{json, Value};

// TODO: pending split phase 2 - move database reads/writes for upset lab generation here.
pub const UPSET_LAB_ALLOWED_DECISIONS: [&str; 5] = [
    "no_odds_scan",
    "scan_only",
    "paper_candidate",
    "tiny_stake_candidate",
    "forbidden",
];

#[derive(Debug, Clone)]
pub struct UpsetDecisionInput<'a> {
    pub play_pool: &'a str,
    pub play_type: &'a str,
    pub odds: f64,
    pub model_prob: f64,
    pub market_prob: f64,
    pub ev: f64,
    pub data_quality_score: f64,
    pub upset_score: f64,
    pub chaos_score: f64,
    pub final_decision: &'a str,
    pub risk_text: &'a str,
    pub consecutive_losses: i64,
}

#[derive(Debug, Clone)]
pub struct UpsetDecision {
    pub final_lab_decision: &'static str,
    pub play_pool: String,
    pub block_reasons: Vec<String>,
}

pub fn is_hard_ban(final_decision: &str, risk_text: &str) -> bool {
    let text = format!("{} {}", final_decision, risk_text).to_lowercase();
    text.contains("hard_ban")
        || text.contains("禁买")
        || text.contains("禁止")
        || text.contains("跳过")
}

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

pub fn score_33_lab_decision(
    chaos_score: f64,
    odds: f64,
    data_quality_score: f64,
    ev: f64,
) -> (&'static str, &'static str) {
    if odds <= 1.0 {
        return ("forbidden", "3:3 缺少有效赔率，禁止进入候选。");
    }
    if data_quality_score < 65.0 {
        return ("scan_only", "3:3 数据质量不足，只能剧本扫描。");
    }
    if chaos_score < 85.0 {
        return ("scan_only", "3:3 需要极高混沌分，当前仅观察。");
    }
    if ev < 0.15 {
        return (
            "paper_candidate",
            "3:3 极端低频，EV 未达到极小仓位要求，仅纸面观察。",
        );
    }
    (
        "paper_candidate",
        "3:3 极端低频，即使有支撑也只允许纸面观察。",
    )
}

pub fn can_write_settleable_paper_trade(decision: &str, odds: Option<f64>) -> bool {
    matches!(decision, "paper_candidate" | "tiny_stake_candidate")
        && odds.map(|value| value > 1.0).unwrap_or(false)
}

pub fn paper_trade_skip_reason(candidate: &Value) -> Option<&'static str> {
    let decision = candidate
        .get("final_lab_decision")
        .and_then(Value::as_str)
        .unwrap_or("");
    if decision == "no_odds_scan" {
        return Some("missing_odds");
    }
    if decision == "scan_only" {
        return Some("scan_only");
    }
    let odds = candidate.get("odds").and_then(Value::as_f64);
    if !can_write_settleable_paper_trade(decision, odds) {
        return Some("not_paper_candidate");
    }
    None
}

pub fn classify_upset_decision(input: &UpsetDecisionInput<'_>) -> UpsetDecision {
    let mut play_pool = input.play_pool.to_string();
    let mut block_reasons = Vec::new();
    let hard_ban = is_hard_ban(input.final_decision, input.risk_text);
    let final_lab_decision = if hard_ban {
        play_pool = "forbidden_upset_pool".to_string();
        block_reasons.push("hard_ban 最高优先级，禁止进入冷门实验候选。".to_string());
        "forbidden"
    } else if input.odds <= 1.0 || input.model_prob <= 0.0 || input.market_prob <= 0.0 {
        play_pool = "forbidden_upset_pool".to_string();
        block_reasons.push("赔率或概率无效。".to_string());
        "forbidden"
    } else if input.odds > 75.0 {
        play_pool = "forbidden_upset_pool".to_string();
        block_reasons.push("赔率高于 75，默认禁碰/娱乐，不作为策略候选。".to_string());
        "forbidden"
    } else if input.ev < -0.15
        && !(input.play_type == "correct_score" && input.chaos_score >= 65.0 && input.odds >= 30.0)
    {
        play_pool = "forbidden_upset_pool".to_string();
        block_reasons.push("EV 低于 -15%，默认禁碰冷门。".to_string());
        "forbidden"
    } else if input.data_quality_score < 45.0 {
        play_pool = "forbidden_upset_pool".to_string();
        block_reasons.push("数据质量严重不足。".to_string());
        "forbidden"
    } else if input.play_pool == "score_3_3_pool" {
        let (decision, reason) = score_33_lab_decision(
            input.chaos_score,
            input.odds,
            input.data_quality_score,
            input.ev,
        );
        block_reasons.push(reason.to_string());
        if decision == "forbidden" {
            play_pool = "forbidden_upset_pool".to_string();
        }
        decision
    } else if input.ev < -0.05 {
        block_reasons.push("EV 偏负，仅冷门扫描，不建议下注。".to_string());
        "scan_only"
    } else if input.ev < 0.0 {
        if input.upset_score >= 62.0 && input.data_quality_score >= 55.0 {
            block_reasons.push("EV 略负，但冷门剧本有一定支撑，纸面观察。".to_string());
            "paper_candidate"
        } else {
            block_reasons.push("EV 略负，仅冷门扫描。".to_string());
            "scan_only"
        }
    } else if input.data_quality_score < 50.0 {
        block_reasons.push("数据不完整，仅冷门扫描。".to_string());
        "scan_only"
    } else if input.upset_score < 45.0 {
        block_reasons.push("冷门扫描分偏低，仅展示参考。".to_string());
        "scan_only"
    } else if input.consecutive_losses >= 8 {
        block_reasons.push("连续亏损达到 8 单，冷门实验室只允许纸面观察。".to_string());
        "paper_candidate"
    } else if input.upset_score >= 72.0
        && input.ev >= 0.08
        && input.data_quality_score >= 70.0
        && !matches!(
            input.play_pool,
            "high_odds_score_pool" | "score_3_3_pool" | "half_fulltime_reversal_pool"
        )
    {
        "tiny_stake_candidate"
    } else if input.chaos_score >= 80.0 && input.ev >= 0.12 {
        "paper_candidate"
    } else {
        "scan_only"
    };
    UpsetDecision {
        final_lab_decision: normalize_upset_lab_decision(final_lab_decision, false),
        play_pool,
        block_reasons,
    }
}

pub fn default_upset_lab_empty_funnel() -> Value {
    json!({
        "today_matches_count": 0,
        "pre_match_snapshot_count": 0,
        "odds_match_count": 0,
        "generated_no_odds_scan_count": 0,
        "generated_scan_only_count": 0,
        "generated_paper_candidate_count": 0,
        "generated_tiny_stake_count": 0,
        "generated_forbidden_count": 0,
        "hard_ban_count": 0,
        "data_insufficient_count": 0,
        "odds_missing_count": 0,
        "empty_reason": "冷门实验室仅做高风险扫描观察，候选不会进入正式推荐。"
    })
}

pub fn summarize_candidates(candidates: &[Value]) -> Value {
    let count = |decision: &str| {
        candidates
            .iter()
            .filter(|item| item.get("final_lab_decision").and_then(Value::as_str) == Some(decision))
            .count()
    };
    json!({
        "candidate_count": candidates.len(),
        "no_odds_scan_count": count("no_odds_scan"),
        "scan_only_count": count("scan_only"),
        "paper_candidate_count": count("paper_candidate"),
        "tiny_stake_candidate_count": count("tiny_stake_candidate"),
        "forbidden_count": count("forbidden"),
        "warning": if candidates.is_empty() {
            "冷门实验室暂无候选，请查看过滤漏斗。"
        } else {
            "冷门实验室仅做高风险观察，不影响正式推荐。"
        }
    })
}
