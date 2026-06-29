from collections import defaultdict, deque
import math

import pandas as pd

from common import MODELS_DIR, PROCESSED_DIR, ensure_dirs, implied_probs, now_iso, write_json


RECENT_N = 5
FEATURE_COLUMNS = [
    "home_recent_matches",
    "away_recent_matches",
    "home_recent_win_rate",
    "away_recent_win_rate",
    "recent_win_rate_diff",
    "home_recent_draw_rate",
    "away_recent_draw_rate",
    "recent_draw_rate_diff",
    "home_recent_goals_for",
    "away_recent_goals_for",
    "recent_goals_for_diff",
    "home_recent_goals_against",
    "away_recent_goals_against",
    "recent_goals_against_diff",
    "home_recent_points_per_game",
    "away_recent_points_per_game",
    "recent_points_diff",
    "home_attack_strength",
    "away_attack_strength",
    "home_defense_strength",
    "away_defense_strength",
    "attack_strength_diff",
    "defense_strength_diff",
    "attack_x_defense_home",
    "attack_x_defense_away",
    "odds_home",
    "odds_draw",
    "odds_away",
    "market_home_prob",
    "market_draw_prob",
    "market_away_prob",
    "market_home_away_prob_diff",
    "market_favorite_prob",
    "market_entropy",
    "market_margin",
    "odds_home_away_ratio",
    "avg_max_home_gap",
    "avg_max_draw_gap",
    "avg_max_away_gap",
    "market_dispersion",
    "is_home_favorite",
    "is_draw_short",
    "home_rest_days",
    "away_rest_days",
    "rest_days_diff",
    "season_progress",
    "is_international_or_cup",
    "is_knockout_stage",
    "is_neutral_site",
    "fifa_rank_diff",
    "elo_proxy_diff",
    "squad_strength_diff",
    "tournament_importance",
    "home_history_low",
    "away_history_low",
    "history_low_any",
]

CUP_OR_INTERNATIONAL_CODES = {
    "WC", "WCP", "EC", "INT", "NATIONS", "CONFED", "COPA", "AFCON", "ASIAN", "GC",
}


def code_flag(value):
    code = str(value or "").upper()
    return 1 if any(token in code for token in CUP_OR_INTERNATIONAL_CODES) else 0


def safe_ratio_gap(base, other):
    try:
        base = float(base)
        other = float(other)
    except (TypeError, ValueError):
        return 0.0
    if base <= 0 or other <= 0:
        return 0.0
    return (other / base) - 1.0


def days_between(current, previous):
    if previous is None or pd.isna(previous):
        return 7.0
    return max(0.0, min(30.0, (current - previous).days))


def recent_summary(items):
    if not items:
        return {
            "matches": 0,
            "win_rate": 0.0,
            "draw_rate": 0.0,
            "goals_for": 1.2,
            "goals_against": 1.2,
            "points_per_game": 1.0,
        }
    n = len(items)
    return {
        "matches": n,
        "win_rate": sum(1 for item in items if item["points"] == 3) / n,
        "draw_rate": sum(1 for item in items if item["points"] == 1) / n,
        "goals_for": sum(item["gf"] for item in items) / n,
        "goals_against": sum(item["ga"] for item in items) / n,
        "points_per_game": sum(item["points"] for item in items) / n,
    }


def points(result, side):
    if result == "D":
        return 1
    if result == side:
        return 3
    return 0


def main():
    ensure_dirs()
    input_path = PROCESSED_DIR / "matches.csv"
    matches = pd.read_csv(input_path) if input_path.exists() else pd.DataFrame()
    if matches.empty:
        features = pd.DataFrame(columns=["match_id", "date", "home_team", "away_team", "result"] + FEATURE_COLUMNS)
        features.to_csv(PROCESSED_DIR / "features.csv", index=False, encoding="utf-8")
        write_json(MODELS_DIR / "feature_schema_v1.json", {
            "model_version": "feature_schema_v1",
            "created_at": now_iso(),
            "recent_matches": RECENT_N,
            "feature_names": FEATURE_COLUMNS,
            "label": "result",
            "sample_count": 0,
            "leakage_guard": "empty dataset",
        })
        print("生成特征样本: 0")
        return

    matches["date"] = pd.to_datetime(matches["date"], errors="coerce")
    matches = matches.sort_values(["date", "match_id"]).reset_index(drop=True)
    recent = defaultdict(lambda: deque(maxlen=RECENT_N))
    last_played = {}
    rows = []
    season_totals = matches.groupby(["season_code", "league_code"]).size().to_dict()
    season_seen = defaultdict(int)

    for match in matches.itertuples(index=False):
        home = match.home_team
        away = match.away_team
        season_key = (getattr(match, "season_code", ""), getattr(match, "league_code", ""))
        season_total = max(1, season_totals.get(season_key, 1))
        season_progress = season_seen[season_key] / season_total
        home_recent = recent_summary(recent[home])
        away_recent = recent_summary(recent[away])
        league_goal_rate = 1.25
        all_recent = [item for team_items in recent.values() for item in team_items]
        if all_recent:
            league_goal_rate = max(0.2, sum(item["gf"] for item in all_recent) / len(all_recent))
        home_attack = home_recent["goals_for"] / league_goal_rate
        away_attack = away_recent["goals_for"] / league_goal_rate
        home_defense = home_recent["goals_against"] / league_goal_rate
        away_defense = away_recent["goals_against"] / league_goal_rate
        mh, md, ma, margin = implied_probs(match.odds_home, match.odds_draw, match.odds_away)
        market_probs = [mh, md, ma]
        market_entropy = -sum(prob * math.log(prob) for prob in market_probs if prob > 0.0)
        hg = int(match.home_goals)
        ag = int(match.away_goals)
        home_history_low = 1 if home_recent["matches"] < RECENT_N else 0
        away_history_low = 1 if away_recent["matches"] < RECENT_N else 0
        avg_max_home_gap = safe_ratio_gap(getattr(match, "avg_home", 0.0), getattr(match, "max_home", 0.0))
        avg_max_draw_gap = safe_ratio_gap(getattr(match, "avg_draw", 0.0), getattr(match, "max_draw", 0.0))
        avg_max_away_gap = safe_ratio_gap(getattr(match, "avg_away", 0.0), getattr(match, "max_away", 0.0))
        market_dispersion = max(avg_max_home_gap, avg_max_draw_gap, avg_max_away_gap)
        home_rest_days = days_between(match.date, last_played.get(home))
        away_rest_days = days_between(match.date, last_played.get(away))
        is_context_match = code_flag(getattr(match, "league_code", ""))
        row = {
            "match_id": match.match_id,
            "date": match.date.strftime("%Y-%m-%d"),
            "home_team": home,
            "away_team": away,
            "result": match.result,
            "home_goals": hg,
            "away_goals": ag,
            "total_goals": hg + ag,
            "league_code": getattr(match, "league_code", ""),
            "season_code": getattr(match, "season_code", ""),
            "home_recent_matches": home_recent["matches"],
            "away_recent_matches": away_recent["matches"],
            "home_recent_win_rate": home_recent["win_rate"],
            "away_recent_win_rate": away_recent["win_rate"],
            "recent_win_rate_diff": home_recent["win_rate"] - away_recent["win_rate"],
            "home_recent_draw_rate": home_recent["draw_rate"],
            "away_recent_draw_rate": away_recent["draw_rate"],
            "recent_draw_rate_diff": home_recent["draw_rate"] - away_recent["draw_rate"],
            "home_recent_goals_for": home_recent["goals_for"],
            "away_recent_goals_for": away_recent["goals_for"],
            "recent_goals_for_diff": home_recent["goals_for"] - away_recent["goals_for"],
            "home_recent_goals_against": home_recent["goals_against"],
            "away_recent_goals_against": away_recent["goals_against"],
            "recent_goals_against_diff": away_recent["goals_against"] - home_recent["goals_against"],
            "home_recent_points_per_game": home_recent["points_per_game"],
            "away_recent_points_per_game": away_recent["points_per_game"],
            "recent_points_diff": home_recent["points_per_game"] - away_recent["points_per_game"],
            "home_attack_strength": home_attack,
            "away_attack_strength": away_attack,
            "home_defense_strength": home_defense,
            "away_defense_strength": away_defense,
            "attack_strength_diff": home_attack - away_attack,
            "defense_strength_diff": away_defense - home_defense,
            "attack_x_defense_home": home_attack * away_defense,
            "attack_x_defense_away": away_attack * home_defense,
            "odds_home": match.odds_home,
            "odds_draw": match.odds_draw,
            "odds_away": match.odds_away,
            "market_home_prob": mh,
            "market_draw_prob": md,
            "market_away_prob": ma,
            "market_home_away_prob_diff": mh - ma,
            "market_favorite_prob": max(market_probs),
            "market_entropy": market_entropy,
            "market_margin": margin,
            "odds_home_away_ratio": match.odds_home / match.odds_away if match.odds_home > 0 and match.odds_away > 0 else 1.0,
            "avg_max_home_gap": avg_max_home_gap,
            "avg_max_draw_gap": avg_max_draw_gap,
            "avg_max_away_gap": avg_max_away_gap,
            "market_dispersion": market_dispersion,
            "is_home_favorite": 1 if mh >= max(md, ma) else 0,
            "is_draw_short": 1 if md >= 0.30 else 0,
            "home_rest_days": home_rest_days,
            "away_rest_days": away_rest_days,
            "rest_days_diff": home_rest_days - away_rest_days,
            "season_progress": season_progress,
            "is_international_or_cup": is_context_match,
            "is_knockout_stage": 0,
            "is_neutral_site": 0,
            "fifa_rank_diff": 0.0,
            "elo_proxy_diff": (ma - mh) * 400.0,
            "squad_strength_diff": 0.0,
            "tournament_importance": 1.0 + is_context_match * 0.35,
            "home_history_low": home_history_low,
            "away_history_low": away_history_low,
            "history_low_any": max(home_history_low, away_history_low),
        }
        rows.append(row)

        recent[home].append({"gf": hg, "ga": ag, "points": points(match.result, "H")})
        recent[away].append({"gf": ag, "ga": hg, "points": points(match.result, "A")})
        last_played[home] = match.date
        last_played[away] = match.date
        season_seen[season_key] += 1

    features = pd.DataFrame(rows)
    features.to_csv(PROCESSED_DIR / "features.csv", index=False, encoding="utf-8")
    write_json(MODELS_DIR / "feature_schema_v1.json", {
        "model_version": "feature_schema_v1",
        "created_at": now_iso(),
        "recent_matches": RECENT_N,
        "feature_names": FEATURE_COLUMNS,
        "label": "result",
        "sample_count": len(features),
        "date_range": {
            "start": features["date"].min(),
            "end": features["date"].max(),
        },
        "leakage_guard": "each row is emitted before updating either team's history with the current match",
    })
    print(f"生成特征样本: {len(features)}")


if __name__ == "__main__":
    main()
