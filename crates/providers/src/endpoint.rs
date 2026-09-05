//! Read-operation descriptors include request bodies in bounded queue accounting.
use serde_json::Value;
#[derive(Clone)]
pub struct Endpoint {
    pub id: String,
    pub url: String,
    pub items: String,
    pub body: Option<Value>,
    pub aws: Option<(String, String, String)>,
}
impl Endpoint {
    pub fn bytes(&self) -> usize {
        let body = self.body.as_ref().map_or(0, |body| {
            serde_json::to_vec(body).map_or(usize::MAX, |bytes| bytes.len())
        });
        let signing = self.aws.as_ref().map_or(0, |(service, region, action)| {
            service.len() + region.len() + action.len()
        });
        body.saturating_add(signing)
            .saturating_add(self.id.len())
            .saturating_add(self.url.len())
            .saturating_add(self.items.len())
            .saturating_mul(4)
            .saturating_add(512)
    }
    pub fn get(id: impl Into<String>, url: impl Into<String>, items: &str) -> Self {
        Self {
            id: id.into(),
            url: url.into(),
            items: items.into(),
            body: None,
            aws: None,
        }
    }
}
