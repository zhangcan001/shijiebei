use super::DataProviderAdapter;

pub(crate) struct TheOddsApiProvider;

impl DataProviderAdapter for TheOddsApiProvider {
    fn provider_id(&self) -> &'static str {
        "the_odds_api"
    }

    fn supported_data_types(&self) -> &'static [&'static str] {
        &["pre_match_odds", "bookmaker_odds", "h2h"]
    }
}
