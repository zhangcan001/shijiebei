pub(crate) fn odds_bucket(odds: f64) -> String {
    if odds < 1.8 {
        "1.00-1.79".to_string()
    } else if odds < 2.5 {
        "1.80-2.49".to_string()
    } else if odds < 4.0 {
        "2.50-3.99".to_string()
    } else if odds < 6.0 {
        "4.00-5.99".to_string()
    } else {
        "6.00+".to_string()
    }
}

pub(crate) fn probability_bucket(probability: f64) -> String {
    if probability < 0.2 {
        "0%-20%".to_string()
    } else if probability < 0.35 {
        "20%-35%".to_string()
    } else if probability < 0.5 {
        "35%-50%".to_string()
    } else if probability < 0.65 {
        "50%-65%".to_string()
    } else {
        "65%+".to_string()
    }
}

pub(crate) fn data_quality_bucket(score: f64) -> String {
    if score < 55.0 {
        "<55 建议跳过".to_string()
    } else if score < 65.0 {
        "55-65 只看预测".to_string()
    } else if score < 75.0 {
        "65-75 观察".to_string()
    } else if score < 85.0 {
        "75-85 可小注".to_string()
    } else {
        "85+ 正式推荐".to_string()
    }
}

pub(crate) fn max_drawdown_from_profit(items: &[f64]) -> f64 {
    let mut equity = 0.0;
    let mut peak = 0.0;
    let mut max_dd = 0.0;
    for profit in items {
        equity += profit;
        if equity > peak {
            peak = equity;
        }
        let dd = peak - equity;
        if dd > max_dd {
            max_dd = dd;
        }
    }
    max_dd
}

pub(crate) fn roi_from_profit(profit_sum: f64, stake_sum: f64) -> f64 {
    if stake_sum > 0.0 {
        profit_sum / stake_sum
    } else {
        0.0
    }
}
