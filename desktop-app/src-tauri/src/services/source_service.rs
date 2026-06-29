#[allow(dead_code)]
pub(crate) fn provider_field_confidence(base: f64, freshness: f64, completeness: f64, consistency: f64) -> f64 {
    (base * freshness * completeness * consistency).clamp(0.0, 100.0)
}

pub(crate) fn cache_freshness_score(updated_at: &str) -> f64 {
    let Ok(ts) = chrono::DateTime::parse_from_rfc3339(updated_at) else {
        return 35.0;
    };
    let age_hours = (Utc::now() - ts.with_timezone(&Utc)).num_minutes().max(0) as f64 / 60.0;
    if age_hours <= 2.0 {
        100.0
    } else if age_hours <= 12.0 {
        85.0
    } else if age_hours <= 24.0 {
        70.0
    } else if age_hours <= 72.0 {
        45.0
    } else {
        20.0
    }
}

pub(crate) fn source_completeness_score(key: &str, count: usize) -> f64 {
    let expected = match key {
        "sporttery" => 6.0,
        "europe_odds" => 8.0,
        "statsbomb_xg" | "stats_data" => 16.0,
        "match_results" | "historical_results" => 16.0,
        "injury_data" | "player_status_data" | "lineup_data" => 1.0,
        _ => 1.0,
    };
    ((count as f64 / expected).min(1.0) * 100.0).clamp(0.0, 100.0)
}

pub(crate) fn source_health_label(ok: bool, freshness: f64, completeness: f64, using_stale_cache: bool) -> String {
    if !ok {
        return "字段缺失".to_string();
    }
    if using_stale_cache {
        return "失败但使用旧缓存".to_string();
    }
    if completeness < 35.0 {
        return "字段缺失".to_string();
    }
    if freshness < 50.0 {
        return "过期".to_string();
    }
    "正常".to_string()
}

pub(crate) fn ensure_provider_registry(conn: &Connection) -> anyhow::Result<()> {
    for provider in default_provider_registry() {
        conn.execute(
            "insert into data_providers(provider_id, name, data_type, requires_key, base_confidence, enabled, daily_limit, hourly_limit, supported_data_types)
             values(?1, ?2, ?3, ?4, ?5, 1, ?6, ?7, ?8)
             on conflict(provider_id) do update set
               name=excluded.name,
               data_type=excluded.data_type,
               requires_key=excluded.requires_key,
               base_confidence=excluded.base_confidence,
               daily_limit=excluded.daily_limit,
               hourly_limit=excluded.hourly_limit,
               supported_data_types=excluded.supported_data_types",
            params![
                provider.provider_id,
                provider.name,
                provider.supported_data_types.join(","),
                if provider.requires_key { 1 } else { 0 },
                provider.base_confidence,
                provider.daily_limit,
                provider.hourly_limit,
                provider.supported_data_types.join(",")
            ],
        )?;
    }
    Ok(())
}

pub(crate) fn provider_api_key(conn: &Connection, provider_id: &str) -> anyhow::Result<Option<String>> {
    conn.query_row(
        "select api_key from provider_credentials where provider_id=?1",
        params![provider_id],
        |row| row.get(0),
    )
    .optional()
    .map_err(Into::into)
}

pub(crate) fn save_provider_api_key(conn: &Connection, provider_id: &str, api_key: &str) -> anyhow::Result<()> {
    conn.execute(
        "insert into provider_credentials(provider_id, api_key, updated_at) values(?1, ?2, ?3)
         on conflict(provider_id) do update set api_key=excluded.api_key, updated_at=excluded.updated_at",
        params![provider_id, api_key, Utc::now().to_rfc3339()],
    )?;
    Ok(())
}

pub(crate) fn clear_provider_api_key(conn: &Connection, provider_id: &str) -> anyhow::Result<()> {
    conn.execute("delete from provider_credentials where provider_id=?1", params![provider_id])?;
    Ok(())
}

pub(crate) fn log_provider_request(conn: &Connection, provider_id: &str, data_type: &str, success: bool, error_message: &str) -> anyhow::Result<()> {
    conn.execute(
        "insert into provider_request_logs(provider_id, data_type, requested_at, success, error_message) values(?1, ?2, ?3, ?4, ?5)",
        params![provider_id, data_type, Utc::now().to_rfc3339(), if success { 1 } else { 0 }, error_message],
    )?;
    Ok(())
}

pub(crate) fn provider_request_count(conn: &Connection, provider_id: &str, hours: i64) -> i64 {
    conn.query_row(
        "select count(*) from provider_request_logs where provider_id=?1 and requested_at >= datetime('now', ?2)",
        params![provider_id, format!("-{} hours", hours)],
        |row| row.get(0),
    )
    .unwrap_or(0)
}

pub(crate) fn request_limit_available(conn: &Connection, provider_id: &str, daily_limit: i64, hourly_limit: i64) -> bool {
    let daily_ok = daily_limit <= 0 || provider_request_count(conn, provider_id, 24) < daily_limit;
    let hourly_ok = hourly_limit <= 0 || provider_request_count(conn, provider_id, 1) < hourly_limit;
    daily_ok && hourly_ok
}

#[allow(dead_code)]
pub(crate) fn provider_key_error(requires_key: bool, key_configured: bool) -> Option<&'static str> {
    if requires_key && !key_configured {
        Some("API Key 未配置，请先保存本地 Key")
    } else {
        None
    }
}

#[allow(dead_code)]
pub(crate) fn save_provider_raw_record(conn: &Connection, record: &ProviderRawRecord) -> anyhow::Result<()> {
    conn.execute(
        "insert into provider_raw_data(provider_id, provider, data_type, match_id, team, field_name, field_value, fetched_at, confidence, raw_payload)
         values(?1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            record.provider_id,
            record.data_type,
            record.match_id,
            record.team,
            record.field_name,
            record.field_value,
            record.fetched_at,
            record.confidence,
            record.raw_payload
        ],
    )?;
    Ok(())
}

pub(crate) fn list_data_providers(conn: &Connection) -> anyhow::Result<Vec<DataProvider>> {
    ensure_provider_registry(conn)?;
    let mut stmt = conn.prepare(
        "select p.provider_id, p.name, p.data_type, p.requires_key, p.base_confidence, p.enabled, p.daily_limit, p.hourly_limit,
                coalesce(h.last_success_at, p.last_success_at), coalesce(h.last_error_at, p.last_error_at),
                coalesce(nullif(h.last_error_message,''), p.last_error_message),
                coalesce(h.freshness_score, p.freshness_score), coalesce(h.completeness_score, p.completeness_score),
                coalesce(h.confidence_score, p.confidence_score), coalesce(h.using_stale_cache, p.using_stale_cache),
                p.supported_data_types,
                case when c.provider_id is null then 0 else 1 end,
                (select count(*) from provider_request_logs r where r.provider_id=p.provider_id and r.requested_at >= datetime('now','-24 hours')),
                (select count(*) from provider_request_logs r where r.provider_id=p.provider_id and r.requested_at >= datetime('now','-1 hours'))
         from data_providers p
         left join source_health h on h.provider_id=p.provider_id
         left join provider_credentials c on c.provider_id=p.provider_id
         order by p.base_confidence desc",
    )?;
    let rows = stmt.query_map([], |row| {
        let freshness: f64 = row.get(11)?;
        let completeness: f64 = row.get(12)?;
        let confidence: f64 = row.get(13)?;
        let stale_int: i64 = row.get(14)?;
        let enabled_int: i64 = row.get(5)?;
        let key_int: i64 = row.get(16)?;
        let supported: String = row.get(15)?;
        let health_label = source_health_label(confidence > 0.0, freshness, completeness, stale_int != 0);
        Ok(DataProvider {
            provider_id: row.get(0)?,
            name: row.get(1)?,
            data_type: row.get(2)?,
            requires_key: row.get::<_, i64>(3)? != 0,
            base_confidence: row.get(4)?,
            enabled: enabled_int != 0,
            daily_limit: row.get(6)?,
            hourly_limit: row.get(7)?,
            last_success_at: row.get(8)?,
            last_error_at: row.get(9)?,
            last_error_message: row.get(10)?,
            freshness_score: freshness,
            completeness_score: completeness,
            confidence_score: confidence,
            using_stale_cache: stale_int != 0,
            supported_data_types: supported.split(',').filter(|item| !item.is_empty()).map(str::to_string).collect(),
            key_configured: key_int != 0,
            today_requests: row.get(17)?,
            hour_requests: row.get(18)?,
            health_label,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};

use crate::models::{DataProvider, ProviderRawRecord};
use crate::services::providers::default_provider_registry;
