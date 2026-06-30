use anyhow::anyhow;
use serde_json::Value;

pub(crate) async fn http_json(url: &str) -> anyhow::Result<Value> {
    let client = reqwest::Client::builder()
        .user_agent("worldcup-odds-desktop/0.1")
        .timeout(std::time::Duration::from_secs(35))
        .build()?;
    let response = client.get(url).send().await?;
    if !response.status().is_success() {
        return Err(anyhow!("HTTP {}", response.status()));
    }
    let text = response.text().await?;
    Ok(serde_json::from_str(text.trim_start_matches('\u{feff}'))?)
}

pub(crate) async fn http_sporttery_mobile_json(url: &str) -> anyhow::Result<Value> {
    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (iPhone; CPU iPhone OS 16_0 like Mac OS X) AppleWebKit/605.1.15 Mobile/15E148")
        .timeout(std::time::Duration::from_secs(35))
        .build()?;
    let response = client
        .get(url)
        .header("Referer", "https://m.sporttery.cn/mjc/styl/index.html")
        .header("Origin", "https://m.sporttery.cn")
        .header("Accept", "application/json,text/plain,*/*")
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(anyhow!("HTTP {}", response.status()));
    }
    let text = response.text().await?;
    Ok(serde_json::from_str(text.trim_start_matches('\u{feff}'))?)
}

pub(crate) async fn http_sporttery_browser_json(url: &str) -> anyhow::Result<Value> {
    let client = reqwest::Client::builder()
        .user_agent(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/126 Safari/537.36",
        )
        .timeout(std::time::Duration::from_secs(35))
        .build()?;
    let response = client
        .get(url)
        .header("Referer", "https://www.sporttery.cn/jc/jsq/zqspf/")
        .header("Origin", "https://www.sporttery.cn")
        .header("Accept", "application/json,text/plain,*/*")
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(anyhow!("HTTP {}", response.status()));
    }
    let text = response.text().await?;
    Ok(serde_json::from_str(text.trim_start_matches('\u{feff}'))?)
}

pub(crate) async fn http_text(url: &str) -> anyhow::Result<String> {
    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 worldcup-odds-desktop/0.1")
        .timeout(std::time::Duration::from_secs(35))
        .build()?;
    let response = client.get(url).send().await?;
    if !response.status().is_success() {
        return Err(anyhow!("HTTP {}", response.status()));
    }
    Ok(response.text().await?)
}

pub(crate) async fn http_json_with_header(
    url: &str,
    header_name: &str,
    header_value: &str,
) -> anyhow::Result<Value> {
    let client = reqwest::Client::builder()
        .user_agent("worldcup-odds-desktop/0.1")
        .timeout(std::time::Duration::from_secs(35))
        .build()?;
    let response = client
        .get(url)
        .header(header_name, header_value)
        .header("Accept", "application/json")
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(anyhow!("HTTP {}", response.status()));
    }
    let text = response.text().await?;
    Ok(serde_json::from_str(text.trim_start_matches('\u{feff}'))?)
}

pub(crate) async fn http_football_data_org_json(url: &str, token: &str) -> anyhow::Result<Value> {
    http_json_with_header(url, "X-Auth-Token", token).await
}
