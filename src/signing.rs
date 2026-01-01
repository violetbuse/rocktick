use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use serde_json::json;
use sha2::Sha256;
use url::Url;

pub struct SignatureBuilder {
    pub signing_key: String,
    pub time: DateTime<Utc>,
    pub url: String,
    pub body: Option<String>,
}

type HmacSha256 = Hmac<Sha256>;

impl SignatureBuilder {
    pub fn signature_header(self) -> anyhow::Result<String> {
        let scheduled_at = self.time.timestamp();
        let url = Url::parse(&self.url)?;
        let pathname = url.path();

        let mut mac = HmacSha256::new_from_slice(self.signing_key.as_bytes())
            .expect("Hmac could not take signing key?");

        let mut message = "".to_string();
        message.push_str(&format!(".{}", scheduled_at));
        message.push_str(&format!(".{}", &pathname));

        if let Some(body) = self.body {
            message.push_str(&format!(".{}", body));
        }

        mac.update(message.as_bytes());
        let result = mac.finalize();
        let code_bytes = result.into_bytes();
        let hex_signature = hex::encode(code_bytes);

        Ok(json!({
          "t": scheduled_at,
          "p": pathname,
          "v1": hex_signature
        })
        .to_string())
    }
}
