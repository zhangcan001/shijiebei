function statusBadge(text) {
  const cls = /可买|盈利|正|ok/i.test(text)
    ? "good"
    : /观察|等待|小注|中|running/i.test(text)
      ? "warn"
      : "bad";
  return `<span class="badge ${cls}">${text || "-"}</span>`;
}

export function dataRefreshProgressHtml(progress, compact = false) {
  if (!progress) return "";
  const total = Number(progress.total || 8);
  const step = Number(progress.step || 0);
  const percent = Math.round(Number(progress.percent ?? (total ? step / total : 0)) * 100);
  return `
    <section class="panel ${compact ? "progress-panel" : "span-12 progress-panel"}">
      <div class="progress-head">
        <strong>${progress.label || "全局刷新数据源"}</strong>
        <span>${percent}% · ${step} / ${total}</span>
      </div>
      <progress value="${step}" max="${total || 1}"></progress>
      <p class="muted">${statusBadge(progress.status || "running")} ${progress.message || "正在刷新数据源..."}</p>
    </section>
  `;
}

export function refreshStatusHtml({ busy, message, refreshMeta, status }) {
  const meta = refreshMeta || {};
  return `
    <div class="refresh-strip">
      <span>${busy ? "正在处理" : "自动刷新就绪"}</span>
      <span>${meta.label || message || "准备就绪"}</span>
      <span>今日 ${meta.lastTodayAt || "-"}</span>
      <span>赔率 ${meta.lastOddsAt || "-"}</span>
      <span>赛果 ${meta.lastResultsAt || "-"}</span>
      <span>数据源 ${status?.sources?.length || "-"} 项</span>
    </div>
  `;
}
