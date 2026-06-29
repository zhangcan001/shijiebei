import hashlib

import pandas as pd

from common import (
    RAW_DIR,
    PROCESSED_DIR,
    ensure_dirs,
    first_existing_numeric,
    number_series,
    parse_date_series,
    read_csv_flexible,
    text_series,
)


OUTPUT_COLUMNS = [
    "match_id",
    "date",
    "home_team",
    "away_team",
    "home_goals",
    "away_goals",
    "result",
    "half_home_goals",
    "half_away_goals",
    "half_result",
    "odds_home",
    "odds_draw",
    "odds_away",
    "avg_home",
    "avg_draw",
    "avg_away",
    "max_home",
    "max_draw",
    "max_away",
    "over25_odds",
    "under25_odds",
    "source_file",
    "league_code",
    "season_code",
]


def stable_match_id(source_file, date, home, away):
    raw = f"{source_file}|{date}|{home}|{away}".encode("utf-8", errors="ignore")
    return hashlib.sha1(raw).hexdigest()[:16]


def normalize_file(path):
    df = read_csv_flexible(path)
    if df.empty:
        return pd.DataFrame(columns=OUTPUT_COLUMNS)

    dates = parse_date_series(text_series(df, "Date"))
    out = pd.DataFrame()
    out["date"] = dates.dt.strftime("%Y-%m-%d")
    out["home_team"] = text_series(df, "HomeTeam")
    out["away_team"] = text_series(df, "AwayTeam")
    out["home_goals"] = number_series(df, "FTHG")
    out["away_goals"] = number_series(df, "FTAG")
    out["result"] = text_series(df, "FTR")
    out["half_home_goals"] = number_series(df, "HTHG")
    out["half_away_goals"] = number_series(df, "HTAG")
    out["half_result"] = text_series(df, "HTR")
    out["odds_home"] = first_existing_numeric(df, ["B365H", "AvgH", "MaxH"])
    out["odds_draw"] = first_existing_numeric(df, ["B365D", "AvgD", "MaxD"])
    out["odds_away"] = first_existing_numeric(df, ["B365A", "AvgA", "MaxA"])
    out["avg_home"] = number_series(df, "AvgH")
    out["avg_draw"] = number_series(df, "AvgD")
    out["avg_away"] = number_series(df, "AvgA")
    out["max_home"] = number_series(df, "MaxH")
    out["max_draw"] = number_series(df, "MaxD")
    out["max_away"] = number_series(df, "MaxA")
    out["over25_odds"] = first_existing_numeric(df, ["B365>2.5", "Avg>2.5"])
    out["under25_odds"] = first_existing_numeric(df, ["B365<2.5", "Avg<2.5"])
    out["source_file"] = path.name
    stem_parts = path.stem.split("_")
    out["season_code"] = stem_parts[0] if len(stem_parts) >= 2 else ""
    out["league_code"] = stem_parts[1] if len(stem_parts) >= 2 else path.stem
    out = out[
        out["date"].notna()
        & out["home_team"].ne("")
        & out["away_team"].ne("")
        & out["result"].isin(["H", "D", "A"])
    ].copy()
    out["match_id"] = [
        stable_match_id(path.name, row.date, row.home_team, row.away_team)
        for row in out.itertuples(index=False)
    ]
    return out[OUTPUT_COLUMNS]


def main():
    ensure_dirs()
    frames = []
    for path in sorted(RAW_DIR.glob("*.csv")):
        frames.append(normalize_file(path))
    matches = pd.concat(frames, ignore_index=True) if frames else pd.DataFrame(columns=OUTPUT_COLUMNS)
    matches = matches.sort_values(["date", "source_file", "home_team", "away_team"]).drop_duplicates("match_id")
    output = PROCESSED_DIR / "matches.csv"
    matches.to_csv(output, index=False, encoding="utf-8")

    missing_odds = int(matches[["odds_home", "odds_draw", "odds_away"]].isna().any(axis=1).sum()) if not matches.empty else 0
    result_counts = matches["result"].value_counts().to_dict() if not matches.empty else {}
    date_min = matches["date"].min() if not matches.empty else "-"
    date_max = matches["date"].max() if not matches.empty else "-"
    print(f"样本数量: {len(matches)}")
    print(f"日期范围: {date_min} ~ {date_max}")
    print(f"缺失赔率数量: {missing_odds}")
    print(f"各结果数量 H/D/A: H={result_counts.get('H', 0)} D={result_counts.get('D', 0)} A={result_counts.get('A', 0)}")


if __name__ == "__main__":
    main()
