//! Model Context Protocol (JSON-RPC 2.0 over stdio).
//!
//! * Client: BurnCraft starts an MCP server the user configured and calls one of its tools with the image,
//!   expecting outlines back (JSON polylines or SVG path data, in pixel coordinates).
//! * Server: `burncraft --mcp` lets an AI assistant trace images and add outlines to projects.

use crate::geom::*;
use crate::settings::McpConfig;
use base64::Engine;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

const PROTOCOL: &str = "2025-06-18";

// ------------------------------------------------------------------ client

pub fn call_outline_tool(cfg: &McpConfig, png: &[u8], w: u32, h: u32) -> Result<Vec<Vec<P2>>, String> {
    if cfg.command.trim().is_empty() {
        return Err("No MCP server is set up yet. Add its command in Settings → AI outlines (MCP).".into());
    }
    let args: Vec<String> = split_args(&cfg.args);
    let mut child = Command::new(cfg.command.trim())
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("Could not start \u{201c}{}\u{201d}: {e}", cfg.command))?;
    let mut stdin = child.stdin.take().ok_or("no stdin")?;
    let stdout = child.stdout.take().ok_or("no stdout")?;
    let (ltx, lrx) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if ltx.send(line).is_err() {
                break;
            }
        }
    });
    let mut send = |v: Value| -> Result<(), String> { writeln!(stdin, "{}", v).map_err(|e| e.to_string()) };
    let wait = |id: i64, secs: u64| -> Result<Value, String> {
        let deadline = std::time::Instant::now() + Duration::from_secs(secs);
        loop {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            if left.is_zero() {
                return Err("The MCP server took too long to answer.".into());
            }
            let line = lrx.recv_timeout(left).map_err(|_| "The MCP server stopped or took too long.".to_string())?;
            let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
            if v.get("id").and_then(|i| i.as_i64()) == Some(id) {
                if let Some(e) = v.get("error") {
                    return Err(format!("MCP error: {}", e.get("message").and_then(|m| m.as_str()).unwrap_or("unknown")));
                }
                return Ok(v.get("result").cloned().unwrap_or(Value::Null));
            }
        }
    };
    send(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":PROTOCOL,"capabilities":{},"clientInfo":{"name":"BurnCraft","version":env!("CARGO_PKG_VERSION")}}}))?;
    wait(1, 30)?;
    send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(png);
    send(json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":cfg.tool,"arguments":{
        "image": b64, "mime_type": "image/png", "width_px": w, "height_px": h, "instructions": cfg.instructions
    }}}))?;
    let result = wait(2, 300);
    let _ = child.kill();
    let result = result?;
    if result.get("isError").and_then(|b| b.as_bool()) == Some(true) {
        return Err(format!("The tool reported an error: {}", content_text(&result)));
    }
    let mut lines = Vec::new();
    if let Some(sc) = result.get("structuredContent") {
        lines.extend(outlines_from_json(sc));
    }
    if lines.is_empty() {
        let text = content_text(&result);
        if let Ok(v) = serde_json::from_str::<Value>(text.trim()) {
            lines.extend(outlines_from_json(&v));
        }
        if lines.is_empty() {
            lines.extend(outlines_from_svg_text(&text));
        }
    }
    if lines.is_empty() {
        return Err("The tool answered, but no outlines were found in its reply.".into());
    }
    Ok(lines)
}

fn content_text(result: &Value) -> String {
    result
        .get("content")
        .and_then(|c| c.as_array())
        .map(|arr| arr.iter().filter_map(|c| c.get("text").and_then(|t| t.as_str())).collect::<Vec<_>>().join("\n"))
        .unwrap_or_default()
}

fn split_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut q: Option<char> = None;
    for c in s.chars() {
        match (q, c) {
            (None, '"' | '\'') => q = Some(c),
            (Some(qc), c2) if c2 == qc => q = None,
            (None, ' ') => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            _ => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Accepts {"paths": [[[x,y],...],...]}, {"polylines": ...}, [[[x,y]...]], {"svg": "..."} or {"d": "..."}.
pub fn outlines_from_json(v: &Value) -> Vec<Vec<P2>> {
    let parse_line = |l: &Value| -> Option<Vec<P2>> {
        let arr = l.as_array()?;
        let pts: Vec<P2> = arr
            .iter()
            .filter_map(|p| {
                if let Some(a) = p.as_array() {
                    Some(p2(a.first()?.as_f64()?, a.get(1)?.as_f64()?))
                } else {
                    Some(p2(p.get("x")?.as_f64()?, p.get("y")?.as_f64()?))
                }
            })
            .collect();
        (pts.len() >= 2).then_some(pts)
    };
    for key in ["paths", "polylines", "outlines", "contours"] {
        if let Some(arr) = v.get(key).and_then(|a| a.as_array()) {
            let mut out = Vec::new();
            for l in arr {
                if let Some(s) = l.as_str() {
                    out.extend(parse_svg_path(s));
                } else if let Some(pts) = parse_line(l) {
                    out.push(pts);
                }
            }
            return out;
        }
    }
    for key in ["svg", "d", "path"] {
        if let Some(s) = v.get(key).and_then(|s| s.as_str()) {
            return outlines_from_svg_text(s);
        }
    }
    if let Some(arr) = v.as_array() {
        return arr.iter().filter_map(parse_line).collect();
    }
    Vec::new()
}

/// Find `d="..."` attributes in SVG text, or treat the whole text as path data.
pub fn outlines_from_svg_text(s: &str) -> Vec<Vec<P2>> {
    let mut out = Vec::new();
    let mut rest = s;
    let mut found = false;
    while let Some(i) = rest.find(" d=\"").or_else(|| rest.find("\nd=\"")) {
        let start = i + 4;
        if let Some(j) = rest[start..].find('"') {
            out.extend(parse_svg_path(&rest[start..start + j]));
            found = true;
            rest = &rest[start + j..];
        } else {
            break;
        }
    }
    if !found && s.trim_start().starts_with(['M', 'm']) {
        out.extend(parse_svg_path(s.trim()));
    }
    out
}

/// Minimal SVG path parser (M L H V C Q Z, absolute and relative), flattened.
pub fn parse_svg_path(d: &str) -> Vec<Vec<P2>> {
    let mut toks: Vec<String> = Vec::new();
    let mut cur = String::new();
    for c in d.chars() {
        if c.is_ascii_alphabetic() && c != 'e' && c != 'E' {
            if !cur.is_empty() {
                toks.push(std::mem::take(&mut cur));
            }
            toks.push(c.to_string());
        } else if c == ',' || c.is_whitespace() {
            if !cur.is_empty() {
                toks.push(std::mem::take(&mut cur));
            }
        } else if c == '-' && !cur.is_empty() && !cur.ends_with(['e', 'E']) {
            toks.push(std::mem::take(&mut cur));
            cur.push(c);
        } else {
            cur.push(c);
        }
    }
    if !cur.is_empty() {
        toks.push(cur);
    }
    let mut out: Vec<Vec<P2>> = Vec::new();
    let mut line: Vec<P2> = Vec::new();
    let mut pos = P2::default();
    let mut start = P2::default();
    let mut cmd = 'M';
    let mut i = 0;
    let num = |i: &mut usize| -> Option<f64> {
        let v = toks.get(*i)?.parse().ok()?;
        *i += 1;
        Some(v)
    };
    while i < toks.len() {
        if toks[i].len() == 1 && toks[i].chars().next().unwrap().is_ascii_alphabetic() {
            cmd = toks[i].chars().next().unwrap();
            i += 1;
            if cmd == 'Z' || cmd == 'z' {
                if line.len() > 1 {
                    line.push(start);
                    out.push(std::mem::take(&mut line));
                }
                pos = start;
                continue;
            }
        }
        let rel = cmd.is_ascii_lowercase();
        let base = if rel { pos } else { P2::default() };
        match cmd.to_ascii_uppercase() {
            'M' => {
                let (Some(x), Some(y)) = (num(&mut i), num(&mut i)) else { break };
                if line.len() > 1 {
                    out.push(std::mem::take(&mut line));
                }
                pos = base + p2(x, y);
                start = pos;
                line = vec![pos];
                cmd = if rel { 'l' } else { 'L' };
            }
            'L' => {
                let (Some(x), Some(y)) = (num(&mut i), num(&mut i)) else { break };
                pos = base + p2(x, y);
                line.push(pos);
            }
            'H' => {
                let Some(x) = num(&mut i) else { break };
                pos = p2(if rel { pos.x + x } else { x }, pos.y);
                line.push(pos);
            }
            'V' => {
                let Some(y) = num(&mut i) else { break };
                pos = p2(pos.x, if rel { pos.y + y } else { y });
                line.push(pos);
            }
            'C' => {
                let v: Vec<f64> = (0..6).filter_map(|_| num(&mut i)).collect();
                if v.len() < 6 {
                    break;
                }
                let (c0, c1, e) = (base + p2(v[0], v[1]), base + p2(v[2], v[3]), base + p2(v[4], v[5]));
                flatten_cubic(pos, c0, c1, e, 0.25, &mut line);
                pos = e;
            }
            'Q' => {
                let v: Vec<f64> = (0..4).filter_map(|_| num(&mut i)).collect();
                if v.len() < 4 {
                    break;
                }
                let (c, e) = (base + p2(v[0], v[1]), base + p2(v[2], v[3]));
                flatten_quad(pos, c, e, 0.25, &mut line);
                pos = e;
            }
            _ => {
                i += 1;
            }
        }
    }
    if line.len() > 1 {
        out.push(line);
    }
    out
}

// ------------------------------------------------------------------ server

fn tool_list() -> Value {
    json!([
        {"name":"trace_image","description":"Trace an image file into vector outlines (pixel coordinates, y down). Returns JSON {width_px,height_px,paths}.",
         "inputSchema":{"type":"object","properties":{
            "path":{"type":"string","description":"Image file (PNG, JPG, BMP...)"},
            "threshold":{"type":"number","description":"0-255, pixels darker than this are traced (default 128)"},
            "smoothness":{"type":"number"},"ignore_less_than":{"type":"number"},"invert":{"type":"boolean"}},
          "required":["path"]}},
        {"name":"project_summary","description":"List the layers and shapes in a BurnCraft / LightBurn project (.lbrn2/.lbrn).",
         "inputSchema":{"type":"object","properties":{"project":{"type":"string"}},"required":["project"]}},
        {"name":"add_outlines","description":"Add outlines (mm, y up) to a project as one path shape on a layer; creates the project if it doesn't exist.",
         "inputSchema":{"type":"object","properties":{
            "project":{"type":"string"},"paths":{"type":"array","items":{"type":"array","items":{"type":"array","items":{"type":"number"}}}},
            "svg_path":{"type":"string","description":"Alternative to paths: SVG path data in mm"},
            "layer":{"type":"integer","description":"0-29 (colour layer)"}},
          "required":["project"]}},
        {"name":"export_gcode","description":"Write GRBL G-code for a project.",
         "inputSchema":{"type":"object","properties":{"project":{"type":"string"},"output":{"type":"string"}},"required":["project","output"]}}
    ])
}

fn text_result(s: String, is_error: bool) -> Value {
    json!({"content":[{"type":"text","text":s}],"isError":is_error})
}

fn call_tool(name: &str, a: &Value) -> Value {
    let s = |k: &str| a.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
    match name {
        "trace_image" => {
            let path = s("path");
            let Ok(bytes) = std::fs::read(&path) else { return text_result(format!("cannot read {path}"), true) };
            let Some(bd) = crate::lbrn::decode_bitmap(bytes, &path) else { return text_result("not an image".into(), true) };
            let mut p = crate::imaging::TraceParams::default();
            if let Some(t) = a.get("threshold").and_then(|v| v.as_f64()) {
                p.threshold = t.clamp(0.0, 255.0) as u8;
            }
            if let Some(t) = a.get("smoothness").and_then(|v| v.as_f64()) {
                p.smoothness = t;
            }
            if let Some(t) = a.get("ignore_less_than").and_then(|v| v.as_f64()) {
                p.ignore_less_than = t;
            }
            p.invert = a.get("invert").and_then(|v| v.as_bool()).unwrap_or(false);
            let gray = crate::imaging::adjusted_gray(&bd, &Default::default());
            let lines = crate::imaging::trace_pixels(&gray, bd.width as usize, bd.height as usize, &p);
            let paths: Vec<Vec<[f64; 2]>> = lines.iter().map(|l| l.iter().map(|q| [(q.x * 100.0).round() / 100.0, (q.y * 100.0).round() / 100.0]).collect()).collect();
            text_result(json!({"width_px":bd.width,"height_px":bd.height,"paths":paths}).to_string(), false)
        }
        "project_summary" => {
            let path = s("project");
            match std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|t| crate::lbrn::load_str(&t)) {
                Ok(l) => {
                    let layers: Vec<Value> = l.doc.cuts.iter().map(|c| json!({"index":c.index,"name":c.name,"mode":c.mode.label(),"speed_mm_s":c.speed,"max_power_pct":c.max_power,"passes":c.passes})).collect();
                    let shapes: Vec<Value> = l.doc.shapes.iter().map(|sh| {
                        let b = sh.bbox();
                        json!({"type":sh.type_name(),"layer":sh.layer,"bbox_mm":[b.min.x,b.min.y,b.max.x,b.max.y]})
                    }).collect();
                    text_result(json!({"layers":layers,"shapes":shapes}).to_string(), false)
                }
                Err(e) => text_result(e, true),
            }
        }
        "add_outlines" => {
            let path = s("project");
            let (mut doc, kept) = match std::fs::read_to_string(&path) {
                Ok(t) => match crate::lbrn::load_str(&t) {
                    Ok(l) => (l.doc, l.kept),
                    Err(e) => return text_result(e, true),
                },
                Err(_) => (crate::model::Document::default(), Vec::new()),
            };
            let mut lines = a.get("paths").map(|v| outlines_from_json(&json!({"paths": v}))).unwrap_or_default();
            let svg = s("svg_path");
            if !svg.is_empty() {
                lines.extend(parse_svg_path(&svg));
            }
            if lines.is_empty() {
                return text_result("no outlines given".into(), true);
            }
            let layer = a.get("layer").and_then(|v| v.as_u64()).unwrap_or(0).min(29) as usize;
            let pd = crate::model::PathData::from_polylines(&lines);
            doc.shapes.push(crate::model::Shape::new(crate::model::ShapeKind::Path(pd), Xform::IDENTITY, layer));
            doc.sync_cuts();
            match std::fs::write(&path, crate::lbrn::save_string(&doc, &kept)) {
                Ok(_) => text_result(format!("added {} outline(s) to {path}", lines.len()), false),
                Err(e) => text_result(e.to_string(), true),
            }
        }
        "export_gcode" => {
            let path = s("project");
            let out = s("output");
            match std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|t| crate::lbrn::load_str(&t)) {
                Ok(l) => {
                    let job = crate::gcode::build_job(&l.doc, &crate::settings::Settings::load().device, None);
                    match std::fs::write(&out, job.gcode) {
                        Ok(_) => text_result(format!("wrote {out}; estimated {:.0} s", job.seconds), false),
                        Err(e) => text_result(e.to_string(), true),
                    }
                }
                Err(e) => text_result(e, true),
            }
        }
        _ => text_result(format!("unknown tool {name}"), true),
    }
}

pub fn handle_request(req: &Value) -> Option<Value> {
    let id = req.get("id").cloned();
    let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let params = req.get("params").cloned().unwrap_or(Value::Null);
    let result = match method {
        "initialize" => json!({
            "protocolVersion": params.get("protocolVersion").and_then(|v| v.as_str()).unwrap_or(PROTOCOL),
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "burncraft", "version": env!("CARGO_PKG_VERSION")}
        }),
        "tools/list" => json!({"tools": tool_list()}),
        "tools/call" => {
            let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
            call_tool(name, params.get("arguments").unwrap_or(&Value::Null))
        }
        "ping" => json!({}),
        _ => {
            id.as_ref()?;
            return Some(json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":format!("method not found: {method}")}}));
        }
    };
    id.map(|id| json!({"jsonrpc":"2.0","id":id,"result":result}))
}

pub fn run_server() {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines().map_while(Result::ok) {
        let Ok(req) = serde_json::from_str::<Value>(&line) else { continue };
        if let Some(resp) = handle_request(&req) {
            let _ = writeln!(stdout, "{}", resp);
            let _ = stdout.flush();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn svg_parse() {
        let l = parse_svg_path("M10 10 L20 10 l0 10 h-10 Z");
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].len(), 5);
        assert_eq!(l[0][2], p2(20.0, 20.0));
    }
    #[test]
    fn json_paths() {
        let v: Value = serde_json::from_str(r#"{"paths":[[[0,0],[1,0],[1,1]]]}"#).unwrap();
        assert_eq!(outlines_from_json(&v)[0].len(), 3);
    }
    #[test]
    fn server_lists_tools() {
        let r = handle_request(&json!({"jsonrpc":"2.0","id":1,"method":"tools/list"})).unwrap();
        assert!(r["result"]["tools"].as_array().unwrap().len() >= 3);
    }
}
