// TODO: pending split phase 2 - migrate remaining data-action handlers from legacyMain.js.
const CLEAN_CORE_ACTIONS = new Set([
  "generate-upset-lab",
  "create-upset-paper",
  "create-upset-paper-trades",
  "settle-upset-paper",
  "settle-upset-paper-trades",
  "create-pre-snapshot",
  "create-pre-match-snapshot",
  "mark-final-snapshot",
  "mark-final-pre-match-snapshot",
  "settle-pre-snapshot",
  "settle-snapshot",
  "export-app-data",
  "export-backup"
]);

export function handleCleanCoreAction(action, event, safeRun, handlers) {
  if (!CLEAN_CORE_ACTIONS.has(action)) return false;
  if (action === "generate-upset-lab") {
    safeRun("生成冷门候选", handlers.refreshUpsetLab);
    return true;
  }
  if (action === "create-upset-paper" || action === "create-upset-paper-trades") {
    safeRun("写入冷门纸面交易", handlers.createUpsetPaperTrades);
    return true;
  }
  if (action === "settle-upset-paper" || action === "settle-upset-paper-trades") {
    safeRun("结算冷门纸面交易", handlers.settleUpsetPaperTrades);
    return true;
  }
  if (action === "create-pre-snapshot" || action === "create-pre-match-snapshot") {
    safeRun("生成当前快照", async () => handlers.createPreMatchSnapshot(event.target.dataset.matchId));
    return true;
  }
  if (action === "mark-final-snapshot" || action === "mark-final-pre-match-snapshot") {
    safeRun("标记最终快照", async () => handlers.markFinalPreMatchSnapshot(event.target.dataset.snapshotId));
    return true;
  }
  if (action === "settle-pre-snapshot" || action === "settle-snapshot") {
    safeRun("赛后结算快照", async () => handlers.settlePreMatchSnapshot(event.target.dataset.snapshotId));
    return true;
  }
  if (action === "export-app-data" || action === "export-backup") {
    safeRun("导出全部数据", handlers.exportAppData);
    return true;
  }
  return false;
}
