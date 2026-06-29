from common import MODELS_DIR, PROCESSED_DIR, REPORTS_DIR, ensure_dirs, now_iso, read_json, write_json

import numpy as np
import pandas as pd
from sklearn.linear_model import LogisticRegression
from sklearn.metrics import accuracy_score, brier_score_loss, log_loss
from sklearn.model_selection import train_test_split
from sklearn.preprocessing import StandardScaler


INPUT = PROCESSED_DIR / "worldcup_closure_samples.csv"
MODEL_OUT = MODELS_DIR / "worldcup_live_correction_v1.json"
REPORT_OUT = REPORTS_DIR / "worldcup_live_correction_report.json"
MIN_SAMPLES = 50


FEATURES = [
    "model_prob",
    "sporttery_prob",
    "europe_prob_filled",
    "europe_missing",
    "current_odds",
    "fair_odds",
    "ev",
    "advantage_rate",
    "stake_pct",
    "data_quality_score",
    "is_score_or_total_goals",
    "has_anomaly",
    "is_formal_recommendation",
]


def prepare(df):
    work = df.copy()
    work["europe_missing"] = (pd.to_numeric(work.get("europe_prob", -1), errors="coerce").fillna(-1) < 0).astype(float)
    fallback = pd.to_numeric(work.get("sporttery_prob", 0), errors="coerce").fillna(0)
    work["europe_prob_filled"] = pd.to_numeric(work.get("europe_prob", -1), errors="coerce").where(lambda col: col >= 0, fallback)
    work["is_score_or_total_goals"] = work.get("market", "").astype(str).str.contains("比分|总进球|CRS|TTG", regex=True).astype(float)
    work["has_anomaly"] = work.get("anomaly_type", "").astype(str).str.strip().ne("").astype(float)
    work["is_formal_recommendation"] = work.get("recommendation_level", "").astype(str).str.contains("重点|稳胆|正式|可买", regex=True).astype(float)
    for col in FEATURES:
        work[col] = pd.to_numeric(work[col], errors="coerce").fillna(0.0)
    work["hit"] = pd.to_numeric(work.get("hit", 0), errors="coerce").fillna(0).astype(int)
    return work


def main():
    ensure_dirs()
    if not INPUT.exists():
        report = {
            "ready": False,
            "reason": "未找到 worldcup_closure_samples.csv，请先在软件中执行赛前闭环采集并赛后结算。",
            "sample_count": 0,
            "created_at": now_iso(),
        }
        write_json(REPORT_OUT, report)
        print(report["reason"])
        return

    df = prepare(pd.read_csv(INPUT))
    sample_count = len(df)
    if sample_count < MIN_SAMPLES or df["hit"].nunique() < 2:
        report = {
            "ready": False,
            "reason": f"世界杯闭环样本不足或标签单一，至少需要 {MIN_SAMPLES} 条且同时包含命中/未中。",
            "sample_count": int(sample_count),
            "hit_count": int(df["hit"].sum()) if sample_count else 0,
            "created_at": now_iso(),
        }
        write_json(REPORT_OUT, report)
        print(report["reason"])
        return

    x = df[FEATURES].to_numpy(dtype=float)
    y = df["hit"].to_numpy(dtype=int)
    # 样本来自同一届杯赛，按时间排序后保留最后20%做近端验证。
    split = max(1, int(len(df) * 0.8))
    x_train, x_valid = x[:split], x[split:]
    y_train, y_valid = y[:split], y[split:]
    if len(np.unique(y_train)) < 2 or len(np.unique(y_valid)) < 2:
        x_train, x_valid, y_train, y_valid = train_test_split(x, y, test_size=0.2, random_state=42, stratify=y)

    scaler = StandardScaler()
    x_train_scaled = scaler.fit_transform(x_train)
    x_valid_scaled = scaler.transform(x_valid)
    model = LogisticRegression(max_iter=1000, class_weight="balanced")
    model.fit(x_train_scaled, y_train)
    probs = model.predict_proba(x_valid_scaled)[:, 1]
    preds = (probs >= 0.5).astype(int)
    metrics = {
        "sample_count": int(sample_count),
        "train_count": int(len(y_train)),
        "valid_count": int(len(y_valid)),
        "accuracy": float(accuracy_score(y_valid, preds)),
        "log_loss": float(log_loss(y_valid, np.column_stack([1 - probs, probs]), labels=[0, 1])),
        "brier_score": float(brier_score_loss(y_valid, probs)),
        "hit_rate": float(np.mean(y)),
    }
    payload = {
        "model_version": "worldcup_live_correction_v1",
        "model_type": "binary_logistic_live_bet_hit_correction",
        "purpose": "只修正世界杯/淘汰赛临场推荐置信度，不替代胜平负概率模型。",
        "feature_names": FEATURES,
        "coefficients": model.coef_[0].tolist(),
        "intercept": float(model.intercept_[0]),
        "scaler_mean": dict(zip(FEATURES, scaler.mean_.tolist())),
        "scaler_scale": dict(zip(FEATURES, scaler.scale_.tolist())),
        "metrics": metrics,
        "created_at": now_iso(),
        "source_file": str(INPUT),
        "base_model_manifest": read_json(MODELS_DIR / "model_manifest.json", {}),
    }
    write_json(MODEL_OUT, payload)
    write_json(REPORT_OUT, {"ready": True, "metrics": metrics, "created_at": now_iso()})
    print(f"worldcup live correction ready samples={sample_count} accuracy={metrics['accuracy']:.3f}")


if __name__ == "__main__":
    main()
