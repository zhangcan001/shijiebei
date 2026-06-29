use super::DataProviderAdapter;
pub(crate) struct ApiFootballProvider;
impl DataProviderAdapter for ApiFootballProvider {
    fn provider_id(&self) -> &'static str { "api_football" }
    fn supported_data_types(&self) -> &'static [&'static str] { &["fixtures", "results", "events", "lineups", "injuries", "odds", "stats"] }
}
