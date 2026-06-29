import pandas as pd
from sklearn.linear_model import LogisticRegression
from sklearn.metrics import accuracy_score, log_loss
from sklearn.preprocessing import StandardScaler

from common import MODELS_DIR, PROCESSED_DIR, REPORTS_DIR, ensure_dirs, now_iso, read_json, write_json


MODEL_VERSION = "handicap_mapping_model_v1"
LINES = [-2.0, -1.0, 1.0, 2.0]
FEATURES = ["home_prob", "draw_prob", "away_prob", "home_lambda", "away_lambda", "handicap_line"]
CLASSES = ["让负", "让平", "让胜"]


def outcome_probs_from_market(row):
    return float(row.get("market_home_prob", 0.0)), float(row.get("market_draw_prob", 0.0)), float(row.get("market_away_prob", 0.0))


def handicap_label(home_goals, away_goals, line):
    diff = home_goals + line - away_goals
    if diff > 0:
        return "让胜"
    if diff == 0:
        return "让平"
    return "让负"


def empty_model():
    return {
        "model_version": MODEL_VERSION,
        "model_type": "sklearn_logistic_regression_multinomial",
        "classes": CLASSES,
        "feature_names": FEATURES,
        "coefficients": {cls: [0.0] * len(FEATURES) for cls in CLASSES},
        "intercept": {cls: 0.0 for cls in CLASSES},
        "scaler_mean": {feature: 0.0 for feature in FEATURES},
        "scaler_scale": {feature: 1.0 for feature in FEATURES},
        "metrics": {"sample_count": 0, "train_count": 0, "valid_count": 0, "accuracy": 0.0, "log_loss": 0.0},
        "created_at": now_iso(),
    }


def main():
    ensure_dirs()
    df = pd.read_csv(PROCESSED_DIR / "features.csv") if (PROCESSED_DIR / "features.csv").exists() else pd.DataFrame()
    if df.empty or len(df) < 30:
        model_json = empty_model()
        write_json(MODELS_DIR / "handicap_mapping_model_v1.json", model_json)
        write_json(REPORTS_DIR / "handicap_metrics.json", model_json["metrics"])
        print("让球模型样本不足，导出空模型")
        return

    rows = []
    for row in df.to_dict("records"):
        home_prob, draw_prob, away_prob = outcome_probs_from_market(row)
        home_lambda = max(0.1, float(row.get("home_recent_goals_for", 1.2) + row.get("away_recent_goals_against", 1.2)) / 2.0)
        away_lambda = max(0.1, float(row.get("away_recent_goals_for", 1.2) + row.get("home_recent_goals_against", 1.2)) / 2.0)
        for line in LINES:
            rows.append({
                "home_prob": home_prob,
                "draw_prob": draw_prob,
                "away_prob": away_prob,
                "home_lambda": home_lambda,
                "away_lambda": away_lambda,
                "handicap_line": line,
                "label": handicap_label(int(row["home_goals"]), int(row["away_goals"]), line),
                "date": row["date"],
            })
    data = pd.DataFrame(rows).sort_values("date").reset_index(drop=True)
    split = int(len(data) * 0.8)
    train_df = data.iloc[:split]
    valid_df = data.iloc[split:]
    scaler = StandardScaler()
    x_train = scaler.fit_transform(train_df[FEATURES].astype(float).values)
    x_valid = scaler.transform(valid_df[FEATURES].astype(float).values)
    y_train = train_df["label"].astype(str).values
    y_valid = valid_df["label"].astype(str).values
    model = LogisticRegression(solver="lbfgs", max_iter=1000, class_weight="balanced")
    model.fit(x_train, y_train)
    probs = model.predict_proba(x_valid)
    preds = model.predict(x_valid)
    classes = [str(cls) for cls in model.classes_]
    metrics = {
        "sample_count": int(len(data)),
        "train_count": int(len(train_df)),
        "valid_count": int(len(valid_df)),
        "accuracy": float(accuracy_score(y_valid, preds)),
        "log_loss": float(log_loss(y_valid, probs, labels=classes)),
    }
    output = {
        "model_version": MODEL_VERSION,
        "model_type": "sklearn_logistic_regression_multinomial",
        "classes": classes,
        "feature_names": FEATURES,
        "coefficients": {cls: model.coef_[idx].astype(float).tolist() for idx, cls in enumerate(classes)},
        "intercept": {cls: float(model.intercept_[idx]) for idx, cls in enumerate(classes)},
        "scaler_mean": {feature: float(scaler.mean_[idx]) for idx, feature in enumerate(FEATURES)},
        "scaler_scale": {feature: float(scaler.scale_[idx] if scaler.scale_[idx] else 1.0) for idx, feature in enumerate(FEATURES)},
        "metrics": metrics,
        "created_at": now_iso(),
    }
    write_json(MODELS_DIR / "handicap_mapping_model_v1.json", output)
    write_json(REPORTS_DIR / "handicap_metrics.json", metrics)
    print(f"让球映射模型完成: sample={len(data)} accuracy={metrics['accuracy']:.3f}")


if __name__ == "__main__":
    main()
