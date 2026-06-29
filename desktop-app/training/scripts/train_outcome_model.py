import numpy as np
import pandas as pd
from sklearn.linear_model import LogisticRegression
from sklearn.metrics import accuracy_score, log_loss
from sklearn.preprocessing import StandardScaler

from common import (
    MODELS_DIR,
    PROCESSED_DIR,
    REPORTS_DIR,
    brier_multiclass,
    ensure_dirs,
    now_iso,
    read_json,
    write_json,
)


MODEL_VERSION = "outcome_model_v1"


def empty_model(schema):
    features = schema.get("feature_names", [])
    return {
        "model_version": MODEL_VERSION,
        "model_type": "sklearn_logistic_regression_multinomial",
        "classes": ["A", "D", "H"],
        "feature_names": features,
        "coefficients": {cls: [0.0] * len(features) for cls in ["A", "D", "H"]},
        "intercept": {"A": 0.0, "D": 0.0, "H": 0.0},
        "scaler_mean": {feature: 0.0 for feature in features},
        "scaler_scale": {feature: 1.0 for feature in features},
        "metrics": {
            "sample_count": 0,
            "train_count": 0,
            "valid_count": 0,
            "accuracy": 0.0,
            "log_loss": 0.0,
            "brier_score": 0.0,
            "date_range": {"start": None, "end": None},
        },
        "created_at": now_iso(),
        "training_data_range": {"start": None, "end": None, "train_count": 0, "valid_count": 0},
    }


def main():
    ensure_dirs()
    schema = read_json(MODELS_DIR / "feature_schema_v1.json", {"feature_names": []})
    path = PROCESSED_DIR / "features.csv"
    features_df = pd.read_csv(path) if path.exists() else pd.DataFrame()
    feature_names = schema.get("feature_names", [])

    if features_df.empty or len(features_df) < 30 or features_df["result"].nunique() < 3:
        model = empty_model(schema)
        write_json(MODELS_DIR / "outcome_model_v1.json", model)
        write_json(REPORTS_DIR / "outcome_metrics.json", model["metrics"])
        print("训练样本不足，已导出空模型并保持规则模型 fallback")
        return

    features_df = features_df.sort_values(["date", "match_id"]).reset_index(drop=True)
    split = int(len(features_df) * 0.8)
    train_df = features_df.iloc[:split]
    valid_df = features_df.iloc[split:]

    x_train = train_df[feature_names].fillna(0.0).astype(float).values
    y_train = train_df["result"].astype(str).values
    x_valid = valid_df[feature_names].fillna(0.0).astype(float).values
    y_valid = valid_df["result"].astype(str).values

    scaler = StandardScaler()
    x_train_scaled = scaler.fit_transform(x_train)
    x_valid_scaled = scaler.transform(x_valid)
    model = LogisticRegression(
        solver="lbfgs",
        max_iter=2000,
        class_weight="balanced",
        random_state=42,
    )
    model.fit(x_train_scaled, y_train)
    probs = model.predict_proba(x_valid_scaled)
    preds = model.predict(x_valid_scaled)
    classes = [str(cls) for cls in model.classes_]
    metrics = {
        "sample_count": int(len(features_df)),
        "train_count": int(len(train_df)),
        "valid_count": int(len(valid_df)),
        "accuracy": float(accuracy_score(y_valid, preds)) if len(valid_df) else 0.0,
        "log_loss": float(log_loss(y_valid, probs, labels=classes)) if len(valid_df) else 0.0,
        "brier_score": brier_multiclass(probs, y_valid, classes) if len(valid_df) else 0.0,
        "date_range": {
            "start": str(features_df["date"].min()),
            "end": str(features_df["date"].max()),
        },
    }
    output = {
        "model_version": MODEL_VERSION,
        "model_type": "sklearn_logistic_regression_multinomial",
        "classes": classes,
        "feature_names": feature_names,
        "coefficients": {cls: model.coef_[idx].astype(float).tolist() for idx, cls in enumerate(classes)},
        "intercept": {cls: float(model.intercept_[idx]) for idx, cls in enumerate(classes)},
        "scaler_mean": {feature: float(scaler.mean_[idx]) for idx, feature in enumerate(feature_names)},
        "scaler_scale": {feature: float(scaler.scale_[idx] if scaler.scale_[idx] else 1.0) for idx, feature in enumerate(feature_names)},
        "metrics": metrics,
        "created_at": now_iso(),
        "training_data_range": {
            "start": str(features_df["date"].min()),
            "end": str(features_df["date"].max()),
            "train_count": int(len(train_df)),
            "valid_count": int(len(valid_df)),
        },
    }
    write_json(MODELS_DIR / "outcome_model_v1.json", output)
    write_json(REPORTS_DIR / "outcome_metrics.json", metrics)
    print(f"训练完成: sample={len(features_df)} train={len(train_df)} valid={len(valid_df)} accuracy={metrics['accuracy']:.3f}")


if __name__ == "__main__":
    main()
