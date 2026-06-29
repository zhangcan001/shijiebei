import numpy as np
import pandas as pd

from common import MODELS_DIR, PROCESSED_DIR, REPORTS_DIR, ensure_dirs, now_iso, read_json, write_json


SELECTION_TO_ODDS = {"H": "odds_home", "D": "odds_draw", "A": "odds_away"}
MARKET_PROB_COLUMNS = {"H": "market_home_prob", "D": "market_draw_prob", "A": "market_away_prob"}
GROUP_DIMENSIONS = [
    "play_type",
    "selection",
    "odds_range",
    "probability_range",
    "ev_range",
    "data_quality_range",
    "risk_tags",
]
POOL_DEFINITIONS = [
    ("recommend_pool", "recommend", "正式推荐池"),
    ("small_stake_pool", "small_stake", "小注池"),
    ("observe_only_pool", "observe_only", "观察池"),
    ("hard_ban_pool", "hard_ban", "禁买池"),
    ("wait_pool", "wait_for_lineup", "等待池"),
]
EV_THRESHOLDS = [0.0, 0.03, 0.05, 0.08, 0.10, 0.12]
ODDS_RANGES = [
    ("1.10-1.30", 1.10, 1.30),
    ("1.30-1.50", 1.30, 1.50),
    ("1.50-1.80", 1.50, 1.80),
    ("1.80-2.20", 1.80, 2.20),
    ("2.20-3.00", 2.20, 3.00),
    ("3.00+", 3.00, None),
]
PROBABILITY_RANGES = [
    ("<40%", None, 0.40),
    ("40%-45%", 0.40, 0.45),
    ("45%-50%", 0.45, 0.50),
    ("50%-55%", 0.50, 0.55),
    ("55%-60%", 0.55, 0.60),
    ("60%-65%", 0.60, 0.65),
    ("65%+", 0.65, None),
]

RULE_DEFINITIONS = {
    "invalid_odds_or_prob": {"rule_name": "无效赔率或概率", "action": "hard_ban"},
    "negative_ev": {"rule_name": "EV为负", "action": "hard_ban"},
    "low_ev": {"rule_name": "EV不足", "action": "observe_only"},
    "odds_out_of_range": {"rule_name": "赔率超出正式区间", "action": "observe_only"},
    "low_model_probability": {"rule_name": "模型概率不足", "action": "observe_only"},
}


def predict_probs(df, model):
    classes = model.get("classes", ["A", "D", "H"])
    features = model.get("feature_names", [])
    if df.empty or not features or model.get("metrics", {}).get("train_count", 0) <= 0:
        return np.zeros((len(df), len(classes))), classes
    x = df[features].fillna(0.0).astype(float).copy()
    for feature in features:
        mean = model.get("scaler_mean", {}).get(feature, 0.0)
        scale = model.get("scaler_scale", {}).get(feature, 1.0) or 1.0
        x[feature] = (x[feature] - mean) / scale
    scores = []
    for cls in classes:
        coeffs = np.array(model.get("coefficients", {}).get(cls, [0.0] * len(features)), dtype=float)
        intercept = float(model.get("intercept", {}).get(cls, 0.0))
        scores.append(x.values @ coeffs + intercept)
    logits = np.vstack(scores).T
    logits = logits - logits.max(axis=1, keepdims=True)
    exp = np.exp(logits)
    return exp / exp.sum(axis=1, keepdims=True), classes


def active_prediction_model():
    ensemble = read_json(MODELS_DIR / "outcome_ensemble_model_v1.json", {})
    if ensemble.get("metrics", {}).get("train_count", 0) > 0:
        primary = ensemble.get("rust_primary_member", "logistic")
        member = next((item for item in ensemble.get("members", []) if item.get("name") == primary), None)
        if member:
            member = dict(member)
            member["metrics"] = ensemble.get("metrics", member.get("metrics", {}))
            return member
    return read_json(MODELS_DIR / "outcome_model_v1.json", {})


def apply_calibrator(probs, classes):
    calibrator = read_json(MODELS_DIR / "calibrator_v1.json", {})
    bins = calibrator.get("bins", {})
    if not bins or probs.size == 0:
        return probs
    adjusted = probs.copy()
    for cls_idx, cls in enumerate(classes):
        for row_idx, value in enumerate(probs[:, cls_idx]):
            for item in bins.get(cls, []):
                if item.get("low", 0.0) <= value < item.get("high", 1.0):
                    empirical = item.get("empirical_prob", value)
                    adjusted[row_idx, cls_idx] = value * 0.55 + empirical * 0.45
                    break
    row_sum = adjusted.sum(axis=1, keepdims=True)
    row_sum[row_sum <= 0] = 1.0
    return adjusted / row_sum


def apply_probability_blend(probs, classes, df):
    blend = read_json(MODELS_DIR / "probability_blend_v1.json", {})
    if not blend or probs.size == 0:
        return probs
    default_model_weight = float(blend.get("model_weight", 1.0) or 1.0)
    default_market_weight = float(blend.get("market_weight", 0.0) or 0.0)
    market_rows = []
    for _, row in df.iterrows():
        values = [float(row.get(MARKET_PROB_COLUMNS.get(cls, ""), 0.0) or 0.0) for cls in classes]
        total = sum(values)
        market_rows.append([1.0 / len(classes)] * len(classes) if total <= 0 else [value / total for value in values])
    market = np.array(market_rows, dtype=float)
    blended = probs.copy()
    segments = blend.get("dynamic_segments", []) or []
    for row_idx, (_, row) in enumerate(df.iterrows()):
        favorite = float(row.get("market_favorite_prob", 0.0) or 0.0)
        segment = next((item for item in segments if float(item.get("low", 0.0)) <= favorite < float(item.get("high", 1.01))), None)
        model_weight = float((segment or {}).get("model_weight", default_model_weight) or default_model_weight)
        market_weight = float((segment or {}).get("market_weight", default_market_weight) or default_market_weight)
        blended[row_idx, :] = probs[row_idx, :] * model_weight + market[row_idx, :] * market_weight
    row_sum = blended.sum(axis=1, keepdims=True)
    row_sum[row_sum <= 0] = 1.0
    return blended / row_sum


def max_drawdown(profits):
    equity = 0.0
    peak = 0.0
    max_dd = 0.0
    for profit in profits:
        equity += profit
        peak = max(peak, equity)
        max_dd = max(max_dd, peak - equity)
    return float(max_dd)


def odds_bucket(value):
    if value < 1.30:
        return "1.10-1.30"
    if value < 1.50:
        return "1.30-1.50"
    if value < 1.80:
        return "1.50-1.80"
    if value < 2.20:
        return "1.80-2.20"
    if value < 3.00:
        return "2.20-3.00"
    return "3.00+"


def probability_bucket(value):
    if value < 0.40:
        return "<40%"
    if value < 0.45:
        return "40%-45%"
    if value < 0.50:
        return "45%-50%"
    if value < 0.55:
        return "50%-55%"
    if value < 0.60:
        return "55%-60%"
    if value < 0.65:
        return "60%-65%"
    return "65%+"


def ev_bucket(value):
    if value < 0:
        return "<0"
    if value < 0.03:
        return "0%-3%"
    if value < 0.05:
        return "3%-5%"
    if value < 0.08:
        return "5%-8%"
    if value < 0.12:
        return "8%-12%"
    return "12%+"


def data_quality_bucket(value):
    if value < 55:
        return "<55"
    if value < 65:
        return "55-65"
    if value < 75:
        return "65-75"
    if value < 85:
        return "75-85"
    return "85+"


def risk_bucket(row, selection):
    risks = []
    if int(row.get("home_history_low", 0) or 0) or int(row.get("away_history_low", 0) or 0):
        risks.append("首发未确认")
        risks.append("伤停不明")
    if float(row.get("market_dispersion", 0.0) or 0.0) >= 0.08:
        risks.append("机构分歧")
    if float(row.get("market_favorite_prob", 0.0) or 0.0) >= 0.65:
        risks.append("热门过热")
    if selection == "D":
        risks.append("赔率异常")
    return risks or ["无明显风险"]


def summarize(rows, total_sample_count=None):
    total = len(rows) if total_sample_count is None else total_sample_count
    if not rows:
        return {
            "sample_count": int(total),
            "bet_count": 0,
            "hit_count": 0,
            "hit_rate": 0.0,
            "total_stake": 0.0,
            "total_profit": 0.0,
            "roi": 0.0,
            "max_drawdown": 0.0,
            "avg_odds": 0.0,
            "avg_ev": 0.0,
            "avg_model_prob": 0.0,
        }
    stake = float(sum(item["stake"] for item in rows))
    profit = float(sum(item["profit"] for item in rows))
    hit = int(sum(item["hit"] for item in rows))
    return {
        "sample_count": int(total),
        "bet_count": len(rows),
        "hit_count": hit,
        "hit_rate": hit / len(rows),
        "total_stake": stake,
        "total_profit": profit,
        "roi": profit / stake if stake else 0.0,
        "max_drawdown": max_drawdown([item["profit"] for item in rows]),
        "avg_odds": float(np.mean([item["odds"] for item in rows])),
        "avg_ev": float(np.mean([item["ev"] for item in rows])),
        "avg_model_prob": float(np.mean([item["model_prob"] for item in rows])),
    }


def decision_for(item):
    rules = []
    if item["model_prob"] <= 0 or item["odds"] <= 1.0:
        return "hard_ban", ["invalid_odds_or_prob"]
    if item["ev"] < 0:
        return "hard_ban", ["negative_ev"]
    if item["ev"] < 0.03:
        rules.append("low_ev")
    if item["odds"] < 1.10 or item["odds"] > 3.50:
        rules.append("odds_out_of_range")
    if item["model_prob"] < 0.40:
        rules.append("low_model_probability")
    if rules:
        return "observe_only", rules
    if item["ev"] >= 0.05 and 1.50 <= item["odds"] <= 3.00 and item["model_prob"] >= 0.45:
        return "recommend", []
    return "small_stake", []


def build_opportunities(valid_df, probs, classes):
    rows = []
    for row_idx, row in enumerate(valid_df.to_dict("records")):
        result = row.get("result")
        for cls_idx, selection in enumerate(classes):
            odds = float(row.get(SELECTION_TO_ODDS.get(selection, "odds_home"), 0.0) or 0.0)
            prob = float(probs[row_idx, cls_idx]) if len(probs) else 0.0
            if odds <= 1.0 or prob <= 0:
                continue
            ev = prob * odds - 1.0
            hit = 1 if result == selection else 0
            item = {
                "play_type": "spf",
                "selection": selection,
                "odds": odds,
                "model_prob": prob,
                "market_prob": float(row.get(MARKET_PROB_COLUMNS.get(selection, ""), 0.0) or 0.0),
                "ev": ev,
                "stake": 1.0,
                "hit": hit,
                "profit": odds - 1.0 if hit else -1.0,
                "data_quality": 85.0,
                "risk_tags_list": risk_bucket(row, selection),
            }
            item["odds_range"] = odds_bucket(item["odds"])
            item["probability_range"] = probability_bucket(item["model_prob"])
            item["ev_range"] = ev_bucket(item["ev"])
            item["data_quality_range"] = data_quality_bucket(item["data_quality"])
            item["decision"], item["matched_rules"] = decision_for(item)
            rows.append(item)
    return rows


def grouped_report(opportunities):
    rows = []
    for dimension in GROUP_DIMENSIONS:
        values = set()
        for item in opportunities:
            if dimension == "risk_tags":
                values.update(item["risk_tags_list"])
            else:
                values.add(item.get(dimension, "unknown"))
        for value in sorted(values):
            sample_items = [
                item for item in opportunities
                if (value in item["risk_tags_list"] if dimension == "risk_tags" else item.get(dimension) == value)
            ]
            bet_items = [item for item in sample_items if item["decision"] in ("recommend", "small_stake")]
            summary = summarize(bet_items, total_sample_count=len(sample_items))
            summary["dimension"] = dimension
            summary["group"] = value
            rows.append(summary)
    columns = [
        "dimension", "group", "sample_count", "bet_count", "hit_count", "hit_rate",
        "total_stake", "total_profit", "roi", "max_drawdown", "avg_odds", "avg_ev", "avg_model_prob",
    ]
    return pd.DataFrame(rows, columns=columns)


def pool_for_decision(decision):
    if decision == "recommend":
        return "recommend_pool"
    if decision == "small_stake":
        return "small_stake_pool"
    if decision == "observe_only":
        return "observe_only_pool"
    if decision == "hard_ban":
        return "hard_ban_pool"
    return "wait_pool"


def shadow_backtest(opportunities):
    summaries = {}
    report_rows = []
    for pool_id, decision, label in POOL_DEFINITIONS:
        pool_rows = [
            item for item in opportunities
            if (pool_for_decision(item.get("decision", "")) == pool_id)
        ]
        summary = summarize(pool_rows, total_sample_count=len(pool_rows))
        summary.update({"pool_id": pool_id, "decision": decision, "label": label})
        summaries[pool_id] = summary
        report_rows.append(summary)
    columns = [
        "pool_id", "decision", "label", "sample_count", "bet_count", "hit_count", "hit_rate",
        "total_stake", "total_profit", "roi", "max_drawdown", "avg_odds", "avg_ev", "avg_model_prob",
    ]
    pd.DataFrame(report_rows, columns=columns).to_csv(REPORTS_DIR / "shadow_backtest_report.csv", index=False, encoding="utf-8")
    payload = {
        "created_at": now_iso(),
        "note": "影子回测只用于诊断规则，不改变正式推荐。",
        "pools": summaries,
    }
    write_json(REPORTS_DIR / "shadow_backtest_summary.json", payload)
    return payload


def classify_rule(summary):
    matched = int(summary["matched_count"])
    roi = float(summary["roi"])
    if matched < 30:
        return "sample_too_small"
    if matched >= 50 and roi <= -0.05:
        return "effective_block"
    if matched >= 50 and roi > 0:
        return "over_strict"
    return "neutral"


def rule_diagnostics(opportunities):
    rows = []
    for rule_id, meta in RULE_DEFINITIONS.items():
        matched = [item for item in opportunities if rule_id in item.get("matched_rules", [])]
        summary = summarize(matched, total_sample_count=len(matched))
        record = {
            "rule_id": rule_id,
            "rule_name": meta["rule_name"],
            "action": meta["action"],
            "matched_count": int(len(matched)),
            "hit_count": int(summary["hit_count"]),
            "hit_rate": summary["hit_rate"],
            "total_stake": summary["total_stake"],
            "total_profit": summary["total_profit"],
            "roi": summary["roi"],
            "max_drawdown": summary["max_drawdown"],
            "avg_odds": summary["avg_odds"],
            "avg_ev": summary["avg_ev"],
            "avg_model_prob": summary["avg_model_prob"],
        }
        record["classification"] = classify_rule(record)
        rows.append(record)
    columns = [
        "rule_id", "rule_name", "action", "matched_count", "hit_count", "hit_rate", "total_stake",
        "total_profit", "roi", "max_drawdown", "avg_odds", "avg_ev", "avg_model_prob", "classification",
    ]
    pd.DataFrame(rows, columns=columns).to_csv(REPORTS_DIR / "rule_diagnostics.csv", index=False, encoding="utf-8")
    payload = {
        "created_at": now_iso(),
        "rules": rows,
        "effective_block": [item for item in rows if item["classification"] == "effective_block"],
        "over_strict": [item for item in rows if item["classification"] == "over_strict"],
        "sample_too_small": [item for item in rows if item["classification"] == "sample_too_small"],
        "neutral": [item for item in rows if item["classification"] == "neutral"],
    }
    write_json(REPORTS_DIR / "rule_diagnostics.json", payload)
    return payload


def in_range(value, low, high):
    if low is not None and value < low:
        return False
    if high is not None and value >= high:
        return False
    return True


def threshold_scan(opportunities):
    rows = []
    for ev_threshold in EV_THRESHOLDS:
        for odds_label, odds_low, odds_high in ODDS_RANGES:
            for prob_label, prob_low, prob_high in PROBABILITY_RANGES:
                matched = [
                    item for item in opportunities
                    if item["ev"] >= ev_threshold
                    and in_range(item["odds"], odds_low, odds_high)
                    and in_range(item["model_prob"], prob_low, prob_high)
                ]
                if len(matched) < 50:
                    continue
                summary = summarize(matched, total_sample_count=len(matched))
                rows.append({
                    "ev_threshold": ev_threshold,
                    "odds_range": odds_label,
                    "probability_range": prob_label,
                    "sample_count": len(matched),
                    "bet_count": summary["bet_count"],
                    "hit_rate": summary["hit_rate"],
                    "roi": summary["roi"],
                    "max_drawdown": summary["max_drawdown"],
                    "avg_odds": summary["avg_odds"],
                    "avg_ev": summary["avg_ev"],
                })
    rows = sorted(rows, key=lambda item: (item["roi"], -item["max_drawdown"], item["sample_count"]), reverse=True)
    columns = [
        "ev_threshold", "odds_range", "probability_range", "sample_count", "bet_count",
        "hit_rate", "roi", "max_drawdown", "avg_odds", "avg_ev",
    ]
    pd.DataFrame(rows, columns=columns).to_csv(REPORTS_DIR / "threshold_scan_report.csv", index=False, encoding="utf-8")
    payload = {
        "created_at": now_iso(),
        "min_bet_count": 50,
        "candidates": rows[:50],
        "best_candidates": rows[:10],
    }
    write_json(REPORTS_DIR / "threshold_scan_summary.json", payload)
    return payload


def write_candidate_strategy(threshold_summary, rule_summary):
    candidates = threshold_summary.get("best_candidates", [])
    allow_candidates = [item for item in candidates if item.get("roi", 0.0) > 0.03]
    best = allow_candidates[0] if allow_candidates else (candidates[0] if candidates else {})
    warnings = ["候选策略默认不启用，只作为观察诊断。"]
    if not allow_candidates:
        warnings.append("未发现样本充足且ROI明显为正的阈值组合。")
    if rule_summary.get("over_strict"):
        warnings.append("存在可能误杀正收益样本的禁买/观察规则，需要人工复核。")
    payload = {
        "strategy_id": "candidate_strategy_v1",
        "status": "observation_only",
        "generated_at": now_iso(),
        "source_reports": [
            "reports/threshold_scan_summary.json",
            "reports/rule_diagnostics.json",
            "reports/shadow_backtest_summary.json",
        ],
        "candidate_rules": (allow_candidates or candidates)[:10],
        "positive_candidate_rules": allow_candidates[:10],
        "expected_roi": float(best.get("roi", 0.0) or 0.0),
        "sample_count": int(best.get("sample_count", 0) or 0),
        "max_drawdown": float(best.get("max_drawdown", 0.0) or 0.0),
        "warnings": warnings,
    }
    write_json(MODELS_DIR / "candidate_strategy_v1.json", payload)
    return payload


def candidate_rule_matches(item, rule):
    if item["ev"] < float(rule.get("ev_threshold", 0.0) or 0.0):
        return False
    if item.get("odds_range") != rule.get("odds_range"):
        return False
    return item.get("probability_range") == rule.get("probability_range")


def rolling_roi(rows, window=30):
    if len(rows) < window:
        return None
    recent = rows[-window:]
    stake = sum(item["stake"] for item in recent)
    profit = sum(item["profit"] for item in recent)
    return profit / stake if stake else None


def candidate_upgrade_check(rows, candidate_strategy, rule_summary):
    summary = summarize(rows, total_sample_count=len(rows))
    recent_roi = rolling_roi(rows, 30)
    blocking = []
    if summary["bet_count"] < 100:
        blocking.append("纸面交易样本少于100")
    if summary["roi"] < 0.03:
        blocking.append("纸面交易ROI低于3%")
    if summary["max_drawdown"] > max(15.0, summary["total_stake"] * 0.20):
        blocking.append("最大回撤偏大")
    if recent_roi is None or recent_roi < 0:
        blocking.append("最近30笔纸面ROI为负或样本不足")
    candidate_rules = candidate_strategy.get("candidate_rules", [])
    low_odds_only = bool(candidate_rules) and all(rule.get("odds_range") in ("1.10-1.30", "1.30-1.50") for rule in candidate_rules)
    if low_odds_only:
        blocking.append("候选策略依赖单一低赔率热门区间")
    if rule_summary.get("over_strict"):
        blocking.append("与现有风控规则存在潜在冲突，需要人工复核")
    can_consider = not blocking
    return {
        "can_consider_upgrade": can_consider,
        "upgrade_reason": "可考虑升级为小注，但不会自动启用" if can_consider else "未达到升级条件",
        "blocking_reasons": blocking,
        "recent_30_roi": recent_roi,
    }


def paper_trading_backtest(opportunities, candidate_strategy, rule_summary):
    candidate_rules = candidate_strategy.get("candidate_rules", [])
    rows = [
        item for item in opportunities
        if any(candidate_rule_matches(item, rule) for rule in candidate_rules)
        and item.get("decision") not in ("recommend", "small_stake")
    ]
    report_rows = []
    for index, item in enumerate(rows, start=1):
        report_rows.append({
            "paper_id": index,
            "strategy_id": candidate_strategy.get("strategy_id", "candidate_strategy_v1"),
            "status": candidate_strategy.get("status", "observation_only"),
            "play_type": item["play_type"],
            "selection": item["selection"],
            "model_prob": item["model_prob"],
            "odds": item["odds"],
            "ev": item["ev"],
            "advantage_rate": item["ev"],
            "data_quality_score": item["data_quality"],
            "risk_tags": "|".join(item["risk_tags_list"]),
            "paper_stake": item["stake"],
            "is_hit": item["hit"],
            "paper_profit": item["profit"],
        })
    columns = [
        "paper_id", "strategy_id", "status", "play_type", "selection", "model_prob", "odds",
        "ev", "advantage_rate", "data_quality_score", "risk_tags", "paper_stake", "is_hit", "paper_profit",
    ]
    pd.DataFrame(report_rows, columns=columns).to_csv(REPORTS_DIR / "paper_trading_backtest_report.csv", index=False, encoding="utf-8")
    summary = summarize(rows, total_sample_count=len(rows))
    upgrade = candidate_upgrade_check(rows, candidate_strategy, rule_summary)
    warning = ""
    if summary["bet_count"] < 50:
        warning = "纸面交易样本不足，不能启用正式推荐"
    elif summary["roi"] <= 0:
        warning = "纸面交易 ROI 未转正，继续观察"
    payload = {
        "strategy_id": candidate_strategy.get("strategy_id", "candidate_strategy_v1"),
        "status": "observation_only",
        "created_at": now_iso(),
        "sample_count": int(summary["sample_count"]),
        "bet_count": int(summary["bet_count"]),
        "hit_count": int(summary["hit_count"]),
        "hit_rate": summary["hit_rate"],
        "total_paper_stake": summary["total_stake"],
        "total_paper_profit": summary["total_profit"],
        "paper_roi": summary["roi"],
        "max_drawdown": summary["max_drawdown"],
        "avg_odds": summary["avg_odds"],
        "avg_ev": summary["avg_ev"],
        "avg_model_prob": summary["avg_model_prob"],
        "warning": warning,
        "candidate_upgrade_check": upgrade,
        "note": "该策略仅用于模拟观察，不构成正式推荐。",
    }
    write_json(REPORTS_DIR / "paper_trading_backtest_summary.json", payload)
    return payload


def summarize_report_rows(rows, total_sample_count=None):
    total = len(rows) if total_sample_count is None else total_sample_count
    if not rows:
        return {
            "sample_count": int(total),
            "bet_count": 0,
            "hit_count": 0,
            "hit_rate": 0.0,
            "total_stake": 0.0,
            "total_profit": 0.0,
            "roi": 0.0,
            "max_drawdown": 0.0,
            "avg_odds": 0.0,
            "avg_ev": 0.0,
            "avg_model_prob": 0.0,
        }
    profits = [float(item.get("paper_profit", 0.0) or 0.0) for item in rows]
    stake = float(sum(float(item.get("paper_stake", 1.0) or 1.0) for item in rows))
    profit = float(sum(profits))
    hit = int(sum(int(item.get("is_hit", 0) or 0) for item in rows))
    return {
        "sample_count": int(total),
        "bet_count": len(rows),
        "hit_count": hit,
        "hit_rate": hit / len(rows),
        "total_stake": stake,
        "total_profit": profit,
        "roi": profit / stake if stake else 0.0,
        "max_drawdown": max_drawdown(profits),
        "avg_odds": float(np.mean([float(item.get("odds", 0.0) or 0.0) for item in rows])),
        "avg_ev": float(np.mean([float(item.get("ev", 0.0) or 0.0) for item in rows])),
        "avg_model_prob": float(np.mean([float(item.get("model_prob", 0.0) or 0.0) for item in rows])),
    }


def rolling_report_roi(rows, window):
    if len(rows) < window:
        return None
    return summarize_report_rows(rows[-window:])["roi"]


def remove_top_profit_roi(rows, count):
    if not rows:
        return 0.0
    sorted_rows = sorted(rows, key=lambda item: float(item.get("paper_profit", 0.0) or 0.0), reverse=True)
    remain = sorted_rows[count:]
    return summarize_report_rows(remain)["roi"] if remain else 0.0


def robustness_check(_opportunities=None, paper_summary=None, candidate_strategy=None, rule_summary=None):
    report_path = REPORTS_DIR / "paper_trading_backtest_report.csv"
    if report_path.exists():
        df = pd.read_csv(report_path).fillna(0)
    else:
        df = pd.DataFrame()
    rows = df.to_dict("records") if not df.empty else []
    paper_summary = paper_summary or read_json(REPORTS_DIR / "paper_trading_backtest_summary.json", {})
    candidate_strategy = candidate_strategy or read_json(MODELS_DIR / "candidate_strategy_v1.json", {})
    rule_summary = rule_summary or read_json(REPORTS_DIR / "rule_diagnostics.json", {})

    report_rows = []
    time_segments = []
    if rows:
        segments = np.array_split(rows, 5)
        positive_segments = 0
        for index, segment in enumerate(segments, start=1):
            segment_rows = list(segment)
            summary = summarize_report_rows(segment_rows)
            if summary["roi"] > 0:
                positive_segments += 1
            record = {
                "check_type": "time_segment",
                "group": f"segment_{index}",
                **summary,
            }
            time_segments.append(record)
            report_rows.append(record)
    else:
        positive_segments = 0
    time_stability = "stable_by_time" if positive_segments >= 3 else "unstable_by_time"

    rolling_30 = rolling_report_roi(rows, 30)
    rolling_50 = rolling_report_roi(rows, 50)
    rolling_100 = rolling_report_roi(rows, 100)
    rolling_rois = [value for value in [rolling_30, rolling_50, rolling_100] if value is not None]
    rolling_min_roi = min(rolling_rois) if rolling_rois else None
    rolling_max_dd = max([
        summarize_report_rows(rows[-window:])["max_drawdown"]
        for window in [30, 50, 100]
        if len(rows) >= window
    ] or [0.0])

    odds_band_rows = []
    positive_odds_bands = []
    for odds_label, odds_low, odds_high in ODDS_RANGES:
        band_rows = [
            item for item in rows
            if in_range(float(item.get("odds", 0.0) or 0.0), odds_low, odds_high)
        ]
        summary = summarize_report_rows(band_rows, total_sample_count=len(band_rows))
        if summary["roi"] > 0 and summary["bet_count"] > 0:
            positive_odds_bands.append(odds_label)
        record = {
            "check_type": "odds_band",
            "group": odds_label,
            **summary,
        }
        odds_band_rows.append(record)
        report_rows.append(record)

    selection_rows = []
    positive_selections = []
    for selection in ["H", "D", "A"]:
        selected_rows = [item for item in rows if item.get("selection") == selection]
        summary = summarize_report_rows(selected_rows, total_sample_count=len(selected_rows))
        if summary["roi"] > 0 and summary["bet_count"] > 0:
            positive_selections.append(selection)
        record = {
            "check_type": "selection",
            "group": selection,
            **summary,
        }
        selection_rows.append(record)
        report_rows.append(record)

    top10_count = max(1, int(len(rows) * 0.10)) if rows else 0
    outlier = {
        "roi_after_remove_top1": remove_top_profit_roi(rows, 1),
        "roi_after_remove_top3": remove_top_profit_roi(rows, 3),
        "roi_after_remove_top5": remove_top_profit_roi(rows, 5),
        "roi_after_remove_top10pct": remove_top_profit_roi(rows, top10_count),
    }

    blocking = []
    warnings = []
    positive_evidence = []
    paper_roi = float(paper_summary.get("paper_roi", 0.0) or 0.0)
    bet_count = int(paper_summary.get("bet_count", 0) or 0)
    if paper_roi < 0.03:
        blocking.append("paper_roi_below_3pct")
    if bet_count < 100:
        blocking.append("paper_bet_count_below_100")
    if time_stability == "unstable_by_time":
        blocking.append("unstable_by_time")
    else:
        positive_evidence.append("多数时间分段ROI为正")
    if rolling_30 is not None and rolling_30 < 0:
        blocking.append("recent_30_roi_negative")
    if rolling_50 is not None and rolling_50 < 0:
        blocking.append("recent_50_roi_negative")
    if rolling_max_dd > max(15.0, bet_count * 0.20):
        blocking.append("rolling_drawdown_too_high")
    if len(positive_odds_bands) == 1:
        blocking.append("depends_on_single_odds_band")
    elif len(positive_odds_bands) > 1:
        positive_evidence.append("正收益不只来自单一赔率区间")
    low_odds_rows = [row for row in odds_band_rows if row["group"] in ("1.10-1.30", "1.30-1.50")]
    if any(row["hit_rate"] >= 0.65 and row["roi"] <= 0.03 and row["bet_count"] >= 30 for row in low_odds_rows):
        warnings.append("low_odds_high_hit_no_edge")
    strong_selection_rows = [row for row in selection_rows if row["bet_count"] >= 30 and row["roi"] > 0]
    if len(strong_selection_rows) == 1:
        blocking.append("depends_on_single_selection")
    elif len(strong_selection_rows) > 1:
        positive_evidence.append("正收益不只来自单一选择方向")
    if outlier["roi_after_remove_top3"] < 0:
        blocking.append("fragile_to_outliers")
    if outlier["roi_after_remove_top10pct"] < -0.01:
        blocking.append("profit_concentrated_in_few_games")
    if rule_summary.get("over_strict"):
        blocking.append("conflicts_with_existing_risk_rules")

    if (
        paper_roi >= 0.03
        and bet_count >= 100
        and time_stability == "stable_by_time"
        and (rolling_50 is not None and rolling_50 > 0)
        and len(positive_odds_bands) > 1
        and len(strong_selection_rows) > 1
        and outlier["roi_after_remove_top5"] > 0
        and not rule_summary.get("over_strict")
    ):
        robustness_level = "strong"
    elif paper_roi > 0 and bet_count >= 100 and (rolling_50 is None or rolling_50 > -0.02) and positive_segments >= 2:
        robustness_level = "moderate"
    else:
        robustness_level = "weak"

    can_consider_upgrade = (
        robustness_level == "strong"
        and paper_roi >= 0.03
        and bet_count >= 100
        and (rolling_50 is not None and rolling_50 > 0)
        and outlier["roi_after_remove_top5"] > 0
        and "depends_on_single_odds_band" not in blocking
        and "depends_on_single_selection" not in blocking
        and not rule_summary.get("over_strict")
    )
    if robustness_level != "strong" and "robustness_not_strong" not in blocking:
        blocking.append("robustness_not_strong")
    if warnings:
        blocking.extend([warning for warning in warnings if warning not in blocking])

    columns = [
        "check_type", "group", "sample_count", "bet_count", "hit_count", "hit_rate",
        "total_stake", "total_profit", "roi", "max_drawdown", "avg_odds", "avg_ev", "avg_model_prob",
    ]
    pd.DataFrame(report_rows, columns=columns).to_csv(REPORTS_DIR / "strategy_robustness_report.csv", index=False, encoding="utf-8")
    payload = {
        "strategy_id": candidate_strategy.get("strategy_id", "candidate_strategy_v1"),
        "status": "observation_only",
        "created_at": now_iso(),
        "robustness_level": robustness_level,
        "can_consider_upgrade": can_consider_upgrade,
        "time_stability": time_stability,
        "time_segments": time_segments,
        "rolling_30_roi": rolling_30,
        "rolling_50_roi": rolling_50,
        "rolling_100_roi": rolling_100,
        "rolling_min_roi": rolling_min_roi,
        "rolling_max_drawdown": rolling_max_dd,
        "odds_band_results": odds_band_rows,
        "selection_results": selection_rows,
        "positive_odds_bands": positive_odds_bands,
        "positive_selections": positive_selections,
        "outlier_sensitivity": outlier,
        "blocking_reasons": blocking,
        "positive_evidence": positive_evidence,
        "warnings": warnings,
        "conclusion": "该策略当前仍处于观察状态。稳健性不足时，不应升级为真实推荐。",
    }
    write_json(REPORTS_DIR / "strategy_robustness_summary.json", payload)
    return payload


def rule_from_group(row):
    bet_count = int(row["bet_count"])
    roi = float(row["roi"])
    if bet_count < 30:
        return "sample_too_small"
    if roi < -0.10 and bet_count >= 50:
        return "hard_ban"
    if roi < -0.05 and bet_count >= 50:
        return "observe_only"
    if row["max_drawdown"] > max(10.0, row["total_stake"] * 0.35):
        return "downgrade"
    if roi > 0.03 and bet_count >= 50:
        return "allow_candidate"
    return "neutral"


def diagnostics_from_groups(grouped):
    records = grouped.to_dict("records") if not grouped.empty else []
    enriched = []
    warnings = []
    for row in records:
        action = rule_from_group(row)
        row = dict(row)
        row["action"] = action
        if row["hit_rate"] >= 0.65 and row["roi"] <= 0 and row["bet_count"] >= 30:
            row["warning"] = "低赔率方向命中率较高但无盈利优势"
            warnings.append(f"{row['dimension']}={row['group']}：低赔率方向命中率较高但无盈利优势")
        enriched.append(row)
    best = sorted([r for r in enriched if r["bet_count"] >= 30], key=lambda r: r["roi"], reverse=True)[:10]
    worst = sorted([r for r in enriched if r["bet_count"] >= 30], key=lambda r: r["roi"])[:10]
    sample_small = [r for r in enriched if r["action"] == "sample_too_small"][:30]
    return {
        "created_at": now_iso(),
        "best_segments": best,
        "worst_segments": worst,
        "sample_too_small_segments": sample_small,
        "recommended_ban_rules": [r for r in enriched if r["action"] == "hard_ban"],
        "recommended_downgrade_rules": [r for r in enriched if r["action"] == "downgrade"],
        "recommended_allow_rules": [r for r in enriched if r["action"] == "allow_candidate"],
        "warnings": warnings,
    }


def main():
    ensure_dirs()
    features_path = PROCESSED_DIR / "features.csv"
    df = pd.read_csv(features_path) if features_path.exists() else pd.DataFrame()
    model = active_prediction_model()
    if df.empty:
        summary = {
            "sample_count": 0,
            "validation_count": 0,
            "candidate_count": 0,
            "final_bet_count": 0,
            "skipped_count": 0,
            "hard_ban_count": 0,
            "observe_only_count": 0,
            "total_stake": 0.0,
            "total_profit": 0.0,
            "roi": 0.0,
            "max_drawdown": 0.0,
            "avg_odds": 0.0,
            "avg_ev": 0.0,
            "avg_model_prob": 0.0,
            "warning": "暂无正式投注样本，不能评估 ROI",
        }
        write_json(REPORTS_DIR / "backtest_summary.json", summary)
        write_json(MODELS_DIR / "strategy_rules_v1.json", {"model_version": "strategy_rules_v1", "created_at": now_iso(), "rules": [], "summary": summary})
        print("回测样本为空")
        return

    df = df.sort_values(["date", "match_id"]).reset_index(drop=True)
    valid_df = df.iloc[int(len(df) * 0.8):].copy()
    probs, classes = predict_probs(valid_df, model)
    probs = apply_calibrator(probs, classes)
    probs = apply_probability_blend(probs, classes, valid_df)
    opportunities = build_opportunities(valid_df, probs, classes)
    final_bets = [item for item in opportunities if item["decision"] in ("recommend", "small_stake")]
    hard_bans = [item for item in opportunities if item["decision"] == "hard_ban"]
    observes = [item for item in opportunities if item["decision"] == "observe_only"]
    grouped = grouped_report(opportunities)
    diagnostics = diagnostics_from_groups(grouped)
    shadow = shadow_backtest(opportunities)
    rule_summary = rule_diagnostics(opportunities)
    threshold_summary = threshold_scan(opportunities)
    candidate_strategy = write_candidate_strategy(threshold_summary, rule_summary)
    paper_summary = paper_trading_backtest(opportunities, candidate_strategy, rule_summary)
    robustness_summary = robustness_check(opportunities, paper_summary, candidate_strategy, rule_summary)
    grouped.to_csv(REPORTS_DIR / "grouped_backtest_report.csv", index=False, encoding="utf-8")
    write_json(REPORTS_DIR / "grouped_backtest_summary.json", {
        "created_at": now_iso(),
        "dimensions": GROUP_DIMENSIONS,
        "groups": grouped.to_dict("records"),
    })
    write_json(REPORTS_DIR / "strategy_diagnostics.json", diagnostics)
    grouped.to_csv(REPORTS_DIR / "backtest_report.csv", index=False, encoding="utf-8")

    final_summary = summarize(final_bets, total_sample_count=len(opportunities))
    warning = ""
    if final_summary["bet_count"] == 0:
        warning = "暂无正式投注样本，不能评估 ROI"
    elif final_summary["bet_count"] < 30:
        warning = "正式投注样本不足，ROI 仅供参考"
    summary = {
        "sample_count": int(len(df)),
        "validation_count": int(len(valid_df)),
        "candidate_count": int(len(opportunities)),
        "final_bet_count": int(final_summary["bet_count"]),
        "recommend_count": int(shadow["pools"]["recommend_pool"]["bet_count"]),
        "small_stake_count": int(shadow["pools"]["small_stake_pool"]["bet_count"]),
        "skipped_count": int(len(opportunities) - len(final_bets)),
        "hard_ban_count": int(len(hard_bans)),
        "observe_only_count": int(len(observes)),
        "total_stake": final_summary["total_stake"],
        "total_profit": final_summary["total_profit"],
        "roi": final_summary["roi"],
        "max_drawdown": final_summary["max_drawdown"],
        "avg_odds": final_summary["avg_odds"],
        "avg_ev": final_summary["avg_ev"],
        "avg_model_prob": final_summary["avg_model_prob"],
        "warning": warning,
        "shadow_backtest_path": "reports/shadow_backtest_summary.json",
        "rule_diagnostics_path": "reports/rule_diagnostics.json",
        "threshold_scan_path": "reports/threshold_scan_summary.json",
        "candidate_strategy_path": "training/models/candidate_strategy_v1.json",
        "paper_trading_backtest_path": "reports/paper_trading_backtest_summary.json",
        "strategy_robustness_path": "reports/strategy_robustness_summary.json",
    }
    write_json(REPORTS_DIR / "backtest_summary.json", summary)
    rules = []
    for row in diagnostics["recommended_ban_rules"]:
        rules.append({"dimension": row["dimension"], "group": row["group"], "action": "hard_ban", "reason": "ROI严重为负且样本充足", "sample_count": row["bet_count"], "roi": row["roi"], "hit_rate": row["hit_rate"]})
    for row in diagnostics["recommended_downgrade_rules"]:
        rules.append({"dimension": row["dimension"], "group": row["group"], "action": "downgrade", "reason": "最大回撤过大", "sample_count": row["bet_count"], "roi": row["roi"], "hit_rate": row["hit_rate"]})
    for row in diagnostics["recommended_allow_rules"]:
        rules.append({"dimension": row["dimension"], "group": row["group"], "action": "allow_candidate", "reason": "ROI为正且样本充足", "sample_count": row["bet_count"], "roi": row["roi"], "hit_rate": row["hit_rate"]})
    write_json(MODELS_DIR / "strategy_rules_v1.json", {
        "model_version": "strategy_rules_v1",
        "created_at": now_iso(),
        "candidate_rule": "all 1X2 opportunities; final_bet requires ev>=3%, odds 1.10-3.50, model_prob>=40%",
        "rules": rules,
        "summary": summary,
        "diagnostics_path": "reports/strategy_diagnostics.json",
        "candidate_strategy_status": candidate_strategy["status"],
        "paper_trading_status": paper_summary["status"],
        "robustness_level": robustness_summary["robustness_level"],
    })
    print(
        f"验证集={len(valid_df)} 机会={len(opportunities)} 正式={summary['final_bet_count']} "
        f"hard_ban={summary['hard_ban_count']} observe={summary['observe_only_count']} "
        f"ROI={summary['roi']:.3f} warning={summary['warning']}"
    )


if __name__ == "__main__":
    main()
