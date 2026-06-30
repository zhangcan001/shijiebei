use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

const SCORE_PRIOR_FILE: &str = "worldcup_knockout_score_priors_v1.json";

pub(crate) fn score_prior_path(root: &Path) -> PathBuf {
    if root.file_name().and_then(|name| name.to_str()) == Some("models") {
        return root.join(SCORE_PRIOR_FILE);
    }
    root.join("training").join("models").join(SCORE_PRIOR_FILE)
}

pub(crate) fn fallback_worldcup_knockout_score_priors() -> Value {
    json!({
        "model_version": "worldcup_knockout_score_priors_v1",
        "scope": "FIFA World Cup knockout stage, 90 minutes only",
        "tournaments": [2018, 2022],
        "sample_count": 32,
        "fallback": true,
        "fallback_reason": "世界杯比分先验未加载，使用内置基础比分先验",
        "notes": [
            "Only 90-minute regulation scores are included.",
            "Extra time and penalty shootout scores are excluded.",
            "This prior is used for score reference, total goals tendency, upset lab scripts, and confidence adjustment only.",
            "It must not override hard_ban or formal recommendation rules."
        ],
        "score_shape_priors": {
            "1-1": 0.1875,
            "2-1_type": 0.15625,
            "2-0_type": 0.15625,
            "1-0_type": 0.09375,
            "0-0": 0.0625,
            "2-2": 0.0625,
            "3-1_type": 0.0625,
            "3-0_type": 0.0625,
            "4-1_type": 0.03125,
            "6-1_type": 0.03125,
            "4-3_type": 0.03125,
            "3-2_type": 0.03125,
            "4-2_type": 0.03125,
            "3-3": 0.0
        },
        "score_shape_counts": {
            "1-1": 6,
            "2-1_type": 5,
            "2-0_type": 5,
            "1-0_type": 3,
            "0-0": 2,
            "2-2": 2,
            "3-1_type": 2,
            "3-0_type": 2,
            "4-1_type": 1,
            "6-1_type": 1,
            "4-3_type": 1,
            "3-2_type": 1,
            "4-2_type": 1,
            "3-3": 0
        },
        "total_goals_priors": {
            "0_goals": 0.0625,
            "1_goal": 0.09375,
            "2_goals": 0.34375,
            "3_goals": 0.21875,
            "4_goals": 0.125,
            "5_goals": 0.0625,
            "6_goals": 0.03125,
            "7_goals": 0.0625,
            "under_2_5": 0.5,
            "over_2_5": 0.5,
            "one_to_two_goals": 0.4375,
            "three_to_four_goals": 0.34375,
            "five_plus_goals": 0.15625
        },
        "total_goals_counts": {
            "0_goals": 2,
            "1_goal": 3,
            "2_goals": 11,
            "3_goals": 7,
            "4_goals": 4,
            "5_goals": 2,
            "6_goals": 1,
            "7_goals": 2,
            "under_2_5": 16,
            "over_2_5": 16,
            "one_to_two_goals": 14,
            "three_to_four_goals": 11,
            "five_plus_goals": 5
        },
        "draw_priors": {
            "draw_90min": 0.3125,
            "0-0": 0.0625,
            "1-1": 0.1875,
            "2-2": 0.0625,
            "3-3": 0.0
        },
        "draw_counts": {
            "draw_90min": 10,
            "0-0": 2,
            "1-1": 6,
            "2-2": 2,
            "3-3": 0
        },
        "recommendation_notes": {
            "highest_score_shape": "1-1",
            "strong_score_shapes": ["1-1", "2-1_type", "2-0_type", "1-0_type"],
            "cold_draw_scores": ["1-1", "0-0", "2-2"],
            "favorite_narrow_win_scores": ["1-0_type", "2-0_type", "2-1_type"],
            "main_total_goals_zones": ["2_goals", "3_goals", "4_goals"],
            "extreme_scores": ["3-3", "4-3_type", "4-2_type", "6-1_type"],
            "score_3_3_policy": "Extreme low-frequency score. It appeared 0 times in 2018+2022 knockout-stage 90-minute samples. Keep as paper-only or tiny entertainment stake."
        }
    })
}

pub(crate) fn load_worldcup_knockout_score_priors(root: &Path) -> Value {
    let path = score_prior_path(root);
    fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .unwrap_or_else(fallback_worldcup_knockout_score_priors)
}

pub(crate) fn score_shape_key(score: &str) -> String {
    let Some((home, away)) = parse_score(score) else {
        return score.to_string();
    };
    if home == away {
        return format!("{}-{}", home, away);
    }
    let high = home.max(away);
    let low = home.min(away);
    format!("{}-{}_type", high, low)
}

fn parse_score(score: &str) -> Option<(u32, u32)> {
    let parts = score
        .replace(':', "-")
        .split('-')
        .map(|part| part.trim().parse::<u32>())
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    if parts.len() == 2 {
        Some((parts[0], parts[1]))
    } else {
        None
    }
}

pub(crate) fn score_prior_probability(priors: &Value, score: &str) -> f64 {
    let key = score_shape_key(score);
    priors
        .get("score_shape_priors")
        .and_then(|value| value.get(&key))
        .and_then(Value::as_f64)
        .unwrap_or(0.0)
}

pub(crate) fn score_prior_weight(data_quality_score: f64) -> f64 {
    if data_quality_score < 60.0 {
        0.30
    } else {
        0.20
    }
}

pub(crate) fn score_33_prior_penalty(
    score: &str,
    chaos_score: f64,
    total_goals_5plus_probability: f64,
) -> f64 {
    if score.replace('-', ":") != "3:3" {
        return 1.0;
    }
    if chaos_score >= 85.0 && total_goals_5plus_probability >= 0.18 {
        0.55
    } else {
        0.35
    }
}

pub(crate) fn apply_score_prior_adjustment(
    priors: &Value,
    score: &str,
    model_score_prob: f64,
    data_quality_score: f64,
    chaos_score: f64,
    total_goals_5plus_probability: f64,
) -> Value {
    let prior_prob = score_prior_probability(priors, score);
    let prior_weight = score_prior_weight(data_quality_score);
    let model_weight = 1.0 - prior_weight;
    let penalty = score_33_prior_penalty(score, chaos_score, total_goals_5plus_probability);
    let adjusted = (model_score_prob * model_weight + prior_prob * prior_weight) * penalty;
    json!({
        "score": score,
        "score_shape_key": score_shape_key(score),
        "model_probability": model_score_prob,
        "prior_probability": prior_prob,
        "prior_weight": prior_weight,
        "adjusted_probability": adjusted.max(0.0),
        "is_high_frequency_shape": matches!(score_shape_key(score).as_str(), "1-1" | "2-1_type" | "2-0_type" | "1-0_type"),
        "is_extreme_score": matches!(score_shape_key(score).as_str(), "3-3" | "4-3_type" | "4-2_type" | "6-1_type"),
        "penalty": penalty,
        "risk_tip": explain_score_prior(priors, score)
    })
}

pub(crate) fn explain_score_prior(_priors: &Value, score: &str) -> String {
    match score_shape_key(score).as_str() {
        "1-1" => "高频冷平比分；近两届世界杯淘汰赛90分钟先验 18.75%。".to_string(),
        "2-1_type" => "常见险胜比分；近两届世界杯淘汰赛90分钟先验 15.63%。".to_string(),
        "2-0_type" => "常见强队小胜/不穿剧本；近两届90分钟先验 15.63%。".to_string(),
        "1-0_type" => "淘汰赛保守小胜形态；近两届90分钟先验 9.38%。".to_string(),
        "0-0" => "冷平小球形态；近两届90分钟先验 6.25%。".to_string(),
        "2-2" => "开放式冷平形态；近两届90分钟先验 6.25%。".to_string(),
        "3-3" => "极端比分；近两届世界杯淘汰赛90分钟内3-3出现0次，仅邪修观察。".to_string(),
        _ => "普通比分形态；本系统按竞彩口径统计90分钟比分，加时和点球不计入比分先验。".to_string(),
    }
}

pub(crate) fn score_prior_bonus(score: &str) -> f64 {
    match score_shape_key(score).as_str() {
        "1-1" => 15.0,
        "0-0" | "2-2" => 8.0,
        "2-1_type" | "2-0_type" => 12.0,
        "1-0_type" => 9.0,
        "3-3" => -15.0,
        _ => 0.0,
    }
}

pub(crate) fn get_score_prior_rankings(priors: &Value) -> Value {
    let mut score_rows = priors
        .get("score_shape_priors")
        .and_then(Value::as_object)
        .map(|map| {
            map.iter()
                .map(|(key, value)| (key.clone(), value.as_f64().unwrap_or(0.0)))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    score_rows.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let mut total_rows = priors
        .get("total_goals_priors")
        .and_then(Value::as_object)
        .map(|map| {
            map.iter()
                .filter(|(key, _)| {
                    key.strip_suffix("_goals")
                        .and_then(|prefix| prefix.parse::<u32>().ok())
                        .is_some()
                })
                .map(|(key, value)| (key.clone(), value.as_f64().unwrap_or(0.0)))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    total_rows.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    json!({
        "score_shapes": score_rows.into_iter().map(|(key, probability)| json!({"key": key, "probability": probability})).collect::<Vec<_>>(),
        "total_goals": total_rows.into_iter().map(|(key, probability)| json!({"key": key, "probability": probability})).collect::<Vec<_>>(),
    })
}

pub(crate) fn score_prior_summary(priors: &Value) -> Value {
    let get = |section: &str, key: &str| {
        priors
            .get(section)
            .and_then(|value| value.get(key))
            .and_then(Value::as_f64)
            .unwrap_or(0.0)
    };
    json!({
        "model_version": priors.get("model_version").and_then(Value::as_str).unwrap_or("worldcup_knockout_score_priors_v1"),
        "sample_count": priors.get("sample_count").and_then(Value::as_i64).unwrap_or(32),
        "scope": priors.get("scope").and_then(Value::as_str).unwrap_or("FIFA World Cup knockout stage, 90 minutes only"),
        "draw_90min": get("draw_priors", "draw_90min"),
        "score_1_1": get("score_shape_priors", "1-1"),
        "score_2_1_type": get("score_shape_priors", "2-1_type"),
        "score_2_0_type": get("score_shape_priors", "2-0_type"),
        "score_1_0_type": get("score_shape_priors", "1-0_type"),
        "score_0_0": get("score_shape_priors", "0-0"),
        "score_2_2": get("score_shape_priors", "2-2"),
        "score_3_3": get("score_shape_priors", "3-3"),
        "two_goals": get("total_goals_priors", "2_goals"),
        "three_goals": get("total_goals_priors", "3_goals"),
        "four_goals": get("total_goals_priors", "4_goals"),
        "under_2_5": get("total_goals_priors", "under_2_5"),
        "over_2_5": get("total_goals_priors", "over_2_5"),
        "message": "本系统按竞彩口径统计90分钟比分，加时和点球不计入比分先验。该先验只用于比分参考和剧本扫描，不直接构成下注建议。"
    })
}
