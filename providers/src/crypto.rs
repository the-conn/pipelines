use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

pub type HmacSha256 = Hmac<Sha256>;

pub fn signature_is_valid(secret: &str, signature: &[u8], body: &[u8]) -> bool {
  let Ok(mut mac) = HmacSha256::new_from_slice(secret.as_bytes()) else {
    return false;
  };
  mac.update(body);
  mac.verify_slice(signature).is_ok()
}
