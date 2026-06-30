export function scorePriorCardHtml(summary = {}, pct) {
  const percent = pct || ((value) => `${(Number(value || 0) * 100).toFixed(1)}%`);
  return `
    <section class="panel span-12">
      <h3>近两届世界杯淘汰赛90分钟比分先验</h3>
      <p class="muted">${summary.message || "本系统按竞彩口径统计90分钟比分，加时和点球不计入比分先验。该先验只用于比分参考和剧本扫描，不直接构成下注建议。"}</p>
      <div class="plan-grid">
        <div><h4>样本</h4><p class="muted">${summary.sample_count || 32} 场</p></div>
        <div><h4>90分钟平局</h4><p class="muted">${percent(summary.draw_90min ?? 0.3125)}</p></div>
        <div><h4>最高频比分</h4><p class="muted">1-1 · ${percent(summary.score_1_1 ?? 0.1875)}</p></div>
        <div><h4>2-1 型</h4><p class="muted">${percent(summary.score_2_1_type ?? 0.15625)}</p></div>
        <div><h4>2-0 型</h4><p class="muted">${percent(summary.score_2_0_type ?? 0.15625)}</p></div>
        <div><h4>1-0 型</h4><p class="muted">${percent(summary.score_1_0_type ?? 0.09375)}</p></div>
        <div><h4>0-0 / 2-2</h4><p class="muted">${percent(summary.score_0_0 ?? 0.0625)} / ${percent(summary.score_2_2 ?? 0.0625)}</p></div>
        <div><h4>3-3</h4><p class="up">${percent(summary.score_3_3 ?? 0)} · 极端低频</p></div>
        <div><h4>2球</h4><p class="muted">${percent(summary.two_goals ?? 0.34375)}</p></div>
        <div><h4>3球 / 4球</h4><p class="muted">${percent(summary.three_goals ?? 0.21875)} / ${percent(summary.four_goals ?? 0.125)}</p></div>
        <div><h4>小于2.5</h4><p class="muted">${percent(summary.under_2_5 ?? 0.5)}</p></div>
        <div><h4>大于2.5</h4><p class="muted">${percent(summary.over_2_5 ?? 0.5)}</p></div>
      </div>
    </section>
  `;
}
