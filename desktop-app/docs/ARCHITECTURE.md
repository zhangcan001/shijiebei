# v0.2-clean-core Architecture

本项目进入 `v0.2-clean-core` 后，目标是稳定运行、模块隔离和数据闭环，不再继续叠加新玩法。所有模块必须遵守：模型概率和投注推荐分离，正式推荐和冷门实验室隔离，复盘只产生观察结论，不自动改正式规则。

## 1. data_center

职责：

- 比赛列表、赛程、赛果
- 体彩赔率、欧洲赔率、赔率快照
- 数据源状态、缓存、请求额度
- Sporttery、The Odds API、football-data.org、OpenFootball、StatsBomb、TheSportsDB、Understat
- 数据质量评分和字段完整性诊断

边界：

- 只提供结构化数据和质量状态。
- 不输出投注建议。
- 不修改推荐规则。

## 2. prediction_center

职责：

- HAD 胜平负概率
- HHAD 让球胜平负概率
- TTG 总进球概率
- CRS 比分概率
- 世界杯比分先验
- 模型版本和 fallback 状态

边界：

- 只输出概率、分布、置信和解释。
- 不判断是否买。
- 不读取 `upset_lab_candidates`。
- 比分先验只能轻度融合，不能覆盖模型概率。

## 3. recommendation_center

职责：

- 今日主推
- 小注候选
- 观察玩法
- 禁买清单
- 仓位建议
- hard_ban 风控

边界：

- 只处理常规推荐。
- 不允许读取冷门实验室候选。
- 不允许把 `scan_only`、`no_odds_scan`、`paper_candidate`、`tiny_stake_candidate` 升级为正式推荐。
- `hard_ban` 永远最高优先级。

## 4. upset_lab

职责：

- 冷平
- 让球爆冷
- 强队险胜
- 极端总进球
- 高赔率比分
- 3:3 专项

允许输出：

- `scan_only`
- `no_odds_scan`
- `paper_candidate`
- `tiny_stake_candidate`
- `forbidden`

边界：

- 冷门实验室是高风险实验区。
- 候选不得进入今日主推、正式推荐或小注候选。
- `no_odds_scan` 不得写入可结算纸面交易。
- `paper_candidate` 必须有赔率才允许写入纸面交易。
- `hard_ban` 命中时，无论冷门分、混沌分多高，都只能是 `forbidden`。

## 5. review_training_center

职责：

- 赛前快照
- 纸面交易
- 赛后结算
- 规则误杀诊断
- 稳健性检查
- 训练样本导出
- 单日复盘摘要

边界：

- 只生成复盘结论、review_note 和候选调整建议。
- 单日样本不能自动改模型或推荐规则。
- `candidate_adjustment` 默认 `observation_only`，必须人工确认后才可进入策略候选。
- 复盘不能自动写入正式推荐规则、比分权重、总进球权重、冷门入池规则、hard_ban 或 observe_only 规则。

## Fixed Snapshot Flow

唯一赛前数据流：

```text
list_matches
→ refresh odds / providers
→ create_pre_match_snapshot
→ mark_final_pre_match_snapshot
→ recommendation / upset_lab 基于 snapshot 生成
→ settle_pre_match_snapshot
→ review
```

规则：

- 没有快照时，今日方案可用即时预测，但必须标注“非冻结快照”。
- 冷门实验室可做 `no_odds_scan`，但必须标注“无赔率，仅剧本扫描”。
- 所有可结算纸面交易必须来自赛前快照，并且有赔率。
- 赛后结算不得覆盖快照冻结字段。

## Overfit Guard

单日复盘只允许生成观察标签：

- 单日样本 `< 10`：只生成 `review_note`。
- 单日样本 `< 30`：不能生成 `candidate_adjustment`。
- 至少连续 3 个比赛日出现同类问题，才允许生成 `candidate_adjustment`。
- `candidate_adjustment` 默认 `observation_only`。

禁止把单日现象写成硬规则：

- 淘汰赛默认 1:1
- 强队热门默认冷平
- 强队让球默认不穿盘
- 总进球默认 2球
- 弱队默认必进球
- 1:0 / 2:0 永久降权
- 1:1 永久升权

## Frontend Refactor Target

目标结构：

```text
src/
  main.js
  state.js
  api.js
  utils/
    format.js
    render.js
  views/
    TodayView.js
    PredictionView.js
    MatchView.js
    SimulationView.js
    OddsMovementView.js
    UpsetLabView.js
    ResultsView.js
    ReviewView.js
    SourcesView.js
  components/
    ScorePriorCard.js
    ModelStatusCard.js
    RefreshStatusBar.js
    DataSourceCards.js
    TableHelpers.js
```

当前 `main.js` 仍偏大，项目健康报告会持续标记 `giant_frontend_file`，直到完成页面级迁移。

## Backend Refactor Target

目标结构：

```text
src-tauri/src/commands/
  mod.rs
  data_commands.rs
  prediction_commands.rs
  recommendation_commands.rs
  upset_lab_commands.rs
  review_commands.rs
  snapshot_commands.rs
  export_commands.rs
  provider_commands.rs
  system_commands.rs
```

当前后端已迁入 `src-tauri/src/commands/mod.rs`，但该文件仍偏大。项目健康报告会持续标记 `giant_commands_file`，直到 command 层继续拆成 `*_commands.rs` facade，并把业务逻辑下沉到 services。
