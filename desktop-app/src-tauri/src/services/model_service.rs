use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub(crate) struct TrainedPrediction {
    pub(crate) model_version: String,
    pub(crate) home_win_prob: f64,
    pub(crate) draw_prob: f64,
    pub(crate) away_win_prob: f64,
    pub(crate) home_goals_lambda: f64,
    pub(crate) away_goals_lambda: f64,
    pub(crate) score_probs_json: Value,
    pub(crate) total_goals_probs_json: Value,
}

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct ActiveModelInfo {
    pub(crate) model_available: bool,
    pub(crate) model_version: String,
    pub(crate) model_type: String,
    pub(crate) sample_count: i64,
    pub(crate) training_data_range: Value,
    pub(crate) accuracy: f64,
    pub(crate) log_loss: f64,
    pub(crate) brier_score: f64,
    pub(crate) backtest_roi: f64,
    pub(crate) backtest_final_bet_count: i64,
    pub(crate) backtest_max_drawdown: f64,
    pub(crate) backtest_avg_odds: f64,
    pub(crate) backtest_avg_ev: f64,
    pub(crate) backtest_warning: String,
    pub(crate) fallback_reason: String,
    pub(crate) strategy_rules_summary: Vec<String>,
    pub(crate) global_models_summary: Vec<String>,
    pub(crate) worldcup_correction_available: bool,
    pub(crate) worldcup_correction_version: String,
    pub(crate) worldcup_correction_sample_count: i64,
    pub(crate) worldcup_correction_accuracy: f64,
    pub(crate) worldcup_correction_log_loss: f64,
    pub(crate) worldcup_correction_brier_score: f64,
    pub(crate) worldcup_correction_note: String,
}

#[derive(Debug, Clone)]
pub(crate) struct ModelFeatureInput {
    pub(crate) elo_diff: f64,
    pub(crate) odds_home: f64,
    pub(crate) odds_draw: f64,
    pub(crate) odds_away: f64,
    pub(crate) market_home_prob: f64,
    pub(crate) market_draw_prob: f64,
    pub(crate) market_away_prob: f64,
    pub(crate) market_margin: f64,
    pub(crate) rule_home_lambda: f64,
    pub(crate) rule_away_lambda: f64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct StrategyRuleDecision {
    pub(crate) action: String,
    pub(crate) reason: String,
    pub(crate) matched_rules: Vec<String>,
}

pub(crate) fn training_models_dir(app_dir: &Path) -> PathBuf {
    let mut roots = vec![app_dir.to_path_buf()];
    if let Ok(current_dir) = std::env::current_dir() {
        roots.push(current_dir);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            roots.push(parent.to_path_buf());
        }
    }

    for root in &roots {
        let direct = root.join("training").join("models");
        if direct.exists() {
            return direct;
        }
        let nested = root.join("desktop-app").join("training").join("models");
        if nested.exists() {
            return nested;
        }
        for ancestor in root.ancestors() {
            let candidate = ancestor.join("training").join("models");
            if candidate.exists() {
                return candidate;
            }
            let candidate = ancestor.join("desktop-app").join("training").join("models");
            if candidate.exists() {
                return candidate;
            }
        }
    }

    app_dir.join("training").join("models")
}

fn read_json(path: &Path) -> Option<Value> {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
}

fn bucket(value: f64, ranges: &[(f64, f64)], labels: &[&str]) -> String {
    for ((low, high), label) in ranges.iter().zip(labels.iter()) {
        if value >= *low && value < *high {
            return (*label).to_string();
        }
    }
    labels.last().copied().unwrap_or("-").to_string()
}

pub(crate) fn strategy_odds_range(value: f64) -> String {
    bucket(
        value,
        &[(0.0, 1.8), (1.8, 2.5), (2.5, 3.51), (3.51, 999.0)],
        &["1.00-1.79", "1.80-2.49", "2.50-3.50", "3.50+"],
    )
}

pub(crate) fn strategy_probability_range(value: f64) -> String {
    bucket(
        value,
        &[
            (0.0, 0.25),
            (0.25, 0.35),
            (0.35, 0.45),
            (0.45, 0.55),
            (0.55, 1.01),
        ],
        &["0%-25%", "25%-35%", "35%-45%", "45%-55%", "55%+"],
    )
}

pub(crate) fn strategy_ev_range(value: f64) -> String {
    bucket(
        value,
        &[
            (-999.0, 0.0),
            (0.0, 0.05),
            (0.05, 0.10),
            (0.10, 0.20),
            (0.20, 999.0),
        ],
        &["负EV", "0-5%", "5-10%", "10-20%", "20%+"],
    )
}

pub(crate) fn strategy_advantage_range(value: f64) -> String {
    bucket(
        value,
        &[
            (-999.0, 0.0),
            (0.0, 0.05),
            (0.05, 0.10),
            (0.10, 0.20),
            (0.20, 999.0),
        ],
        &["负优势", "0-5%", "5-10%", "10-20%", "20%+"],
    )
}

pub(crate) fn strategy_selection(pick: &str) -> Option<&'static str> {
    match pick {
        "主胜" => Some("H"),
        "平局" => Some("D"),
        "客胜" => Some("A"),
        _ => None,
    }
}

fn action_rank(action: &str) -> i32 {
    match action {
        "hard_ban" => 4,
        "observe_only" => 3,
        "downgrade" => 2,
        "sample_too_small" => 1,
        "allow_candidate" => 0,
        _ => 0,
    }
}

pub(crate) fn strategy_rule_decision(
    app_dir: &Path,
    pick: &str,
    odds: f64,
    probability: f64,
    ev: f64,
    advantage: f64,
) -> StrategyRuleDecision {
    let dir = training_models_dir(app_dir);
    let Some(rules_payload) = read_json(&dir.join("strategy_rules_v1.json")) else {
        return StrategyRuleDecision {
            action: "none".to_string(),
            reason: "未加载训练策略规则".to_string(),
            matched_rules: Vec::new(),
        };
    };
    let Some(rules) = rules_payload.get("rules").and_then(Value::as_array) else {
        return StrategyRuleDecision {
            action: "none".to_string(),
            reason: "训练策略规则为空".to_string(),
            matched_rules: Vec::new(),
        };
    };
    let selection = strategy_selection(pick).unwrap_or("");
    let candidates = [
        ("selection", selection.to_string()),
        ("odds_range", strategy_odds_range(odds)),
        ("probability_range", strategy_probability_range(probability)),
        ("ev_range", strategy_ev_range(ev)),
        ("advantage_range", strategy_advantage_range(advantage)),
        (
            "result_type",
            if selection == "D" {
                "draw".to_string()
            } else {
                "home_away".to_string()
            },
        ),
    ];
    let mut best_action = "allow_candidate".to_string();
    let mut reasons = Vec::new();
    let mut matched = Vec::new();
    for (dimension, group) in candidates {
        for rule in rules {
            let same_dimension =
                rule.get("dimension").and_then(Value::as_str).unwrap_or("") == dimension;
            let same_group = rule.get("group").and_then(Value::as_str).unwrap_or("") == group;
            if !same_dimension || !same_group {
                continue;
            }
            let action = rule
                .get("action")
                .and_then(Value::as_str)
                .unwrap_or("allow_candidate");
            let reason = rule.get("reason").and_then(Value::as_str).unwrap_or("");
            if action_rank(action) > action_rank(&best_action) {
                best_action = action.to_string();
            }
            if !reason.is_empty() {
                reasons.push(format!("{}={}：{}", dimension, group, reason));
            }
            matched.push(format!("{}={}", dimension, group));
        }
    }
    StrategyRuleDecision {
        action: best_action,
        reason: if reasons.is_empty() {
            "训练规则未命中明显风险区间".to_string()
        } else {
            reasons.join("；")
        },
        matched_rules: matched,
    }
}

fn model_has_samples(model: &Value) -> bool {
    model
        .pointer("/training_data_range/train_count")
        .and_then(Value::as_i64)
        .or_else(|| {
            model
                .pointer("/metrics/train_count")
                .and_then(Value::as_i64)
        })
        .unwrap_or(0)
        > 0
}

fn feature_value(name: &str, input: &ModelFeatureInput) -> f64 {
    let market_probs = [
        input.market_home_prob,
        input.market_draw_prob,
        input.market_away_prob,
    ];
    let market_favorite_prob = market_probs.iter().copied().fold(0.0, f64::max);
    let market_entropy = market_probs
        .iter()
        .filter(|prob| **prob > 0.0)
        .map(|prob| -prob * prob.ln())
        .sum::<f64>();
    match name {
        "elo_diff" => input.elo_diff,
        "odds_home" => input.odds_home,
        "odds_draw" => input.odds_draw,
        "odds_away" => input.odds_away,
        "market_home_prob" => input.market_home_prob,
        "market_draw_prob" => input.market_draw_prob,
        "market_away_prob" => input.market_away_prob,
        "market_home_away_prob_diff" => input.market_home_prob - input.market_away_prob,
        "market_favorite_prob" => market_favorite_prob,
        "market_entropy" => market_entropy,
        "market_margin" => input.market_margin,
        "odds_home_away_ratio" => {
            if input.odds_home > 0.0 && input.odds_away > 0.0 {
                input.odds_home / input.odds_away
            } else {
                1.0
            }
        }
        "avg_max_home_gap" | "avg_max_draw_gap" | "avg_max_away_gap" | "market_dispersion" => 0.0,
        "is_home_favorite" => {
            if input.market_home_prob >= input.market_draw_prob
                && input.market_home_prob >= input.market_away_prob
            {
                1.0
            } else {
                0.0
            }
        }
        "is_draw_short" => {
            if input.market_draw_prob >= 0.30 {
                1.0
            } else {
                0.0
            }
        }
        "home_recent_goals_for" | "home_attack_strength" => input.rule_home_lambda,
        "away_recent_goals_for" | "away_attack_strength" => input.rule_away_lambda,
        "home_recent_goals_against" | "home_defense_strength" => input.rule_away_lambda,
        "away_recent_goals_against" | "away_defense_strength" => input.rule_home_lambda,
        "recent_goals_for_diff" => input.rule_home_lambda - input.rule_away_lambda,
        "recent_goals_against_diff" => input.rule_away_lambda - input.rule_home_lambda,
        "attack_strength_diff" | "defense_strength_diff" => {
            input.rule_home_lambda - input.rule_away_lambda
        }
        "attack_x_defense_home" => input.rule_home_lambda,
        "attack_x_defense_away" => input.rule_away_lambda,
        "home_recent_win_rate" | "away_recent_win_rate" => 0.33,
        "recent_win_rate_diff" | "recent_draw_rate_diff" | "recent_points_diff" => 0.0,
        "home_recent_draw_rate" | "away_recent_draw_rate" => 0.25,
        "home_recent_points_per_game" | "away_recent_points_per_game" => 1.2,
        "home_recent_matches" | "away_recent_matches" => 5.0,
        "home_rest_days" | "away_rest_days" => 7.0,
        "rest_days_diff" => 0.0,
        "season_progress" => 0.5,
        "is_international_or_cup" => 1.0,
        "is_knockout_stage" => 1.0,
        "is_neutral_site" => 1.0,
        "fifa_rank_diff" => -input.elo_diff / 5.5,
        "elo_proxy_diff" => input.elo_diff,
        "squad_strength_diff" => input.elo_diff,
        "tournament_importance" => 1.35,
        "home_history_low" | "away_history_low" | "history_low_any" => 0.0,
        _ => 0.0,
    }
}

fn normalized_feature(model: &Value, feature: &str, input: &ModelFeatureInput) -> f64 {
    let mean = model
        .pointer(&format!("/scaler_mean/{}", feature))
        .or_else(|| model.pointer(&format!("/normalization/mean/{}", feature)))
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    let std = model
        .pointer(&format!("/scaler_scale/{}", feature))
        .or_else(|| model.pointer(&format!("/normalization/std/{}", feature)))
        .and_then(Value::as_f64)
        .unwrap_or(1.0)
        .max(0.0001);
    (feature_value(feature, input) - mean) / std
}

fn softmax(scores: &[f64]) -> Vec<f64> {
    let max_score = scores.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let exps = scores
        .iter()
        .map(|score| (score - max_score).exp())
        .collect::<Vec<_>>();
    let sum: f64 = exps.iter().sum();
    if sum <= 0.0 {
        return vec![1.0 / scores.len() as f64; scores.len()];
    }
    exps.into_iter().map(|value| value / sum).collect()
}

fn predict_outcome(model: &Value, input: &ModelFeatureInput) -> Option<(f64, f64, f64)> {
    if !model_has_samples(model) {
        return None;
    }
    let features = model
        .get("feature_names")
        .or_else(|| model.get("feature_schema"))?
        .as_array()?;
    let classes_value = model.get("classes").and_then(Value::as_array)?;
    let mut scores = Vec::new();
    let mut classes = Vec::new();
    for class_name in classes_value.iter().filter_map(Value::as_str) {
        classes.push(class_name.to_string());
        let mut score = model
            .pointer(&format!("/intercept/{}", class_name))
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        let coeffs = model
            .pointer(&format!("/coefficients/{}", class_name))
            .and_then(Value::as_array)?;
        for (idx, feature) in features.iter().filter_map(Value::as_str).enumerate() {
            let coeff = coeffs.get(idx).and_then(Value::as_f64).unwrap_or(0.0);
            score += coeff * normalized_feature(model, feature, input);
        }
        scores.push(score);
    }
    let probs = softmax(&scores);
    let find = |target: &str| -> f64 {
        classes
            .iter()
            .position(|class_name| class_name == target)
            .and_then(|idx| probs.get(idx).copied())
            .unwrap_or(0.0)
    };
    Some((find("H"), find("D"), find("A")))
}

fn prediction_outcome_model(dir: &Path) -> Option<Value> {
    let ensemble = read_json(&dir.join("outcome_ensemble_model_v1.json"));
    if let Some(ensemble) = ensemble {
        if model_has_samples(&ensemble) {
            let primary = ensemble
                .get("rust_primary_member")
                .and_then(Value::as_str)
                .unwrap_or("logistic");
            if let Some(member) =
                ensemble
                    .get("members")
                    .and_then(Value::as_array)
                    .and_then(|members| {
                        members
                            .iter()
                            .find(|item| item.get("name").and_then(Value::as_str) == Some(primary))
                    })
            {
                let mut member = member.clone();
                member["model_version"] = ensemble
                    .get("model_version")
                    .cloned()
                    .unwrap_or_else(|| Value::String("outcome_ensemble_model_v1".to_string()));
                member["training_data_range"] = ensemble
                    .get("training_data_range")
                    .cloned()
                    .unwrap_or(Value::Null);
                return Some(member);
            }
        }
    }
    read_json(&dir.join("outcome_model_v1.json"))
}

fn predict_poisson_lambda(model: &Value, input: &ModelFeatureInput) -> Option<f64> {
    if !model_has_samples(model) {
        return None;
    }
    let features = model.get("feature_names")?.as_array()?;
    let coeffs = model.get("coefficients")?.as_array()?;
    let mut score = model
        .get("intercept")
        .and_then(Value::as_f64)
        .unwrap_or(1.2f64.ln());
    for (idx, feature) in features.iter().filter_map(Value::as_str).enumerate() {
        score += coeffs.get(idx).and_then(Value::as_f64).unwrap_or(0.0)
            * normalized_feature(model, feature, input);
    }
    Some(score.clamp(-2.0, 2.0).exp().clamp(0.2, 4.8))
}

fn handicap_feature_value(
    name: &str,
    home_prob: f64,
    draw_prob: f64,
    away_prob: f64,
    home_lambda: f64,
    away_lambda: f64,
    line: f64,
) -> f64 {
    match name {
        "home_prob" => home_prob,
        "draw_prob" => draw_prob,
        "away_prob" => away_prob,
        "home_lambda" => home_lambda,
        "away_lambda" => away_lambda,
        "handicap_line" => line,
        _ => 0.0,
    }
}

pub(crate) fn predict_handicap_with_training_models(
    app_dir: &Path,
    input: &ModelFeatureInput,
    line: f64,
) -> Option<(f64, f64, f64)> {
    let dir = training_models_dir(app_dir);
    let manifest = read_json(&dir.join("model_manifest.json"))?;
    if !manifest
        .get("ready")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return None;
    }
    let outcome = prediction_outcome_model(&dir)?;
    let handicap = read_json(&dir.join("handicap_mapping_model_v1.json"))?;
    if !model_has_samples(&handicap) {
        return None;
    }
    let raw = predict_outcome(&outcome, input)?;
    let calibrated = calibrate_probs(read_json(&dir.join("calibrator_v1.json")).as_ref(), raw);
    let (home_prob, draw_prob, away_prob) = blend_with_market(
        read_json(&dir.join("probability_blend_v1.json")).as_ref(),
        calibrated,
        input,
    );
    let home_lambda = read_json(&dir.join("goals_home_model_v1.json"))
        .as_ref()
        .and_then(|model| predict_poisson_lambda(model, input))
        .unwrap_or(input.rule_home_lambda);
    let away_lambda = read_json(&dir.join("goals_away_model_v1.json"))
        .as_ref()
        .and_then(|model| predict_poisson_lambda(model, input))
        .unwrap_or(input.rule_away_lambda);
    let features = handicap.get("feature_names")?.as_array()?;
    let classes_value = handicap.get("classes")?.as_array()?;
    let mut scores = Vec::new();
    let mut classes = Vec::new();
    for class_name in classes_value.iter().filter_map(Value::as_str) {
        classes.push(class_name.to_string());
        let mut score = handicap
            .pointer(&format!("/intercept/{}", class_name))
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        let coeffs = handicap
            .pointer(&format!("/coefficients/{}", class_name))
            .and_then(Value::as_array)?;
        for (idx, feature) in features.iter().filter_map(Value::as_str).enumerate() {
            let mean = handicap
                .pointer(&format!("/scaler_mean/{}", feature))
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            let std = handicap
                .pointer(&format!("/scaler_scale/{}", feature))
                .and_then(Value::as_f64)
                .unwrap_or(1.0)
                .max(0.0001);
            let value = handicap_feature_value(
                feature,
                home_prob,
                draw_prob,
                away_prob,
                home_lambda,
                away_lambda,
                line,
            );
            score +=
                coeffs.get(idx).and_then(Value::as_f64).unwrap_or(0.0) * ((value - mean) / std);
        }
        scores.push(score);
    }
    let probs = softmax(&scores);
    let find = |target: &str| -> f64 {
        classes
            .iter()
            .position(|class_name| class_name == target)
            .and_then(|idx| probs.get(idx).copied())
            .unwrap_or(0.0)
    };
    Some((find("让胜"), find("让平"), find("让负")))
}

fn calibrate_one(class_bins: Option<&Value>, probability: f64) -> f64 {
    let Some(bins) = class_bins.and_then(Value::as_array) else {
        return probability;
    };
    for bin in bins {
        let low = bin.get("low").and_then(Value::as_f64).unwrap_or(0.0);
        let high = bin.get("high").and_then(Value::as_f64).unwrap_or(1.0);
        if probability >= low && probability < high {
            let empirical = bin
                .get("empirical_prob")
                .and_then(Value::as_f64)
                .unwrap_or(probability);
            return (probability * 0.55 + empirical * 0.45).clamp(0.001, 0.999);
        }
    }
    probability
}

fn calibrate_probs(calibrator: Option<&Value>, probs: (f64, f64, f64)) -> (f64, f64, f64) {
    let Some(calibrator) = calibrator else {
        return probs;
    };
    let h = calibrate_one(calibrator.pointer("/bins/H"), probs.0);
    let d = calibrate_one(calibrator.pointer("/bins/D"), probs.1);
    let a = calibrate_one(calibrator.pointer("/bins/A"), probs.2);
    let sum = h + d + a;
    if sum <= 0.0 {
        probs
    } else {
        (h / sum, d / sum, a / sum)
    }
}

fn blend_with_market(
    blend: Option<&Value>,
    probs: (f64, f64, f64),
    input: &ModelFeatureInput,
) -> (f64, f64, f64) {
    let Some(blend) = blend else {
        return probs;
    };
    let mut model_weight = blend
        .get("model_weight")
        .and_then(Value::as_f64)
        .unwrap_or(1.0)
        .clamp(0.0, 1.0);
    let mut market_weight = blend
        .get("market_weight")
        .and_then(Value::as_f64)
        .unwrap_or(1.0 - model_weight)
        .clamp(0.0, 1.0);
    let market_sum = input.market_home_prob + input.market_draw_prob + input.market_away_prob;
    if market_sum <= 0.0 {
        return probs;
    }
    let favorite = input
        .market_home_prob
        .max(input.market_draw_prob)
        .max(input.market_away_prob);
    if let Some(segment) = blend
        .get("dynamic_segments")
        .and_then(Value::as_array)
        .and_then(|segments| {
            segments.iter().find(|item| {
                let low = item.get("low").and_then(Value::as_f64).unwrap_or(0.0);
                let high = item.get("high").and_then(Value::as_f64).unwrap_or(1.01);
                favorite >= low && favorite < high
            })
        })
    {
        model_weight = segment
            .get("model_weight")
            .and_then(Value::as_f64)
            .unwrap_or(model_weight)
            .clamp(0.0, 1.0);
        market_weight = segment
            .get("market_weight")
            .and_then(Value::as_f64)
            .unwrap_or(1.0 - model_weight)
            .clamp(0.0, 1.0);
    }
    let h = probs.0 * model_weight + (input.market_home_prob / market_sum) * market_weight;
    let d = probs.1 * model_weight + (input.market_draw_prob / market_sum) * market_weight;
    let a = probs.2 * model_weight + (input.market_away_prob / market_sum) * market_weight;
    let sum = h + d + a;
    if sum <= 0.0 {
        probs
    } else {
        (h / sum, d / sum, a / sum)
    }
}

fn poisson(k: u32, lambda: f64) -> f64 {
    let factorial = (1..=k).fold(1.0, |acc, item| acc * item as f64);
    (-lambda).exp() * lambda.powi(k as i32) / factorial
}

pub(crate) fn trained_score_probs(
    home_lambda: f64,
    away_lambda: f64,
    max_goals: u32,
) -> (Value, Value) {
    let mut scores = Vec::new();
    let mut total = 0.0;
    for home in 0..=max_goals {
        for away in 0..=max_goals {
            let probability = poisson(home, home_lambda) * poisson(away, away_lambda);
            total += probability;
            scores.push((home, away, probability));
        }
    }
    let total = total.max(0.0001);
    let score_rows = scores
        .iter()
        .map(|(home, away, probability)| serde_json::json!({ "score": format!("{}:{}", home, away), "probability": probability / total }))
        .collect::<Vec<_>>();
    let mut totals = vec![0.0f64; 8];
    for (home, away, probability) in scores {
        totals[((home + away).min(7)) as usize] += probability / total;
    }
    let total_rows = totals
        .iter()
        .enumerate()
        .map(|(idx, probability)| serde_json::json!({ "total_goals": if idx == 7 { "7+".to_string() } else { idx.to_string() }, "probability": probability }))
        .collect::<Vec<_>>();
    (Value::Array(score_rows), Value::Array(total_rows))
}

pub(crate) fn predict_with_training_models(
    app_dir: &Path,
    input: &ModelFeatureInput,
) -> Option<TrainedPrediction> {
    let dir = training_models_dir(app_dir);
    let manifest = read_json(&dir.join("model_manifest.json"))?;
    if !manifest
        .get("ready")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return None;
    }
    let outcome = prediction_outcome_model(&dir)?;
    let raw_probs = predict_outcome(&outcome, input)?;
    let calibrated = calibrate_probs(
        read_json(&dir.join("calibrator_v1.json")).as_ref(),
        raw_probs,
    );
    let probs = blend_with_market(
        read_json(&dir.join("probability_blend_v1.json")).as_ref(),
        calibrated,
        input,
    );
    let home_lambda = read_json(&dir.join("goals_home_model_v1.json"))
        .as_ref()
        .and_then(|model| predict_poisson_lambda(model, input))
        .unwrap_or(input.rule_home_lambda);
    let away_lambda = read_json(&dir.join("goals_away_model_v1.json"))
        .as_ref()
        .and_then(|model| predict_poisson_lambda(model, input))
        .unwrap_or(input.rule_away_lambda);
    let (score_probs_json, total_goals_probs_json) =
        trained_score_probs(home_lambda, away_lambda, 5);
    Some(TrainedPrediction {
        model_version: outcome
            .get("model_version")
            .and_then(Value::as_str)
            .unwrap_or("outcome_model_v1")
            .to_string(),
        home_win_prob: probs.0,
        draw_prob: probs.1,
        away_win_prob: probs.2,
        home_goals_lambda: home_lambda,
        away_goals_lambda: away_lambda,
        score_probs_json,
        total_goals_probs_json,
    })
}

pub(crate) fn active_model_info(app_dir: &Path) -> ActiveModelInfo {
    let dir = training_models_dir(app_dir);
    let Some(manifest) = read_json(&dir.join("model_manifest.json")) else {
        return ActiveModelInfo {
            model_available: false,
            model_version: "rules-dixon-coles-v1".to_string(),
            model_type: "规则模型 fallback".to_string(),
            sample_count: 0,
            training_data_range: Value::Null,
            accuracy: 0.0,
            log_loss: 0.0,
            brier_score: 0.0,
            backtest_roi: 0.0,
            backtest_final_bet_count: 0,
            backtest_max_drawdown: 0.0,
            backtest_avg_odds: 0.0,
            backtest_avg_ev: 0.0,
            backtest_warning: "暂无正式投注样本，不能评估 ROI".to_string(),
            fallback_reason: "未检测到 training/models/model_manifest.json，当前使用规则模型。"
                .to_string(),
            strategy_rules_summary: Vec::new(),
            global_models_summary: Vec::new(),
            worldcup_correction_available: false,
            worldcup_correction_version: String::new(),
            worldcup_correction_sample_count: 0,
            worldcup_correction_accuracy: 0.0,
            worldcup_correction_log_loss: 0.0,
            worldcup_correction_brier_score: 0.0,
            worldcup_correction_note: "未加载训练模型，世界杯临场修正层不可用。".to_string(),
        };
    };
    let outcome = read_json(&dir.join("outcome_model_v1.json")).unwrap_or(Value::Null);
    let ensemble = read_json(&dir.join("outcome_ensemble_model_v1.json")).unwrap_or(Value::Null);
    let metrics = manifest.get("metrics_summary").unwrap_or(&Value::Null);
    let backtest = manifest.get("backtest_summary").unwrap_or(&Value::Null);
    let ready = manifest
        .get("ready")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        && outcome
            .pointer("/metrics/train_count")
            .and_then(Value::as_i64)
            .unwrap_or(0)
            > 0;
    let missing = manifest
        .get("missing_files")
        .and_then(Value::as_array)
        .map(|items| items.len())
        .unwrap_or(0);
    let worldcup_status = manifest
        .pointer("/global_models/worldcup_live_correction_status")
        .unwrap_or(&Value::Null);
    let worldcup_report = worldcup_status.get("report").unwrap_or(&Value::Null);
    let worldcup_metrics = worldcup_report.get("metrics").unwrap_or(&Value::Null);
    let worldcup_available = worldcup_status
        .get("ready")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || manifest
            .pointer("/global_models/worldcup_live_correction")
            .and_then(Value::as_str)
            .is_some();
    let strategy_rules_summary = read_json(&dir.join("strategy_rules_v1.json"))
        .and_then(|payload| payload.get("rules").and_then(Value::as_array).cloned())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|rule| {
            let action = rule.get("action").and_then(Value::as_str).unwrap_or("");
            if action == "allow_candidate" {
                return None;
            }
            Some(format!(
                "{}={}：{}（样本{}，ROI {:.2}%）",
                rule.get("dimension").and_then(Value::as_str).unwrap_or("-"),
                rule.get("group").and_then(Value::as_str).unwrap_or("-"),
                action,
                rule.get("sample_count")
                    .and_then(Value::as_i64)
                    .unwrap_or(0),
                rule.get("roi").and_then(Value::as_f64).unwrap_or(0.0) * 100.0
            ))
        })
        .take(8)
        .collect::<Vec<_>>();
    ActiveModelInfo {
        model_available: ready,
        model_version: if ready {
            manifest
                .get("active_model_version")
                .and_then(Value::as_str)
                .unwrap_or("outcome_model_v1")
                .to_string()
        } else {
            "rules-dixon-coles-v1".to_string()
        },
        model_type: if ready {
            ensemble
                .get("model_type")
                .or_else(|| outcome.get("model_type"))
                .and_then(Value::as_str)
                .unwrap_or("sklearn_logistic_regression_multinomial")
                .to_string()
        } else {
            "规则模型 fallback".to_string()
        },
        sample_count: metrics
            .get("sample_count")
            .and_then(Value::as_i64)
            .unwrap_or(0),
        training_data_range: manifest
            .get("training_data_range")
            .cloned()
            .unwrap_or(Value::Null),
        accuracy: metrics
            .get("accuracy")
            .and_then(Value::as_f64)
            .unwrap_or(0.0),
        log_loss: metrics
            .get("log_loss")
            .and_then(Value::as_f64)
            .unwrap_or(0.0),
        brier_score: metrics
            .get("brier_score")
            .and_then(Value::as_f64)
            .unwrap_or(0.0),
        backtest_roi: backtest.get("roi").and_then(Value::as_f64).unwrap_or(0.0),
        backtest_final_bet_count: backtest
            .get("final_bet_count")
            .and_then(Value::as_i64)
            .unwrap_or(0),
        backtest_max_drawdown: backtest
            .get("max_drawdown")
            .and_then(Value::as_f64)
            .unwrap_or(0.0),
        backtest_avg_odds: backtest
            .get("avg_odds")
            .and_then(Value::as_f64)
            .unwrap_or(0.0),
        backtest_avg_ev: backtest
            .get("avg_ev")
            .and_then(Value::as_f64)
            .unwrap_or(0.0),
        backtest_warning: backtest
            .get("warning")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        fallback_reason: if ready {
            String::new()
        } else if missing > 0 {
            "训练模型文件不完整，当前使用规则模型。".to_string()
        } else {
            "训练样本为空或不足，当前使用规则模型。".to_string()
        },
        strategy_rules_summary,
        global_models_summary: manifest
            .get("global_models")
            .and_then(Value::as_object)
            .map(|models| {
                let mut rows = vec![
                    format!(
                        "胜平负：{}",
                        models.get("outcome").and_then(Value::as_str).unwrap_or("-")
                    ),
                    format!(
                        "胜平负集成：{}",
                        models
                            .get("outcome_ensemble")
                            .and_then(Value::as_str)
                            .unwrap_or("-")
                    ),
                    format!(
                        "概率校准：{}",
                        models
                            .get("calibrator")
                            .and_then(Value::as_str)
                            .unwrap_or("-")
                    ),
                    format!(
                        "市场概率融合：{}",
                        models
                            .get("probability_blend")
                            .and_then(Value::as_str)
                            .unwrap_or("-")
                    ),
                    format!(
                        "主队进球：{}",
                        models
                            .get("goals_home")
                            .and_then(Value::as_str)
                            .unwrap_or("-")
                    ),
                    format!(
                        "客队进球：{}",
                        models
                            .get("goals_away")
                            .and_then(Value::as_str)
                            .unwrap_or("-")
                    ),
                    format!(
                        "让球映射：{}",
                        models
                            .get("handicap")
                            .and_then(Value::as_str)
                            .unwrap_or("-")
                    ),
                    format!(
                        "世界杯临场修正：{}",
                        models
                            .get("worldcup_live_correction")
                            .and_then(Value::as_str)
                            .unwrap_or("样本不足")
                    ),
                ];
                if let Some(layers) = manifest
                    .get("worldcup_model_v2_layers")
                    .and_then(Value::as_array)
                {
                    rows.push(format!("七层模型：已登记 {} 层", layers.len()));
                }
                rows
            })
            .unwrap_or_default(),
        worldcup_correction_available: worldcup_available,
        worldcup_correction_version: manifest
            .pointer("/global_models/worldcup_live_correction")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        worldcup_correction_sample_count: worldcup_metrics
            .get("sample_count")
            .and_then(Value::as_i64)
            .unwrap_or(0),
        worldcup_correction_accuracy: worldcup_metrics
            .get("accuracy")
            .and_then(Value::as_f64)
            .unwrap_or(0.0),
        worldcup_correction_log_loss: worldcup_metrics
            .get("log_loss")
            .and_then(Value::as_f64)
            .unwrap_or(0.0),
        worldcup_correction_brier_score: worldcup_metrics
            .get("brier_score")
            .and_then(Value::as_f64)
            .unwrap_or(0.0),
        worldcup_correction_note: if worldcup_available {
            "已用2018/2022世界杯历史赛前赔率训练，只修正投注推荐置信度，不改写胜平负真实概率。"
                .to_string()
        } else {
            worldcup_report
                .get("reason")
                .and_then(Value::as_str)
                .unwrap_or("世界杯临场修正层样本不足。")
                .to_string()
        },
    }
}
