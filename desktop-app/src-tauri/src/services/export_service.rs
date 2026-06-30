use serde_json::{json, Value};

pub fn export_security_summary() -> Value {
    json!({
        "service": "export_service",
        "api_key_policy": "备份和CSV导出不得包含 API Key 明文，只能显示 configured=true/false。",
        "failure_policy": "导出失败必须返回明确错误，不能影响软件运行。"
    })
}
