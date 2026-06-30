import { pct, odds, signedPct, money, rankedTeam } from "../utils/format.js";
import { scorePriorCardHtml } from "../components/ScorePriorCard.js";

// TODO: pending split phase 2 - move cold-lab data shaping from legacyMain into this view.
function badge(text) {
  const cls = /可买|盈利|正/.test(text) ? "good" : /观察|等待|小注|中/.test(text) ? "warn" : "bad";
  return `<span class="badge ${cls}">${text || "-"}</span>`;
}

function poolLabel(pool = "") {
  const labels = {
    cold_draw_pool: "冷平池",
    handicap_upset_pool: "让球爆冷池",
    favorite_narrow_win_pool: "强队险胜池",
    underdog_first_goal_script_pool: "弱队先进球剧本池",
    half_fulltime_reversal_pool: "半全场反转池",
    extreme_total_goals_pool: "总进球极端池",
    high_odds_score_pool: "高赔率比分池",
    score_3_3_pool: "3:3 专项池",
    forbidden_upset_pool: "禁碰冷门"
  };
  return labels[pool] || pool || "-";
}

function decisionLabel(decision = "") {
  const labels = {
    no_odds_scan: "剧本扫描",
    scan_only: "扫描观察",
    paper_candidate: "纸面候选",
    tiny_stake_candidate: "极小仓位候选",
    observe_only: "观察",
    forbidden: "禁碰",
    wait_for_lineup: "等待阵容",
    wait_for_odds: "等待赔率",
    insufficient_data: "数据不足"
  };
  return labels[decision] || decision || "-";
}

function reasonText(value) {
  if (!value) return "-";
  if (Array.isArray(value)) return value.map(item => typeof item === "string" ? item : JSON.stringify(item)).join("；");
  if (value.reasons && Array.isArray(value.reasons)) return value.reasons.join("；");
  if (value.risk_text) return value.risk_text;
  return typeof value === "string" ? value : JSON.stringify(value);
}

function upsetRows(state, pool, emptyText = "暂无候选。") {
  const rows = (state.upsetLabCandidates || []).filter(item => item.play_pool === pool);
  if (!rows.length) return `<tr><td colspan="14" class="muted">${emptyText}</td></tr>`;
  return rows.map(item => {
    const decision = item.final_lab_decision || "";
    const noOdds = decision === "no_odds_scan";
    const rowClass = decision === "forbidden" ? "prediction-miss" : decision === "tiny_stake_candidate" ? "row-watch" : "";
    return `
      <tr class="${rowClass}">
        <td>${item.match_time || "-"}</td>
        <td><strong>${rankedTeam(item.home_team)} vs ${rankedTeam(item.away_team)}</strong><div class="muted">${item.stage || item.source_snapshot_type || "-"}</div></td>
        <td>${poolLabel(item.play_pool)}<div class="muted">${item.play_type || "-"}</div></td>
        <td><strong>${item.selection || "-"}</strong></td>
        <td>${noOdds || item.odds == null ? "-" : odds(item.odds)}</td>
        <td>${pct(item.model_prob)}</td>
        <td>${noOdds || item.market_prob == null ? "-" : pct(item.market_prob)}</td>
        <td>${noOdds || item.ev == null ? "-" : signedPct(item.ev)}</td>
        <td>${Number(item.scan_score || 0).toFixed(0)}</td>
        <td>${Number(item.upset_score || 0).toFixed(0)}</td>
        <td>${Number(item.chaos_score || 0).toFixed(0)}</td>
        <td>${badge(decisionLabel(decision))}<div class="muted">${item.risk_level || "-"}</div></td>
        <td>${money(item.stake_pct || 0)}<div class="muted">${item.stake_advice || "纸面观察"}</div></td>
        <td class="muted">${reasonText(decision === "forbidden" ? item.block_reasons : item.trigger_reasons)}</td>
      </tr>
    `;
  }).join("");
}

function backtestRows(rows = []) {
  if (!rows.length) return `<tr><td colspan="10" class="muted">暂无已结算纸面交易样本。</td></tr>`;
  return rows.map(item => `
    <tr>
      <td>${poolLabel(item.group)}</td>
      <td>${item.bet_count || 0}</td>
      <td>${item.hit_count || 0}</td>
      <td>${pct(item.hit_rate)}</td>
      <td class="${Number(item.roi || 0) >= 0 ? "down" : "up"}">${signedPct(item.roi)}</td>
      <td>${signedPct(item.total_profit || 0)}</td>
      <td>${signedPct(item.max_drawdown || 0)}</td>
      <td>${odds(item.avg_odds)}</td>
      <td>${signedPct(item.avg_ev)}</td>
      <td>${Number(item.avg_scan_score || 0).toFixed(0)} / ${Number(item.avg_upset_score || 0).toFixed(0)} / ${Number(item.avg_chaos_score || 0).toFixed(0)}</td>
    </tr>
  `).join("");
}

function funnelHtml(state) {
  const funnel = state.upsetLabSummary?.scan_funnel || {};
  return `
    <section class="panel span-12">
      <h3>空态过滤漏斗</h3>
      <div class="plan-grid">
        <div><h4>今日比赛</h4><p class="muted">${funnel.match_count || 0}</p></div>
        <div><h4>赛前快照</h4><p class="muted">${funnel.snapshot_count || 0}</p></div>
        <div><h4>有赔率比赛</h4><p class="muted">${funnel.odds_match_count || 0}</p></div>
        <div><h4>基础赔率区间</h4><p class="muted">${funnel.base_odds_band_count || 0}</p></div>
        <div><h4>冷平扫描</h4><p class="muted">${funnel.cold_draw_scan_count || 0}</p></div>
        <div><h4>让球爆冷扫描</h4><p class="muted">${funnel.handicap_scan_count || 0}</p></div>
        <div><h4>比分扫描</h4><p class="muted">${funnel.score_scan_count || 0}</p></div>
        <div><h4>hard_ban/禁碰</h4><p class="muted">${funnel.hard_ban_count || 0}</p></div>
        <div><h4>数据不足</h4><p class="muted">${funnel.data_insufficient_count || 0}</p></div>
        <div><h4>赔率缺失</h4><p class="muted">${funnel.missing_odds_count || 0}</p></div>
      </div>
    </section>
  `;
}

function robustnessHtml(state) {
  const item = state.upsetLabRobustness || {};
  const reasons = item.blocking_reasons || [];
  const warnings = item.warnings || [];
  return `
    <section class="panel span-12">
      <h3>稳健性观察</h3>
      <div class="plan-grid">
        <div><h4>稳健性等级</h4><p class="muted">${badge(item.robustness_level || "weak")}</p></div>
        <div><h4>样本数</h4><p class="muted">${item.bet_count || 0}</p></div>
        <div><h4>整体 ROI</h4><p class="${Number(item.roi || 0) >= 0 ? "down" : "up"}">${signedPct(item.roi || 0)}</p></div>
        <div><h4>最近 30 / 50</h4><p class="muted">${signedPct(item.rolling_30_roi || 0)} / ${signedPct(item.rolling_50_roi || 0)}</p></div>
        <div><h4>去 Top5 后</h4><p class="${Number(item.roi_after_remove_top5 || 0) >= 0 ? "down" : "up"}">${signedPct(item.roi_after_remove_top5 || 0)}</p></div>
        <div><h4>极小仓位</h4><p class="muted">${item.can_consider_tiny_stake ? "可考虑，但不自动启用" : "不可升级"}</p></div>
      </div>
      <p class="muted">${[...reasons, ...warnings].join("；") || "样本仍在观察中。"}</p>
    </section>
  `;
}

function debugHtml(state) {
  const debug = state.upsetLabDebug || {};
  const funnel = debug.data_funnel || state.upsetLabSummary?.scan_funnel || {};
  return `
    <section class="panel span-12">
      <h3>调试信息</h3>
      <p class="muted">empty_reason：${debug.empty_reason || funnel.empty_reason || "-"}</p>
      <div class="plan-grid">
        <div><h4>前 5 场比赛</h4><p class="muted">${(debug.first_5_today_matches || []).map(item => `${item.home_team} vs ${item.away_team}`).join("；") || "-"}</p></div>
        <div><h4>快照ID</h4><p class="muted">${(debug.snapshot_match_ids || []).slice(0, 8).join("；") || "-"}</p></div>
        <div><h4>赔率ID</h4><p class="muted">${(debug.odds_match_ids || []).slice(0, 8).join("；") || "-"}</p></div>
        <div><h4>候选预览</h4><p class="muted">${(debug.generated_candidates_preview || []).map(item => `${item.play_pool || "-"}:${item.selection || "-"}`).join("；") || "-"}</p></div>
      </div>
    </section>
  `;
}

export function renderUpsetLabView(state) {
  const summary = state.upsetLabSummary || {};
  const backtest = state.upsetLabBacktest || {};
  const funnel = summary.scan_funnel || {};
  const noOddsMode = Number(funnel.generated_no_odds_scan_count || 0) > 0 || Number(funnel.odds_missing_count || funnel.missing_odds_count || 0) > 0;
  const noSnapshotMode = Number(funnel.pre_match_snapshot_count ?? funnel.snapshot_count ?? 0) === 0 && Number(funnel.today_matches_count ?? funnel.match_count ?? 0) > 0;
  const pools = [
    ["cold_draw_pool", "冷平池"],
    ["handicap_upset_pool", "让球爆冷池"],
    ["favorite_narrow_win_pool", "强队险胜池"],
    ["underdog_first_goal_script_pool", "弱队先进球剧本池"],
    ["half_fulltime_reversal_pool", "半全场反转池"],
    ["extreme_total_goals_pool", "总进球极端池"],
    ["high_odds_score_pool", "高赔率比分池"],
    ["score_3_3_pool", "3:3 专项池"],
    ["forbidden_upset_pool", "禁碰冷门"]
  ];
  return `
    <div class="grid">
      <section class="panel span-12 toolbar">
        <button class="btn" data-action="generate-upset-lab">生成冷门候选</button>
        <button class="btn secondary" data-action="create-upset-paper">写入纸面交易</button>
        <button class="btn secondary" data-action="settle-upset-paper">结算纸面交易</button>
        <span class="muted">冷门实验室为高赔率高风险实验玩法，只建议纸面观察或极小仓位，不影响正式推荐，不保证盈利。</span>
      </section>
      <section class="panel span-12 notice">冷门候选不会进入今日主推、正式推荐或小注候选；hard_ban 永远最高优先级。比分、3:3、半全场默认高风险。</section>
      ${scorePriorCardHtml(state.scorePriors?.summary || state.practicalAdvice?.score_prior || {}, pct)}
      ${noOddsMode ? `<section class="panel span-12 notice">当前缺少赔率数据，已切换为冷门剧本扫描模式。以下内容仅用于判断冷门可能性，不计算EV，不建议下注。</section>` : ""}
      ${noSnapshotMode ? `<section class="panel span-12 notice">当前无赛前快照，已使用即时模型/静态球队数据进行轻量扫描。建议先生成赛前快照以提高准确度。</section>` : ""}
      <section class="panel span-3 metric"><span>总扫描候选</span><strong>${summary.candidate_count || 0}</strong><div class="muted">${summary.warning || "等待生成"}</div></section>
      <section class="panel span-3 metric"><span>剧本扫描</span><strong>${summary.no_odds_scan_count || 0}</strong><div class="muted">无赔率，不计算EV</div></section>
      <section class="panel span-3 metric"><span>可交易候选</span><strong>${(summary.paper_candidate_count || 0) + (summary.tiny_stake_candidate_count || 0)}</strong><div class="muted">可写入纸面交易</div></section>
      <section class="panel span-3 metric"><span>扫描观察</span><strong>${summary.scan_only_count || 0}</strong><div class="muted">仅展示，不建议下注</div></section>
      <section class="panel span-3 metric"><span>纸面候选</span><strong>${summary.paper_candidate_count || 0}</strong><div class="muted">不进入正式推荐</div></section>
      <section class="panel span-3 metric"><span>极小仓位候选</span><strong>${summary.tiny_stake_candidate_count || 0}</strong><div class="muted">仅冷门实验室显示</div></section>
      <section class="panel span-3 metric"><span>禁碰冷门</span><strong>${summary.forbidden_count || 0}</strong><div class="muted">hard_ban / 深负EV / 数据无效</div></section>
      <section class="panel span-3 metric"><span>赔率缺失比赛</span><strong>${funnel.odds_missing_count ?? funnel.missing_odds_count ?? 0}</strong><div class="muted">自动走剧本扫描</div></section>
      <section class="panel span-3 metric"><span>快照缺失比赛</span><strong>${Math.max(0, Number(funnel.today_matches_count ?? funnel.match_count ?? 0) - Number(funnel.pre_match_snapshot_count ?? funnel.snapshot_count ?? 0))}</strong><div class="muted">使用轻量扫描</div></section>
      ${(summary.candidate_count || 0) === 0 ? funnelHtml(state) : ""}
      <section class="panel span-12"><h3>预算与暂停规则</h3><div class="plan-grid">
        <div><h4>单日预算上限</h4><p class="muted">${pct(summary.max_daily_budget_ratio ?? 0.005)}</p></div>
        <div><h4>连续亏损</h4><p class="${summary.pause_triggered ? "up" : "muted"}">${summary.consecutive_losses || 0} 单</p></div>
        <div><h4>模式</h4><p class="muted">${summary.default_mode || "paper_only"}</p></div>
        <div><h4>状态</h4><p class="muted">${summary.paper_only_triggered ? "只允许纸面" : summary.pause_triggered ? "建议暂停" : "观察中"}</p></div>
      </div></section>
      <section class="panel span-12"><h3>纸面交易回测</h3><div class="plan-grid">
        <div><h4>样本数</h4><p class="muted">${backtest.bet_count || 0}</p></div>
        <div><h4>命中率</h4><p class="muted">${pct(backtest.hit_rate || 0)}</p></div>
        <div><h4>ROI</h4><p class="${Number(backtest.roi || 0) >= 0 ? "down" : "up"}">${signedPct(backtest.roi || 0)}</p></div>
        <div><h4>最大回撤</h4><p class="muted">${signedPct(backtest.max_drawdown || 0)}</p></div>
        <div><h4>平均赔率</h4><p class="muted">${odds(backtest.avg_odds || 0)}</p></div>
        <div><h4>提示</h4><p class="muted">${backtest.warning || "等待更多结算样本"}</p></div>
      </div></section>
      ${robustnessHtml(state)}
      ${debugHtml(state)}
      <section class="panel span-12 table-panel"><h3>按玩法池纸面表现</h3><div class="scroll-table"><table><thead><tr><th>池</th><th>样本</th><th>命中</th><th>命中率</th><th>ROI</th><th>盈亏</th><th>回撤</th><th>均赔</th><th>均EV</th><th>扫描/冷门/混沌</th></tr></thead><tbody>${backtestRows(backtest.by_play_pool || [])}</tbody></table></div></section>
      ${pools.map(([pool, title]) => `<section class="panel span-12 table-panel"><h3>${title}</h3><div class="scroll-table"><table><thead><tr><th>时间</th><th>比赛</th><th>池/玩法</th><th>选择</th><th>赔率</th><th>模型</th><th>市场</th><th>EV</th><th>扫描分</th><th>冷门分</th><th>混沌分</th><th>决策</th><th>仓位</th><th>理由</th></tr></thead><tbody>${upsetRows(state, pool, pool === "forbidden_upset_pool" ? "暂无禁碰冷门。" : "今日无明显候选，但已完成扫描。")}</tbody></table></div></section>`).join("")}
    </div>
  `;
}
