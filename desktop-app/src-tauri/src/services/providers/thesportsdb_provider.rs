use super::DataProviderAdapter;
pub(crate) struct TheSportsDbProvider;
impl DataProviderAdapter for TheSportsDbProvider {
    fn provider_id(&self) -> &'static str {
        "thesportsdb"
    }
    fn supported_data_types(&self) -> &'static [&'static str] {
        &["teams", "players", "fixtures", "results"]
    }
}
