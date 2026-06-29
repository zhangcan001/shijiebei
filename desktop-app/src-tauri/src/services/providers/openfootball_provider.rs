use super::DataProviderAdapter;
pub(crate) struct OpenFootballProvider;
impl DataProviderAdapter for OpenFootballProvider {
    fn provider_id(&self) -> &'static str { "openfootball_worldcup" }
    fn supported_data_types(&self) -> &'static [&'static str] { &["fixtures", "teams", "groups", "historical_results"] }
}
