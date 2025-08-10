use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    println!("Testing Ollama connection...");

    let client = reqwest::Client::new();
    let api_url = "http://192.168.0.207:11434";
    let model = "llama3.2:3b";

    let request_body = serde_json::json!({
        "model": model,
        "prompt": "What is 2+2? Answer in one word.",
        "stream": false
    });

    println!("Sending request to {}/api/generate", api_url);
    println!(
        "Request body: {}",
        serde_json::to_string_pretty(&request_body)?
    );

    let response = client
        .post(format!("{}/api/generate", api_url))
        .json(&request_body)
        .send()
        .await?;

    println!("Response status: {}", response.status());

    if !response.status().is_success() {
        let error_text = response.text().await?;
        eprintln!("Ollama API error: {}", error_text);
        return Ok(());
    }

    let response_json: serde_json::Value = response.json().await?;
    println!(
        "Response JSON: {}",
        serde_json::to_string_pretty(&response_json)?
    );

    let content = response_json["response"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Invalid response from Ollama API"))?;

    println!("Ollama says: {}", content);

    Ok(())
}
