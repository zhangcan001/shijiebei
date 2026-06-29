import numpy as np
import pandas as pd
from sklearn.ensemble import ExtraTreesClassifier, HistGradientBoostingClassifier
from sklearn.linear_model import LogisticRegression
from sklearn.metrics import accuracy_score, log_loss
from sklearn.preprocessing import StandardScaler

from common import MODELS_DIR, PROCESSED_DIR, REPORTS_DIR, brier_multiclass, ensure_dirs, now_iso, read_json, write_json


MODEL_VERSION = "outcome_ensemble_model_v1"
BASE_MODELS = [
    ("logistic", LogisticRegression(solver="lbfgs", max_iter=2000, class_weight="balanced", random_state=42)),
    ("hist_gbdt", HistGradientBoostingClassifier(max_iter=180, learning_rate=0.045, l2_regularization=0.05, random_state=42)),
    ("extra_trees", ExtraTreesClassifier(n_estimators=220, min_samples_leaf=12, max_features="sqrt", class_weight="balanced", random_state=42, n_jobs=-1)),
]


def empty_model(feature_names):
    return {
        "model_version": MODEL_VERSION,
        "model_type": "weighted_probability_ensemble",
        "classes": ["A", "D", "H"],
        "feature_names": feature_names,
        "members": [],
        "weights": {},
        "metrics": {"sample_count": 0, "train_count": 0, "valid_count": 0, "accuracy": 0.0, "log_loss": 0.0, "brier_score": 0.0},
        "created_at": now_iso(),
        "training_data_range": {"start": None, "end": None, "train_count": 0, "valid_count": 0},
    }


def align_probs(probs, model_classes, classes):
    out = np.zeros((probs.shape[0], len(classes)))
    for idx, cls in enumerate(model_classes):
        if str(cls) in classes:
            out[:, classes.index(str(cls))] = probs[:, idx]
    row_sum = out.sum(axis=1, keepdims=True)
    row_sum[row_sum <= 0] = 1.0
    return out / row_sum


def serialize_member(name, model, scaler, feature_names, classes, valid_probs, valid_y):
    metrics = {
        "accuracy": float(accuracy_score(valid_y, np.array(classes)[valid_probs.argmax(axis=1)])),
        "log_loss": float(log_loss(valid_y, valid_probs, labels=classes)),
        "brier_score": brier_multiclass(valid_probs, valid_y, classes),
    }
    member = {
        "name": name,
        "classes": classes,
        "feature_names": feature_names,
        "scaler_mean": {feature: float(scaler.mean_[idx]) for idx, feature in enumerate(feature_names)},
        "scaler_scale": {feature: float(scaler.scale_[idx] if scaler.scale_[idx] else 1.0) for idx, feature in enumerate(feature_names)},
        "metrics": metrics,
    }
    if name == "logistic":
        member["model_type"] = "logistic_regression"
        member["coefficients"] = {cls: model.coef_[idx].astype(float).tolist() for idx, cls in enumerate(classes)}
        member["intercept"] = {cls: float(model.intercept_[idx]) for idx, cls in enumerate(classes)}
    elif name == "hist_gbdt":
        member["model_type"] = "hist_gradient_boosting"
        member["unsupported_in_rust"] = True
        member["validation_probs"] = []
    else:
        member["model_type"] = "extra_trees"
        member["unsupported_in_rust"] = True
        member["validation_probs"] = []
    return member


def main():
    ensure_dirs()
    schema = read_json(MODELS_DIR / "feature_schema_v1.json", {"feature_names": []})
    feature_names = schema.get("feature_names", [])
    df = pd.read_csv(PROCESSED_DIR / "features.csv") if (PROCESSED_DIR / "features.csv").exists() else pd.DataFrame()
    if df.empty or len(df) < 100 or df["result"].nunique() < 3:
        model = empty_model(feature_names)
        write_json(MODELS_DIR / "outcome_ensemble_model_v1.json", model)
        write_json(REPORTS_DIR / "outcome_ensemble_metrics.json", model["metrics"])
        print("集成模型样本不足，导出空模型")
        return

    df = df.sort_values(["date", "match_id"]).reset_index(drop=True)
    split = int(len(df) * 0.8)
    train_df = df.iloc[:split]
    valid_df = df.iloc[split:]
    classes = sorted(df["result"].astype(str).unique().tolist())
    y_train = train_df["result"].astype(str).values
    y_valid = valid_df["result"].astype(str).values
    x_train_raw = train_df[feature_names].fillna(0.0).astype(float).values
    x_valid_raw = valid_df[feature_names].fillna(0.0).astype(float).values

    members = []
    prob_rows = []
    raw_weights = {}
    for name, estimator in BASE_MODELS:
        scaler = StandardScaler()
        x_train = scaler.fit_transform(x_train_raw)
        x_valid = scaler.transform(x_valid_raw)
        estimator.fit(x_train, y_train)
        probs = align_probs(estimator.predict_proba(x_valid), estimator.classes_, classes)
        member = serialize_member(name, estimator, scaler, feature_names, classes, probs, y_valid)
        members.append(member)
        prob_rows.append(probs)
        raw_weights[name] = 1.0 / max(member["metrics"]["log_loss"], 0.01)

    weight_sum = sum(raw_weights.values()) or 1.0
    weights = {name: value / weight_sum for name, value in raw_weights.items()}
    ensemble_probs = sum(prob_rows[idx] * weights[BASE_MODELS[idx][0]] for idx in range(len(prob_rows)))
    ensemble_probs = ensemble_probs / ensemble_probs.sum(axis=1, keepdims=True)
    preds = np.array(classes)[ensemble_probs.argmax(axis=1)]
    metrics = {
        "sample_count": int(len(df)),
        "train_count": int(len(train_df)),
        "valid_count": int(len(valid_df)),
        "accuracy": float(accuracy_score(y_valid, preds)),
        "log_loss": float(log_loss(y_valid, ensemble_probs, labels=classes)),
        "brier_score": brier_multiclass(ensemble_probs, y_valid, classes),
        "date_range": {"start": str(df["date"].min()), "end": str(df["date"].max())},
        "member_metrics": {member["name"]: member["metrics"] for member in members},
    }
    output = {
        "model_version": MODEL_VERSION,
        "model_type": "weighted_probability_ensemble",
        "classes": classes,
        "feature_names": feature_names,
        "members": members,
        "weights": weights,
        "rust_primary_member": "logistic",
        "metrics": metrics,
        "created_at": now_iso(),
        "training_data_range": {
            "start": str(df["date"].min()),
            "end": str(df["date"].max()),
            "train_count": int(len(train_df)),
            "valid_count": int(len(valid_df)),
        },
    }
    write_json(MODELS_DIR / "outcome_ensemble_model_v1.json", output)
    write_json(REPORTS_DIR / "outcome_ensemble_metrics.json", metrics)
    print(f"集成模型完成: accuracy={metrics['accuracy']:.3f} logloss={metrics['log_loss']:.4f} weights={weights}")


if __name__ == "__main__":
    main()
