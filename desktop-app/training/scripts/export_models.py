from common import MODELS_DIR, REPORTS_DIR, ensure_dirs, now_iso, read_json, write_json


REQUIRED = {
    "outcome_model": MODELS_DIR / "outcome_model_v1.json",
    "outcome_ensemble_model": MODELS_DIR / "outcome_ensemble_model_v1.json",
    "calibrator": MODELS_DIR / "calibrator_v1.json",
    "probability_blend": MODELS_DIR / "probability_blend_v1.json",
    "goals_home_model": MODELS_DIR / "goals_home_model_v1.json",
    "goals_away_model": MODELS_DIR / "goals_away_model_v1.json",
    "handicap_model": MODELS_DIR / "handicap_mapping_model_v1.json",
    "feature_schema": MODELS_DIR / "feature_schema_v1.json",
    "strategy_rules": MODELS_DIR / "strategy_rules_v1.json",
    "backtest_summary": REPORTS_DIR / "backtest_summary.json",
}


def main():
    ensure_dirs()
    missing = [name for name, path in REQUIRED.items() if not path.exists()]
    outcome = read_json(REQUIRED["outcome_model"], {})
    ensemble = read_json(REQUIRED["outcome_ensemble_model"], {})
    calibrator = read_json(REQUIRED["calibrator"], {})
    probability_blend = read_json(REQUIRED["probability_blend"], {})
    schema = read_json(REQUIRED["feature_schema"], {})
    strategy = read_json(REQUIRED["strategy_rules"], {})
    home_goals = read_json(REQUIRED["goals_home_model"], {})
    away_goals = read_json(REQUIRED["goals_away_model"], {})
    handicap = read_json(REQUIRED["handicap_model"], {})
    worldcup_correction = read_json(MODELS_DIR / "worldcup_live_correction_v1.json", {})
    worldcup_correction_report = read_json(REPORTS_DIR / "worldcup_live_correction_report.json", {})
    backtest = read_json(REQUIRED["backtest_summary"], {})
    paper_trading = read_json(REPORTS_DIR / "paper_trading_backtest_summary.json", {})
    robustness = read_json(REPORTS_DIR / "strategy_robustness_summary.json", {})
    blend_metrics = probability_blend.get("metrics", {}).get("selected", {})
    base_metrics = ensemble.get("metrics", outcome.get("metrics", {}))
    final_metrics = dict(base_metrics)
    if blend_metrics:
        final_metrics["accuracy"] = blend_metrics.get("accuracy", final_metrics.get("accuracy", 0.0))
        final_metrics["log_loss"] = blend_metrics.get("log_loss", final_metrics.get("log_loss", 0.0))
        final_metrics["brier_score"] = blend_metrics.get("brier_score", final_metrics.get("brier_score", 0.0))
        final_metrics["probability_blend"] = blend_metrics
    layer_plan = [
        {
            "layer": "1_market_baseline",
            "status": "implemented",
            "purpose": "用欧赔/平均赔率/最大赔率隐含概率作为市场基准，并计算水分、离散度和盘口强弱。",
            "artifacts": ["feature_schema_v1.json", "probability_blend_v1.json"],
        },
        {
            "layer": "2_team_strength",
            "status": "implemented_with_proxy",
            "purpose": "用近期状态、攻防强度、Elo 代理、排名/阵容强度代理生成赛前强弱特征。",
            "artifacts": ["features.csv", "outcome_ensemble_model_v1.json"],
            "limitation": "Football-Data 联赛 CSV 缺少稳定的国家队实时 Elo、FIFA 排名和国家队阵容身价，当前以可获得特征和运行时数据做代理。",
        },
        {
            "layer": "3_lineup_injury_adjustment",
            "status": "runtime_framework",
            "purpose": "首发、伤停、xG 缺失时影响推荐等级和比分/总进球风险，不直接伪造训练标签。",
            "artifacts": ["source_health", "recommendation_service", "strategy_rules_v1.json"],
            "limitation": "历史 CSV 不含赛前确认首发和伤停，训练阶段不把首发次数当确认首发。",
        },
        {
            "layer": "4_stage_knockout_context",
            "status": "implemented_with_defaults",
            "purpose": "加入杯赛/淘汰赛/中立场/赛程重要性字段，让后端预测能按淘汰赛语境组装特征。",
            "artifacts": ["feature_schema_v1.json", "model_service.rs"],
            "limitation": "联赛历史样本中的淘汰赛字段较少，真实世界杯淘汰赛需要赛后持续复盘校准。",
        },
        {
            "layer": "5_goals_score_model",
            "status": "implemented",
            "purpose": "单独训练主客进球 Poisson 模型，输出比分矩阵和总进球概率，不再只从胜平负硬推比分。",
            "artifacts": ["goals_home_model_v1.json", "goals_away_model_v1.json"],
        },
        {
            "layer": "6_dynamic_probability_fusion",
            "status": "implemented",
            "purpose": "按市场热门程度动态融合模型概率和市场概率，降低模型在强热门/弱热门区间的过拟合。",
            "artifacts": ["probability_blend_v1.json"],
        },
        {
            "layer": "7_recommendation_backtest",
            "status": "implemented",
            "purpose": "把概率预测和投注推荐拆开，输出盈利策略、禁买/降级规则、ROI和回撤控制。",
            "artifacts": ["strategy_rules_v1.json", "backtest_summary.json"],
        },
    ]
    manifest = {
        "active_model_version": "+".join(filter(None, [
            ensemble.get("model_version", outcome.get("model_version", "rules-fallback")),
            probability_blend.get("model_version"),
        ])),
        "outcome_model_path": "outcome_model_v1.json",
        "outcome_ensemble_model_path": "outcome_ensemble_model_v1.json",
        "calibrator_path": "calibrator_v1.json",
        "probability_blend_path": "probability_blend_v1.json",
        "feature_schema_path": "feature_schema_v1.json",
        "strategy_rules_path": "strategy_rules_v1.json",
        "goals_home_model_path": "goals_home_model_v1.json",
        "goals_away_model_path": "goals_away_model_v1.json",
        "handicap_model_path": "handicap_mapping_model_v1.json",
        "created_at": now_iso(),
        "training_data_range": outcome.get("training_data_range", schema.get("date_range", {})),
        "metrics_summary": final_metrics,
        "backtest_summary": backtest,
        "paper_trading_summary": paper_trading,
        "strategy_robustness_summary": robustness,
        "model_objectives": {
            "primary": ["log_loss", "brier_score", "calibration_quality"],
            "betting": ["roi", "max_drawdown", "ev_realization"],
            "note": "模型状态和推荐逻辑以概率校准、ROI、回撤和风险控制为核心。",
        },
        "global_models": {
            "outcome": outcome.get("model_version"),
            "outcome_ensemble": ensemble.get("model_version"),
            "calibrator": calibrator.get("model_version"),
            "probability_blend": probability_blend.get("model_version"),
            "goals_home": home_goals.get("model_version"),
            "goals_away": away_goals.get("model_version"),
            "handicap": handicap.get("model_version"),
            "worldcup_live_correction": worldcup_correction.get("model_version"),
            "goals_metrics": {
                "home": home_goals.get("metrics", {}),
                "away": away_goals.get("metrics", {}),
            },
            "handicap_metrics": handicap.get("metrics", {}),
            "worldcup_live_correction_status": {
                "ready": bool(worldcup_correction.get("model_version")),
                "report": worldcup_correction_report,
            },
        },
        "worldcup_model_v2_layers": layer_plan,
        "missing_files": missing,
        "ready": not missing and ensemble.get("metrics", outcome.get("metrics", {})).get("train_count", 0) > 0,
        "note": "v0.8 增加国家队/淘汰赛上下文字段、动态市场概率融合、命中率策略与盈利策略分离；优先优化概率校准、Log Loss 和市场错误识别。",
    }
    write_json(MODELS_DIR / "model_manifest.json", manifest)
    print(f"manifest ready={manifest['ready']} missing={missing}")


if __name__ == "__main__":
    main()
