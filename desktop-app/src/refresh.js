export function refreshFacadeStatus() {
  return {
    delegated_to: "legacyMain",
    status: "clean_core_facade",
    note: "刷新入口已预留；当前为兼容层，后续逐步从 legacyMain.js 迁移刷新流程。"
  };
}
