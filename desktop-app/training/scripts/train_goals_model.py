import math

import pandas as pd
from sklearn.linear_model import PoissonRegressor
from sklearn.metrics import mean_absolute_error, mean_poisson_deviance
from sklearn.preprocessing import StandardScaler

from common import MODELS_DIR, PROCESSED_DIR, REPORTS_DIR, ensure_dirs, now_iso, read_json, write_json


def empty_model(version, target, feature_names):
    return {
        "model_version": version,
        "model_type": "sklearn_poisson_regression",
        "target": target,
        "feature_names": feature_names,
        "coefficients": [0.0] * len(feature_names),
        "intercept": math.log(1.25),
        "scaler_mean": {feature: 0.0 for feature in feature_names},
        "scaler_scale": {feature: 1.0 for feature in feature_names},
        "metrics": {"sample_count": 0, "train_count": 0, "valid_count": 0, "mae": 0.0, "mean_poisson_deviance": 0.0},
        "created_at": now_iso(),
        "training_data_range": {"start": None, "end": None, "train_count": 0, "valid_count": 0},
    }


def train_one(df, feature_names, target, version):
    if df.empty or len(df) < 30:
        return empty_model(version, target, feature_names)
    df = df.sort_values(["date", "match_id"]).reset_index(drop=True)
    split = int(len(df) * 0.8)
    train_df = df.iloc[:split]
    valid_df = df.iloc[split:]
    x_train = train_df[feature_names].fillna(0.0).astype(float).values
    y_train = train_df[target].fillna(0).astype(float).clip(lower=0).values
    x_valid = valid_df[feature_names].fillna(0.0).astype(float).values
    y_valid = valid_df[target].fillna(0).astype(float).clip(lower=0).values

    scaler = StandardScaler()
    x_train_scaled = scaler.fit_transform(x_train)
    x_valid_scaled = scaler.transform(x_valid)
    model = PoissonRegressor(alpha=0.001, max_iter=1000)
    model.fit(x_train_scaled, y_train)
    pred = model.predict(x_valid_scaled).clip(min=0.01)
    metrics = {
        "sample_count": int(len(df)),
        "train_count": int(len(train_df)),
        "valid_count": int(len(valid_df)),
        "mae": float(mean_absolute_error(y_valid, pred)) if len(valid_df) else 0.0,
        "mean_poisson_deviance": float(mean_poisson_deviance(y_valid, pred)) if len(valid_df) else 0.0,
    }
    return {
        "model_version": version,
        "model_type": "sklearn_poisson_regression",
        "target": target,
        "feature_names": feature_names,
        "coefficients": model.coef_.astype(float).tolist(),
        "intercept": float(model.intercept_),
        "scaler_mean": {feature: float(scaler.mean_[idx]) for idx, feature in enumerate(feature_names)},
        "scaler_scale": {feature: float(scaler.scale_[idx] if scaler.scale_[idx] else 1.0) for idx, feature in enumerate(feature_names)},
        "metrics": metrics,
        "created_at": now_iso(),
        "training_data_range": {
            "start": str(df["date"].min()),
            "end": str(df["date"].max()),
            "train_count": int(len(train_df)),
            "valid_count": int(len(valid_df)),
        },
    }


def main():
    ensure_dirs()
    schema = read_json(MODELS_DIR / "feature_schema_v1.json", {"feature_names": []})
    feature_names = schema.get("feature_names", [])
    df = pd.read_csv(PROCESSED_DIR / "features.csv") if (PROCESSED_DIR / "features.csv").exists() else pd.DataFrame()
    home_model = train_one(df, feature_names, "home_goals", "goals_home_model_v1")
    away_model = train_one(df, feature_names, "away_goals", "goals_away_model_v1")
    write_json(MODELS_DIR / "goals_home_model_v1.json", home_model)
    write_json(MODELS_DIR / "goals_away_model_v1.json", away_model)
    write_json(REPORTS_DIR / "goals_metrics.json", {
        "home": home_model["metrics"],
        "away": away_model["metrics"],
    })
    print(f"进球模型完成: home_mae={home_model['metrics']['mae']:.3f} away_mae={away_model['metrics']['mae']:.3f}")


if __name__ == "__main__":
    main()
