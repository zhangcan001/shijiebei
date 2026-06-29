use super::DataProviderAdapter;
pub(crate) struct FootballDataOrgProvider;
impl DataProviderAdapter for FootballDataOrgProvider {
    fn provider_id(&self) -> &'static str { "football_data_org" }
    fn supported_data_types(&self) -> &'static [&'static str] { &["fixtures", "delayed_results", "standings"] }
}
