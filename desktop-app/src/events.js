export function bindAppEvents() {
  return {
    delegated_to: "legacyMain",
    status: "clean_core_facade",
    note: "事件处理入口已预留；当前为兼容层，后续逐步从 legacyMain.js 迁移 data-action 处理。"
  };
}
