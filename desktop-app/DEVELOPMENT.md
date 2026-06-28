# WorldCupOddsPro 开发说明

## 推荐闭环升级

本版本保留原有比赛中心、预测中心、模拟对决、买球推荐、赔率异动、赛果中心、复盘中心、资金管理和数据源页面，在现有结构上新增推荐闭环。

### 预测层与投注层

- 预测层只负责输出真实概率：胜平负、让球胜平负、比分、总进球。
- 投注层基于模型概率、体彩去水概率、欧洲共识、当前赔率、公平赔率、EV、优势率、数据质量、赔率异常、首发状态和玩法风险判断是否值得投注。
- 推荐页面现在显示：模型概率、体彩去水、欧洲概率、公平赔率、当前赔率、EV、优势率、推荐等级、数据建议、是否值得投注、操作建议。

### 数据库迁移

新增表：

- `prediction_snapshots`：赛前冻结模型输入、赔率、概率、风险标签和数据质量。
- `bet_recommendations`：保存每条投注推荐的赛前状态。
- `match_results`：保存赛后比分结果。
- `bet_results`：预留投注结算结果表。
- `odds_anomalies`：保存赔率异常类型、严重度、影响方向和处理建议。
- `match_lineup_sources`：保存阵容来源、状态和置信度。
- `match_lineups`：保存球员级首发信息；历史首发率为 `start_rate`，不等于确认首发。
- `provider_raw_data`：保存多源原始字段，不覆盖原始数据。
- `provider_final_values`：保存融合后的最终字段值和置信度。

### 首发状态

`lineup_status` 支持：

- `unknown`
- `predicted`
- `probable`
- `reported`
- `confirmed`
- `official`

当 `confirmed_lineup_confidence < 80` 或状态低于 `confirmed` 时，推荐最高只能到小注/观察。

### 数据质量操作建议

- `<55`：建议跳过
- `55-65`：只看预测，不建议购买
- `65-75`：观察或极小注
- `75-85`：可小注
- `>=85`：可进入正式推荐

比分玩法默认只做观察；总进球默认观察或小注。

### 赔率异常

基于 `odds_snapshots` 自动写入 `odds_anomalies`，类型包括：

- 热门过热
- 机构分歧
- 临场降赔
- 反向升赔
- 临场剧烈波动

推荐页会读取本玩法最新异常，并自动降级高严重度异常。

### 今日下注方案

新增“今日方案”页面：

- 今日预算
- 最大亏损
- 单关候选
- 二串一候选
- 禁买清单
- 观察清单
- 等首发/等赔率提示
- 赛后复盘入口

仓位规则保守：

- 普通小注：0.25%-0.5%
- 重点关注：0.5%-1%
- 比分默认不下注或极小观察
- 串关总额不超过今日预算 20%

## 测试说明

后端规则测试：

```powershell
cd C:\Users\ADMIN\Documents\Codex\2026-06-23\nen\desktop-app\src-tauri
cargo test
```

后端编译：

```powershell
cargo check
```

前端构建：

```powershell
cd C:\Users\ADMIN\Documents\Codex\2026-06-23\nen\desktop-app
npm run build
```

桌面打包：

```powershell
npm run desktop:build
```

使用建议：

1. 在“比赛中心”刷新核心数据。
2. 在“买球推荐”查看预测层和投注层分离后的推荐。
3. 赛前点击“冻结赛前快照”。
4. 赛后在“赛果中心”刷新赛果，在“复盘中心”自动或手动结算。
5. 查看“增强回测结论”和“今日方案”，逐步形成禁买规则。
