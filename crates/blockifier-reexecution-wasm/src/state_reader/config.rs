#[derive(Clone, Debug)]
pub struct RpcStateReaderConfig {
    pub url: String,
    pub json_rpc_version: &'static str,
}

impl RpcStateReaderConfig {
    pub fn from_url(url: String) -> Self {
        Self { url, json_rpc_version: "2.0" }
    }
}
