import argparse
from pathlib import Path
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen

from common import RAW_DIR, ensure_dirs


BASE_URL = "https://www.football-data.co.uk/mmz4281/{season}/{league}.csv"
DEFAULT_LEAGUES = [
    "E0", "E1", "E2", "E3",
    "D1", "D2",
    "I1", "I2",
    "SP1", "SP2",
    "F1", "F2",
    "N1", "B1", "P1", "T1", "G1",
    "SC0", "SC1",
]
DEFAULT_SEASONS = ["1920", "2021", "2122", "2223", "2324", "2425", "2526"]


def fetch(url):
    request = Request(url, headers={"User-Agent": "WorldCupOddsPro/0.1 training downloader"})
    with urlopen(request, timeout=20) as response:
        content_type = response.headers.get("Content-Type", "")
        data = response.read()
        return data, content_type


def looks_like_csv(data):
    head = data[:512].decode("latin1", errors="ignore")
    return "Date" in head and ("HomeTeam" in head or "AwayTeam" in head)


def download_one(season, league, overwrite=False):
    ensure_dirs()
    output = RAW_DIR / f"{season}_{league}.csv"
    if output.exists() and not overwrite:
        return {"season": season, "league": league, "ok": True, "status": "cached", "bytes": output.stat().st_size}
    url = BASE_URL.format(season=season, league=league)
    try:
        data, content_type = fetch(url)
        if not looks_like_csv(data):
            return {"season": season, "league": league, "ok": False, "status": "not_csv", "url": url, "content_type": content_type}
        output.write_bytes(data)
        return {"season": season, "league": league, "ok": True, "status": "downloaded", "url": url, "bytes": len(data)}
    except HTTPError as error:
        return {"season": season, "league": league, "ok": False, "status": f"http_{error.code}", "url": url}
    except URLError as error:
        return {"season": season, "league": league, "ok": False, "status": str(error.reason), "url": url}
    except Exception as error:
        return {"season": season, "league": league, "ok": False, "status": str(error), "url": url}


def main():
    parser = argparse.ArgumentParser(description="Download Football-Data.co.uk CSV files into datasets/raw.")
    parser.add_argument("--seasons", default=",".join(DEFAULT_SEASONS), help="Comma-separated seasons, e.g. 2324,2425,2526")
    parser.add_argument("--leagues", default=",".join(DEFAULT_LEAGUES), help="Comma-separated league codes, e.g. E0,E1,D1")
    parser.add_argument("--overwrite", action="store_true", help="Re-download existing files")
    args = parser.parse_args()

    seasons = [item.strip() for item in args.seasons.split(",") if item.strip()]
    leagues = [item.strip() for item in args.leagues.split(",") if item.strip()]
    results = [download_one(season, league, args.overwrite) for season in seasons for league in leagues]
    ok = [item for item in results if item["ok"]]
    downloaded = [item for item in results if item["status"] == "downloaded"]
    failed = [item for item in results if not item["ok"]]
    print(f"下载/缓存成功: {len(ok)}，本次新下载: {len(downloaded)}，失败/跳过: {len(failed)}")
    if failed:
        print("失败示例:")
        for item in failed[:10]:
            print(f"  {item['season']} {item['league']} {item['status']}")


if __name__ == "__main__":
    main()
