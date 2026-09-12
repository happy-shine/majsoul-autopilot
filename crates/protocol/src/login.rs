use hmac::{Hmac, Mac};
use liqi::pb;
use sha2::Sha256;

pub const DEFAULT_RESOURCE_VERSION: &str = "0.16.275";
pub const DEFAULT_PACKAGE_VERSION: &str = "4.0.46";
pub const RESOURCE_VERSION: &str = DEFAULT_RESOURCE_VERSION;
pub const PACKAGE_VERSION: &str = DEFAULT_PACKAGE_VERSION;
pub const LOGIN_BEAT_CONTRACT: &str = "DF2vkXCnfeXp4WoGrBGNcJBufZiMN3uP";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientVersion {
    pub resource: String,
    pub package: String,
}

impl Default for ClientVersion {
    fn default() -> Self {
        Self {
            resource: DEFAULT_RESOURCE_VERSION.to_string(),
            package: DEFAULT_PACKAGE_VERSION.to_string(),
        }
    }
}

impl ClientVersion {
    pub fn new(resource: impl Into<String>, package: impl Into<String>) -> Self {
        Self {
            resource: resource.into(),
            package: package.into(),
        }
    }

    pub fn client_version_string(&self) -> String {
        format!("WebGL_2022-{}", self.resource)
    }
}

pub fn client_version_string() -> String {
    format!("WebGL_2022-{}", DEFAULT_RESOURCE_VERSION)
}

pub fn extract_package_version(html: &str) -> Option<String> {
    if let Some(idx) = html.find("productVersion:") {
        let rest = &html[idx + "productVersion:".len()..];
        if let Some(quote_start) = rest.find('"') {
            let after_quote = &rest[quote_start + 1..];
            if let Some(quote_end) = after_quote.find('"') {
                let ver = after_quote[..quote_end].trim();
                if !ver.is_empty() {
                    return Some(ver.to_string());
                }
            }
        }
    }
    if let Some(idx) = html.find("-release-") {
        let rest = &html[idx + "-release-".len()..];
        if let Some(end) = rest.find('(') {
            let ver = rest[..end].trim();
            if !ver.is_empty() {
                return Some(ver.to_string());
            }
        }
    }
    None
}

pub fn extract_version_bundle_name(bytes: &[u8]) -> Option<String> {
    const PATTERN: &[u8] = b"td1bd8bb002s_";
    if let Some(pos) = bytes.windows(PATTERN.len()).position(|window| window == PATTERN) {
        let hash_start = pos + PATTERN.len();
        if bytes.len() >= hash_start + 20 {
            let hash_bytes = &bytes[hash_start..hash_start + 20];
            if hash_bytes.iter().all(|b| b.is_ascii_hexdigit()) {
                if let Ok(hash_str) = std::str::from_utf8(hash_bytes) {
                    return Some(format!("td1bd8bb002s_{hash_str}"));
                }
            }
        }
    }
    None
}

pub fn extract_resource_version(bytes: &[u8]) -> Option<String> {
    let platform_idx = bytes
        .windows(b"\"platform\"".len())
        .position(|w| w == b"\"platform\"")?;
    let prefix = &bytes[..platform_idx];

    let colon_idx = prefix.iter().rposition(|&b| b == b':')?;
    let after_colon = &prefix[colon_idx + 1..];

    let quote_start = after_colon.iter().position(|&b| b == b'"')?;
    let candidate_bytes = &after_colon[quote_start + 1..];

    let quote_end = candidate_bytes.iter().position(|&b| b == b'"')?;
    let ver_str = std::str::from_utf8(&candidate_bytes[..quote_end]).ok()?;
    let trimmed = ver_str.trim();

    let parts: Vec<&str> = trimmed.split('.').collect();
    if parts.len() >= 2 && parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit())) {
        Some(trimmed.to_string())
    } else {
        None
    }
}

pub async fn fetch_remote_client_version() -> Result<ClientVersion, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(6))
        .build()
        .map_err(|e| format!("failed to build HTTP client: {e}"))?;

    // 1. Fetch package_version from index.html
    let html = client
        .get("https://game.maj-soul.com/1/index.html")
        .send()
        .await
        .map_err(|e| format!("failed to fetch index.html: {e}"))?
        .text()
        .await
        .map_err(|e| format!("failed to read index.html body: {e}"))?;

    let package = extract_package_version(&html)
        .ok_or_else(|| "could not extract package version from index.html".to_string())?;

    // 2. Fetch bundle_info_so.majset
    let bundle_info = client
        .get("https://game.maj-soul.com/assetbundles/ASTC/bundle_info_so.majset")
        .send()
        .await
        .map_err(|e| format!("failed to fetch bundle_info_so.majset: {e}"))?
        .bytes()
        .await
        .map_err(|e| format!("failed to read bundle_info_so.majset: {e}"))?;

    let bundle_name = extract_version_bundle_name(&bundle_info)
        .ok_or_else(|| "could not extract version bundle name from bundle_info_so.majset".to_string())?;

    // 3. Fetch version majset (try u6v_ prefix first, then without u6v_)
    let mut v_bytes = None;
    for candidate_name in [format!("u6v_{bundle_name}"), bundle_name.clone()] {
        let v_url = format!("https://game.maj-soul.com/assetbundles/ASTC/{candidate_name}.majset");
        if let Ok(resp) = client.get(&v_url).send().await {
            if resp.status().is_success() {
                if let Ok(bytes) = resp.bytes().await {
                    v_bytes = Some(bytes);
                    break;
                }
            }
        }
    }

    let v_bytes = v_bytes
        .ok_or_else(|| format!("failed to fetch version bundle for {bundle_name}"))?;

    let resource = extract_resource_version(&v_bytes)
        .ok_or_else(|| "could not extract resource version from version bundle".to_string())?;

    Ok(ClientVersion { resource, package })
}

pub async fn resolve_client_version() -> ClientVersion {
    if let (Ok(res), Ok(pkg)) = (
        std::env::var("MAJSOUL_RESOURCE_VERSION"),
        std::env::var("MAJSOUL_PACKAGE_VERSION"),
    ) {
        if !res.is_empty() && !pkg.is_empty() {
            eprintln!(
                "[protocol] using Majsoul version from environment: package={pkg}, resource={res}"
            );
            return ClientVersion {
                resource: res,
                package: pkg,
            };
        }
    }

    match fetch_remote_client_version().await {
        Ok(v) => {
            eprintln!(
                "[protocol] dynamically resolved Majsoul client version: package={}, resource={}",
                v.package, v.resource
            );
            v
        }
        Err(err) => {
            eprintln!(
                "[protocol] warning: failed to fetch dynamic client version ({err}), falling back to default: package={}, resource={}",
                DEFAULT_PACKAGE_VERSION, DEFAULT_RESOURCE_VERSION
            );
            ClientVersion::default()
        }
    }
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
    login_payload_with_version(
        username,
        password,
        device_id,
        reconnect,
        &ClientVersion::default(),
    )
}

pub fn login_payload_with_version(
    username: &str,
    password: &str,
    device_id: &str,
    reconnect: bool,
    version: &ClientVersion,
) -> pb::ReqLogin {
    pb::ReqLogin {
        account: username.to_string(),
        password: password_digest(password),
        reconnect,
        device: Some(device_payload()),
        random_key: device_id.to_string(),
        client_version: Some(pb::ClientVersionInfo {
            resource: version.resource.clone(),
            package: version.package.clone(),
        }),
        gen_access_token: !reconnect,
        currency_platforms: vec![1, 2, 5, 6, 8, 10, 11],
        r#type: 0,
        client_version_string: version.client_version_string(),
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

    #[test]
    fn extract_package_version_finds_version_in_html() {
        let sample_html = r#"
            companyName: "CatfoodStudio",
            productName: "雀魂麻將",
            productVersion: "4.0.46",
            matchWebGLToCanvasSize: false,
        "#;
        assert_eq!(extract_package_version(sample_html), Some("4.0.46".to_string()));

        let fallback_html = r#"
            <script src="Build/chs_t-WebGL-release-4.0.46(46).loader.js"></script>
        "#;
        assert_eq!(extract_package_version(fallback_html), Some("4.0.46".to_string()));
    }

    #[test]
    fn extract_version_bundle_name_finds_hash() {
        let sample_bytes = b"something\x00td1bd8bb002s_45646c88bc76c3dad0b44\x00other";
        assert_eq!(
            extract_version_bundle_name(sample_bytes),
            Some("td1bd8bb002s_45646c88bc76c3dad0b4".to_string())
        );
    }

    #[test]
    fn extract_resource_version_finds_version() {
        let sample = b"\x00/4\x00\x01\x00\x0f\x00\xaf\x07qversion{\x03#{\"\x0e\x00\xf3\x11\":\"0.16.275\",\"platform\":\"WebGL\"}@";
        assert_eq!(extract_resource_version(sample), Some("0.16.275".to_string()));

        let plain_json = br#"{"version": "0.16.275", "platform": "WebGL"}"#;
        assert_eq!(extract_resource_version(plain_json), Some("0.16.275".to_string()));
    }

    #[test]
    fn login_payload_with_custom_version() {
        let ver = ClientVersion::new("0.16.999", "4.1.0");
        let payload = login_payload_with_version("user", "pass", "dev", false, &ver);
        let cv = payload.client_version.unwrap();
        assert_eq!(cv.resource, "0.16.999");
        assert_eq!(cv.package, "4.1.0");
        assert_eq!(payload.client_version_string, "WebGL_2022-0.16.999");
    }
}
