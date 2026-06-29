# 模型训练闭环 v0.1

本目录只训练“赛果胜平负概率模型”，不直接训练投注推荐模型。投注推荐仍由桌面端基于模型概率、赔率、EV、数据质量和风险标签独立判断。

桌面端 `v0.1-live-observation` 是真实赛前样本采集和纸面交易观察版，不作为自动投注工具；`live_pre_match` 样本先用于观察和后续研究，未达到样本、ROI、回撤和稳健性要求前不会进入正式推荐规则。

## 1. 获取原始数据

推荐自动下载 Football-Data.co.uk 历史 CSV：

```powershell
python scripts/download_football_data.py
```

脚本会把 CSV 缓存到：

```text
desktop-app/training/datasets/raw/
```

也可以手动把 CSV 放进这个目录。可以一次放多个 CSV。导入脚本会自动尝试 `utf-8-sig`、`utf-8`、`latin1`、`gbk` 编码读取。

## 2. 创建 Python 环境

```powershell
cd desktop-app/training
python -m venv .venv
.venv\Scripts\activate
pip install -r requirements.txt
```

## 3. 执行训练

```powershell
python scripts/import_football_data.py
python scripts/build_features.py
python scripts/train_outcome_model.py
python scripts/train_outcome_ensemble.py
python scripts/calibrate_probs.py
python scripts/train_probability_blend.py
python scripts/train_goals_model.py
python scripts/train_handicap_model.py
python scripts/backtest_strategy.py
python scripts/import_worldcup_history.py
python scripts/train_worldcup_correction.py
python scripts/export_models.py
```

或者一键执行：

```powershell
python scripts/download_football_data.py
python scripts/import_football_data.py
python scripts/build_features.py
python scripts/train_outcome_model.py
python scripts/train_outcome_ensemble.py
python scripts/calibrate_probs.py
python scripts/train_probability_blend.py
python scripts/train_goals_model.py
python scripts/train_handicap_model.py
python scripts/backtest_strategy.py
python scripts/import_worldcup_history.py
python scripts/train_worldcup_correction.py
python scripts/export_models.py
```

## 4. 输出文件

- `datasets/processed/matches.csv`：标准化后的 Football-Data 历史比赛。
- `datasets/processed/features.csv`：按时间顺序生成的赛前特征，禁止数据穿越。
- `models/feature_schema_v1.json`：特征字段定义。
- `models/outcome_model_v1.json`：胜平负 LogisticRegression 模型 JSON。
- `models/outcome_ensemble_model_v1.json`：胜平负集成模型 JSON。
- `models/calibrator_v1.json`：胜平负概率分桶校准器 JSON。
- `models/probability_blend_v1.json`：训练模型概率与市场去水概率的动态融合权重，按热门程度分段选择。
- `models/goals_home_model_v1.json` / `models/goals_away_model_v1.json`：主客队进球 Poisson 模型 JSON。
- `models/handicap_mapping_model_v1.json`：让球胜平负映射模型 JSON。
- `reports/outcome_metrics.json`：验证集准确率、Log Loss、Brier Score。
- `reports/backtest_report.csv`：验证集投注候选分组回测。
- `reports/backtest_summary.json`：整体回测表现，重点看 ROI、最大回撤和候选数量。
- `datasets/processed/worldcup_closure_samples.csv`：2018/2022世界杯历史赛前赔率与赛果样本，以及本届赛前闭环样本。
- `models/worldcup_live_correction_v1.json`：世界杯临场推荐置信修正层，只修正是否值得投注，不改写胜平负真实概率。
- `models/strategy_rules_v1.json`：基于回测生成的投注规则建议。
- `models/model_manifest.json`：桌面端读取的模型清单。

## 5. 桌面端读取模型

桌面端启动或预测时会查找：

```text
desktop-app/training/models/model_manifest.json
```

如果模型文件不存在、样本为空或结构不完整，会自动回退到现有规则模型，并在页面显示“规则模型 fallback”。

## 6. 重要边界

v0.8 主概率模型仍使用 Football-Data.co.uk 历史 CSV 训练。世界杯临场修正层会额外使用 Football-Data WorldCup Excel 中的 2018/2022 世界杯赛前赔率与赛果样本。本届赛前快照与赛后结算样本会继续追加到 `worldcup_closure_samples.csv`。

已经废弃固定命中率目标。当前训练核心目标是概率校准、Log Loss、Brier Score、ROI 和最大回撤控制；低赔率强热门命中率不再作为模型状态或推荐硬门槛。

当前训练核心已经改为“市场基准 + 模型修正 + 动态校准”。`build_features.py` 包含国家队/淘汰赛/赛程上下文字段，其中 Football-Data 联赛样本无法提供的字段会保持中性值；桌面端实时预测会用已有 FIFA 排名/Elo 代理和赛事阶段信息补足这些字段。
