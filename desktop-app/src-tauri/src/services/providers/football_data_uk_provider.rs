use super::DataProviderAdapter;
pub(crate) struct FootballDataUkProvider;
impl DataProviderAdapter for FootballDataUkProvider {
    fn provider_id(&self) -> &'static str { "football_data_uk" }
    fn supported_data_types(&self) -> &'static [&'static str] { &["historical_results", "historical_odds", "stats"] }
}
