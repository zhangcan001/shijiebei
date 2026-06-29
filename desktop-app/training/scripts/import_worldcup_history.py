from common import PROCESSED_DIR, RAW_DIR, ensure_dirs, implied_probs, now_iso, read_csv_flexible, write_json

import hashlib
from pathlib import Path
from urllib.request import Request, urlopen

import pandas as pd


SOURCE_URL = "https://www.football-data.co.uk/WorldCup2022.xlsx"
RAW_FILE = RAW_DIR / "WorldCup2022.xlsx"
OUTPUT = PROCESSED_DIR / "worldcup_closure_samples.csv"
REPORT = PROCESSED_DIR / "worldcup_history_import_report.json"
SHEETS = ["WorldCup2018", "WorldCup2022"]


def download_workbook():
    ensure_dirs()
    if RAW_FILE.exists() and RAW_FILE.stat().st_size > 1000:
        return "cached"
    request = Request(SOURCE_URL, headers={"User-Agent": "WorldCupOddsPro/0.1 worldcup trainer"})
    with urlopen(request, timeout=30) as response:
        RAW_FILE.write_bytes(response.read())
    return "downloaded"


def stable_id(year, date, home, away):
    raw = f"{year}|{date}|{home}|{away}".encode("utf-8", errors="ignore")
    return hashlib.sha1(raw).hexdigest()[:16]


def pick_result(home_goals, away_goals):
    if home_goals > away_goals:
        return "主胜"
    if home_goals == away_goals:
        return "平局"
    return "客胜"


def odds_value(row, *names):
    for name in names:
        if name in row and pd.notna(row[name]):
            try:
                value = float(row[name])
                if value > 1.0:
                    return value
            except Exception:
                pass
    return None


def normalize_sheet(path, sheet):
    df = pd.read_excel(path, sheet_name=sheet)
    year = "".join(ch for ch in sheet if ch.isdigit()) or sheet
    rows = []
    for item in df.to_dict("records"):
        home = str(item.get("Home", "")).strip()
        away = str(item.get("Away", "")).strip()
        date = pd.to_datetime(item.get("Date"), errors="coerce")
        if not home or not away or pd.isna(date):
            continue
        try:
            home_goals = int(item.get("HGFT"))
            away_goals = int(item.get("AGFT"))
        except Exception:
            continue
        bet365_home = odds_value(item, "bet365-H", "B365H")
        bet365_draw = odds_value(item, "bet365-D", "B365D")
        bet365_away = odds_value(item, "bet365-A", "B365A")
        avg_home = odds_value(item, "H-Avg", "AvgH", "H_Avg")
        avg_draw = odds_value(item, "D-Avg", "AvgD", "D_Avg")
        avg_away = odds_value(item, "A-Avg", "AvgA", "A_Avg")
        max_home = odds_value(item, "H-Max", "MaxH", "H_Max")
        max_draw = odds_value(item, "D-Max", "MaxD", "D_Max")
        max_away = odds_value(item, "A-Max", "MaxA", "A_Max")
        if not all([bet365_home, bet365_draw, bet365_away]) and not all([avg_home, avg_draw, avg_away]):
            continue
        market_home, market_draw, market_away, _ = implied_probs(
            avg_home or bet365_home,
            avg_draw or bet365_draw,
            avg_away or bet365_away,
        )
        sport_home, sport_draw, sport_away, _ = implied_probs(
            bet365_home or avg_home,
            bet365_draw or avg_draw,
            bet365_away or avg_away,
        )
        date_text = date.strftime("%Y-%m-%d")
        match_id = stable_id(year, date_text, home, away)
        match_label = f"{home} vs {away}"
        actual = pick_result(home_goals, away_goals)
        picks = [
            ("主胜", market_home, sport_home, bet365_home or avg_home),
            ("平局", market_draw, sport_draw, bet365_draw or avg_draw),
            ("客胜", market_away, sport_away, bet365_away or avg_away),
        ]
        for pick, model_prob, sporttery_prob, odds in picks:
            fair_odds = 1.0 / max(model_prob, 0.0001)
            ev = model_prob * odds - 1.0
            advantage = odds / fair_odds - 1.0
            rows.append({
                "frozen_at": f"{date_text}T00:00:00Z",
                "settled_at": f"{date_text}T23:59:59Z",
                "match_id": match_id,
                "match_num": f"WC{year}",
                "match_time": f"{date_text} 00:00",
                "match_label": match_label,
                "market": "历史世界杯HAD胜平负",
                "pick": pick,
                "model_prob": model_prob,
                "sporttery_prob": sporttery_prob,
                "europe_prob": model_prob,
                "fair_odds": fair_odds,
                "current_odds": odds,
                "ev": ev,
                "advantage_rate": advantage,
                "recommendation_level": "历史样本",
                "action_advice": "仅用于世界杯临场修正层预训练",
                "stake_pct": 0.0,
                "data_quality_score": 88.0,
                "data_quality_grade": "历史赔率",
                "risk_tags": "football-data-worldcup;pretrained",
                "play_type_risk_level": "低",
                "anomaly_type": "",
                "anomaly_severity": "",
                "result_score": f"{home_goals}:{away_goals}",
                "actual_outcome": actual,
                "hit": 1 if pick == actual else 0,
                "profit": 0.0,
                "stage": str(item.get("Competition", f"World Cup {year}")).strip() or f"World Cup {year}",
                "sample_source": f"football-data:{sheet}",
                "avg_home": avg_home,
                "avg_draw": avg_draw,
                "avg_away": avg_away,
                "max_home": max_home,
                "max_draw": max_draw,
                "max_away": max_away,
            })
    return rows


def main():
    ensure_dirs()
    status = download_workbook()
    rows = []
    for sheet in SHEETS:
        rows.extend(normalize_sheet(RAW_FILE, sheet))
    historical = pd.DataFrame(rows)
    existing = pd.DataFrame()
    if OUTPUT.exists():
        try:
            existing = read_csv_flexible(OUTPUT)
            existing = existing[existing.get("sample_source", "").astype(str).str.contains("football-data:") == False]
        except Exception:
            existing = pd.DataFrame()
    combined = pd.concat([existing, historical], ignore_index=True, sort=False)
    combined = combined.drop_duplicates(["match_id", "market", "pick", "sample_source"], keep="last")
    combined = combined.sort_values(["match_time", "match_id", "pick"])
    combined.to_csv(OUTPUT, index=False, encoding="utf-8")
    report = {
        "ok": True,
        "download_status": status,
        "source_url": SOURCE_URL,
        "sheets": SHEETS,
        "historical_rows": int(len(historical)),
        "combined_rows": int(len(combined)),
        "matches": int(historical["match_id"].nunique()) if not historical.empty else 0,
        "output": str(OUTPUT),
        "created_at": now_iso(),
    }
    write_json(REPORT, report)
    print(f"世界杯历史训练样本: {report['historical_rows']} 行，比赛 {report['matches']} 场，合并后 {report['combined_rows']} 行")


if __name__ == "__main__":
    main()
