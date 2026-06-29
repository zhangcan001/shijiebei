export const pct = value => `${((Number(value) || 0) * 100).toFixed(2)}%`;
export const odds = value => Number(value || 0).toFixed(2);
export const signedPct = value => `${Number(value || 0) >= 0 ? "+" : ""}${((Number(value) || 0) * 100).toFixed(2)}%`;
export const money = value => `${(Number(value || 0) * 100).toFixed(2)}%`;
export const ciText = (low, high) => `${pct(low)} - ${pct(high)}`;

export const teamRanks = {
  "法国": 1, "西班牙": 2, "阿根廷": 3, "英格兰": 4, "葡萄牙": 5,
  "巴西": 6, "荷兰": 7, "摩洛哥": 8, "比利时": 9, "德国": 10,
  "克罗地亚": 11, "哥伦比亚": 13, "塞内加尔": 14, "墨西哥": 15,
  "美国": 16, "乌拉圭": 17, "日本": 18, "瑞士": 19, "伊朗": 21,
  "土耳其": 22, "厄瓜多尔": 23, "奥地利": 24, "韩国": 25,
  "澳大利亚": 27, "阿尔及利亚": 28, "埃及": 29, "加拿大": 30,
  "挪威": 31, "巴拿马": 33, "科特迪瓦": 34, "瑞典": 38,
  "巴拉圭": 40, "捷克": 41, "苏格兰": 43, "突尼斯": 44,
  "民主刚果": 46, "乌兹别克斯坦": 50, "卡塔尔": 55, "伊拉克": 57,
  "南非": 60, "沙特": 61, "约旦": 63, "波黑": 65, "佛得角": 69,
  "加纳": 74, "库拉索": 82, "海地": 83, "新西兰": 85
};

export function cleanTeamName(name = "") {
  return String(name).replace(/（第\d+）|\(第\d+\)/g, "").trim();
}

export function teamRank(name = "") {
  const clean = cleanTeamName(name);
  const hit = Object.entries(teamRanks).find(([team]) => clean.includes(team) || team.includes(clean));
  return hit?.[1] || null;
}

export function rankedTeam(name = "") {
  const clean = cleanTeamName(name);
  const rank = teamRank(clean);
  return rank ? `${clean}（第${rank}）` : clean || "-";
}

export function rankedMatchLabel(label = "") {
  const parts = String(label).split(/\s+vs\s+|\s+VS\s+| 对 /i);
  if (parts.length === 2) {
    return `${rankedTeam(parts[0])} vs ${rankedTeam(parts[1])}`;
  }
  return label || "-";
}
