use super::DataProviderAdapter;
pub(crate) struct StatsBombProvider;
impl DataProviderAdapter for StatsBombProvider {
    fn provider_id(&self) -> &'static str { "statsbomb_open_data" }
    fn supported_data_types(&self) -> &'static [&'static str] { &["historical_events", "historical_xg", "shots"] }
}
