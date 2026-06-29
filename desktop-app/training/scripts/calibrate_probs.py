import numpy as np
import pandas as pd

from common import MODELS_DIR, PROCESSED_DIR, REPORTS_DIR, ensure_dirs, now_iso, read_json, softmax, write_json


BINS = [(0.0, 0.25), (0.25, 0.35), (0.35, 0.45), (0.45, 0.55), (0.55, 0.65), (0.65, 0.80), (0.80, 1.01)]


def predict_logistic_member(member, df):
    features = member.get("feature_names", [])
    classes = member.get("classes", ["A", "D", "H"])
    rows = []
    for _, row in df.iterrows():
        scores = []
        for cls in classes:
            score = member.get("intercept", {}).get(cls, 0.0)
            coeffs = member.get("coefficients", {}).get(cls, [])
            for idx, feature in enumerate(features):
                mean = member.get("scaler_mean", {}).get(feature, 0.0)
                scale = member.get("scaler_scale", {}).get(feature, 1.0) or 1.0
                x = (float(row.get(feature, 0.0) or 0.0) - mean) / scale
                score += (coeffs[idx] if idx < len(coeffs) else 0.0) * x
            scores.append(score)
        rows.append(softmax(scores))
    return np.array(rows), classes


def main():
    ensure_dirs()
    model = read_json(MODELS_DIR / "outcome_ensemble_model_v1.json", {})
    df = pd.read_csv(PROCESSED_DIR / "features.csv") if (PROCESSED_DIR / "features.csv").exists() else pd.DataFrame()
    if df.empty or not model.get("members"):
        calibrator = {"model_version": "calibrator_v1", "method": "bucket_calibration", "bins": {}, "created_at": now_iso(), "metrics": {"sample_count": 0}}
        write_json(MODELS_DIR / "calibrator_v1.json", calibrator)
        write_json(REPORTS_DIR / "calibrator_metrics.json", calibrator["metrics"])
        print("校准样本不足，导出空校准器")
        return
    df = df.sort_values(["date", "match_id"]).reset_index(drop=True)
    valid_df = df.iloc[int(len(df) * 0.8):].copy()
    logistic = next((member for member in model.get("members", []) if member.get("name") == "logistic"), model["members"][0])
    probs, classes = predict_logistic_member(logistic, valid_df)
    bins = {}
    for cls_idx, cls in enumerate(classes):
        cls_bins = []
        for low, high in BINS:
            mask = (probs[:, cls_idx] >= low) & (probs[:, cls_idx] < high)
            count = int(mask.sum())
            if count:
                empirical = float((valid_df["result"].values[mask] == cls).mean())
                avg_pred = float(probs[:, cls_idx][mask].mean())
            else:
                avg_pred = (low + min(high, 1.0)) / 2.0
                empirical = avg_pred
            cls_bins.append({"low": low, "high": min(high, 1.0), "count": count, "avg_pred": avg_pred, "empirical_prob": empirical})
        bins[cls] = cls_bins
    calibrator = {
        "model_version": "calibrator_v1",
        "method": "per_class_bucket_calibration",
        "classes": classes,
        "bins": bins,
        "created_at": now_iso(),
        "metrics": {"sample_count": int(len(valid_df))},
    }
    write_json(MODELS_DIR / "calibrator_v1.json", calibrator)
    write_json(REPORTS_DIR / "calibrator_metrics.json", calibrator["metrics"])
    print(f"概率校准完成: validation={len(valid_df)}")


if __name__ == "__main__":
    main()
