export function projectHealthHtml(projectHealth = {}) {
  const item = projectHealth || {};
  const flags = item.risk_flags || [];
  return `
    <section class="panel span-12">
      <h3>项目健康</h3>
      <div class="plan-grid">
        <div><h4>阶段</h4><p class="muted">${item.current_version || "v0.2-clean-core"}</p></div>
        <div><h4>main.js</h4><p class="${Number(item.main_js_size || 0) > 800 ? "up" : "muted"}">${item.main_js_size || 0} 行</p></div>
        <div><h4>commands.rs</h4><p class="${Number(item.commands_rs_size || 0) > 2000 ? "up" : "muted"}">${item.commands_rs_size || 0} 行</p></div>
        <div><h4>Commands / Tests</h4><p class="muted">${item.command_count || 0} / ${item.test_count || 0}</p></div>
        <div><h4>Services / Views</h4><p class="muted">${item.service_count || 0} / ${item.view_count || 0}</p></div>
        <div><h4>Tables</h4><p class="muted">${item.table_count || 0}</p></div>
      </div>
      <div class="notice">风险标记：${flags.length ? flags.join("；") : "暂无严重风险"}</div>
      <p class="muted">${(item.notes || []).join("；")}</p>
    </section>
  `;
}
