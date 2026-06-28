import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./styles.css";

const state = {
  view: "dashboard",
  status: null,
  matches: [],
  predictions: [],
  recommendations: [],
  analyses: [],
  movements: [],
  anomalies: [],
  oddsHistory: [],
  results: [],
  backtest: null,
  todayPlan: null,
  simulation: null,
  bankroll: null,
  externalConfig: null,
  modelSettings: null,
  probeResult: null,
  diagnostics: null,
  simulationProgress: null,
  selectedSimMatchId: "",
  recFilter: "全部",
  autoRefreshTimer: null,
  busy: false,
  message: "准备就绪"
};

const pct = value => `${((Number(value) || 0) * 100).toFixed(2)}%`;
const odds = value => Number(value || 0).toFixed(2);
const signedPct = value => `${Number(value || 0) >= 0 ? "+" : ""}${((Number(value) || 0) * 100).toFixed(2)}%`;
const money = value => `${(Number(value || 0) * 100).toFixed(2)}%`;
const ciText = (low, high) => `${pct(low)} - ${pct(high)}`;

const teamRanks = {
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

function cleanTeamName(name = "") {
  return String(name).replace(/（第\d+）|\(第\d+\)/g, "").trim();
}

function teamRank(name = "") {
  const clean = cleanTeamName(name);
  const hit = Object.entries(teamRanks).find(([team]) => clean.includes(team) || team.includes(clean));
  return hit?.[1] || null;
}

function rankedTeam(name = "") {
  const clean = cleanTeamName(name);
  const rank = teamRank(clean);
  return rank ? `${clean}（第${rank}）` : clean || "-";
}

function rankedMatchLabel(label = "") {
  const parts = String(label).split(/\s+vs\s+|\s+VS\s+| 对 /i);
  if (parts.length === 2) {
    return `${rankedTeam(parts[0])} vs ${rankedTeam(parts[1])}`;
  }
  return label || "-";
}

const views = [
  ["dashboard", "比赛中心"],
  ["prediction", "预测中心"],
  ["match", "单场分析"],
  ["sim", "模拟对决"],
  ["today", "今日方案"],
  ["recommend", "买球推荐"],
  ["movements", "赔率异动"],
  ["results", "赛果中心"],
  ["review", "复盘中心"],
  ["bankroll", "资金管理"],
  ["sources", "数据源"]
];

function setBusy(message) {
  state.busy = true;
  state.message = message;
  render();
}

function clearBusy(message = "完成") {
  state.busy = false;
  state.message = message;
  render();
}

async function safeRun(message, fn) {
  try {
    setBusy(message);
    await fn();
    clearBusy("完成");
  } catch (error) {
    clearBusy(error?.message || String(error));
  }
}

async function loadStatus() {
  state.status = await invoke("app_status");
  state.matches = await invoke("list_matches");
  if (!state.selectedSimMatchId && state.matches.length) {
    state.selectedSimMatchId = state.matches[0].id;
  }
  state.predictions = await invoke("list_predictions");
  state.movements = await invoke("list_odds_movements");
  state.anomalies = await invoke("list_odds_anomalies");
  state.oddsHistory = await invoke("list_odds_history");
  state.results = await invoke("list_results");
  state.bankroll = await invoke("get_bankroll_settings");
  state.externalConfig = await invoke("get_external_source_config");
  state.modelSettings = await invoke("get_model_settings");
  state.diagnostics = await invoke("model_diagnostics");
  state.backtest = await invoke("backtest_report");
  try {
    state.recommendations = await invoke("list_recommendations");
    state.analyses = await invoke("list_match_analyses");
    state.todayPlan = await invoke("today_bet_plan");
  } catch {
    state.recommendations = [];
    state.analyses = [];
    state.todayPlan = null;
  }
}

async function refreshCore() {
  await invoke("refresh_core_data", {
    oddsApiKey: document.querySelector("#odds-key")?.value || "",
    region: document.querySelector("#odds-region")?.value || "eu"
  });
  await loadStatus();
}

async function refreshXg() {
  await invoke("refresh_statsbomb_xg");
  await loadStatus();
}

async function refreshSportteryInjuries() {
  state.probeResult = await invoke("refresh_sporttery_injuries");
  state.status = await invoke("app_status");
}

async function refreshResults() {
  state.results = await invoke("refresh_results");
}

async function importHistoricalResults() {
  const csvText = document.querySelector("#historical-csv")?.value || "";
  state.probeResult = await invoke("import_historical_results_csv", { csvText });
  await loadStatus();
}

async function importPlayerStatus() {
  const csvText = document.querySelector("#player-status-csv")?.value || "";
  state.probeResult = await invoke("import_player_status_csv", { csvText });
  await loadStatus();
}

async function importTeamStats() {
  const csvText = document.querySelector("#team-stats-csv")?.value || "";
  state.probeResult = await invoke("import_team_stats_csv", { csvText });
  await loadStatus();
}

async function autoSettle() {
  await invoke("auto_settle_predictions");
  state.predictions = await invoke("list_predictions");
  state.diagnostics = await invoke("model_diagnostics");
}

async function runSimulation() {
  const selectedMatchId = document.querySelector("#sim-match")?.value || "";
  const selectedMatch = state.matches.find(match => match.id === selectedMatchId);
  state.selectedSimMatchId = selectedMatchId;
  const request = {
    match_id: selectedMatchId,
    home: selectedMatch?.home || document.querySelector("#sim-home").value.trim() || state.matches[0]?.home || "Argentina",
    away: selectedMatch?.away || document.querySelector("#sim-away").value.trim() || state.matches[0]?.away || "France",
    home_lambda: numberOrNull(document.querySelector("#sim-home-lambda").value),
    away_lambda: numberOrNull(document.querySelector("#sim-away-lambda").value),
    max_goals: 8,
    simulations: Math.max(50000, Math.min(500000, Math.round(Number(document.querySelector("#sim-count")?.value || 50000)))),
    knockout_mode: document.querySelector("#sim-knockout")?.checked ?? true
  };
  state.simulationProgress = { done: 0, total: request.simulations, percent: 0, message: "准备模拟" };
  render();
  state.simulation = await invoke("simulate_match", { request });
  state.simulationProgress = { done: request.simulations, total: request.simulations, percent: 1, message: "模拟完成" };
}

async function saveRecommendation(index) {
  const item = state.recommendations[index];
  if (!item) return;
  await invoke("save_prediction", {
    record: {
      match_label: item.match_label,
      market: item.market,
      pick: item.pick,
      probability: item.model_prob,
      odds: item.odds,
      safety_margin: item.probability_gap,
      decision: item.decision,
      stake_pct: item.stake_pct
    }
  });
  state.predictions = await invoke("list_predictions");
  state.diagnostics = await invoke("model_diagnostics");
}

async function deletePrediction(id) {
  await invoke("delete_prediction", { id: Number(id) });
  state.predictions = await invoke("list_predictions");
  state.diagnostics = await invoke("model_diagnostics");
}

async function settlePrediction(id, hit) {
  await invoke("settle_prediction", { id: Number(id), hit });
  state.predictions = await invoke("list_predictions");
  state.diagnostics = await invoke("model_diagnostics");
  state.backtest = await invoke("backtest_report");
}

async function saveBankroll() {
  const settings = {
    bankroll: Number(document.querySelector("#bankroll")?.value || 1000),
    daily_budget_pct: Number(document.querySelector("#daily-budget")?.value || 3) / 100,
    max_loss_pct: Number(document.querySelector("#max-loss")?.value || 6) / 100,
    auto_refresh_minutes: Number(document.querySelector("#auto-refresh")?.value || 0)
  };
  await invoke("save_bankroll_settings", { settings });
  state.bankroll = await invoke("get_bankroll_settings");
  setupAutoRefresh();
}

async function saveExternalConfig() {
  const config = {
    injury_url: document.querySelector("#injury-url")?.value || "",
    lineup_url: document.querySelector("#lineup-url")?.value || "",
    stats_url: document.querySelector("#stats-url")?.value || "",
    notes: document.querySelector("#source-notes")?.value || ""
  };
  await invoke("save_external_source_config", { config });
  state.externalConfig = await invoke("get_external_source_config");
}

async function refreshExternalSources() {
  state.probeResult = await invoke("refresh_external_sources");
  state.status = await invoke("app_status");
}

async function probeExternal(url) {
  state.probeResult = await invoke("probe_external_source", { url });
}

async function saveModelSettings() {
  const settings = {
    buy_edge: Number(document.querySelector("#buy-edge")?.value || 8) / 100,
    buy_gap: Number(document.querySelector("#buy-gap")?.value || 2.5) / 100,
    watch_edge: Number(document.querySelector("#watch-edge")?.value || 3.5) / 100,
    watch_gap: Number(document.querySelector("#watch-gap")?.value || 1) / 100,
    max_odds: Number(document.querySelector("#max-odds")?.value || 8),
    high_odds_limit: Number(document.querySelector("#high-odds-limit")?.value || 8),
    mode: document.querySelector("#model-mode")?.value || "手动"
  };
  await invoke("save_model_settings", { settings });
  state.modelSettings = await invoke("get_model_settings");
  state.recommendations = await invoke("list_recommendations");
}

async function autoTuneModel() {
  state.modelSettings = await invoke("auto_tune_model");
  state.recommendations = await invoke("list_recommendations");
}

async function freezeRecommendations() {
  state.probeResult = await invoke("freeze_current_recommendations");
  state.backtest = await invoke("backtest_report");
}

function numberOrNull(value) {
  const num = Number(value);
  return Number.isFinite(num) && num > 0 ? num : null;
}

function sourceCards() {
  const sources = state.status?.sources || [];
  return sources.map(source => `
    <div class="panel span-4 metric">
      <span>${source.label}</span>
      <strong>${source.ok ? "已缓存" : "未缓存"}</strong>
      <div class="muted">${source.updated_at || source.message}</div>
      <div>${source.count ? `${source.count} 条` : ""}</div>
    </div>
  `).join("");
}

function matchRows(limit = 80) {
  if (!state.matches.length) {
    return `<tr><td colspan="6" class="muted">暂无比赛数据。先点击“刷新核心数据”。</td></tr>`;
  }
  return state.matches.slice(0, limit).map(match => `
    <tr>
      <td>${match.match_num || "-"}</td>
      <td>${match.time || "-"}</td>
      <td>${match.league || "-"}</td>
      <td><strong>${rankedTeam(match.home)}</strong></td>
      <td><strong>${rankedTeam(match.away)}</strong></td>
      <td>${match.status || "-"}</td>
    </tr>
  `).join("");
}

function predictionRows() {
  if (!state.predictions.length) {
    return `<tr><td colspan="12" class="muted">暂无复盘记录。</td></tr>`;
  }
  return state.predictions.map(record => `
    <tr>
      <td>${record.created_at || "-"}</td>
      <td>${rankedMatchLabel(record.match_label)}</td>
      <td>${record.market}</td>
      <td>${record.pick}</td>
      <td>${pct(record.probability)}</td>
      <td>${odds(record.odds)}</td>
      <td>${signedPct(record.safety_margin)}</td>
      <td>${money(record.stake_pct || 0)}</td>
      <td>${badge(record.decision)}</td>
      <td>${record.actual_result || "未结算"}</td>
      <td class="${(record.profit || 0) >= 0 ? "down" : "up"}">${signedPct(record.profit || 0)}</td>
      <td>
        <button class="mini" data-action="settle-hit" data-id="${record.id}">命中</button>
        <button class="mini" data-action="settle-miss" data-id="${record.id}">未中</button>
        <button class="mini danger" data-action="delete-prediction" data-id="${record.id}">删除</button>
      </td>
    </tr>
  `).join("");
}

function badge(text) {
  const cls = /可买|盈利|正/.test(text) ? "good" : /观察|等待|小注|中/.test(text) ? "warn" : "bad";
  return `<span class="badge ${cls}">${text || "-"}</span>`;
}

function recommendationRows(limit = 80) {
  const rows = state.recommendations.filter(item => state.recFilter === "全部" || item.tier === state.recFilter || item.decision === state.recFilter);
  if (!rows.length) {
    return `<tr><td colspan="21" class="muted">暂无推荐。先在比赛中心点击“刷新核心数据”。</td></tr>`;
  }
  return rows.slice(0, limit).map((item) => {
    const index = state.recommendations.indexOf(item);
    return `
    <tr class="${item.decision === "可买" ? "row-buy" : item.decision === "观察" ? "row-watch" : ""}">
      <td>${item.match_time || "-"}</td>
      <td><strong>${rankedMatchLabel(item.match_label)}</strong><div class="muted">${item.match_num || "-"}</div></td>
      <td>${item.market}</td>
      <td><strong>${item.pick}</strong></td>
      <td>${badge(item.tier)}</td>
      <td>${pct(item.model_prob)}</td>
      <td>${pct(item.fair_prob)}</td>
      <td>${item.europe_prob == null ? "-" : pct(item.europe_prob)}</td>
      <td>${signedPct(item.probability_gap)}</td>
      <td>${item.europe_gap == null ? "-" : signedPct(item.europe_gap)}</td>
      <td>${odds(item.fair_odds)}</td>
      <td>${odds(item.odds)}</td>
      <td>${item.europe_odds == null ? "-" : odds(item.europe_odds)}</td>
      <td>${signedPct(item.expected_return)}</td>
      <td>${signedPct(item.advantage_rate)}</td>
      <td><strong>${item.data_grade || "-"}</strong><div class="muted">${Number(item.data_score || 0).toFixed(0)}分</div></td>
      <td>${item.quality_action || "-"}</td>
      <td>${money(item.stake_pct)}<div class="muted">${state.bankroll ? Math.round(item.stake_pct * state.bankroll.bankroll) : 0}</div></td>
      <td>${badge(item.decision)} <span class="badge">${item.confidence}</span><div class="muted reason">${item.action_advice || item.play_style}</div><div class="muted reason">风险等级：${item.play_type_risk_level || "-"}</div><div class="muted reason">赔率异常：${item.anomaly_type || "-"} ${item.anomaly_severity || ""} ${item.anomaly_advice || ""}</div><div class="muted reason">${item.reason}</div><div class="muted reason">支持：${item.support_factors || "-"}</div><div class="muted reason">风险：${item.risk_factors || "-"}</div></td>
      <td>${item.combo_group || "-"}</td>
      <td><button class="mini" data-action="save-rec" data-index="${index}">存复盘</button></td>
    </tr>
  `}).join("");
}

function pickSummary(item) {
  return `${rankedMatchLabel(item.match_label)}｜${item.market}｜${item.pick}｜赔率 ${odds(item.odds)}｜仓位 ${money(item.stake_pct)}`;
}

function purchasePlanHtml() {
  const playable = state.recommendations.filter(item => item.decision === "可买" && item.stake_pct > 0);
  const bankers = playable.filter(item => item.tier === "稳胆" || item.tier === "让球稳胆").slice(0, 4);
  const valueSingles = playable.filter(item => item.tier === "价值小注" || item.tier === "进球数小注").slice(0, 4);
  const longshots = playable.filter(item => item.tier === "冷门小注").slice(0, 3);
  const comboCandidates = bankers
    .filter((item, index, arr) => arr.findIndex(other => other.match_id === item.match_id) === index)
    .slice(0, 3);
  const comboText = comboCandidates.length >= 2
    ? comboCandidates.map(pickSummary).join("<br>")
    : "暂不建议串关：稳胆候选不足或同场相关性过高。";

  return `
    <section class="panel span-12 plan-panel">
      <h3>一键购买方案</h3>
      <div class="plan-grid">
        <div>
          <h4>优先单关</h4>
          <p class="muted">${bankers.length ? bankers.map(pickSummary).join("<br>") : "暂无稳胆单关。宁可空仓，不硬买。"}</p>
        </div>
        <div>
          <h4>二串一候选</h4>
          <p class="muted">${comboText}</p>
        </div>
        <div>
          <h4>价值小注</h4>
          <p class="muted">${valueSingles.length ? valueSingles.map(pickSummary).join("<br>") : "暂无价值小注。"}</p>
        </div>
        <div>
          <h4>冷门小注</h4>
          <p class="muted">${longshots.length ? longshots.map(pickSummary).join("<br>") : "暂无冷门小注。高赔率默认不追。"}</p>
        </div>
      </div>
    </section>
  `;
}

function compactPickRows(items = [], empty = "暂无") {
  if (!items.length) return `<tr><td colspan="7" class="muted">${empty}</td></tr>`;
  return items.map(item => `
    <tr>
      <td>${item.match_time || "-"}</td>
      <td><strong>${rankedMatchLabel(item.match_label)}</strong><div class="muted">${item.match_num || "-"}</div></td>
      <td>${item.market}</td>
      <td>${item.pick}</td>
      <td>${pct(item.model_prob)}<div class="muted">公平 ${odds(item.fair_odds)}</div></td>
      <td>${odds(item.odds)}<div class="muted">EV ${signedPct(item.expected_return)}</div></td>
      <td>${badge(item.decision)}<div class="muted">${item.action_advice || "-"}</div><div class="muted">${item.quality_action || "-"}</div></td>
    </tr>
  `).join("");
}

function todayPlanHtml() {
  const plan = state.todayPlan;
  if (!plan) {
    return `<section class="panel span-12 muted">暂无今日方案。先刷新核心数据，再点击一键推荐买球。</section>`;
  }
  const comboBudget = plan.daily_budget * 0.2;
  return `
    <div class="grid">
      <section class="panel span-3 metric"><span>今日预算</span><strong>${Math.round(plan.daily_budget)}</strong><div class="muted">本金 ${Math.round(plan.bankroll)}</div></section>
      <section class="panel span-3 metric"><span>最大亏损</span><strong>${Math.round(plan.max_loss)}</strong><div class="muted">触发后停止下注</div></section>
      <section class="panel span-3 metric"><span>串关上限</span><strong>${Math.round(comboBudget)}</strong><div class="muted">不超过今日预算20%</div></section>
      <section class="panel span-3 metric"><span>方案口径</span><strong>保守</strong><div class="muted">小注优先，比分默认不下注</div></section>
      <section class="panel span-12 toolbar">
        <button class="btn" data-action="refresh-recommend">刷新今日方案</button>
        <button class="btn secondary" data-action="freeze-rec">冻结赛前快照</button>
        <button class="btn secondary" data-view="review">赛后复盘入口</button>
        <span class="muted">${plan.review_hint || ""}</span>
      </section>
      <section class="panel span-12"><h3>等首发/等赔率提示</h3><p class="muted">${plan.wait_notes?.length ? plan.wait_notes.join("；") : "暂无等待提示。"}</p></section>
      <section class="panel span-12 table-panel"><h3>单关候选</h3><div class="scroll-table"><table><thead><tr><th>时间</th><th>比赛</th><th>玩法</th><th>方向</th><th>模型</th><th>赔率/EV</th><th>操作建议</th></tr></thead><tbody>${compactPickRows(plan.singles, "暂无单关候选。")}</tbody></table></div></section>
      <section class="panel span-12 table-panel"><h3>二串一候选</h3><p class="muted">只从稳胆/让球稳胆里挑不同场次，总额不超过今日预算20%。</p><div class="scroll-table"><table><thead><tr><th>时间</th><th>比赛</th><th>玩法</th><th>方向</th><th>模型</th><th>赔率/EV</th><th>操作建议</th></tr></thead><tbody>${compactPickRows(plan.combos, "暂无合格二串一候选。")}</tbody></table></div></section>
      <section class="panel span-6 table-panel"><h3>观察清单</h3><div class="scroll-table"><table><thead><tr><th>时间</th><th>比赛</th><th>玩法</th><th>方向</th><th>模型</th><th>赔率/EV</th><th>操作建议</th></tr></thead><tbody>${compactPickRows(plan.watch, "暂无观察项。")}</tbody></table></div></section>
      <section class="panel span-6 table-panel"><h3>禁买清单</h3><div class="scroll-table"><table><thead><tr><th>时间</th><th>比赛</th><th>玩法</th><th>方向</th><th>模型</th><th>赔率/EV</th><th>操作建议</th></tr></thead><tbody>${compactPickRows(plan.banned, "暂无禁买项。")}</tbody></table></div></section>
    </div>
  `;
}

function probMiniTable(items) {
  return `
    <div class="prob-table">
      <div class="prob-row head">
        <span>方向</span>
        <span>模型概率</span>
        <span>体彩去水</span>
        <span>概率差</span>
        <span>体彩赔率</span>
        <span>模型公平</span>
      </div>
      ${items.map(item => `
        <div class="prob-row">
          <span>${item.pick}</span>
          <strong>${pct(item.probability)}</strong>
          <span>${item.sporttery_prob == null ? "-" : pct(item.sporttery_prob)}</span>
          <span class="${(item.probability_gap || 0) >= 0 ? "down" : "up"}">${item.probability_gap == null ? "-" : signedPct(item.probability_gap)}</span>
          <span>${item.sporttery_odds == null ? "-" : odds(item.sporttery_odds)}</span>
          <span>${odds(item.fair_odds)}</span>
        </div>
      `).join("")}
    </div>
  `;
}

function analysisCards() {
  if (!state.analyses.length) {
    return `<section class="panel span-12 muted">暂无单场分析。先刷新核心数据。</section>`;
  }
  return state.analyses.slice(0, 24).map(item => `
    <section class="panel span-6 analysis-card">
      <div class="card-head">
        <div>
          <h3>${rankedMatchLabel(item.match_label)}</h3>
          <p>${item.match_num || "-"} · ${item.match_time || "-"}</p>
        </div>
        <span class="badge warn">淘汰赛90分钟</span>
      </div>
      <div class="muted">λ主 ${odds(item.lambda_home)} / λ客 ${odds(item.lambda_away)} · ${item.europe_note}</div>
      <h4>胜平负修正概率</h4>
      ${probMiniTable(item.had)}
      <h4>让球胜平负修正概率</h4>
      ${probMiniTable(item.hhad)}
      <h4>总进球数概率</h4>
      ${probMiniTable(item.ttg)}
      <h4>比分 Top</h4>
      ${probMiniTable(item.scores)}
      <p class="muted">${item.knockout_note}</p>
    </section>
  `).join("");
}

function bestProb(items = []) {
  return [...items].sort((a, b) => (b.probability || 0) - (a.probability || 0))[0] || null;
}

function predictionCenterHtml() {
  if (!state.analyses.length) {
    return `<section class="panel span-12 muted">暂无预测数据。先刷新核心数据，再点击刷新预测。</section>`;
  }
  return `
    <div class="grid">
      <section class="panel span-12 toolbar">
        <button class="btn" data-action="refresh-analysis">刷新预测</button>
        <span class="muted">这里只显示真实概率预测，不代表值得下注；下注请看“买球推荐”。</span>
      </section>
      ${state.analyses.map(item => {
        const had = bestProb(item.had);
        const hhad = bestProb(item.hhad);
        const ttg = bestProb(item.ttg);
        const score = bestProb(item.scores);
        const risk = had && had.probability < 0.45 ? "分散" : had && had.probability >= 0.58 ? "较清晰" : "中等";
        return `
          <section class="panel span-6 analysis-card">
            <div class="card-head">
              <div>
                <h3>${rankedMatchLabel(item.match_label)}</h3>
                <p>${item.match_num || "-"} · ${item.match_time || "-"}</p>
              </div>
              <span class="badge ${risk === "较清晰" ? "good" : risk === "中等" ? "warn" : "bad"}">${risk}</span>
            </div>
            <div class="grid">
              <div class="panel span-3 metric"><span>90分钟首选</span><strong>${had?.pick || "-"}</strong><div class="muted">${had ? pct(had.probability) : "-"}</div></div>
              <div class="panel span-3 metric"><span>让球倾向</span><strong>${hhad?.pick || "-"}</strong><div class="muted">${hhad ? pct(hhad.probability) : "-"}</div></div>
              <div class="panel span-3 metric"><span>总进球</span><strong>${ttg?.pick || "-"}</strong><div class="muted">${ttg ? pct(ttg.probability) : "-"}</div></div>
              <div class="panel span-3 metric"><span>比分Top</span><strong>${score?.pick || "-"}</strong><div class="muted">${score ? pct(score.probability) : "-"}</div></div>
            </div>
            <p class="muted">${item.knockout_note}</p>
          </section>
        `;
      }).join("")}
    </div>
  `;
}

function movementRows(rows = state.movements, emptyText = "暂无赔率记录。请至少刷新两次核心数据。") {
  if (!rows.length) {
    return `<tr><td colspan="9" class="muted">${emptyText}</td></tr>`;
  }
  return rows.map(item => `
    <tr>
      <td>${item.created_at || "-"}</td>
      <td><strong>${rankedMatchLabel(item.match_label)}</strong></td>
      <td>${item.market}</td>
      <td>${item.pick}</td>
      <td>${odds(item.initial_odds)}</td>
      <td>${odds(item.previous_odds)}</td>
      <td>${odds(item.current_odds)}</td>
      <td>${item.direction}</td>
      <td class="${item.delta_pct > 0 ? "up" : item.delta_pct < 0 ? "down" : "muted"}">${odds(item.delta_abs)} / ${signedPct(item.delta_pct)}</td>
    </tr>
  `).join("");
}

function anomalyRows() {
  if (!state.anomalies.length) {
    return `<tr><td colspan="9" class="muted">暂无赔率异常。刷新赔率并记录快照后自动识别。</td></tr>`;
  }
  return state.anomalies.map(item => `
    <tr>
      <td>${item.created_at || "-"}</td>
      <td><strong>${rankedMatchLabel(item.match_label)}</strong></td>
      <td>${item.market}</td>
      <td>${item.pick}</td>
      <td>${badge(item.anomaly_type)}</td>
      <td>${item.severity}</td>
      <td>${item.impact_direction}</td>
      <td>${item.advice}</td>
      <td>${signedPct(item.delta_pct)}</td>
    </tr>
  `).join("");
}

function resultRows() {
  if (!state.results.length) {
    return `<tr><td colspan="6" class="muted">暂无赛果缓存。点击“刷新赛果”。</td></tr>`;
  }
  return state.results.map(result => `
    <tr>
      <td>${result.stage || "-"}</td>
      <td>${result.status || "-"}</td>
      <td><strong>${rankedTeam(result.home)}</strong></td>
      <td><strong>${result.score}</strong></td>
      <td><strong>${rankedTeam(result.away)}</strong></td>
      <td>${result.half_score || "-"}</td>
    </tr>
  `).join("");
}

function sourceHealthHtml() {
  const sources = state.status?.sources || [];
  return `
    <section class="panel span-12 table-panel">
      <h3>数据源健康监控</h3>
      <table><thead><tr><th>数据源</th><th>状态</th><th>上次成功</th><th>数量</th><th>说明</th></tr></thead><tbody>
        ${sources.map(source => `
          <tr>
            <td>${source.label}</td>
            <td>${badge(source.ok ? "正常" : "缺失")}</td>
            <td>${source.updated_at || "-"}</td>
            <td>${source.count || 0}</td>
            <td>${source.ok ? "可用于模型计算" : "请在比赛中心刷新；外部接口失败时会保留旧缓存"}</td>
          </tr>
        `).join("")}
      </tbody></table>
    </section>
  `;
}

function backtestRows() {
  const groups = state.backtest?.groups || [];
  if (!groups.length) {
    return `<tr><td colspan="11" class="muted">暂无已结算样本。先把推荐存入复盘并结算命中/未中。</td></tr>`;
  }
  return groups.map(item => `
    <tr>
      <td>${item.dimension}</td>
      <td>${item.group}</td>
      <td>${item.count}</td>
      <td>${pct(item.hit_rate)}</td>
      <td>${signedPct(item.roi)}</td>
      <td>${signedPct(item.total_profit)}</td>
      <td>${signedPct(item.max_drawdown)}</td>
      <td>${odds(item.avg_odds)}</td>
      <td>${signedPct(item.avg_advantage_rate)}</td>
      <td>${Number(item.brier_score || 0).toFixed(3)}</td>
      <td>${Number(item.log_loss || 0).toFixed(3)}</td>
    </tr>
  `).join("");
}

function bankrollHtml() {
  const settings = state.bankroll || { bankroll: 1000, daily_budget_pct: 0.03, max_loss_pct: 0.06, auto_refresh_minutes: 0 };
  const diag = state.diagnostics || { total: 0, settled: 0, hit_rate: 0, roi: 0, brier_score: 0, log_loss: 0, calibration: [], market_calibration: [], advice: "暂无复盘样本。" };
  const model = state.modelSettings || { buy_edge: 0.08, buy_gap: 0.025, watch_edge: 0.035, watch_gap: 0.01, max_odds: 8, high_odds_limit: 8, mode: "正常" };
  return `
    <div class="grid">
      <section class="panel span-3 metric"><span>复盘总数</span><strong>${diag.total}</strong><div class="muted">已保存推荐</div></section>
      <section class="panel span-3 metric"><span>已结算</span><strong>${diag.settled}</strong><div class="muted">命中/未中</div></section>
      <section class="panel span-3 metric"><span>命中率</span><strong>${pct(diag.hit_rate)}</strong><div class="muted">仅结算样本</div></section>
      <section class="panel span-3 metric"><span>ROI</span><strong>${signedPct(diag.roi)}</strong><div class="muted">按仓位百分比</div></section>
      <section class="panel span-3 metric"><span>Brier Score</span><strong>${odds(diag.brier_score)}</strong><div class="muted">越低越好，概率校准</div></section>
      <section class="panel span-3 metric"><span>Log Loss</span><strong>${odds(diag.log_loss)}</strong><div class="muted">越低越好，惩罚过度自信</div></section>
      <section class="panel span-12 toolbar">
        <label>本金
          <input id="bankroll" type="number" min="1" value="${settings.bankroll}">
        </label>
        <label>每日预算 %
          <input id="daily-budget" type="number" min="0" step="0.1" value="${(settings.daily_budget_pct * 100).toFixed(1)}">
        </label>
        <label>最大止损 %
          <input id="max-loss" type="number" min="0" step="0.1" value="${(settings.max_loss_pct * 100).toFixed(1)}">
        </label>
        <label>自动刷新 分钟
          <input id="auto-refresh" type="number" min="0" step="1" value="${settings.auto_refresh_minutes}">
        </label>
        <button class="btn" data-action="save-bankroll">保存设置</button>
      </section>
      <section class="panel span-12">
        <h3>回测调权建议</h3>
        <p class="muted">${diag.advice}</p>
        <p class="muted">样本少时自动调权容易过拟合；系统低于20条结算样本不会强制调整。</p>
      </section>
      <section class="panel span-12 table-panel">
        <h3>概率分档校准</h3>
        <table><thead><tr><th>预测概率区间</th><th>样本数</th><th>平均预测概率</th><th>实际命中率</th><th>偏差</th></tr></thead><tbody>
          ${(diag.calibration || []).length ? diag.calibration.map(row => `
            <tr>
              <td>${row.bucket}</td>
              <td>${row.count}</td>
              <td>${pct(row.avg_probability)}</td>
              <td>${pct(row.hit_rate)}</td>
              <td class="${(row.hit_rate - row.avg_probability) >= 0 ? "down" : "up"}">${signedPct(row.hit_rate - row.avg_probability)}</td>
            </tr>
          `).join("") : `<tr><td colspan="5" class="muted">暂无已结算样本。</td></tr>`}
        </tbody></table>
      </section>
      <section class="panel span-12 table-panel">
        <h3>玩法级校准</h3>
        <table><thead><tr><th>玩法</th><th>样本数</th><th>平均预测概率</th><th>实际命中率</th><th>Brier</th><th>ROI</th></tr></thead><tbody>
          ${(diag.market_calibration || []).length ? diag.market_calibration.map(row => `
            <tr>
              <td>${row.market}</td>
              <td>${row.count}</td>
              <td>${pct(row.avg_probability)}</td>
              <td>${pct(row.hit_rate)}</td>
              <td>${odds(row.brier_score)}</td>
              <td class="${row.roi >= 0 ? "down" : "up"}">${signedPct(row.roi)}</td>
            </tr>
          `).join("") : `<tr><td colspan="6" class="muted">暂无已结算样本。</td></tr>`}
        </tbody></table>
      </section>
      <section class="panel span-12">
        <h3>模型阈值</h3>
        <div class="toolbar">
          <label>可买期望 %
            <input id="buy-edge" type="number" step="0.1" value="${(model.buy_edge * 100).toFixed(1)}">
          </label>
          <label>可买概率差 %
            <input id="buy-gap" type="number" step="0.1" value="${(model.buy_gap * 100).toFixed(1)}">
          </label>
          <label>观察期望 %
            <input id="watch-edge" type="number" step="0.1" value="${(model.watch_edge * 100).toFixed(1)}">
          </label>
          <label>观察概率差 %
            <input id="watch-gap" type="number" step="0.1" value="${(model.watch_gap * 100).toFixed(1)}">
          </label>
          <label>最高赔率
            <input id="max-odds" type="number" step="0.1" value="${model.max_odds}">
          </label>
          <label>高赔压制
            <input id="high-odds-limit" type="number" step="0.1" value="${model.high_odds_limit}">
          </label>
          <label>模式
            <input id="model-mode" value="${model.mode}">
          </label>
          <button class="btn" data-action="save-model">保存阈值</button>
          <button class="btn secondary" data-action="auto-tune">按复盘自动调权</button>
        </div>
      </section>
    </div>
  `;
}

function externalSourcesHtml() {
  const config = state.externalConfig || { injury_url: "", lineup_url: "", stats_url: "", notes: "" };
  return `
    <section class="panel span-12">
      <h3>外部数据源配置</h3>
      <div class="toolbar">
        <label>伤停 JSON / 代理URL
          <input id="injury-url" value="${config.injury_url || ""}" placeholder="https://...">
        </label>
        <label>首发 JSON / 代理URL
          <input id="lineup-url" value="${config.lineup_url || ""}" placeholder="https://...">
        </label>
        <label>统计/xG JSON / 代理URL
          <input id="stats-url" value="${config.stats_url || ""}" placeholder="https://...">
        </label>
      </div>
      <label>备注
        <input id="source-notes" value="${config.notes || ""}">
      </label>
      <div class="toolbar source-actions">
        <button class="btn" data-action="save-external">保存外部源</button>
        <button class="btn secondary" data-action="refresh-sporttery-injury">刷新竞彩网伤停</button>
        <button class="btn secondary" data-action="refresh-external">刷新外部源缓存</button>
        <button class="btn secondary" data-action="probe-injury">测试伤停源</button>
        <button class="btn secondary" data-action="probe-lineup">测试首发源</button>
        <button class="btn secondary" data-action="probe-stats">测试统计源</button>
      </div>
      <p class="muted">${state.probeResult ? `测试成功：${state.probeResult.bytes} bytes；预览：${state.probeResult.preview}` : "这里不伪造伤停/首发数据。填入稳定免费源或本地代理后，可继续接入模型修正。"}</p>
      <h3>球员状态/预计首发导入</h3>
      <p class="muted">CSV表头支持：team,player,status,position,importance,starter。status 可填 out、doubt、available、starting；position 可填 GK/DF/MF/FW 或中文位置。</p>
      <textarea id="player-status-csv" rows="8" placeholder="team,player,status,position,importance,starter&#10;法国,姆巴佩,starting,FW,2.3,1&#10;法国,主力中卫,out,DF,1.8,0"></textarea>
      <div class="toolbar source-actions">
        <button class="btn" data-action="import-player-status">导入球员状态</button>
      </div>
      <h3>实时球队统计/xG导入</h3>
      <p class="muted">CSV表头支持：team,matches,xg,xga,shots,shots_on_target,box_touches,set_piece_xg。导入后优先替代历史StatsBomb xG。</p>
      <textarea id="team-stats-csv" rows="8" placeholder="team,matches,xg,xga,shots,shots_on_target,box_touches,set_piece_xg&#10;法国,3,5.8,2.1,42,18,86,0.7&#10;瑞典,3,3.2,4.4,28,9,41,0.4"></textarea>
      <div class="toolbar source-actions">
        <button class="btn" data-action="import-team-stats">导入实时统计/xG</button>
      </div>
    </section>
  `;
}

function simHtml() {
  const sim = state.simulation;
  const selectedMatch = state.matches.find(match => match.id === state.selectedSimMatchId) || state.matches[0];
  const selectedHome = selectedMatch?.home || sim?.home || "Argentina";
  const selectedAway = selectedMatch?.away || sim?.away || "France";
  const selectedMeta = selectedMatch ? `${selectedMatch.time || "-"} · ${selectedMatch.match_num || "-"} · ID ${selectedMatch.id}` : "手动输入";
  return `
    <div class="grid">
      <section class="panel span-12 toolbar">
        <label>选择今日/缓存比赛
          <select id="sim-match">
            ${state.matches.length ? state.matches.map((match, index) => `
              <option value="${match.id}" ${(state.selectedSimMatchId ? match.id === state.selectedSimMatchId : index === 0) ? "selected" : ""}>${match.time || "-"} · ${match.match_num || "-"} · ${rankedTeam(match.home)} vs ${rankedTeam(match.away)}</option>
            `).join("") : `<option value="">暂无比赛，请先刷新核心数据</option>`}
          </select>
        </label>
        <button class="btn secondary" data-action="use-sim-match">使用选中比赛</button>
        <label>主队
          <input id="sim-home" value="${rankedTeam(selectedHome)}" ${selectedMatch ? "readonly" : ""}>
        </label>
        <label>客队
          <input id="sim-away" value="${rankedTeam(selectedAway)}" ${selectedMatch ? "readonly" : ""}>
        </label>
        <label>手动λ主，可空
          <input id="sim-home-lambda" type="number" min="0.1" step="0.05">
        </label>
        <label>手动λ客，可空
          <input id="sim-away-lambda" type="number" min="0.1" step="0.05">
        </label>
        <label>模拟场次
          <input id="sim-count" type="number" min="50000" max="500000" step="10000" value="${sim?.simulations || 50000}">
        </label>
        <label class="checkline">
          <input id="sim-knockout" type="checkbox" checked>
          淘汰赛模式
        </label>
        <button class="btn" data-action="simulate">运行模拟</button>
        <div class="badge">当前：${rankedTeam(selectedHome)} vs ${rankedTeam(selectedAway)} · ${selectedMeta}</div>
      </section>
      ${state.simulationProgress ? `
        <section class="panel span-12">
          <div class="progress-head">
            <strong>真实模拟进度</strong>
            <span>${Math.round((state.simulationProgress.percent || 0) * 100)}% · ${state.simulationProgress.done || 0} / ${state.simulationProgress.total || 0} 场</span>
          </div>
          <progress value="${state.simulationProgress.done || 0}" max="${state.simulationProgress.total || 1}"></progress>
          <p class="muted">${state.simulationProgress.message || ""}</p>
        </section>
      ` : ""}
      ${sim ? `
        <section class="panel span-3 metric"><span>主胜</span><strong>${pct(sim.home_win)}</strong><div class="muted">95% ${ciText(sim.home_win_low, sim.home_win_high)} · λ ${sim.lambda_home.toFixed(2)}</div></section>
        <section class="panel span-3 metric"><span>平局</span><strong>${pct(sim.draw)}</strong><div class="muted">95% ${ciText(sim.draw_low, sim.draw_high)}</div></section>
        <section class="panel span-3 metric"><span>客胜</span><strong>${pct(sim.away_win)}</strong><div class="muted">95% ${ciText(sim.away_win_low, sim.away_win_high)} · λ ${sim.lambda_away.toFixed(2)}</div></section>
        <section class="panel span-3 metric"><span>大2.5 / 双方进球</span><strong>${pct(sim.over_25)}</strong><div class="muted">大2.5区间 ${ciText(sim.over_25_low, sim.over_25_high)} · BTTS ${pct(sim.btts)}</div></section>
        <section class="panel span-8">
          <h3>综合概率对比</h3>
          <table>
            <thead>
              <tr>
                <th>方向</th>
                  <th>模型修正后</th>
                  <th>95%区间</th>
                  <th>体彩去水</th>
                  <th>欧洲共识</th>
                  <th>差值</th>
                <th>公平/体彩赔率</th>
              </tr>
            </thead>
            <tbody>
              ${(sim.market_rows || []).map(row => `
                <tr>
                  <td><strong>${row.pick}</strong></td>
                  <td>${pct(row.model_prob)}</td>
                  <td>${ciText(row.ci_low, row.ci_high)}</td>
                  <td>${row.sporttery_prob == null ? "-" : pct(row.sporttery_prob)}</td>
                  <td>${row.europe_prob == null ? "-" : pct(row.europe_prob)}</td>
                  <td>
                    <div>对体彩 ${row.gap_vs_sporttery == null ? "-" : signedPct(row.gap_vs_sporttery)}</div>
                    <div class="muted">对欧洲 ${row.gap_vs_europe == null ? "-" : signedPct(row.gap_vs_europe)}</div>
                  </td>
                  <td>
                    <div>公平 ${odds(row.fair_odds)}</div>
                    <div class="muted">体彩 ${row.sporttery_odds == null ? "-" : odds(row.sporttery_odds)}</div>
                  </td>
                </tr>
              `).join("")}
            </tbody>
          </table>
        </section>
        <section class="panel span-4">
          <h3>修正依据</h3>
          <div class="note-list">
            ${(sim.adjustment_notes || []).map(note => `<p>${note}</p>`).join("")}
          </div>
          <p class="muted">${sim.knockout_note || ""}</p>
          <p class="muted">赔率异动：${sim.movement_note || "暂无"}</p>
        </section>
        <section class="panel span-12">
          <h3>总进球数概率</h3>
          <div class="score-grid compact">
            ${(sim.total_goals || []).map(item => `<div class="score-card"><strong>${item.score}</strong><span>${pct(item.probability)}</span></div>`).join("")}
          </div>
          <p class="muted">来自本次蒙特卡洛真实模拟计数，7球及以上合并为7+球。</p>
        </section>
        <section class="panel span-12">
          <h3>比分 Top</h3>
          <div class="score-grid">
            ${sim.top_scores.map(score => `<div class="score-card"><strong>${score.score}</strong><span>${pct(score.probability)}</span></div>`).join("")}
          </div>
          <p class="muted">${sim.simulation_note || ""}</p>
          <p class="muted">${sim.source_note}</p>
        </section>
      ` : `<section class="panel span-12 muted">输入两队后运行模拟。若已刷新 StatsBomb xG，会自动用历史xG估算λ。</section>`}
    </div>
  `;
}

function viewHtml() {
  if (state.view === "dashboard") {
    return `
      <div class="grid">
        <section class="panel span-12 toolbar">
          <label>The Odds API Key
            <input id="odds-key" type="password" placeholder="已内置，留空即可；填写则覆盖默认key">
          </label>
          <label>欧洲地区
            <select id="odds-region"><option value="eu">EU</option><option value="uk">UK</option><option value="us">US</option><option value="au">AU</option></select>
          </label>
          <button class="btn" data-action="refresh-core">刷新核心数据</button>
          <button class="btn secondary" data-action="refresh-xg">刷新 StatsBomb xG</button>
        </section>
        ${sourceCards()}
        <section class="panel span-12">
          <h3>比赛中心</h3>
          <table><thead><tr><th>编号</th><th>时间</th><th>赛事</th><th>主队</th><th>客队</th><th>状态</th></tr></thead><tbody>${matchRows(40)}</tbody></table>
        </section>
      </div>
    `;
  }
  if (state.view === "prediction") return predictionCenterHtml();
  if (state.view === "match") {
    return `
      <div class="grid">
        <section class="panel span-12 toolbar">
          <button class="btn" data-action="refresh-analysis">刷新单场分析</button>
          <span class="muted">这里显示模型修正后的胜平负、让球胜平负、总进球和比分概率。淘汰赛口径统一为90分钟。</span>
        </section>
        ${analysisCards()}
      </div>
    `;
  }
  if (state.view === "sim") return simHtml();
  if (state.view === "today") return todayPlanHtml();
  if (state.view === "recommend") {
    const buyCount = state.recommendations.filter(item => item.decision === "可买").length;
    const watchCount = state.recommendations.filter(item => item.decision === "观察").length;
    const bankerCount = state.recommendations.filter(item => item.tier === "稳胆" || item.tier === "让球稳胆").length;
    return `
      <div class="grid">
        <section class="panel span-3 metric"><span>可买</span><strong>${buyCount}</strong><div class="muted">满足概率差与期望收益</div></section>
        <section class="panel span-3 metric"><span>观察</span><strong>${watchCount}</strong><div class="muted">只做候选，不强买</div></section>
        <section class="panel span-3 metric"><span>稳胆候选</span><strong>${bankerCount}</strong><div class="muted">优先单关</div></section>
        <section class="panel span-3 metric"><span>推荐口径</span><strong>稳健</strong><div class="muted">压制高赔率幻觉</div></section>
        <section class="panel span-12 toolbar">
          <button class="btn" data-action="refresh-recommend">一键推荐买球</button>
          <button class="btn secondary" data-action="refresh-core">刷新赔率并重算</button>
          <button class="btn secondary" data-action="freeze-rec">冻结赛前快照</button>
          <select id="rec-filter" data-action="filter-rec">
            ${["全部","可买","观察","稳胆","让球稳胆","价值小注","进球数小注","冷门小注","禁止"].map(item => `<option value="${item}" ${state.recFilter === item ? "selected" : ""}>${item}</option>`).join("")}
          </select>
          <span class="muted">建议优先单关/小额分散；串关只适合把“可买”里相关性低的 2 场做极小仓位。</span>
        </section>
        ${purchasePlanHtml()}
        <section class="panel span-12 table-panel">
          <h3>推荐榜</h3>
          <div class="scroll-table">
            <table><thead><tr><th>时间</th><th>比赛</th><th>玩法</th><th>方向</th><th>推荐等级</th><th>模型概率</th><th>体彩去水</th><th>欧洲概率</th><th>对体彩差</th><th>对欧洲差</th><th>公平赔率</th><th>当前赔率</th><th>欧洲均赔</th><th>EV</th><th>优势率</th><th>数据</th><th>数据建议</th><th>仓位</th><th>是否值得投注</th><th>组合组</th><th>操作</th></tr></thead><tbody>${recommendationRows(100)}</tbody></table>
          </div>
        </section>
      </div>
    `;
  }
  if (state.view === "movements") {
    return `
      <section class="panel span-12 toolbar">
        <button class="btn" data-action="refresh-core">刷新赔率并记录快照</button>
        <span class="muted">至少刷新两次后才有“上次赔率”可比较。</span>
      </section>
      <section class="panel table-panel">
        <h3>赔率异常类型</h3>
        <p class="muted">基于快照变化自动识别热门过热、机构分歧、临场降赔、反向升赔、诱盘风险和剧烈波动。</p>
        <div class="scroll-table">
          <table><thead><tr><th>时间</th><th>比赛</th><th>玩法</th><th>方向</th><th>异常类型</th><th>严重度</th><th>影响方向</th><th>处理建议</th><th>变化</th></tr></thead><tbody>${anomalyRows()}</tbody></table>
        </div>
      </section>
      <section class="panel table-panel">
        <h3>异常异动记录</h3>
        <p class="muted">只显示达到阈值的升赔/降赔。当前阈值：绝对变化不小于 0.01 且比例变化不小于 0.1%。</p>
        <div class="scroll-table">
          <table><thead><tr><th>时间</th><th>比赛</th><th>玩法</th><th>方向</th><th>初盘</th><th>上次</th><th>当前</th><th>走势</th><th>变化</th></tr></thead><tbody>${movementRows(state.movements, "暂无异常异动。下面可查看完整赔率变化记录。")}</tbody></table>
        </div>
      </section>
      <section class="panel table-panel">
        <h3>完整赔率变化记录</h3>
        <p class="muted">包含持平记录，用来确认快照确实写入。按最新快照倒序排列。</p>
        <div class="scroll-table">
          <table><thead><tr><th>时间</th><th>比赛</th><th>玩法</th><th>方向</th><th>初盘</th><th>上次</th><th>当前</th><th>走势</th><th>变化</th></tr></thead><tbody>${movementRows(state.oddsHistory)}</tbody></table>
        </div>
      </section>
    `;
  }
  if (state.view === "results") {
    return `
      <section class="panel span-12 toolbar">
        <button class="btn" data-action="refresh-results">刷新赛果</button>
        <button class="btn secondary" data-action="auto-settle">按赛果自动结算复盘</button>
        <span class="muted">赛果源：足彩网世界杯赛程页；若页面结构变化，仍可手动结算。</span>
      </section>
      <section class="panel table-panel">
        <h3>世界杯赛果</h3>
        <div class="scroll-table">
          <table><thead><tr><th>阶段</th><th>状态</th><th>主队</th><th>比分</th><th>客队</th><th>半场</th></tr></thead><tbody>${resultRows()}</tbody></table>
        </div>
      </section>
      <section class="panel span-12">
        <h3>历史赛果样本导入</h3>
        <p class="muted">CSV表头支持：home,away,score,stage,status,half_score。导入后会参与动态Elo和后续模型校准。</p>
        <textarea id="historical-csv" rows="8" placeholder="home,away,score,stage,status,half_score&#10;法国,德国,2:1,世界杯,完场,1:0"></textarea>
        <div class="actions"><button class="btn" data-action="import-historical">导入历史样本</button></div>
      </section>
    `;
  }
  if (state.view === "review") {
    return `
      <section class="panel span-12 toolbar">
        <button class="btn" data-action="refresh-results">刷新赛果</button>
        <button class="btn secondary" data-action="auto-settle">自动结算复盘</button>
        <span class="muted">支持胜平负、让球胜平负、总进球、比分。未匹配到赛果的记录会保留未结算。</span>
      </section>
      <section class="panel span-12">
        <h3>增强回测结论</h3>
        <div class="plan-grid">
          <div><h4>最赚钱模块</h4><p class="muted">${state.backtest?.most_profitable || "样本不足"}</p></div>
          <div><h4>最亏钱模块</h4><p class="muted">${state.backtest?.most_loss || "样本不足"}</p></div>
          <div><h4>建议禁买规则</h4><p class="muted">${state.backtest?.ban_rule_advice || "样本不足"}</p></div>
        </div>
      </section>
      <section class="panel table-panel">
        <h3>历史回测分组</h3>
        <div class="scroll-table">
          <table><thead><tr><th>维度</th><th>分组</th><th>次数</th><th>命中率</th><th>ROI</th><th>总盈利</th><th>最大回撤</th><th>平均赔率</th><th>平均优势率</th><th>Brier</th><th>LogLoss</th></tr></thead><tbody>${backtestRows()}</tbody></table>
        </div>
      </section>
      <section class="panel table-panel"><h3>复盘中心</h3><table><thead><tr><th>时间</th><th>比赛</th><th>玩法</th><th>选择</th><th>模型概率</th><th>赔率</th><th>概率差</th><th>仓位</th><th>决策</th><th>赛果</th><th>盈亏</th><th>操作</th></tr></thead><tbody>${predictionRows()}</tbody></table></section>
    `;
  }
  if (state.view === "bankroll") {
    return bankrollHtml();
  }
  return `
    <div class="grid">
      ${sourceCards()}
      ${sourceHealthHtml()}
      ${externalSourcesHtml()}
      <section class="panel span-12">
        <h3>本地数据库</h3>
        <p class="muted">${state.status?.dbPath || "未初始化"}</p>
      </section>
    </div>
  `;
}

function render() {
  document.querySelector("#app").innerHTML = `
    <div class="app">
      <aside class="sidebar">
        <div class="brand">
          <h1>世界杯盈利模型</h1>
          <p>本地缓存 · 模拟对决 · 安全边际</p>
        </div>
        <nav class="nav">
          ${views.map(([id, label]) => `<button class="${state.view === id ? "active" : ""}" data-view="${id}">${label}</button>`).join("")}
        </nav>
        <div class="muted">${state.busy ? "正在处理..." : state.message}</div>
      </aside>
      <main class="content">
        <div class="topbar">
          <div>
            <h2>${views.find(([id]) => id === state.view)?.[1]}</h2>
            <p>${state.busy ? "后台任务运行中，界面仍可浏览。" : "本地缓存、稳健推荐、异动记录、模拟对决。"}</p>
          </div>
          <div class="actions">
            <button class="btn ghost" data-action="reload">刷新状态</button>
          </div>
        </div>
        ${viewHtml()}
      </main>
    </div>
  `;
}

document.addEventListener("click", event => {
  const view = event.target?.dataset?.view;
  const action = event.target?.dataset?.action;
  if (view) {
    state.view = view;
    render();
  }
  if (action === "reload") safeRun("刷新状态", loadStatus);
  if (action === "refresh-core") safeRun("刷新核心数据", refreshCore);
  if (action === "refresh-xg") safeRun("刷新 StatsBomb xG", refreshXg);
  if (action === "refresh-results") safeRun("刷新赛果", async () => {
    await refreshResults();
    state.diagnostics = await invoke("model_diagnostics");
    state.backtest = await invoke("backtest_report");
  });
  if (action === "import-historical") safeRun("导入历史样本", importHistoricalResults);
  if (action === "import-player-status") safeRun("导入球员状态", importPlayerStatus);
  if (action === "import-team-stats") safeRun("导入实时统计/xG", importTeamStats);
  if (action === "use-sim-match") {
    const selectedId = document.querySelector("#sim-match")?.value || "";
    const selectedMatch = state.matches.find(match => match.id === selectedId);
    if (selectedMatch) {
      state.selectedSimMatchId = selectedId;
      state.simulation = null;
      state.simulationProgress = null;
      render();
    }
  }
  if (action === "auto-settle") safeRun("自动结算", async () => {
    await autoSettle();
    state.backtest = await invoke("backtest_report");
  });
  if (action === "simulate") safeRun("运行模拟", runSimulation);
  if (action === "refresh-recommend") safeRun("生成推荐", async () => {
    state.recommendations = await invoke("list_recommendations");
    state.todayPlan = await invoke("today_bet_plan");
  });
  if (action === "refresh-analysis") safeRun("刷新单场分析", async () => {
    state.analyses = await invoke("list_match_analyses");
  });
  if (action === "save-rec") safeRun("保存复盘", async () => saveRecommendation(event.target.dataset.index));
  if (action === "freeze-rec") safeRun("冻结赛前快照", freezeRecommendations);
  if (action === "delete-prediction") safeRun("删除复盘", async () => deletePrediction(event.target.dataset.id));
  if (action === "settle-hit") safeRun("结算命中", async () => settlePrediction(event.target.dataset.id, true));
  if (action === "settle-miss") safeRun("结算未中", async () => settlePrediction(event.target.dataset.id, false));
  if (action === "save-bankroll") safeRun("保存资金设置", saveBankroll);
  if (action === "save-external") safeRun("保存外部源", saveExternalConfig);
  if (action === "refresh-sporttery-injury") safeRun("刷新竞彩网伤停", refreshSportteryInjuries);
  if (action === "refresh-external") safeRun("刷新外部源", refreshExternalSources);
  if (action === "probe-injury") safeRun("测试伤停源", async () => probeExternal(document.querySelector("#injury-url")?.value || ""));
  if (action === "probe-lineup") safeRun("测试首发源", async () => probeExternal(document.querySelector("#lineup-url")?.value || ""));
  if (action === "probe-stats") safeRun("测试统计源", async () => probeExternal(document.querySelector("#stats-url")?.value || ""));
  if (action === "save-model") safeRun("保存模型阈值", saveModelSettings);
  if (action === "auto-tune") safeRun("自动调权", autoTuneModel);
});

document.addEventListener("change", event => {
  if (event.target?.id === "rec-filter") {
    state.recFilter = event.target.value;
    render();
  }
  if (event.target?.id === "sim-match") {
    const selectedMatch = state.matches.find(match => match.id === event.target.value);
    if (selectedMatch) {
      state.selectedSimMatchId = event.target.value;
      state.simulation = null;
      state.simulationProgress = null;
      render();
    }
  }
});

function setupAutoRefresh() {
  if (state.autoRefreshTimer) {
    clearInterval(state.autoRefreshTimer);
    state.autoRefreshTimer = null;
  }
  const minutes = Number(state.bankroll?.auto_refresh_minutes || 0);
  if (minutes > 0) {
    state.autoRefreshTimer = setInterval(() => {
      refreshCore().catch(() => {});
    }, minutes * 60 * 1000);
  }
}

listen("simulation-progress", event => {
  state.simulationProgress = event.payload;
  if (state.view === "sim") {
    render();
  }
}).catch(() => {});

safeRun("初始化", async () => {
  await loadStatus();
  setupAutoRefresh();
});
