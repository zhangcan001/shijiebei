// TODO: pending split phase 2 - migrate the remaining refresh contexts from legacyMain.js.
export async function refreshUpsetLabData({ api, state, loadOptional, markRefresh }) {
  await api.invokeCommand("generate_upset_lab_candidates");
  state.upsetLabCandidates = await api.invokeCommand("get_upset_lab_candidates");
  state.upsetLabSummary = await api.invokeCommand("get_upset_lab_summary");
  state.upsetLabBacktest = await api.invokeCommand("get_upset_lab_backtest_summary");
  state.upsetLabRobustness = await api.invokeCommand("get_upset_lab_robustness_summary");
  state.upsetLabDebug = await api.invokeCommand("debug_upset_lab_generation");
  state.scorePriors = await loadOptional("get_worldcup_knockout_score_priors", state.scorePriors);
  markRefresh("lastTodayAt", "冷门实验室已更新", "手动");
}

export async function refreshSnapshots({ state, loadOptional, markRefresh }, source = "手动") {
  state.preMatchSnapshots = await loadOptional("get_pre_match_snapshots", state.preMatchSnapshots || []);
  state.snapshotAuditLogs = await loadOptional("get_snapshot_audit_logs", state.snapshotAuditLogs || []);
  state.livePaperSummary = await loadOptional("get_live_paper_trading_summary", state.livePaperSummary);
  state.livePaperRecords = await loadOptional("get_live_paper_trading_records", state.livePaperRecords || []);
  markRefresh("lastHealthAt", "赛前快照已更新", source);
}

export async function refreshProjectHealth({ state, loadOptional, markRefresh }, source = "手动") {
  state.projectHealth = await loadOptional("get_project_health_report", state.projectHealth);
  markRefresh("lastHealthAt", "项目健康已更新", source);
}
