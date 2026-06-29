import { api } from "./api.js";
import { listen } from "@tauri-apps/api/event";
import "./styles.css";

import { state } from "./state.js";
import { pct, odds, signedPct, money, ciText, rankedTeam, rankedMatchLabel } from "./utils/format.js";
const views = [
  ["dashboard", "比赛中心"],
  ["prediction", "预测中心"],
  ["match", "单场分析"],
  ["sim", "模拟对决"],
  ["today", "今日方案"],
  ["snapshots", "赛前快照"],
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
  state.status = await api.invokeCommand("app_status");
  state.providers = state.status?.providers || await api.invokeCommand("list_providers");
  state.matches = await api.invokeCommand("list_matches");
  if (!state.selectedSimMatchId && state.matches.length) {
    state.selectedSimMatchId = state.matches[0].id;
  }
  state.predictions = await api.invokeCommand("list_predictions");
  state.movements = await api.invokeCommand("list_odds_movements");
  state.anomalies = await api.invokeCommand("list_odds_anomalies");
  state.oddsHistory = await api.invokeCommand("list_odds_history");
  state.results = await api.invokeCommand("list_results");
  state.bankroll = await api.invokeCommand("get_bankroll_settings");
  state.externalConfig = await api.invokeCommand("get_external_source_config");
  state.modelSettings = await api.invokeCommand("get_model_settings");
  state.diagnostics = await api.invokeCommand("model_diagnostics");
  state.activeModel = await api.invokeCommand("get_active_model_info");
  state.systemStatus = await api.invokeCommand("get_system_status");
  state.backtest = await api.invokeCommand("backtest_report");
  try {
    state.recommendations = await api.invokeCommand("list_recommendations");
    state.analyses = await api.invokeCommand("list_match_analyses");
    state.todayPlan = await api.invokeCommand("today_bet_plan");
    state.preMatchSnapshots = await api.invokeCommand("get_pre_match_snapshots");
    state.snapshotAuditLogs = await api.invokeCommand("get_snapshot_audit_logs");
    state.livePaperSummary = await api.invokeCommand("get_live_paper_trading_summary");
    state.livePaperRecords = await api.invokeCommand("get_live_paper_trading_records");
    state.systemStatus = await api.invokeCommand("get_system_status");
  } catch {
    state.recommendations = [];
    state.analyses = [];
    state.todayPlan = null;
    state.preMatchSnapshots = [];
    state.snapshotAuditLogs = [];
    state.livePaperSummary = null;
    state.livePaperRecords = [];
  }
}

async function refreshCore() {
  await api.invokeCommand("refresh_core_data", {
    oddsApiKey: document.querySelector("#odds-key")?.value || "",
    region: document.querySelector("#odds-region")?.value || "eu"
  });
  await loadStatus();
}

async function refreshXg() {
  await api.invokeCommand("refresh_statsbomb_xg");
  await loadStatus();
}

async function refreshSportteryInjuries() {
  state.probeResult = await api.invokeCommand("refresh_sporttery_injuries");
  state.status = await api.invokeCommand("app_status");
}

async function refreshResults() {
  state.results = await api.invokeCommand("refresh_results");
}

async function importHistoricalResults() {
  const csvText = document.querySelector("#historical-csv")?.value || "";
  state.probeResult = await api.invokeCommand("import_historical_results_csv", { csvText });
  await loadStatus();
}

async function importPlayerStatus() {
  const csvText = document.querySelector("#player-status-csv")?.value || "";
  state.probeResult = await api.invokeCommand("import_player_status_csv", { csvText });
  await loadStatus();
}

async function importTeamStats() {
  const csvText = document.querySelector("#team-stats-csv")?.value || "";
  state.probeResult = await api.invokeCommand("import_team_stats_csv", { csvText });
  await loadStatus();
}

async function autoSettle() {
  await api.invokeCommand("auto_settle_predictions");
  state.predictions = await api.invokeCommand("list_predictions");
  state.diagnostics = await api.invokeCommand("model_diagnostics");
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
  state.simulation = await api.invokeCommand("simulate_match", { request });
  state.simulationProgress = { done: request.simulations, total: request.simulations, percent: 1, message: "模拟完成" };
}

async function saveRecommendation(index) {
  const item = state.recommendations[index];
  if (!item) return;
  await api.invokeCommand("save_prediction", {
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
  state.predictions = await api.invokeCommand("list_predictions");
  state.diagnostics = await api.invokeCommand("model_diagnostics");
}

async function deletePrediction(id) {
  await api.invokeCommand("delete_prediction", { id: Number(id) });
  state.predictions = await api.invokeCommand("list_predictions");
  state.diagnostics = await api.invokeCommand("model_diagnostics");
}

async function settlePrediction(id, hit) {
  await api.invokeCommand("settle_prediction", { id: Number(id), hit });
  state.predictions = await api.invokeCommand("list_predictions");
  state.diagnostics = await api.invokeCommand("model_diagnostics");
  state.backtest = await api.invokeCommand("backtest_report");
}

async function saveBankroll() {
  const settings = {
    bankroll: Number(document.querySelector("#bankroll")?.value || 1000),
    daily_budget_pct: Number(document.querySelector("#daily-budget")?.value || 3) / 100,
    max_loss_pct: Number(document.querySelector("#max-loss")?.value || 6) / 100,
    auto_refresh_minutes: Number(document.querySelector("#auto-refresh")?.value || 0)
  };
  await api.invokeCommand("save_bankroll_settings", { settings });
  state.bankroll = await api.invokeCommand("get_bankroll_settings");
  setupAutoRefresh();
}

async function saveExternalConfig() {
  const config = {
    injury_url: document.querySelector("#injury-url")?.value || "",
    lineup_url: document.querySelector("#lineup-url")?.value || "",
    stats_url: document.querySelector("#stats-url")?.value || "",
    notes: document.querySelector("#source-notes")?.value || ""
  };
  await api.invokeCommand("save_external_source_config", { config });
  state.externalConfig = await api.invokeCommand("get_external_source_config");
}

async function saveProviderCredential(providerId) {
  const apiKey = document.querySelector(`#provider-key-${providerId}`)?.value || "";
  state.probeResult = await api.invokeCommand("save_provider_credential", {
    input: { provider_id: providerId, api_key: apiKey }
  });
  state.providers = await api.invokeCommand("list_providers");
}

async function clearProviderCredential(providerId) {
  state.probeResult = await api.invokeCommand("clear_provider_credential", { providerId });
  state.providers = await api.invokeCommand("list_providers");
}

async function testProvider(providerId) {
  state.probeResult = await api.invokeCommand("test_provider_connection", { providerId });
  state.providers = await api.invokeCommand("list_providers");
  state.status = await api.invokeCommand("app_status");
}

async function toggleProvider(providerId, enabled) {
  await api.invokeCommand("set_provider_enabled", { providerId, enabled });
  state.providers = await api.invokeCommand("list_providers");
}

async function clearProviderCache(providerId) {
  state.probeResult = await api.invokeCommand("clear_provider_cache", { providerId });
  state.providers = await api.invokeCommand("list_providers");
  state.status = await api.invokeCommand("app_status");
}

async function refreshExternalSources() {
  state.probeResult = await api.invokeCommand("refresh_all_data_sources");
  state.status = await api.invokeCommand("app_status");
  state.providers = state.status?.providers || await api.invokeCommand("list_providers");
  state.activeModel = await api.invokeCommand("get_active_model_info");
}

async function runTrainingPipeline() {
  state.probeResult = await api.invokeCommand("run_training_pipeline");
  state.activeModel = await api.invokeCommand("get_active_model_info");
}

async function probeExternal(url) {
  state.probeResult = await api.invokeCommand("probe_external_source", { url });
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
  await api.invokeCommand("save_model_settings", { settings });
  state.modelSettings = await api.invokeCommand("get_model_settings");
  state.recommendations = await api.invokeCommand("list_recommendations");
}

async function autoTuneModel() {
  state.modelSettings = await api.invokeCommand("auto_tune_model");
  state.recommendations = await api.invokeCommand("list_recommendations");
}

async function freezeRecommendations() {
  state.probeResult = await api.invokeCommand("freeze_current_recommendations");
  state.backtest = await api.invokeCommand("backtest_report");
}

async function collectWorldcupSnapshot() {
  state.probeResult = await api.invokeCommand("collect_worldcup_pre_match_snapshot");
  await loadStatus();
}

async function createPreMatchSnapshot(matchId) {
  state.probeResult = await api.invokeCommand("create_pre_match_snapshot", { matchId });
  state.preMatchSnapshots = await api.invokeCommand("get_pre_match_snapshots");
}

async function createTodayPreMatchSnapshots() {
  state.probeResult = await api.invokeCommand("create_today_pre_match_snapshots");
  state.preMatchSnapshots = await api.invokeCommand("get_pre_match_snapshots");
  state.livePaperSummary = await api.invokeCommand("get_live_paper_trading_summary");
}

async function markFinalPreMatchSnapshot(snapshotId) {
  state.probeResult = await api.invokeCommand("mark_final_pre_match_snapshot", { snapshotId: Number(snapshotId) });
  state.preMatchSnapshots = await api.invokeCommand("get_pre_match_snapshots");
  state.snapshotAuditLogs = await api.invokeCommand("get_snapshot_audit_logs");
  state.livePaperSummary = await api.invokeCommand("get_live_paper_trading_summary");
}

async function settlePreMatchSnapshot(snapshotId) {
  const score = prompt("输入赛果比分，例如 2:1");
  if (!score) return;
  const parts = score.split(":");
  if (parts.length !== 2) throw new Error("比分格式应为 2:1");
  state.probeResult = await api.invokeCommand("settle_pre_match_snapshot", {
    snapshotId: Number(snapshotId),
    homeScore: Number(parts[0]),
    awayScore: Number(parts[1])
  });
  state.preMatchSnapshots = await api.invokeCommand("get_pre_match_snapshots");
  state.livePaperSummary = await api.invokeCommand("get_live_paper_trading_summary");
  state.livePaperRecords = await api.invokeCommand("get_live_paper_trading_records");
}

async function settleAllFinishedSnapshots() {
  state.probeResult = await api.invokeCommand("settle_all_finished_snapshots");
  state.preMatchSnapshots = await api.invokeCommand("get_pre_match_snapshots");
  state.livePaperSummary = await api.invokeCommand("get_live_paper_trading_summary");
  state.livePaperRecords = await api.invokeCommand("get_live_paper_trading_records");
}

async function auditPreMatchSnapshots() {
  state.probeResult = await api.invokeCommand("audit_pre_match_snapshots");
  state.snapshotAuditLogs = await api.invokeCommand("get_snapshot_audit_logs");
  state.livePaperSummary = await api.invokeCommand("get_live_paper_trading_summary");
  state.systemStatus = await api.invokeCommand("get_system_status");
}

async function exportAppData() {
  state.probeResult = await api.invokeCommand("export_app_data");
  state.systemStatus = await api.invokeCommand("get_system_status");
}

async function exportSnapshots() {
  state.probeResult = await api.invokeCommand("export_snapshots");
}

async function exportLivePaperTrading() {
  state.probeResult = await api.invokeCommand("export_live_paper_trading");
}

async function exportAuditLogs() {
  state.probeResult = await api.invokeCommand("export_audit_logs");
}

async function exportSnapshotResults() {
  state.probeResult = await api.invokeCommand("export_snapshot_results");
}

async function exportStrategyDiagnostics() {
  state.probeResult = await api.invokeCommand("export_strategy_diagnostics");
}

async function openBackupDir() {
  state.probeResult = await api.invokeCommand("open_backup_dir");
}

async function settleBetRecommendations() {
  state.probeResult = await api.invokeCommand("settle_bet_recommendations");
  state.diagnostics = await api.invokeCommand("model_diagnostics");
  state.backtest = await api.invokeCommand("backtest_report");
}

async function exportWorldcupSamples() {
  state.probeResult = await api.invokeCommand("export_worldcup_training_samples");
}

async function runWorldcupClosureCycle() {
  state.probeResult = await api.invokeCommand("run_worldcup_closure_cycle");
  await loadStatus();
}

function numberOrNull(value) {
  const num = Number(value);
  return Number.isFinite(num) && num > 0 ? num : null;
}

function fileSize(bytes) {
  const value = Number(bytes || 0);
  if (value < 1024) return `${value} B`;
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KB`;
  if (value < 1024 * 1024 * 1024) return `${(value / 1024 / 1024).toFixed(1)} MB`;
  return `${(value / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

function sourceCards() {
  const sources = state.status?.sources || [];
  return sources.map(source => `
    <div class="panel span-4 metric">
      <span>${source.label}</span>
      <strong>${source.health_label || (source.ok ? "正常" : "缺失")}</strong>
      <div class="muted">${source.updated_at || source.message}</div>
      <div class="muted">置信 ${Number(source.confidence_score || 0).toFixed(0)} · 新鲜 ${Number(source.freshness_score || 0).toFixed(0)} · 完整 ${Number(source.completeness_score || 0).toFixed(0)}</div>
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
    return `<tr><td colspan="23" class="muted">暂无推荐。先在比赛中心点击“刷新核心数据”。</td></tr>`;
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
      <td>${item.worldcup_correction_action || "-"}</td>
      <td>${money(item.stake_pct)}<div class="muted">${state.bankroll ? Math.round(item.stake_pct * state.bankroll.bankroll) : 0}</div></td>
      <td>${badge(item.final_decision || item.decision)}<div class="muted">${badge(item.decision)} <span class="badge">${item.confidence}</span></div><div class="muted reason">${item.action_advice || item.play_style}</div><div class="muted reason">风险等级：${item.play_type_risk_level || "-"}</div><div class="muted reason">赔率异常：${item.anomaly_type || "-"} ${item.anomaly_severity || ""} ${item.anomaly_advice || ""}</div><div class="muted reason">${item.reason}</div><div class="muted reason">支持：${item.support_factors || "-"}</div><div class="muted reason">风险：${item.risk_factors || "-"}</div></td>
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

function paperTradingSummaryHtml(source = state.backtest?.paper_trading) {
  const paper = source || {};
  const robustness = state.backtest?.strategy_robustness || {};
  const upgrade = paper.candidate_upgrade_check || {};
  const blocking = upgrade.blocking_reasons || [];
  const robustBlocking = robustness.blocking_reasons || [];
  const outlier = robustness.outlier_sensitivity || {};
  const timeSegments = robustness.time_segments || [];
  return `
    <section class="panel span-12">
      <h3>策略观察 / 纸面交易</h3>
      <p class="muted">该策略仅用于模拟观察，不构成正式推荐。未达到样本和 ROI 要求前，不进入真实推荐。</p>
      <div class="plan-grid">
        <div><h4>候选策略</h4><p class="muted">${paper.strategy_id || "candidate_strategy_v1"}</p></div>
        <div><h4>状态</h4><p class="muted">${paper.status === "observation_only" ? "观察中" : (paper.status || "未生成")}</p></div>
        <div><h4>纸面样本</h4><p class="muted">${paper.bet_count || 0}</p></div>
        <div><h4>纸面命中率</h4><p class="muted">${pct(paper.hit_rate || 0)}</p></div>
      </div>
      <div class="plan-grid">
        <div><h4>纸面 ROI</h4><p class="muted">${signedPct(paper.paper_roi || 0)}</p></div>
        <div><h4>最大回撤</h4><p class="muted">${Number(paper.max_drawdown || 0).toFixed(2)}</p></div>
        <div><h4>平均赔率</h4><p class="muted">${paper.avg_odds ? odds(paper.avg_odds) : "-"}</p></div>
        <div><h4>平均 EV</h4><p class="muted">${signedPct(paper.avg_ev || 0)}</p></div>
      </div>
      <div class="muted">升级检查：${upgrade.can_consider_upgrade ? "可考虑升级为小注，但不会自动启用" : "暂不可升级"}。${blocking.length ? `阻塞原因：${blocking.join("；")}` : (upgrade.upgrade_reason || "")}</div>
      <div class="muted">风险提示：${paper.warning || "继续观察，不能自动改正式推荐规则。"}</div>
      <h4>稳健性检验</h4>
      <p class="muted">该策略当前仍处于观察状态。稳健性不足时，不应升级为真实推荐。</p>
      <div class="plan-grid">
        <div><h4>稳健性等级</h4><p class="muted">${robustness.robustness_level || "未生成"}</p></div>
        <div><h4>最近30笔 ROI</h4><p class="muted">${robustness.rolling_30_roi == null ? "-" : signedPct(robustness.rolling_30_roi)}</p></div>
        <div><h4>最近50笔 ROI</h4><p class="muted">${robustness.rolling_50_roi == null ? "-" : signedPct(robustness.rolling_50_roi)}</p></div>
        <div><h4>最近100笔 ROI</h4><p class="muted">${robustness.rolling_100_roi == null ? "-" : signedPct(robustness.rolling_100_roi)}</p></div>
      </div>
      <div class="plan-grid">
        <div><h4>时间稳定性</h4><p class="muted">${robustness.time_stability || "-"}</p></div>
        <div><h4>赔率区间依赖</h4><p class="muted">${(robustness.blocking_reasons || []).includes("depends_on_single_odds_band") ? "依赖单一区间" : "未见单一区间依赖"}</p></div>
        <div><h4>方向依赖</h4><p class="muted">${(robustness.blocking_reasons || []).includes("depends_on_single_selection") ? "依赖单一方向" : "未见单一方向依赖"}</p></div>
        <div><h4>极端样本</h4><p class="muted">去Top5后 ${outlier.roi_after_remove_top5 == null ? "-" : signedPct(outlier.roi_after_remove_top5)}</p></div>
      </div>
      <div class="muted">稳健阻塞：${robustBlocking.length ? robustBlocking.join("；") : "暂无阻塞。"}</div>
      <div class="muted">分段 ROI：${timeSegments.length ? timeSegments.map(row => `${row.group}:${signedPct(row.roi || 0)}`).join("；") : "暂无分段数据。"}</div>
    </section>
  `;
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
      <section class="panel span-12 table-panel"><h3>正式推荐</h3><p class="muted">${plan.singles?.length ? "通过当前风控的正式投注样本。" : "当前无通过风控的正式投注样本。"}</p><div class="scroll-table"><table><thead><tr><th>时间</th><th>比赛</th><th>玩法</th><th>方向</th><th>模型</th><th>赔率/EV</th><th>操作建议</th></tr></thead><tbody>${compactPickRows(plan.singles, "今日暂无通过风控的正式推荐。")}</tbody></table></div></section>
      <section class="panel span-12 table-panel"><h3>小注候选</h3><p class="muted">小注候选仍受风控约束，不等于自动下注。</p><div class="scroll-table"><table><thead><tr><th>时间</th><th>比赛</th><th>玩法</th><th>方向</th><th>模型</th><th>赔率/EV</th><th>操作建议</th></tr></thead><tbody>${compactPickRows(plan.small_stake, "暂无小注候选。")}</tbody></table></div></section>
      <section class="panel span-12 table-panel"><h3>策略观察</h3><p class="muted">模拟，不建议真实下注。</p><div class="scroll-table"><table><thead><tr><th>时间</th><th>比赛</th><th>玩法</th><th>方向</th><th>模型</th><th>赔率/EV</th><th>操作建议</th></tr></thead><tbody>${compactPickRows(plan.strategy_observation, "暂无策略观察候选。")}</tbody></table></div></section>
      ${paperTradingSummaryHtml()}
      <section class="panel span-12 table-panel"><h3>二串一候选</h3><p class="muted">只从稳胆/让球稳胆里挑不同场次，总额不超过今日预算20%。</p><div class="scroll-table"><table><thead><tr><th>时间</th><th>比赛</th><th>玩法</th><th>方向</th><th>模型</th><th>赔率/EV</th><th>操作建议</th></tr></thead><tbody>${compactPickRows(plan.combos, "暂无合格二串一候选。")}</tbody></table></div></section>
      <section class="panel span-6 table-panel"><h3>观察清单</h3><div class="scroll-table"><table><thead><tr><th>时间</th><th>比赛</th><th>玩法</th><th>方向</th><th>模型</th><th>赔率/EV</th><th>操作建议</th></tr></thead><tbody>${compactPickRows(plan.watch, "暂无观察项。")}</tbody></table></div></section>
      <section class="panel span-6 table-panel"><h3>禁买清单</h3><div class="scroll-table"><table><thead><tr><th>时间</th><th>比赛</th><th>玩法</th><th>方向</th><th>模型</th><th>赔率/EV</th><th>操作建议</th></tr></thead><tbody>${compactPickRows(plan.banned, "暂无禁买项。")}</tbody></table></div></section>
    </div>
  `;
}

function snapshotForMatch(matchId) {
  return (state.preMatchSnapshots || []).find(item => item.match_id === matchId && item.is_final_pre_match)
    || (state.preMatchSnapshots || []).find(item => item.match_id === matchId);
}

function snapshotRows() {
  const rows = state.matches || [];
  if (!rows.length) {
    return `<tr><td colspan="13" class="muted">暂无比赛。先刷新核心数据。</td></tr>`;
  }
  return rows.map(match => {
    const snap = snapshotForMatch(match.id);
    const model = snap?.model_probs_json?.[0] || {};
    const oddsRow = snap?.odds_json?.[0] || {};
    const evRow = snap?.ev_json?.[0] || {};
    return `
      <tr>
        <td>${match.match_num || "-"}</td>
        <td>${match.time || "-"}</td>
        <td><strong>${rankedTeam(match.home)}</strong> vs <strong>${rankedTeam(match.away)}</strong></td>
        <td>${snap ? snap.snapshot_time : "未生成"}</td>
        <td>${snap?.is_final_pre_match ? badge("最终") : badge("普通")}</td>
        <td>${model.pick ? `${model.pick} ${pct(model.model_prob || 0)}` : "-"}</td>
        <td>${oddsRow.odds ? odds(oddsRow.odds) : "-"}</td>
        <td>${evRow.ev == null ? "-" : signedPct(evRow.ev)}</td>
        <td>${snap ? `${Number(snap.data_quality_score || 0).toFixed(0)}分` : "-"}</td>
        <td>${snap ? `${snap.lineup_status} / ${Number(snap.lineup_confidence || 0).toFixed(0)}` : "-"}</td>
        <td>${snap ? `${snap.injury_status} / ${Number(snap.injury_confidence || 0).toFixed(0)}` : "-"}</td>
        <td>${snap ? badge(snap.final_decision) : "-"}</td>
        <td>
          <button class="mini" data-action="create-pre-snapshot" data-match-id="${match.id}">生成当前快照</button>
          ${snap ? `<button class="mini" data-action="mark-final-snapshot" data-snapshot-id="${snap.id}">标记最终</button>` : ""}
          ${snap ? `<button class="mini" data-action="settle-pre-snapshot" data-snapshot-id="${snap.id}">赛后结算</button>` : ""}
        </td>
      </tr>
    `;
  }).join("");
}

function auditRows() {
  const rows = state.snapshotAuditLogs || [];
  if (!rows.length) {
    return `<tr><td colspan="7" class="muted">暂无审计问题。点击“运行快照审计”检查当前快照。</td></tr>`;
  }
  return rows.slice(0, 80).map(item => `
    <tr>
      <td>${rankedMatchLabel(item.match_label || item.match_id)}</td>
      <td>${item.snapshot_time || "-"}</td>
      <td>${item.kickoff_time || "-"}</td>
      <td>${item.audit_type}</td>
      <td>${badge(item.severity)}</td>
      <td>${item.message}</td>
      <td>${item.resolved ? "已解决" : "未解决"}</td>
    </tr>
  `).join("");
}

function livePaperRows() {
  const rows = state.livePaperRecords || [];
  if (!rows.length) {
    return `<tr><td colspan="9" class="muted">暂无 live_pre_match 纸面交易记录。</td></tr>`;
  }
  return rows.slice(0, 10).map(item => `
    <tr>
      <td>${item.created_at || "-"}</td>
      <td>${item.match_id || "-"}</td>
      <td>${item.selection}</td>
      <td>${item.play_type}</td>
      <td>${pct(item.model_prob || 0)}</td>
      <td>${odds(item.odds || 0)}</td>
      <td>${signedPct(item.ev || 0)}</td>
      <td>${item.result_status || "pending"}</td>
      <td class="${(item.paper_profit || 0) >= 0 ? "down" : "up"}">${signedPct(item.paper_profit || 0)}</td>
    </tr>
  `).join("");
}

function systemStatusHtml() {
  const status = state.systemStatus || {};
  const lastBackup = status.last_backup || {};
  return `
    <section class="panel span-12">
      <h3>系统状态</h3>
      <div class="plan-grid">
        <div><h4>应用版本</h4><p class="muted">${status.app_version || "v0.1-live-observation"}</p></div>
        <div><h4>当前模型</h4><p class="muted">${status.model_version || state.activeModel?.model_version || "-"}</p></div>
        <div><h4>世界杯修正层</h4><p class="muted">${status.worldcup_correction_version || state.activeModel?.worldcup_correction_version || "-"}</p></div>
        <div><h4>策略状态</h4><p class="muted">${status.strategy_status || "observation_only"} · ${status.official_recommendation_status || "风控开启"}</p></div>
      </div>
      <div class="plan-grid">
        <div><h4>数据库</h4><p class="muted">${fileSize(status.db_size_bytes || 0)}</p></div>
        <div><h4>快照</h4><p class="muted">${status.snapshot_count || 0} 条 · final ${status.final_snapshot_count || 0}</p></div>
        <div><h4>live 样本</h4><p class="muted">${status.live_pre_match_sample_count || 0} 条 · 已结算 ${status.live_pre_match_settled_count || 0}</p></div>
        <div><h4>审计</h4><p class="muted">Critical ${status.audit_critical_count || 0} · Warning ${status.audit_warning_count || 0}</p></div>
      </div>
      <div class="plan-grid">
        <div><h4>API-Football</h4><p class="muted">今日 ${status.api_football_today_requests || 0} / ${status.api_football_daily_limit || 100} · 剩余 ${status.api_football_remaining_requests ?? "-"}</p></div>
        <div><h4>最近备份</h4><p class="muted">${lastBackup.created_at || "尚未备份"}</p></div>
        <div><h4>未结算</h4><p class="muted">${status.live_pre_match_unsettled_count || 0} 条</p></div>
        <div><h4>数据库路径</h4><p class="muted">${status.db_path || "-"}</p></div>
      </div>
    </section>
  `;
}

function snapshotWorkflowHtml() {
  return `
    <section class="panel span-12">
      <h3>日常使用流程</h3>
      <div class="plan-grid">
        ${["同步今日比赛", "生成赛前快照", "临近开赛同步首发/伤停", "标记 final snapshot", "比赛结束后结算", "查看 live_pre_match 纸面交易", "每天结束后备份数据"].map((step, index) => `
          <div><h4>${index + 1}. ${step}</h4><p class="muted">${index === 6 ? "导出 ZIP 和 CSV，不包含 API Key 明文。" : "保持赛前数据冻结，赛后只写结算表。"}</p></div>
        `).join("")}
      </div>
      <p class="muted">本版本用于真实赛前样本采集和纸面交易观察，不作为自动投注工具。</p>
    </section>
  `;
}

function preMatchSnapshotHtml() {
  const snapshots = state.preMatchSnapshots || [];
  const paperCount = snapshots.filter(item => item.paper_trade_enabled).length;
  const audits = state.snapshotAuditLogs || [];
  const criticalCount = audits.filter(item => item.severity === "critical" && !item.resolved).length;
  const warningCount = audits.filter(item => item.severity === "warning" && !item.resolved).length;
  const live = state.livePaperSummary || {};
  return `
    <div class="grid">
      <section class="panel span-12 toolbar">
        <button class="btn" data-action="create-today-pre-snapshots">批量生成今日快照</button>
        <button class="btn secondary" data-action="refresh-external">同步 API-Football 数据</button>
        <button class="btn secondary" data-action="audit-pre-snapshots">运行快照审计</button>
        <button class="btn secondary" data-action="settle-all-pre-snapshots">赛后批量结算</button>
        <button class="btn secondary" data-action="export-app-data">导出全部数据</button>
        <button class="btn secondary" data-action="export-snapshots">导出赛前快照</button>
        <button class="btn secondary" data-action="export-snapshot-results">导出赛后结算</button>
        <button class="btn secondary" data-action="export-live-paper">导出纸面交易</button>
        <button class="btn secondary" data-action="export-audit-logs">导出审计日志</button>
        <button class="btn secondary" data-action="export-strategy-diagnostics">导出策略诊断</button>
        <button class="btn ghost" data-action="open-backup-dir">打开备份目录</button>
        <span class="muted">赛前快照一旦生成，不会被赛后结果覆盖；结算只写入独立结果表。</span>
      </section>
      ${systemStatusHtml()}
      ${snapshotWorkflowHtml()}
      <section class="panel span-3 metric"><span>快照数</span><strong>${snapshots.length}</strong><div class="muted">允许同场多快照</div></section>
      <section class="panel span-3 metric"><span>最终快照</span><strong>${snapshots.filter(item => item.is_final_pre_match).length}</strong><div class="muted">每场最多一个</div></section>
      <section class="panel span-3 metric"><span>纸面观察</span><strong>${paperCount}</strong><div class="muted">模拟，不建议真实下注</div></section>
      <section class="panel span-3 metric"><span>数据提示</span><strong>保守</strong><div class="muted">首发/伤停缺失自动降级</div></section>
      <section class="panel span-3 metric"><span>审计问题</span><strong>${audits.filter(item => !item.resolved).length}</strong><div class="muted">Critical ${criticalCount} · Warning ${warningCount}</div></section>
      <section class="panel span-3 metric"><span>live 样本</span><strong>${live.sample_count || 0}</strong><div class="muted">已结算 ${live.settled_count || 0}</div></section>
      <section class="panel span-3 metric"><span>live 命中率</span><strong>${pct(live.hit_rate || 0)}</strong><div class="muted">${live.warning || "真实赛前纸面交易"}</div></section>
      <section class="panel span-3 metric"><span>live ROI</span><strong>${signedPct(live.paper_roi || 0)}</strong><div class="muted">近30 ${signedPct(live.recent_30_roi || 0)}</div></section>
      <section class="panel span-3 metric"><span>live 盈亏</span><strong>${signedPct(live.total_paper_profit || 0)}</strong><div class="muted">投入 ${Number(live.total_paper_stake || 0).toFixed(0)}</div></section>
      <section class="panel span-12">
        <h3>赛前快照中心</h3>
        <p class="muted">首发/伤停数据未确认时，当前使用基础模型，相关玩法降级观察。以下为策略观察样本，仅用于模拟记录，不建议真实下注。</p>
        <div class="scroll-table">
          <table><thead><tr><th>编号</th><th>开赛</th><th>比赛</th><th>快照时间</th><th>类型</th><th>模型概率</th><th>赔率</th><th>EV</th><th>数据</th><th>首发</th><th>伤停</th><th>决策</th><th>操作</th></tr></thead><tbody>${snapshotRows()}</tbody></table>
        </div>
      </section>
      <section class="panel span-12 table-panel">
        <h3>快照质量审计</h3>
        <div class="scroll-table"><table><thead><tr><th>比赛</th><th>快照时间</th><th>开赛时间</th><th>审计类型</th><th>严重程度</th><th>问题说明</th><th>状态</th></tr></thead><tbody>${auditRows()}</tbody></table></div>
      </section>
      <section class="panel span-12 table-panel">
        <h3>live_pre_match 真实纸面交易</h3>
        <p class="muted">${live.warning || "只有 final snapshot 的赛前纸面交易默认进入统计。"}</p>
        <div class="scroll-table"><table><thead><tr><th>时间</th><th>比赛ID</th><th>方向</th><th>玩法</th><th>模型</th><th>赔率</th><th>EV</th><th>状态</th><th>纸面盈亏</th></tr></thead><tbody>${livePaperRows()}</tbody></table></div>
      </section>
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

function modelStatusCard() {
  const model = state.activeModel || {};
  const range = model.training_data_range || {};
  const isAvailable = !!model.model_available;
  const strategyRules = model.strategy_rules_summary || [];
  const globalModels = model.global_models_summary || [];
  const hasBacktestBets = Number(model.backtest_final_bet_count || 0) > 0;
  const roiText = hasBacktestBets ? signedPct(model.backtest_roi || 0) : "暂无有效投注回测样本";
  return `
    <section class="panel span-12">
      <h3>模型状态</h3>
      <div class="plan-grid">
        <div><h4>当前模型</h4><p class="muted">${isAvailable ? "训练模型" : "规则模型"} · ${model.model_version || "rules-dixon-coles-v1"}</p></div>
        <div><h4>样本数量</h4><p class="muted">${model.sample_count || 0}</p></div>
        <div><h4>数据范围</h4><p class="muted">${range.start || "-"} ~ ${range.end || "-"}</p></div>
        <div><h4>回测ROI</h4><p class="muted">${roiText}</p></div>
      </div>
      <div class="plan-grid">
        <div><h4>Accuracy</h4><p class="muted">${pct(model.accuracy || 0)}</p></div>
        <div><h4>Log Loss</h4><p class="muted">${Number(model.log_loss || 0).toFixed(4)}</p></div>
        <div><h4>Brier Score</h4><p class="muted">${Number(model.brier_score || 0).toFixed(4)}</p></div>
        <div><h4>状态说明</h4><p class="muted">${isAvailable ? "训练模型只用于胜平负概率预测，不直接等于投注推荐。" : (model.fallback_reason || "未检测到训练模型，当前使用规则模型。")}</p></div>
      </div>
      <div class="plan-grid">
        <div><h4>正式投注样本</h4><p class="muted">${model.backtest_final_bet_count || 0}</p></div>
        <div><h4>最大回撤</h4><p class="muted">${Number(model.backtest_max_drawdown || 0).toFixed(2)}</p></div>
        <div><h4>平均赔率</h4><p class="muted">${model.backtest_avg_odds ? odds(model.backtest_avg_odds) : "-"}</p></div>
        <div><h4>平均EV</h4><p class="muted">${model.backtest_final_bet_count ? signedPct(model.backtest_avg_ev || 0) : "-"}</p></div>
      </div>
      <div class="muted">回测诊断：${model.backtest_warning || "按EV、概率差、风险标签和数据质量评估正式投注样本。"}</div>
      <div class="plan-grid">
        <div><h4>世界杯修正层</h4><p class="muted">${model.worldcup_correction_available ? `已启用 · ${model.worldcup_correction_version || "worldcup_live_correction_v1"}` : "未启用"}</p></div>
        <div><h4>世界杯样本</h4><p class="muted">${model.worldcup_correction_sample_count || 0}</p></div>
        <div><h4>修正层Accuracy</h4><p class="muted">${pct(model.worldcup_correction_accuracy || 0)}</p></div>
        <div><h4>修正层LogLoss</h4><p class="muted">${Number(model.worldcup_correction_log_loss || 0).toFixed(4)}</p></div>
      </div>
      <div class="muted">世界杯修正层说明：${model.worldcup_correction_note || "只影响是否值得投注和推荐降级，不改写模型真实概率。"}</div>
      <div class="toolbar source-actions">
        <button class="btn" data-action="run-training-pipeline">自动下载训练数据并训练</button>
        <span class="muted">${state.probeResult?.model ? `训练完成：${state.probeResult.model.model_version}` : "数据来自 Football-Data.co.uk CSV，API-Football 暂不参与历史训练。"}</span>
      </div>
      <div class="muted">${globalModels.length ? `全局模型覆盖：${globalModels.join("；")}` : "当前只显示规则模型覆盖。"}</div>
      <div class="muted">${strategyRules.length ? `训练规则摘要：${strategyRules.join("；")}` : "暂无训练禁买规则摘要。"}</div>
    </section>
  `;
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
      <table><thead><tr><th>数据源</th><th>状态</th><th>上次成功</th><th>数量</th><th>诊断</th><th>模型影响</th><th>下一步</th></tr></thead><tbody>
        ${sources.map(source => `
          <tr>
            <td>${source.label}</td>
            <td>${badge(source.health_label || (source.ok ? "正常" : "缺失"))}</td>
            <td>${source.last_success_at || source.updated_at || "-"}</td>
            <td>${source.count || 0}</td>
            <td>
              置信 ${Number(source.confidence_score || 0).toFixed(0)}
              · 新鲜 ${Number(source.freshness_score || 0).toFixed(0)}
              · 完整 ${Number(source.completeness_score || 0).toFixed(0)}
              <div class="muted">${source.diagnosis || (source.using_stale_cache ? "失败但使用旧缓存" : source.message)}</div>
              <div class="muted">${source.last_error_message || ""}</div>
            </td>
            <td>${source.impact || "-"}</td>
            <td>${source.next_action || "-"}</td>
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

function banRuleRows() {
  const rules = state.backtest?.ban_rules || [];
  if (!rules.length) {
    return `<tr><td colspan="8" class="muted">暂无结构化禁买规则。至少需要若干条已结算复盘样本。</td></tr>`;
  }
  return rules.map(rule => `
    <tr>
      <td>${rule.dimension}</td>
      <td>${rule.group}</td>
      <td>${rule.count}</td>
      <td>${pct(rule.hit_rate)}</td>
      <td class="${rule.roi >= 0 ? "down" : "up"}">${signedPct(rule.roi)}</td>
      <td>${odds(rule.avg_odds)}</td>
      <td>${rule.reason}</td>
      <td>${rule.action}</td>
    </tr>
  `).join("");
}

function strategyPoolRows() {
  const pools = state.backtest?.shadow_backtest?.pools || {};
  const ordered = ["recommend_pool", "small_stake_pool", "observe_only_pool", "hard_ban_pool", "wait_pool"];
  if (!Object.keys(pools).length) {
    return `<tr><td colspan="8" class="muted">暂无影子回测数据。运行训练回测后生成。</td></tr>`;
  }
  return ordered.map(id => pools[id]).filter(Boolean).map(pool => `
    <tr>
      <td>${pool.label || pool.pool_id}</td>
      <td>${pool.bet_count || pool.sample_count || 0}</td>
      <td>${pct(pool.hit_rate || 0)}</td>
      <td class="${(pool.roi || 0) >= 0 ? "down" : "up"}">${signedPct(pool.roi || 0)}</td>
      <td>${Number(pool.max_drawdown || 0).toFixed(2)}</td>
      <td>${pool.avg_odds ? odds(pool.avg_odds) : "-"}</td>
      <td>${signedPct(pool.avg_ev || 0)}</td>
      <td>${pct(pool.avg_model_prob || 0)}</td>
    </tr>
  `).join("");
}

function ruleDiagnosticsRows(kind) {
  const source = state.backtest?.rule_diagnostics || {};
  const rows = source[kind] || [];
  const labels = {
    effective_block: "有效防亏",
    over_strict: "可能误杀",
    sample_too_small: "样本不足",
    neutral: "暂不调整"
  };
  if (!rows.length) {
    return `<tr><td colspan="9" class="muted">暂无${labels[kind] || "规则"}。</td></tr>`;
  }
  return rows.map(rule => `
    <tr>
      <td>${rule.rule_name || rule.rule_id}</td>
      <td>${rule.action}</td>
      <td>${rule.matched_count || 0}</td>
      <td>${pct(rule.hit_rate || 0)}</td>
      <td class="${(rule.roi || 0) >= 0 ? "down" : "up"}">${signedPct(rule.roi || 0)}</td>
      <td>${Number(rule.max_drawdown || 0).toFixed(2)}</td>
      <td>${rule.avg_odds ? odds(rule.avg_odds) : "-"}</td>
      <td>${signedPct(rule.avg_ev || 0)}</td>
      <td>${labels[rule.classification] || rule.classification || "-"}</td>
    </tr>
  `).join("");
}

function candidateStrategyRows() {
  const strategy = state.backtest?.candidate_strategy || {};
  const rules = strategy.candidate_rules || [];
  if (!rules.length) {
    return `<tr><td colspan="7" class="muted">${(strategy.warnings || []).join("；") || "暂无候选策略。"}</td></tr>`;
  }
  return rules.map(rule => `
    <tr>
      <td>${rule.ev_threshold != null ? signedPct(rule.ev_threshold) : "-"}</td>
      <td>${rule.odds_range || "-"}</td>
      <td>${rule.probability_range || "-"}</td>
      <td>${rule.bet_count || rule.sample_count || 0}</td>
      <td>${pct(rule.hit_rate || 0)}</td>
      <td class="${(rule.roi || 0) >= 0 ? "down" : "up"}">${signedPct(rule.roi || 0)}</td>
      <td>${Number(rule.max_drawdown || 0).toFixed(2)}</td>
    </tr>
  `).join("");
}

function strategyDiagnosticsHtml() {
  const strategy = state.backtest?.candidate_strategy || {};
  return `
    <section class="panel table-panel span-12">
      <h3>策略诊断</h3>
      <p class="muted">影子回测只诊断规则误杀，不会自动放宽正式推荐。候选策略状态：${strategy.status === "observation_only" ? "候选策略观察中" : (strategy.status || "未生成")}</p>
      <div class="scroll-table">
        <table><thead><tr><th>池子</th><th>样本</th><th>命中率</th><th>ROI</th><th>最大回撤</th><th>平均赔率</th><th>平均EV</th><th>平均模型概率</th></tr></thead><tbody>${strategyPoolRows()}</tbody></table>
      </div>
    </section>
    <section class="panel table-panel span-12">
      <h3>规则误杀诊断</h3>
      <div class="scroll-table">
        <table><thead><tr><th>规则</th><th>动作</th><th>命中样本</th><th>命中率</th><th>ROI</th><th>最大回撤</th><th>平均赔率</th><th>平均EV</th><th>结论</th></tr></thead><tbody>
          ${ruleDiagnosticsRows("effective_block")}
          ${ruleDiagnosticsRows("over_strict")}
          ${ruleDiagnosticsRows("sample_too_small")}
        </tbody></table>
      </div>
    </section>
    ${paperTradingSummaryHtml()}
    <section class="panel table-panel span-12">
      <h3>候选策略观察</h3>
      <p class="muted">${(strategy.warnings || []).join("；") || "候选策略默认不启用，需要人工确认后才能进入正式规则。"}</p>
      <div class="scroll-table">
        <table><thead><tr><th>EV阈值</th><th>赔率区间</th><th>概率区间</th><th>样本</th><th>命中率</th><th>ROI</th><th>最大回撤</th></tr></thead><tbody>${candidateStrategyRows()}</tbody></table>
      </div>
    </section>
  `;
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
  const providerRows = (state.providers || []).map(provider => `
    <tr>
      <td>
        <strong>${provider.name}</strong>
        <div class="muted">${provider.provider_id}</div>
      </td>
      <td>${provider.supported_data_types?.join(" / ") || provider.data_type || "-"}</td>
      <td>${provider.enabled ? badge("启用") : badge("禁用")}</td>
      <td>${provider.requires_key ? (provider.key_configured ? badge("已配置") : badge("未配置")) : "无需 Key"}</td>
      <td>${provider.last_success_at || "-"}</td>
      <td>${provider.last_error_message || "-"}</td>
      <td>${provider.today_requests || 0} / ${provider.daily_limit || "不限"}</td>
      <td>${provider.hour_requests || 0} / ${provider.hourly_limit || "不限"}</td>
      <td>${badge(provider.health_label || "字段缺失")}<div class="muted">置信 ${Number(provider.confidence_score || 0).toFixed(0)}</div></td>
      <td>
        ${provider.requires_key ? `<input id="provider-key-${provider.provider_id}" type="password" placeholder="${provider.key_configured ? "已配置，输入新 Key 可覆盖" : "输入 API Key"}">` : ""}
        <div class="toolbar source-actions">
          ${provider.requires_key ? `<button class="mini" data-action="save-provider-key" data-provider="${provider.provider_id}">保存Key</button><button class="mini danger" data-action="clear-provider-key" data-provider="${provider.provider_id}">清Key</button>` : ""}
          <button class="mini" data-action="test-provider" data-provider="${provider.provider_id}">测试</button>
          <button class="mini" data-action="toggle-provider" data-provider="${provider.provider_id}" data-enabled="${provider.enabled ? "false" : "true"}">${provider.enabled ? "禁用" : "启用"}</button>
          <button class="mini danger" data-action="clear-provider-cache" data-provider="${provider.provider_id}">清缓存</button>
        </div>
      </td>
    </tr>
  `).join("");
  return `
    <section class="panel span-12">
      <h3>免费数据源 Provider Registry</h3>
      <p class="muted">Key 只保存在本地设置中，页面不显示明文。网页抓取源只作为低可信补充，不当作官方强源。</p>
      <div class="scroll-table">
        <table><thead><tr><th>Provider</th><th>支持数据</th><th>启用</th><th>Key</th><th>最后成功</th><th>失败原因</th><th>今日请求</th><th>小时请求</th><th>健康度</th><th>操作</th></tr></thead><tbody>${providerRows || `<tr><td colspan="10" class="muted">暂无 provider registry。</td></tr>`}</tbody></table>
      </div>
      <p class="muted">${state.probeResult ? `${state.probeResult.message || "操作完成"}` : ""}</p>
    </section>
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
        <button class="btn secondary" data-action="refresh-external">全局刷新数据源</button>
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
        <section class="panel span-12"><h3>当前概率模型</h3><p class="muted">${sim.model_version || "rules-dixon-coles-v1"}；投注推荐层独立按赔率、EV、数据质量和风险标签过滤，不直接训练“买/不买”。</p></section>
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
        ${modelStatusCard()}
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
  if (state.view === "snapshots") return preMatchSnapshotHtml();
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
            <table><thead><tr><th>时间</th><th>比赛</th><th>玩法</th><th>方向</th><th>推荐等级</th><th>模型概率</th><th>体彩去水</th><th>欧洲概率</th><th>对体彩差</th><th>对欧洲差</th><th>公平赔率</th><th>当前赔率</th><th>欧洲均赔</th><th>EV</th><th>优势率</th><th>数据</th><th>数据建议</th><th>世界杯修正</th><th>仓位</th><th>最终决策</th><th>组合组</th><th>操作</th></tr></thead><tbody>${recommendationRows(100)}</tbody></table>
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
        <button class="btn" data-action="collect-worldcup-snapshot">赛前闭环采集</button>
        <button class="btn" data-action="refresh-results">刷新赛果</button>
        <button class="btn secondary" data-action="settle-bet-recs">结算推荐样本</button>
        <button class="btn secondary" data-action="export-worldcup-samples">导出训练样本</button>
        <button class="btn danger" data-action="run-worldcup-cycle">一键闭环</button>
        <button class="btn secondary" data-action="auto-settle">按赛果自动结算复盘</button>
        <span class="muted">赛前冻结赔率/概率，赛后写入真实比分，形成世界杯修正层训练样本。</span>
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
        <button class="btn" data-action="collect-worldcup-snapshot">赛前闭环采集</button>
        <button class="btn" data-action="refresh-results">刷新赛果</button>
        <button class="btn secondary" data-action="settle-bet-recs">结算推荐样本</button>
        <button class="btn secondary" data-action="export-worldcup-samples">导出训练样本</button>
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
      ${strategyDiagnosticsHtml()}
      <section class="panel table-panel">
        <h3>历史回测分组</h3>
        <div class="scroll-table">
          <table><thead><tr><th>维度</th><th>分组</th><th>次数</th><th>命中率</th><th>ROI</th><th>总盈利</th><th>最大回撤</th><th>平均赔率</th><th>平均优势率</th><th>Brier</th><th>LogLoss</th></tr></thead><tbody>${backtestRows()}</tbody></table>
        </div>
      </section>
      <section class="panel table-panel">
        <h3>禁买规则明细</h3>
        <div class="scroll-table">
          <table><thead><tr><th>维度</th><th>分组</th><th>样本</th><th>命中率</th><th>ROI</th><th>均赔</th><th>原因</th><th>操作</th></tr></thead><tbody>${banRuleRows()}</tbody></table>
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
      ${modelStatusCard()}
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
    state.diagnostics = await api.invokeCommand("model_diagnostics");
    state.backtest = await api.invokeCommand("backtest_report");
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
    state.backtest = await api.invokeCommand("backtest_report");
  });
  if (action === "simulate") safeRun("运行模拟", runSimulation);
  if (action === "refresh-recommend") safeRun("生成推荐", async () => {
    state.recommendations = await api.invokeCommand("list_recommendations");
    state.todayPlan = await api.invokeCommand("today_bet_plan");
  });
  if (action === "refresh-analysis") safeRun("刷新单场分析", async () => {
    state.analyses = await api.invokeCommand("list_match_analyses");
  });
  if (action === "save-rec") safeRun("保存复盘", async () => saveRecommendation(event.target.dataset.index));
  if (action === "freeze-rec") safeRun("冻结赛前快照", freezeRecommendations);
  if (action === "collect-worldcup-snapshot") safeRun("赛前闭环采集", collectWorldcupSnapshot);
  if (action === "create-pre-snapshot") safeRun("生成当前快照", async () => createPreMatchSnapshot(event.target.dataset.matchId));
  if (action === "create-today-pre-snapshots") safeRun("批量生成今日快照", createTodayPreMatchSnapshots);
  if (action === "audit-pre-snapshots") safeRun("运行快照审计", auditPreMatchSnapshots);
  if (action === "mark-final-snapshot") safeRun("标记最终快照", async () => markFinalPreMatchSnapshot(event.target.dataset.snapshotId));
  if (action === "settle-pre-snapshot") safeRun("赛后结算快照", async () => settlePreMatchSnapshot(event.target.dataset.snapshotId));
  if (action === "settle-all-pre-snapshots") safeRun("批量结算快照", settleAllFinishedSnapshots);
  if (action === "export-app-data") safeRun("导出全部数据", exportAppData);
  if (action === "export-snapshots") safeRun("导出赛前快照", exportSnapshots);
  if (action === "export-snapshot-results") safeRun("导出赛后结算", exportSnapshotResults);
  if (action === "export-live-paper") safeRun("导出纸面交易", exportLivePaperTrading);
  if (action === "export-audit-logs") safeRun("导出审计日志", exportAuditLogs);
  if (action === "export-strategy-diagnostics") safeRun("导出策略诊断", exportStrategyDiagnostics);
  if (action === "open-backup-dir") safeRun("打开备份目录", openBackupDir);
  if (action === "settle-bet-recs") safeRun("结算推荐样本", settleBetRecommendations);
  if (action === "export-worldcup-samples") safeRun("导出训练样本", exportWorldcupSamples);
  if (action === "run-worldcup-cycle") safeRun("执行世界杯闭环", runWorldcupClosureCycle);
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
  if (action === "save-provider-key") safeRun("保存 Provider Key", async () => saveProviderCredential(event.target.dataset.provider));
  if (action === "clear-provider-key") safeRun("清除 Provider Key", async () => clearProviderCredential(event.target.dataset.provider));
  if (action === "test-provider") safeRun("测试 Provider", async () => testProvider(event.target.dataset.provider));
  if (action === "toggle-provider") safeRun("切换 Provider", async () => toggleProvider(event.target.dataset.provider, event.target.dataset.enabled === "true"));
  if (action === "clear-provider-cache") safeRun("清除 Provider 缓存", async () => clearProviderCache(event.target.dataset.provider));
  if (action === "run-training-pipeline") safeRun("自动下载训练数据并训练", runTrainingPipeline);
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
