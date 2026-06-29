import numpy as np
import pandas as pd
from sklearn.metrics import accuracy_score, log_loss

from backtest_strategy import active_prediction_model, apply_calibrator, predict_probs
from common import MODELS_DIR, PROCESSED_DIR, REPORTS_DIR, brier_multiclass, ensure_dirs, now_iso, write_json


MODEL_VERSION = "probability_blend_v1"
WEIGHTS = [round(item / 20, 2) for item in range(0, 21)]
SEGMENTS = [
    ("balanced", 0.0, 0.45),
    ("lean", 0.45, 0.60),
    ("favorite", 0.60, 0.72),
    ("heavy_favorite", 0.72, 1.01),
]


def market_probs(df, classes):
    columns = {"H": "market_home_prob", "D": "market_draw_prob", "A": "market_away_prob"}
    rows = []
    for _, row in df.iterrows():
        values = [float(row.get(columns.get(cls, ""), 0.0) or 0.0) for cls in classes]
        total = sum(values)
        if total <= 0.0:
            values = [1.0 / len(classes)] * len(classes)
        else:
            values = [value / total for value in values]
        rows.append(values)
    return np.array(rows, dtype=float)


def blend_probs(model_probs, market, model_weight):
    blended = model_probs * model_weight + market * (1.0 - model_weight)
    row_sum = blended.sum(axis=1, keepdims=True)
    row_sum[row_sum <= 0.0] = 1.0
    return blended / row_sum


def metrics_for(probs, labels, classes):
    preds = np.array(classes)[probs.argmax(axis=1)]
    return {
        "accuracy": float(accuracy_score(labels, preds)) if len(labels) else 0.0,
        "log_loss": float(log_loss(labels, probs, labels=classes)) if len(labels) else 0.0,
        "brier_score": brier_multiclass(probs, labels, classes) if len(labels) else 0.0,
    }


def best_weight_for(model_probs, market, labels, classes, mask=None):
    if mask is None:
        mask = np.ones(len(labels), dtype=bool)
    if int(mask.sum()) == 0:
        return None
    best = None
    for model_weight in WEIGHTS:
        probs = blend_probs(model_probs[mask], market[mask], model_weight)
        row = {
            "model_weight": model_weight,
            "market_weight": round(1.0 - model_weight, 2),
            "sample_count": int(mask.sum()),
            **metrics_for(probs, labels[mask], classes),
        }
        if best is None or row["log_loss"] < best["log_loss"]:
            best = row
    return best


def main():
    ensure_dirs()
    df = pd.read_csv(PROCESSED_DIR / "features.csv") if (PROCESSED_DIR / "features.csv").exists() else pd.DataFrame()
    model = active_prediction_model()
    if df.empty:
        output = {
            "model_version": MODEL_VERSION,
            "model_type": "market_model_probability_blend",
            "classes": ["A", "D", "H"],
            "model_weight": 1.0,
            "market_weight": 0.0,
            "created_at": now_iso(),
            "metrics": {"sample_count": 0},
        }
        write_json(MODELS_DIR / "probability_blend_v1.json", output)
        write_json(REPORTS_DIR / "probability_blend_metrics.json", output["metrics"])
        print("概率融合样本为空")
        return

    df = df.sort_values(["date", "match_id"]).reset_index(drop=True)
    valid_df = df.iloc[int(len(df) * 0.8):].copy()
    model_probs, classes = predict_probs(valid_df, model)
    model_probs = apply_calibrator(model_probs, classes)
    market = market_probs(valid_df, classes)
    labels = valid_df["result"].astype(str).values

    model_metrics = metrics_for(model_probs, labels, classes)
    market_metrics = metrics_for(market, labels, classes)
    grid = []
    for model_weight in WEIGHTS:
        probs = blend_probs(model_probs, market, model_weight)
        row = {
            "model_weight": model_weight,
            "market_weight": round(1.0 - model_weight, 2),
            **metrics_for(probs, labels, classes),
        }
        grid.append(row)
    best = min(grid, key=lambda item: item["log_loss"])

    segments = []
    favorite = valid_df["market_favorite_prob"].fillna(0.0).astype(float).values
    for name, low, high in SEGMENTS:
        mask = (favorite >= low) & (favorite < high)
        segment_best = best_weight_for(model_probs, market, labels, classes, mask)
        if segment_best is None:
            segment_best = dict(best)
            segment_best["sample_count"] = 0
        segment_best.update({"name": name, "low": low, "high": high})
        segments.append(segment_best)

    output = {
        "model_version": MODEL_VERSION,
        "model_type": "market_model_probability_blend",
        "classes": classes,
        "model_weight": best["model_weight"],
        "market_weight": best["market_weight"],
        "dynamic_segments": segments,
        "segment_field": "market_favorite_prob",
        "created_at": now_iso(),
        "training_data_range": {
            "start": str(df["date"].min()),
            "end": str(df["date"].max()),
            "train_count": int(len(df) * 0.8),
            "valid_count": int(len(valid_df)),
        },
        "metrics": {
            "sample_count": int(len(df)),
            "valid_count": int(len(valid_df)),
            "selected": best,
            "model_only": model_metrics,
            "market_only": market_metrics,
            "grid": grid,
        },
    }
    write_json(MODELS_DIR / "probability_blend_v1.json", output)
    write_json(REPORTS_DIR / "probability_blend_metrics.json", output["metrics"])
    print(
        "概率融合完成: "
        f"model_weight={best['model_weight']:.2f} "
        f"market_weight={best['market_weight']:.2f} "
        f"logloss={best['log_loss']:.4f} "
        f"brier={best['brier_score']:.4f}"
    )


if __name__ == "__main__":
    main()
