use hmac::{Hmac, Mac};
use liqi::pb;
use sha2::Sha256;

pub const RESOURCE_VERSION: &str = "0.16.231";
pub const PACKAGE_VERSION: &str = "4.0.44";
pub const LOGIN_BEAT_CONTRACT: &str = "DF2vkXCnfeXp4WoGrBGNcJBufZiMN3uP";

pub fn client_version_string() -> String {
    format!("WebGL_2022-{}", RESOURCE_VERSION.trim_end_matches(".w"))
}

pub fn password_digest(password: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(b"lailai").expect("static HMAC key is valid");
    mac.update(password.as_bytes());
    hex_lower(&mac.finalize().into_bytes())
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

pub fn login_payload(
    username: &str,
    password: &str,
    device_id: &str,
    reconnect: bool,
) -> pb::ReqLogin {
    pb::ReqLogin {
        account: username.to_string(),
        password: password_digest(password),
        reconnect,
        device: Some(device_payload()),
        random_key: device_id.to_string(),
        client_version: Some(pb::ClientVersionInfo {
            resource: RESOURCE_VERSION.to_string(),
            package: PACKAGE_VERSION.to_string(),
        }),
        gen_access_token: !reconnect,
        currency_platforms: vec![1, 2, 5, 6, 8, 10, 11],
        r#type: 0,
        client_version_string: client_version_string(),
        tag: "cn".to_string(),
        ..Default::default()
    }
}

fn device_payload() -> pb::ClientDeviceInfo {
    pb::ClientDeviceInfo {
        platform: "pc".to_string(),
        hardware: "pc".to_string(),
        os: "mac".to_string(),
        is_browser: true,
        software: "Chrome".to_string(),
        sale_platform: "web".to_string(),
        screen_width: 1280,
        screen_height: 720,
        os_version: String::new(),
        hardware_vendor: String::new(),
        model_number: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_digest_uses_hmac_sha256_with_lailai_key() {
        assert_eq!(
            password_digest("qazwsxedc"),
            "6767460a12df0390c6847f15fb95c8d495196528804552155855574f54263bc6"
        );
    }

    #[test]
    fn login_payload_matches_python_shape_for_fresh_login() {
        let payload = login_payload("jojo.song@yahoo.com", "qazwsxedc", "device-1", false);
        assert_eq!(payload.account, "jojo.song@yahoo.com");
        assert_eq!(
            payload.password,
            "6767460a12df0390c6847f15fb95c8d495196528804552155855574f54263bc6"
        );
        assert!(!payload.reconnect);
        assert!(payload.gen_access_token);
        assert_eq!(payload.r#type, 0);
        assert_eq!(payload.random_key, "device-1");
        assert_eq!(payload.client_version_string, client_version_string());
        assert_eq!(payload.tag, "cn");
        assert_eq!(payload.currency_platforms, vec![1, 2, 5, 6, 8, 10, 11]);

        let device = payload.device.expect("device");
        assert_eq!(device.platform, "pc");
        assert_eq!(device.os, "mac");
        assert!(device.is_browser);
        assert_eq!(device.software, "Chrome");
        assert_eq!(device.screen_width, 1280);
        assert_eq!(device.screen_height, 720);

        let version = payload.client_version.expect("client version");
        assert_eq!(version.resource, RESOURCE_VERSION);
        assert_eq!(version.package, PACKAGE_VERSION);
    }

    #[test]
    fn reconnect_login_does_not_request_new_access_token() {
        let payload = login_payload("u", "p", "d", true);
        assert!(payload.reconnect);
        assert!(!payload.gen_access_token);
    }
}
