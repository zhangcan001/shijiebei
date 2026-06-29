#![allow(dead_code)]

use anyhow::{anyhow, Result};
use serde_json::Value;

use crate::models::ProviderRegistryItem;

pub mod openfootball_provider;
pub mod football_data_uk_provider;
pub mod api_football_provider;
pub mod odds_api_io_provider;
pub mod football_data_org_provider;
pub mod statsbomb_provider;
pub mod thesportsdb_provider;
pub mod understat_provider;

pub(crate) trait DataProviderAdapter {
    fn provider_id(&self) -> &'static str;
    fn supported_data_types(&self) -> &'static [&'static str];

    fn fetch_fixtures(&self) -> Result<Value> { Err(not_supported(self.provider_id(), "fixtures")) }
    fn fetch_results(&self) -> Result<Value> { Err(not_supported(self.provider_id(), "results")) }
    fn fetch_odds(&self) -> Result<Value> { Err(not_supported(self.provider_id(), "odds")) }
    fn fetch_lineups(&self) -> Result<Value> { Err(not_supported(self.provider_id(), "lineups")) }
    fn fetch_injuries(&self) -> Result<Value> { Err(not_supported(self.provider_id(), "injuries")) }
    fn fetch_stats(&self) -> Result<Value> { Err(not_supported(self.provider_id(), "stats")) }
    fn fetch_xg(&self) -> Result<Value> { Err(not_supported(self.provider_id(), "xg")) }
}

pub(crate) fn not_supported(provider_id: &str, data_type: &str) -> anyhow::Error {
    anyhow!("Provider {} does not support {}", provider_id, data_type)
}

pub(crate) fn default_provider_registry() -> Vec<ProviderRegistryItem> {
    vec![
        ProviderRegistryItem { provider_id: "openfootball_worldcup", name: "OpenFootball WorldCup", supported_data_types: &["fixtures", "teams", "groups", "historical_results"], requires_key: false, base_confidence: 90.0, daily_limit: 0, hourly_limit: 0 },
        ProviderRegistryItem { provider_id: "football_data_uk", name: "football-data.co.uk", supported_data_types: &["historical_results", "historical_odds", "stats"], requires_key: false, base_confidence: 88.0, daily_limit: 0, hourly_limit: 0 },
        ProviderRegistryItem { provider_id: "api_football", name: "API-Football", supported_data_types: &["fixtures", "results", "events", "lineups", "injuries", "odds", "stats"], requires_key: true, base_confidence: 82.0, daily_limit: 100, hourly_limit: 0 },
        ProviderRegistryItem { provider_id: "odds_api_io", name: "Odds-API.io", supported_data_types: &["pre_match_odds", "live_odds", "bookmaker_odds"], requires_key: true, base_confidence: 80.0, daily_limit: 0, hourly_limit: 100 },
        ProviderRegistryItem { provider_id: "football_data_org", name: "football-data.org", supported_data_types: &["fixtures", "delayed_results", "standings"], requires_key: true, base_confidence: 78.0, daily_limit: 0, hourly_limit: 0 },
        ProviderRegistryItem { provider_id: "statsbomb_open_data", name: "StatsBomb Open Data", supported_data_types: &["historical_events", "historical_xg", "shots"], requires_key: false, base_confidence: 85.0, daily_limit: 0, hourly_limit: 0 },
        ProviderRegistryItem { provider_id: "thesportsdb", name: "TheSportsDB", supported_data_types: &["teams", "players", "fixtures", "results"], requires_key: false, base_confidence: 68.0, daily_limit: 0, hourly_limit: 0 },
        ProviderRegistryItem { provider_id: "understat", name: "Understat", supported_data_types: &["low_frequency_xg"], requires_key: false, base_confidence: 60.0, daily_limit: 0, hourly_limit: 0 },
    ]
}
