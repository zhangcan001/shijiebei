use super::DataProviderAdapter;
pub(crate) struct UnderstatProvider;
impl DataProviderAdapter for UnderstatProvider {
    fn provider_id(&self) -> &'static str { "understat" }
    fn supported_data_types(&self) -> &'static [&'static str] { &["low_frequency_xg"] }
}
