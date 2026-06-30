pub(crate) fn classify_anomaly(
    market: &str,
    pick: &str,
    delta_abs: f64,
    delta_pct: f64,
    odds: f64,
) -> Option<(String, String, String, String)> {
    let abs_pct = delta_pct.abs();
    if abs_pct >= 0.12 || delta_abs.abs() >= 0.60 {
        return Some((
            "临场剧烈波动".to_string(),
            "高".to_string(),
            if delta_abs < 0.0 {
                "市场加强该方向"
            } else {
                "市场削弱该方向"
            }
            .to_string(),
            "暂停自动推荐，等待下一次快照确认".to_string(),
        ));
    }
    if delta_abs < -0.08 && odds <= 2.2 {
        return Some((
            "热门过热".to_string(),
            "中".to_string(),
            "低赔方向继续降温".to_string(),
            "降低仓位，防止追热门".to_string(),
        ));
    }
    if delta_abs < -0.04 {
        return Some((
            "临场降赔".to_string(),
            "中".to_string(),
            "市场支持该方向".to_string(),
            "仅在模型同向时保留".to_string(),
        ));
    }
    if delta_abs > 0.08 {
        return Some((
            "反向升赔".to_string(),
            "中".to_string(),
            "市场削弱该方向".to_string(),
            "推荐降级或跳过".to_string(),
        ));
    }
    if market.starts_with("HAD") && pick == "平局" && abs_pct >= 0.04 {
        return Some((
            "机构分歧".to_string(),
            "低".to_string(),
            "平局价格敏感".to_string(),
            "只作为风险标签".to_string(),
        ));
    }
    None
}
