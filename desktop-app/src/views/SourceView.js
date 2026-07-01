import { dataRefreshProgressHtml } from "../components/RefreshStatusBar.js";

function badge(text, kind = "") {
  return `<span class="badge ${kind}">${text || "-"}</span>`;
}

function safe(value, fallback = "-") {
  return value == null || value === "" ? fallback : String(value);
}

function providerKind(provider) {
  const id = provider.provider_id || "";
  if (id.includes("api_football")) return "API-Football";
  if (id.includes("odds")) return "Odds API";
  if (id.includes("football_data")) return "football-data.org";
  if (id.includes("sporttery")) return "Sporttery / 体彩";
  return provider.name || id || "本地缓存";
}

function providerCard(provider) {
  const needsKey = Boolean(provider.requires_key);
  const keyState = needsKey ? (provider.key_configured ? "已配置" : "未配置") : "无需 Key";
  const health = provider.health_label || (provider.enabled ? "正常" : "禁用");
  const healthKind = health.includes("正常") ? "good" : health.includes("错误") || health.includes("缺失") ? "bad" : "warn";
  return `
    <div class="panel source-card">
      <div class="card-head">
        <div>
          <h4>${providerKind(provider)}</h4>
          <p class="muted wrap-text">${safe(provider.provider_id)}</p>
        </div>
        ${badge(provider.enabled ? "启用" : "禁用", provider.enabled ? "good" : "warn")}
      </div>
      <div class="source-meta">
        <div><span class="muted">Key</span><strong>${keyState}</strong></div>
        <div><span class="muted">Base URL</span><span class="wrap-text">${safe(provider.base_url || provider.url || provider.endpoint || "内置/缓存")}</span></div>
        <div><span class="muted">今日请求</span><span>${provider.today_requests || 0} / ${provider.daily_limit || "不限"}</span></div>
        <div><span class="muted">剩余额度</span><span>${provider.daily_limit ? Math.max(0, Number(provider.daily_limit || 0) - Number(provider.today_requests || 0)) : "不限"}</span></div>
        <div><span class="muted">最近成功</span><span>${safe(provider.last_success_at)}</span></div>
        <div><span class="muted">最近错误</span><span class="wrap-text">${safe(provider.last_error_message)}</span></div>
        <div><span class="muted">健康</span><span>${badge(health, healthKind)}</span></div>
      </div>
      ${needsKey ? `<input id="provider-key-${provider.provider_id}" type="password" autocomplete="off" placeholder="${provider.key_configured ? "已配置，输入新 Key 可覆盖" : "输入 API Key"}">` : ""}
      <div class="actions source-actions">
        ${needsKey ? `<button class="mini" data-action="save-source-key" data-provider-id="${provider.provider_id}">保存Key</button><button class="mini danger" data-action="clear-source-key" data-provider-id="${provider.provider_id}">清Key</button>` : ""}
        <button class="mini" data-action="test-source" data-provider-id="${provider.provider_id}">测试</button>
        <button class="mini" data-action="toggle-source" data-provider-id="${provider.provider_id}" data-enabled="${provider.enabled ? "false" : "true"}">${provider.enabled ? "禁用" : "启用"}</button>
        <button class="mini danger" data-action="clear-source-cache" data-provider-id="${provider.provider_id}">清缓存</button>
      </div>
    </div>
  `;
}

function healthCards(state) {
  const status = state.systemStatus || {};
  const debug = state.snapshotDebug || {};
  const audits = state.snapshotAuditLogs || [];
  const critical = audits.filter(item => item.severity === "critical" && !item.resolved).length;
  const warning = audits.filter(item => item.severity === "warning" && !item.resolved).length;
  const matchCount = state.matches?.length || debug.today_matches_count || 0;
  const oddsCount = debug.odds_available_count || 0;
  const snapshotCount = status.snapshot_count ?? debug.snapshots_count ?? 0;
  const finalCount = status.final_snapshot_count ?? debug.final_snapshots_count ?? 0;
  const resultCount = state.results?.length || 0;
  return [
    ["今日比赛", matchCount],
    ["有赔率快照", oddsCount],
    ["缺赔率比赛", Math.max(0, Number(matchCount) - Number(oddsCount))],
    ["赛果数量", resultCount],
    ["赛前快照", snapshotCount],
    ["final snapshot", finalCount],
    ["平均质量", snapshotCount ? `${Math.round((state.preMatchSnapshots || []).reduce((sum, item) => sum + Number(item.data_quality_score || 0), 0) / Math.max(1, (state.preMatchSnapshots || []).length))}分` : "-"],
    ["审计问题", `W ${warning} / C ${critical}`]
  ].map(([label, value]) => `
    <section class="panel metric">
      <span>${label}</span>
      <strong>${value}</strong>
    </section>
  `).join("");
}

function logRows(state) {
  const providers = state.providers || [];
  const rows = providers
    .filter(provider => provider.last_error_message || provider.last_error_at || provider.last_success_at)
    .slice(0, 10);
  if (!rows.length) {
    return `<tr><td colspan="4" class="muted">暂无数据源日志。</td></tr>`;
  }
  return rows.map(provider => `
    <tr>
      <td>${safe(provider.name || provider.provider_id)}</td>
      <td>${safe(provider.last_error_at || provider.last_success_at)}</td>
      <td>${safe(provider.last_error_message || provider.health_label || "正常")}</td>
      <td>${safe(provider.next_action || provider.diagnosis || "保持观察")}</td>
    </tr>
  `).join("");
}

function stepBadge(status) {
  const text = {
    pending: "等待",
    running: "执行中",
    success: "成功",
    warning: "警告",
    failed: "失败",
    skipped: "跳过",
    timeout: "超时"
  }[status] || "-";
  const kind = status === "success" ? "good" : ["warning", "running", "skipped"].includes(status) ? "warn" : ["failed", "timeout"].includes(status) ? "bad" : "";
  return badge(text, kind);
}

function globalRefreshProgressHtml(state) {
  const refresh = state.globalRefresh || {};
  const steps = refresh.steps || [];
  const started = refresh.startedAt ? new Date(refresh.startedAt).toLocaleTimeString("zh-CN", { hour12: false }) : "-";
  const elapsed = refresh.startedAt
    ? Math.max(0, Math.round(((refresh.finishedAt ? new Date(refresh.finishedAt).getTime() : Date.now()) - new Date(refresh.startedAt).getTime()) / 1000))
    : 0;
  const current = steps.find(step => step.key === refresh.currentStep) || steps.find(step => step.status === "running");
  const statusText = refresh.running ? "正在刷新" : refresh.error ? "失败" : refresh.warning ? "完成但有提示" : refresh.finishedAt ? "完成" : "空闲";
  return `
    <section class="panel span-12">
      <div class="card-head">
        <div>
          <h3>全局刷新进度</h3>
          <p class="muted">当前状态：${statusText} · 开始：${started} · 已耗时：${elapsed}s · 当前步骤：${current?.label || "-"}</p>
        </div>
        <div class="actions source-actions">
          <button class="btn ghost" data-action="reset-global-refresh-state">重置刷新状态</button>
        </div>
      </div>
      ${refresh.error || refresh.warning ? `<p class="muted wrap-text">${safe(refresh.error || refresh.warning)}</p>` : ""}
      <div class="scroll-table">
        <table>
          <thead><tr><th>步骤</th><th>状态</th><th>耗时</th><th>说明</th><th>错误</th></tr></thead>
          <tbody>
            ${steps.length ? steps.map(step => `
              <tr>
                <td>${step.label}</td>
                <td>${stepBadge(step.status)}</td>
                <td>${step.durationMs ? `${Math.round(step.durationMs / 1000)}s` : "-"}</td>
                <td class="wrap-text">${safe(step.message)}</td>
                <td class="wrap-text">${safe(step.error)}</td>
              </tr>
            `).join("") : `<tr><td colspan="5" class="muted">尚未执行全局刷新。</td></tr>`}
          </tbody>
        </table>
      </div>
    </section>
  `;
}

export function renderSourceView(state) {
  const providers = state.providers || state.status?.providers || [];
  const config = state.externalConfig || {};
  const refresh = state.refreshMeta || {};
  const startup = state.startupHealth || {};
  const warnings = [
    ...(startup.suggested_actions || []),
    ...(state.snapshotDebug?.suggested_actions || [])
  ];
  const apiFootball = providers.find(provider => (provider.provider_id || "").includes("api_football"));
  const oddsMissing = Number(state.snapshotDebug?.odds_available_count || 0) === 0;
  const refreshRunning = Boolean(state.globalRefresh?.running);
  return `
    <div class="source-page">
      ${dataRefreshProgressHtml(state.dataRefreshProgress)}

      <section class="panel span-12 source-hero">
        <div class="card-head">
          <div>
            <h3>数据源管理</h3>
            <p class="muted">当前数据状态：${state.status ? "已加载" : "等待加载"} · 最近刷新：${refresh.lastHealthAt || refresh.lastGlobalAt || "-"}</p>
          </div>
          <div class="actions source-actions">
            <button class="btn" data-action="global-refresh" ${refreshRunning ? "disabled" : ""}>${refreshRunning ? "正在刷新..." : "全局刷新"}</button>
            <button class="btn secondary" data-action="save-source-config">保存配置</button>
            <button class="btn secondary" data-action="create-today-pre-match-snapshots">生成今日快照</button>
            <button class="btn ghost" data-action="open-backup-dir">打开备份目录</button>
          </div>
        </div>
      </section>

      ${globalRefreshProgressHtml(state)}

      <section class="panel span-12">
        <div class="card-head"><h3>API 配置</h3></div>
        ${providers.length ? `<div class="source-grid">${providers.map(providerCard).join("")}</div>` : `<p class="muted">尚未配置外部数据源，当前仅使用本地缓存/基础数据。</p>`}
        ${apiFootball && !apiFootball.key_configured ? `<p class="muted">API-Football 未配置，首发/伤停/赛程补强不可用。</p>` : ""}
      </section>

      <section class="source-health-grid">
        ${healthCards(state)}
      </section>

      <section class="panel span-12">
        <div class="card-head"><h3>操作区</h3></div>
        <div class="actions source-actions grouped">
          <button class="btn secondary" data-action="refresh-core">同步今日比赛</button>
          <button class="btn secondary" data-action="refresh-core">同步赔率</button>
          <button class="btn secondary" data-action="refresh-results">同步赛果</button>
          <button class="btn secondary" data-action="refresh-sporttery-injury">同步伤停/首发</button>
          <button class="btn" data-action="global-refresh" ${refreshRunning ? "disabled" : ""}>${refreshRunning ? "正在刷新..." : "全局刷新"}</button>
          <button class="btn secondary" data-action="create-today-pre-match-snapshots">生成今日快照</button>
          <button class="btn ghost" data-action="open-backup-dir">打开备份目录</button>
        </div>
        ${oddsMissing ? `<p class="muted">赔率缺失，EV、赔率异动、冷门实验室和纸面交易将受影响。</p>` : ""}
      </section>

      <section class="panel span-12">
        <div class="card-head"><h3>外部数据源配置</h3></div>
        <div class="source-grid">
          <label>伤停 JSON / 代理URL<input id="injury-url" value="${safe(config.injury_url, "")}" placeholder="https://..."></label>
          <label>首发 JSON / 代理URL<input id="lineup-url" value="${safe(config.lineup_url, "")}" placeholder="https://..."></label>
          <label>统计/xG JSON / 代理URL<input id="stats-url" value="${safe(config.stats_url, "")}" placeholder="https://..."></label>
        </div>
        <label>备注<input id="source-notes" value="${safe(config.notes, "")}"></label>
      </section>

      <section class="panel span-12 table-panel">
        <div class="card-head"><h3>日志 / 错误</h3></div>
        ${(warnings || []).length ? `<p class="muted">建议：${warnings.join("；")}</p>` : `<p class="muted">暂无阻断性建议。</p>`}
        <div class="scroll-table">
          <table><thead><tr><th>来源</th><th>时间</th><th>错误/状态</th><th>建议</th></tr></thead><tbody>${logRows(state)}</tbody></table>
        </div>
      </section>
    </div>
  `;
}
