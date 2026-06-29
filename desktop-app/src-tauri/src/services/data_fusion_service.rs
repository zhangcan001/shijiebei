#![allow(dead_code)]

use std::collections::BTreeMap;

use crate::models::{FusionResult, ProviderRawRecord};

pub(crate) fn source_agreement_score(values: &[String]) -> (f64, bool) {
    if values.is_empty() {
        return (0.0, false);
    }
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for value in values {
        *counts.entry(value.trim().to_lowercase()).or_default() += 1;
    }
    let max_count = counts.values().copied().max().unwrap_or(1);
    let conflict = counts.len() > 1;
    let agreement = max_count as f64 / values.len() as f64;
    let score = if values.len() >= 2 && !conflict {
        1.12
    } else if conflict {
        (0.55 + agreement * 0.35).clamp(0.45, 0.85)
    } else {
        1.0
    };
    (score, conflict)
}

pub(crate) fn final_field_confidence(
    base_confidence: f64,
    freshness_score: f64,
    completeness_score: f64,
    source_agreement_score: f64,
) -> f64 {
    (base_confidence * (freshness_score / 100.0) * (completeness_score / 100.0) * source_agreement_score)
        .clamp(0.0, 100.0)
}

pub(crate) fn fuse_provider_records(records: &[ProviderRawRecord], base_confidence: f64, freshness_score: f64, completeness_score: f64) -> Option<FusionResult> {
    let first = records.first()?;
    let values = records.iter().map(|record| record.field_value.clone()).collect::<Vec<_>>();
    let (agreement, conflict) = source_agreement_score(&values);
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for value in &values {
        *counts.entry(value.clone()).or_default() += 1;
    }
    let final_value = counts
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(value, _)| value)
        .unwrap_or_else(|| first.field_value.clone());
    Some(FusionResult {
        data_type: first.data_type.clone(),
        match_id: first.match_id.clone(),
        team: first.team.clone(),
        field_name: first.field_name.clone(),
        final_value,
        confidence: final_field_confidence(base_confidence, freshness_score, completeness_score, agreement),
        provider_count: records.len() as i64,
        conflict,
    })
}

pub(crate) fn lineup_status_from_source(source_status: &str, start_rate: f64, confirmed: bool) -> (&'static str, f64) {
    if confirmed {
        return ("api_confirmed", 85.0);
    }
    match source_status {
        "official" => ("official", 95.0),
        "api_confirmed" => ("api_confirmed", 85.0),
        "reported" => ("reported", 72.0),
        "predicted" => ("predicted", 55.0),
        _ if start_rate > 0.0 => ("historical", (start_rate * 45.0).clamp(5.0, 45.0)),
        _ => ("unknown", 0.0),
    }
}

pub(crate) fn downgrade_for_missing_realtime_xg(market: &str, decision: &mut String, confidence: &mut String, reason: &mut Vec<String>) {
    if market.starts_with("CRS") || market.starts_with("TTG") {
        if decision == "可买" {
            *decision = "观察".to_string();
        }
        if confidence == "高" {
            *confidence = "中".to_string();
        }
        reason.push("xG 数据缺失，比分/总进球仅供观察".to_string());
    }
}
