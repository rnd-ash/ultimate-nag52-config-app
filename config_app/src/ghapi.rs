
use ehttp::*;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ApiQueryError(String);

impl From<ehttp::Error> for ApiQueryError {
    fn from(value: ehttp::Error) -> Self {
        Self(value.to_string())
    }
}

impl From<serde_json::Error> for ApiQueryError {
    fn from(value: serde_json::Error) -> Self {
        Self(value.to_string())
    }
}

pub struct ConfigAppRelease {
    date: String,
    osx_url: Option<String>,
    unix_url: Option<String>,
    windows_url: Option<String>,
    name: String
}

pub type ApiQueryResult<T> = std::result::Result<T, ApiQueryError>;

fn query_gh_api(url: &str) -> ApiQueryResult<Value> {
    let req = Request::get(url);
    let resp = fetch_blocking(&req)?;
    let s = String::from_utf8(resp.bytes).unwrap();
    let v = serde_json::from_str::<serde_json::Value>(&s)?;
    Ok(v)
}

pub fn query_firmware_releases(branch: &str) -> ApiQueryResult<Vec<Value>> {
    let v = query_gh_api("https://api.github.com/repos/rnd-ash/ultimate-nag52-config-app/releases")?;
    let mut res : Vec<Value> = Vec::new();
    for x in v.as_array().unwrap_or(&Vec::new()) {
        if let Some(obj) = x.as_object() {
            if obj.get("html_url").unwrap().as_str().unwrap().contains(&format!("{branch}-")) {
                println!("Found {branch} release {:?}", obj);
                res.push(x.clone())
            }
        }
    }
    
    Ok(res)
}

pub fn query_config_app_releases(branch: &str) -> ApiQueryResult<Value> {
    query_gh_api("https://api.github.com/repos/rnd-ash/ultimate-nag52-config-app/releases")
}

#[cfg(test)]
pub mod ehttp_tests {
    use crate::ghapi::query_firmware_releases;

    #[test]
    pub fn test_req() {
        query_firmware_releases("dev");
    }
}