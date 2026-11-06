//! Small, forgiving XML reader/writer — enough for LightBurn project files.

#[derive(Clone, Debug, Default)]
pub struct Node {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Node>,
    pub text: String,
}

impl Node {
    pub fn new(name: &str) -> Node {
        Node { name: name.to_string(), ..Default::default() }
    }
    pub fn attr(&self, k: &str) -> Option<&str> {
        self.attrs.iter().find(|(a, _)| a == k).map(|(_, v)| v.as_str())
    }
    pub fn attr_f64(&self, k: &str) -> Option<f64> {
        self.attr(k).and_then(|v| v.trim().parse().ok())
    }
    pub fn attr_bool(&self, k: &str) -> Option<bool> {
        self.attr(k).map(|v| matches!(v.trim(), "1" | "True" | "true"))
    }
    pub fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.name == name)
    }
    pub fn set(&mut self, k: &str, v: impl Into<String>) -> &mut Self {
        self.attrs.push((k.to_string(), v.into()));
        self
    }
    pub fn push(&mut self, n: Node) -> &mut Self {
        self.children.push(n);
        self
    }
    pub fn with_text(mut self, t: impl Into<String>) -> Node {
        self.text = t.into();
        self
    }

    pub fn write(&self, out: &mut String, depth: usize) {
        let pad = "    ".repeat(depth);
        out.push_str(&pad);
        out.push('<');
        out.push_str(&self.name);
        for (k, v) in &self.attrs {
            out.push(' ');
            out.push_str(k);
            out.push_str("=\"");
            out.push_str(&escape(v));
            out.push('"');
        }
        if self.children.is_empty() && self.text.is_empty() {
            out.push_str("/>\n");
            return;
        }
        out.push('>');
        if !self.children.is_empty() {
            out.push('\n');
            for c in &self.children {
                c.write(out, depth + 1);
            }
            if !self.text.is_empty() {
                out.push_str(&pad);
                out.push_str("    ");
                out.push_str(&escape(&self.text));
                out.push('\n');
            }
            out.push_str(&pad);
        } else {
            out.push_str(&escape(&self.text));
        }
        out.push_str("</");
        out.push_str(&self.name);
        out.push_str(">\n");
    }
}

pub fn escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\n' => o.push_str("&#10;"),
            _ => o.push(c),
        }
    }
    o
}

pub fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut o = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        o.push_str(&rest[..i]);
        rest = &rest[i..];
        if let Some(j) = rest.find(';') {
            let ent = &rest[1..j];
            let rep = match ent {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                _ if ent.starts_with("#x") || ent.starts_with("#X") => u32::from_str_radix(&ent[2..], 16).ok().and_then(char::from_u32),
                _ if ent.starts_with('#') => ent[1..].parse::<u32>().ok().and_then(char::from_u32),
                _ => None,
            };
            if let Some(c) = rep {
                o.push(c);
                rest = &rest[j + 1..];
                continue;
            }
        }
        o.push('&');
        rest = &rest[1..];
    }
    o.push_str(rest);
    o
}

pub fn parse(src: &str) -> Result<Node, String> {
    let b = src.as_bytes();
    let mut i = 0usize;
    let mut stack: Vec<Node> = vec![Node::new("#document")];
    while i < b.len() {
        if b[i] == b'<' {
            if src[i..].starts_with("<!--") {
                i = src[i..].find("-->").map(|j| i + j + 3).ok_or("unterminated comment")?;
            } else if src[i..].starts_with("<![CDATA[") {
                let end = src[i..].find("]]>").map(|j| i + j).ok_or("unterminated CDATA")?;
                stack.last_mut().unwrap().text.push_str(&src[i + 9..end]);
                i = end + 3;
            } else if src[i..].starts_with("<?") || src[i..].starts_with("<!") {
                i = src[i..].find('>').map(|j| i + j + 1).ok_or("unterminated declaration")?;
            } else if src[i..].starts_with("</") {
                let end = src[i..].find('>').map(|j| i + j).ok_or("unterminated end tag")?;
                let name = src[i + 2..end].trim();
                i = end + 1;
                if stack.len() > 1 {
                    let mut node = stack.pop().unwrap();
                    if node.name != name {
                        // tolerate mismatches: close anyway
                    }
                    node.text = unescape(node.text.trim());
                    stack.last_mut().unwrap().children.push(node);
                }
            } else {
                // start tag
                let mut j = i + 1;
                while j < b.len() && !b[j].is_ascii_whitespace() && b[j] != b'>' && b[j] != b'/' {
                    j += 1;
                }
                let mut node = Node::new(&src[i + 1..j]);
                // attributes
                loop {
                    while j < b.len() && b[j].is_ascii_whitespace() {
                        j += 1;
                    }
                    if j >= b.len() {
                        return Err("unexpected end in tag".into());
                    }
                    if b[j] == b'>' {
                        j += 1;
                        stack.push(node);
                        break;
                    }
                    if b[j] == b'/' {
                        let k = src[j..].find('>').map(|k| j + k + 1).ok_or("bad empty tag")?;
                        j = k;
                        stack.last_mut().unwrap().children.push(node);
                        break;
                    }
                    let ks = j;
                    while j < b.len() && b[j] != b'=' && !b[j].is_ascii_whitespace() && b[j] != b'>' {
                        j += 1;
                    }
                    let key = src[ks..j].to_string();
                    while j < b.len() && b[j].is_ascii_whitespace() {
                        j += 1;
                    }
                    if j < b.len() && b[j] == b'=' {
                        j += 1;
                        while j < b.len() && b[j].is_ascii_whitespace() {
                            j += 1;
                        }
                        let q = b[j];
                        if q == b'"' || q == b'\'' {
                            let vs = j + 1;
                            let ve = b[vs..].iter().position(|c| *c == q).map(|p| vs + p).ok_or("unterminated attribute")?;
                            node.attrs.push((key, unescape(&src[vs..ve])));
                            j = ve + 1;
                        } else {
                            let vs = j;
                            while j < b.len() && !b[j].is_ascii_whitespace() && b[j] != b'>' {
                                j += 1;
                            }
                            node.attrs.push((key, unescape(&src[vs..j])));
                        }
                    } else {
                        node.attrs.push((key, String::new()));
                    }
                }
                i = j;
            }
        } else {
            let end = src[i..].find('<').map(|j| i + j).unwrap_or(b.len());
            stack.last_mut().unwrap().text.push_str(&src[i..end]);
            i = end;
        }
    }
    while stack.len() > 1 {
        let node = stack.pop().unwrap();
        stack.last_mut().unwrap().children.push(node);
    }
    let doc = stack.pop().unwrap();
    doc.children.into_iter().next().ok_or_else(|| "no root element".to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn parse_basic() {
        let n = super::parse("<?xml version=\"1.0\"?><a x=\"1 &amp; 2\"><b>hi</b><c/></a>").unwrap();
        assert_eq!(n.name, "a");
        assert_eq!(n.attr("x"), Some("1 & 2"));
        assert_eq!(n.child("b").unwrap().text, "hi");
        assert!(n.child("c").is_some());
    }
}
