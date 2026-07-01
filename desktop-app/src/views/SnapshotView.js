import { pct, signedPct } from "../utils/format.js";

// TODO: pending split phase 2 - move snapshot row builders and commands from legacyMain into this view.
function fileSize(bytes) {
  const value = Number(bytes || 0);
  if (value < 1024) return `${value} B`;
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KB`;
  if (value < 1024 * 1024 * 1024) return `${(value / 1024 / 1024).toFixed(1)} MB`;
  return `${(value / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

function systemStatusHtml(state) {
  const status = state.systemStatus || {};
  const lastBackup = status.last_backup || {};
  return `
    <section class="panel span-12">
      <h3>系统状态</h3>
      <div class="plan-grid">
        <div><h4>应用版本</h4><p class="muted">${status.app_version || "v0.2-clean-core"}</p></div>
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
        <div><h4>数据源策略</h4><p class="muted">使用体彩、football-data.org、The Odds API、StatsBomb 和自定义外部源</p></div>
        <div><h4>最近备份</h4><p class="muted">${lastBackup.created_at || "尚未备份"}</p></div>
        <div><h4>未结算</h4><p class="muted">${status.live_pre_match_unsettled_count || 0} 条</p></div>
        <div><h4>数据库路径</h4><p class="muted">${status.db_path || "-"}</p></div>
      </div>
    </section>
  `;
}

function workflowHtml() {
  const steps = ["同步今日比赛", "生成赛前快照", "临近开赛同步伤停/赔率", "标记 final snapshot", "比赛结束后结算", "查看 live_pre_match 纸面交易", "每天结束后备份数据"];
  return `
    <section class="panel span-12">
      <h3>日常使用流程</h3>
      <div class="plan-grid">
        ${steps.map((step, index) => `<div><h4>${index + 1}. ${step}</h4><p class="muted">${index === 6 ? "导出 ZIP 和 CSV，不包含 API Key 明文。" : "保持赛前数据冻结，赛后只写结算表。"}</p></div>`).join("")}
      </div>
      <p class="muted">本版本用于真实赛前样本采集和纸面交易观察，不作为自动投注工具。</p>
    </section>
  `;
}

function defaultRows(colspan, text) {
  return `<tr><td colspan="${colspan}" class="muted">${text}</td></tr>`;
}

export function renderSnapshotView(state, helpers = {}) {
  const snapshots = state.preMatchSnapshots || [];
  const paperCount = snapshots.filter(item => item.paper_trade_enabled).length;
  const audits = state.snapshotAuditLogs || [];
  const criticalCount = audits.filter(item => item.severity === "critical" && !item.resolved).length;
  const warningCount = audits.filter(item => item.severity === "warning" && !item.resolved).length;
  const live = state.livePaperSummary || {};
  const snapshotRows = helpers.snapshotRows || (() => defaultRows(12, "暂无快照数据。"));
  const snapshotHistoryRows = helpers.snapshotHistoryRows || (() => defaultRows(9, "点击“查看历史”查看单场快照。"));
  const auditRows = helpers.auditRows || (() => defaultRows(7, "暂无审计记录。"));
  const livePaperRows = helpers.livePaperRows || (() => defaultRows(9, "暂无 live_pre_match 纸面交易。"));
  return `
    <div class="grid">
      <section class="panel span-12 toolbar">
        <button class="btn" data-action="one-click-gpt-package" ${state.oneClickGptPackage?.running ? "disabled" : ""}>${state.oneClickGptPackage?.running ? "正在生成..." : "一键生成 GPT 分析包"}</button>
        <button class="btn secondary" data-action="open-gpt-exports-dir">打开分析包目录</button>
        <button class="btn" data-action="create-today-pre-snapshots">批量生成今日快照</button>
        <button class="btn secondary" data-action="refresh-external">全局刷新数据源</button>
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
      ${helpers.oneClickGptPackageProgressHtml ? helpers.oneClickGptPackageProgressHtml() : ""}
      ${systemStatusHtml(state)}
      ${workflowHtml()}
      <section class="panel span-3 metric"><span>快照数</span><strong>${snapshots.length}</strong><div class="muted">允许同场多快照</div></section>
      <section class="panel span-3 metric"><span>最终快照</span><strong>${snapshots.filter(item => item.is_final_pre_match).length}</strong><div class="muted">每场最多一个</div></section>
      <section class="panel span-3 metric"><span>纸面观察</span><strong>${paperCount}</strong><div class="muted">模拟，不建议真实下注</div></section>
      <section class="panel span-3 metric"><span>数据提示</span><strong>保守</strong><div class="muted">伤停、赔率、质量异常自动降级</div></section>
      <section class="panel span-3 metric"><span>审计问题</span><strong>${audits.filter(item => !item.resolved).length}</strong><div class="muted">Critical ${criticalCount} · Warning ${warningCount}</div></section>
      <section class="panel span-3 metric"><span>live 样本</span><strong>${live.sample_count || 0}</strong><div class="muted">已结算 ${live.settled_count || 0}</div></section>
      <section class="panel span-3 metric"><span>live 命中率</span><strong>${pct(live.hit_rate || 0)}</strong><div class="muted">${live.warning || "真实赛前纸面交易"}</div></section>
      <section class="panel span-3 metric"><span>live ROI</span><strong>${signedPct(live.paper_roi || 0)}</strong><div class="muted">近30 ${signedPct(live.recent_30_roi || 0)}</div></section>
      <section class="panel span-3 metric"><span>live 盈亏</span><strong>${signedPct(live.total_paper_profit || 0)}</strong><div class="muted">投入 ${Number(live.total_paper_stake || 0).toFixed(0)}</div></section>
      <section class="panel span-12">
        <h3>赛前快照中心</h3>
        <p class="muted">伤停或赔率数据未确认时，当前使用基础模型，相关玩法降级观察。以下为策略观察样本，仅用于模拟记录，不建议真实下注。</p>
        <div class="scroll-table">
          <table><thead><tr><th>编号</th><th>开赛</th><th>比赛</th><th>快照时间</th><th>类型</th><th>模型概率</th><th>赔率</th><th>EV</th><th>数据</th><th>伤停</th><th>决策</th><th>操作</th></tr></thead><tbody>${snapshotRows()}</tbody></table>
        </div>
      </section>
      <section class="panel span-12 table-panel">
        <h3>快照历史 ${state.selectedSnapshotMatchLabel ? `· ${state.selectedSnapshotMatchLabel}` : ""}</h3>
        <p class="muted">这里查看同一场比赛的所有快照，方便比较 latest / final / 赛前生成 / 赔率缺失等状态。</p>
        <div class="scroll-table">
          <table><thead><tr><th>ID</th><th>快照时间</th><th>开赛时间</th><th>类型</th><th>生成口径</th><th>赔率</th><th>EV</th><th>数据质量</th><th>决策/风险</th></tr></thead><tbody>${snapshotHistoryRows()}</tbody></table>
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
