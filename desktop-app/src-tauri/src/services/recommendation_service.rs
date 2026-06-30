pub(crate) fn quality_action(score: f64) -> &'static str {
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

pub(crate) fn play_type_risk_level(market: &str) -> &'static str {
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

pub(crate) fn apply_quality_and_play_rules(
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
    } else if data_score < 60.0 {
        if decision == "可买" {
            *decision = "观察".to_string();
        }
        *stake = stake.min(0.0015);
        reasons.push("数据质量建议：只看预测，不建议购买".to_string());
    } else if data_score < 72.0 {
        *stake = stake.min(0.004);
        reasons.push("淘汰赛模式：数据质量中等，允许小注候选但不重仓".to_string());
    } else if data_score < 85.0 {
        *stake = stake.min(0.008);
        reasons.push("淘汰赛模式：数据质量可用，可主动小注".to_string());
    } else {
        reasons.push("数据质量建议：可进入正式推荐".to_string());
    }

    let _ = (lineup_status, lineup_confidence);

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

pub(crate) fn action_advice(decision: &str, tier: &str, stake: f64, market: &str) -> String {
    if decision == "禁止" || stake <= 0.0 {
        "建议跳过".to_string()
    } else if decision == "观察" {
        "观察候选，等赔率确认".to_string()
    } else if market.starts_with("CRS") {
        "比分默认不下注，可做极小观察".to_string()
    } else if tier == "稳胆" || tier == "让球稳胆" {
        "可单关小注，不建议重仓".to_string()
    } else {
        "可小注单关，谨慎串关".to_string()
    }
}
