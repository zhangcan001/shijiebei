use super::DataProviderAdapter;
pub(crate) struct OddsApiIoProvider;
impl DataProviderAdapter for OddsApiIoProvider {
    fn provider_id(&self) -> &'static str { "odds_api_io" }
    fn supported_data_types(&self) -> &'static [&'static str] { &["pre_match_odds", "live_odds", "bookmaker_odds"] }
}
