import json
from datetime import datetime
from pathlib import Path

import numpy as np
import pandas as pd


ROOT = Path(__file__).resolve().parents[1]
RAW_DIR = ROOT / "datasets" / "raw"
PROCESSED_DIR = ROOT / "datasets" / "processed"
MODELS_DIR = ROOT / "models"
REPORTS_DIR = ROOT / "reports"


ENCODINGS = ["utf-8-sig", "utf-8", "latin1", "gbk"]


def ensure_dirs():
    for path in (RAW_DIR, PROCESSED_DIR, MODELS_DIR, REPORTS_DIR):
        path.mkdir(parents=True, exist_ok=True)


def now_iso():
    return datetime.utcnow().replace(microsecond=0).isoformat() + "Z"


def write_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as handle:
        json.dump(value, handle, ensure_ascii=False, indent=2)


def read_json(path, default=None):
    if not path.exists():
        return default
    with path.open("r", encoding="utf-8") as handle:
        return json.load(handle)


def read_csv_flexible(path):
    last_error = None
    for encoding in ENCODINGS:
        try:
            return pd.read_csv(path, encoding=encoding)
        except UnicodeDecodeError as error:
            last_error = error
        except Exception as error:
            last_error = error
    raise RuntimeError(f"无法读取CSV {path}: {last_error}")


def parse_date_series(series):
    parsed = pd.to_datetime(series, dayfirst=True, errors="coerce")
    fallback = pd.to_datetime(series, dayfirst=False, errors="coerce", format="mixed")
    parsed = parsed.fillna(fallback)
    return parsed


def number_series(df, column, default=np.nan):
    if column not in df.columns:
        return pd.Series([default] * len(df), index=df.index)
    return pd.to_numeric(df[column], errors="coerce")


def text_series(df, column, default=""):
    if column not in df.columns:
        return pd.Series([default] * len(df), index=df.index, dtype="object")
    return df[column].fillna(default).astype(str).str.strip()


def first_existing_numeric(df, columns):
    result = pd.Series([np.nan] * len(df), index=df.index)
    for column in columns:
        if column in df.columns:
            result = result.fillna(pd.to_numeric(df[column], errors="coerce"))
    return result


def implied_probs(home, draw, away):
    odds = np.array([home, draw, away], dtype=float)
    inv = np.zeros_like(odds, dtype=float)
    np.divide(1.0, odds, out=inv, where=odds > 1.0)
    margin = float(inv.sum())
    if margin <= 0:
        return 0.0, 0.0, 0.0, 0.0
    probs = inv / margin
    return float(probs[0]), float(probs[1]), float(probs[2]), margin - 1.0


def brier_multiclass(probs, labels, classes):
    if len(labels) == 0:
        return 0.0
    total = 0.0
    for row, label in zip(probs, labels):
        total += sum((row[i] - (1.0 if classes[i] == label else 0.0)) ** 2 for i in range(len(classes))) / len(classes)
    return float(total / len(labels))


def softmax(scores):
    values = np.array(scores, dtype=float)
    values = values - np.max(values)
    exp = np.exp(values)
    denom = exp.sum()
    if denom <= 0:
        return [1.0 / len(values)] * len(values)
    return (exp / denom).tolist()


def bucket(value, ranges, labels):
    for (low, high), label in zip(ranges, labels):
        if low <= value < high:
            return label
    return labels[-1]


def odds_range(value):
    return bucket(value, [(0, 1.8), (1.8, 2.5), (2.5, 3.51), (3.51, 999)], ["1.00-1.79", "1.80-2.49", "2.50-3.50", "3.50+"])


def probability_range(value):
    return bucket(value, [(0, .25), (.25, .35), (.35, .45), (.45, .55), (.55, 1.01)], ["0%-25%", "25%-35%", "35%-45%", "45%-55%", "55%+"])


def ev_range(value):
    return bucket(value, [(-999, 0), (0, .05), (.05, .10), (.10, .20), (.20, 999)], ["负EV", "0-5%", "5-10%", "10-20%", "20%+"])


def advantage_range(value):
    return bucket(value, [(-999, 0), (0, .05), (.05, .10), (.10, .20), (.20, 999)], ["负优势", "0-5%", "5-10%", "10-20%", "20%+"])
