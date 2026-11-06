//! "AI outline": ask the user's AI tool of choice to find outlines in an image.
//!
//! Providers: Claude (Anthropic Messages API, raw HTTPS), any OpenAI-compatible chat endpoint
//! (OpenAI, Ollama, LM Studio, llama.cpp server, OpenRouter…), or an MCP server (see `mcp.rs`).
//! All answers are read as JSON polylines or SVG path data in image pixels.

use crate::geom::*;
use crate::settings::{AiConfig, AiProvider, McpConfig};
use base64::Engine;
use serde_json::{json, Value};
#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;

#[cfg(not(target_arch = "wasm32"))]
const ANTHROPIC_URL: &str = "https://api.anthropic.com/v1/messages";
#[cfg(not(target_arch = "wasm32"))]
const ANTHROPIC_VERSION: &str = "2026-10-21";
#[cfg(not(target_arch = "wasm32"))]
const ANTHROPIC_FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";

pub fn prompt(instructions: &str, w: u32, h: u32) -> String {
    format!(
        "You trace images for a laser cutter. The image is {w}×{h} pixels (x to the right, y down, origin top-left).\n\
         Task: {instructions}\n\
         Reply with only a JSON object, no prose: {{\"paths\": [[[x,y],[x,y],...], ...]}}. Each path is one closed outline \
         in pixel coordinates; repeat the first point at the end to close it. Use enough points to follow curves \
         (roughly one every 5-15 pixels). Inner holes are separate paths."
    )
}

#[cfg(not(target_arch = "wasm32"))]
fn key_for(cfg: &AiConfig, env: &str) -> String {
    if cfg.api_key.trim().is_empty() {
        std::env::var(env).unwrap_or_default()
    } else {
        cfg.api_key.trim().to_string()
    }
}

/// Pull the first `{...}` JSON object (or SVG) out of a model's text reply.
pub fn outlines_from_text(text: &str) -> Vec<Vec<P2>> {
    if let (Some(a), Some(b)) = (text.find('{'), text.rfind('}')) {
        if b > a {
            if let Ok(v) = serde_json::from_str::<Value>(&text[a..=b]) {
                let l = crate::mcp::outlines_from_json(&v);
                if !l.is_empty() {
                    return l;
                }
            }
        }
    }
    crate::mcp::outlines_from_svg_text(text)
}

#[cfg(not(target_arch = "wasm32"))]
fn agent() -> ureq::Agent {
    ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(300))).http_status_as_error(false).build().into()
}

#[cfg(not(target_arch = "wasm32"))]
fn anthropic(cfg: &AiConfig, instructions: &str, png: &[u8], w: u32, h: u32) -> Result<Vec<Vec<P2>>, String> {
    let key = key_for(cfg, "ANTHROPIC_API_KEY");
    if key.is_empty() {
        return Err("Add your Anthropic API key in Settings → AI outlines (or set ANTHROPIC_API_KEY).".into());
    }
    let model = if cfg.anthropic_model.trim().is_empty() { "claude-opus-5-5" } else { cfg.anthropic_model.trim() };
    let body = json!({
        "model": model,
        "max_tokens": 16000,
        "fallbacks": "default",
        "messages": [{
            "role": "user",
            "content": [
                {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": base64::engine::general_purpose::STANDARD.encode(png)}},
                {"type": "text", "text": prompt(instructions, w, h)}
            ]
        }]
    });
    let mut resp = agent()
        .post(ANTHROPIC_URL)
        .header("x-api-key", &key)
        .header("anthropic-version", ANTHROPIC_VERSION)
        .header("anthropic-beta", ANTHROPIC_FALLBACK_BETA)
        .header("content-type", "application/json")
        .send_json(&body)
        .map_err(|e| format!("Couldn't reach Claude: {e}"))?;
    let status = resp.status().as_u16();
    let v: Value = resp.body_mut().read_json().map_err(|e| format!("Unexpected reply from Claude: {e}"))?;
    if status >= 400 {
        let msg = v.pointer("/error/message").and_then(|m| m.as_str()).unwrap_or("unknown error");
        return Err(format!("Claude returned an error ({status}): {msg}"));
    }
    if v.get("stop_reason").and_then(|s| s.as_str()) == Some("refusal") {
        return Err("Claude declined this request.".into());
    }
    let text: String = v
        .get("content")
        .and_then(|c| c.as_array())
        .map(|blocks| blocks.iter().filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("text")).filter_map(|b| b.get("text").and_then(|t| t.as_str())).collect::<Vec<_>>().join("\n"))
        .unwrap_or_default();
    let lines = outlines_from_text(&text);
    if lines.is_empty() {
        return Err("Claude answered, but no outlines were found in the reply.".into());
    }
    Ok(lines)
}

#[cfg(not(target_arch = "wasm32"))]
fn openai_compatible(cfg: &AiConfig, instructions: &str, png: &[u8], w: u32, h: u32) -> Result<Vec<Vec<P2>>, String> {
    let base = cfg.openai_base_url.trim().trim_end_matches('/');
    if base.is_empty() || cfg.openai_model.trim().is_empty() {
        return Err("Set the server address and a vision model in Settings → AI outlines.".into());
    }
    let key = key_for(cfg, "OPENAI_API_KEY");
    let data_url = format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png));
    let body = json!({
        "model": cfg.openai_model.trim(),
        "messages": [{
            "role": "user",
            "content": [
                {"type": "text", "text": prompt(instructions, w, h)},
                {"type": "image_url", "image_url": {"url": data_url}}
            ]
        }]
    });
    let mut req = agent().post(format!("{base}/chat/completions")).header("content-type", "application/json");
    if !key.is_empty() {
        req = req.header("authorization", format!("Bearer {key}"));
    }
    let mut resp = req.send_json(&body).map_err(|e| format!("Couldn't reach {base}: {e}"))?;
    let status = resp.status().as_u16();
    let v: Value = resp.body_mut().read_json().map_err(|e| format!("Unexpected reply: {e}"))?;
    if status >= 400 {
        let msg = v.pointer("/error/message").and_then(|m| m.as_str()).unwrap_or("unknown error");
        return Err(format!("The AI service returned an error ({status}): {msg}"));
    }
    let text = v.pointer("/choices/0/message/content").and_then(|c| c.as_str()).unwrap_or_default();
    let lines = outlines_from_text(text);
    if lines.is_empty() {
        return Err("The AI answered, but no outlines were found in the reply.".into());
    }
    Ok(lines)
}

/// Ask the configured AI tool for outlines (pixel coordinates). Blocking; run on a thread.
pub fn derive_outlines(cfg: &AiConfig, mcp: &McpConfig, png: &[u8], w: u32, h: u32) -> Result<Vec<Vec<P2>>, String> {
    #[cfg(not(target_arch = "wasm32"))]
    return match cfg.provider {
        AiProvider::Anthropic => anthropic(cfg, &mcp.instructions, png, w, h),
        AiProvider::OpenAiCompatible => openai_compatible(cfg, &mcp.instructions, png, w, h),
        AiProvider::Mcp => crate::mcp::call_outline_tool(mcp, png, w, h),
    };
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (cfg, mcp, png, w, h);
        Err("AI outlines aren't available in the browser version yet; use the desktop app.".into())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn reads_json_inside_prose() {
        let l = super::outlines_from_text("Sure! ```json\n{\"paths\": [[[0,0],[10,0],[10,10],[0,0]]]}\n```");
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].len(), 4);
    }
}
