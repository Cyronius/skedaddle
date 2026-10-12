#[derive(Debug, Clone, Default)]
pub struct Prop {
    pub name: String,
    pub params: Vec<(String, String)>,
    pub value: String,
}

impl Prop {
    pub fn param(&self, key: &str) -> Option<&str> {
        self.params.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v.as_str())
    }
}

#[derive(Debug, Clone, Default)]
pub struct Component {
    pub name: String,
    pub props: Vec<Prop>,
    pub children: Vec<Component>,
}

impl Component {
    pub fn prop(&self, name: &str) -> Option<&Prop> {
        self.props.iter().find(|p| p.name == name)
    }

    pub fn props_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Prop> {
        self.props.iter().filter(move |p| p.name == name)
    }

    pub fn text(&self, name: &str) -> Option<String> {
        self.prop(name).map(|p| unescape(&p.value))
    }

    pub fn events(&self) -> Vec<&Component> {
        let mut out = Vec::new();
        self.collect_events(&mut out);
        out
    }

    fn collect_events<'a>(&'a self, out: &mut Vec<&'a Component>) {
        for c in &self.children {
            if c.name == "VEVENT" {
                out.push(c);
            } else {
                c.collect_events(out);
            }
        }
    }
}

pub fn unfold(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for raw in text.split('\n') {
        let raw = raw.strip_suffix('\r').unwrap_or(raw);
        if let Some(rest) = raw.strip_prefix([' ', '\t']) {
            if let Some(last) = lines.last_mut() {
                last.push_str(rest);
                continue;
            }
        }
        lines.push(raw.to_string());
    }
    lines
}

pub fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('n') | Some('N') => out.push('\n'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

pub fn parse_line(line: &str) -> Option<Prop> {
    let b = line.as_bytes();
    let mut i = 0;
    while i < b.len() && b[i] != b';' && b[i] != b':' {
        i += 1;
    }
    if i == 0 || i >= b.len() {
        return None;
    }
    let name = line[..i].to_ascii_uppercase();
    let mut params = Vec::new();
    while b[i] == b';' {
        i += 1;
        let ks = i;
        while i < b.len() && b[i] != b'=' && b[i] != b';' && b[i] != b':' {
            i += 1;
        }
        let key = line[ks..i].to_ascii_uppercase();
        let mut val = String::new();
        if i < b.len() && b[i] == b'=' {
            i += 1;
            loop {
                if i < b.len() && b[i] == b'"' {
                    i += 1;
                    let vs = i;
                    while i < b.len() && b[i] != b'"' {
                        i += 1;
                    }
                    val.push_str(&line[vs..i]);
                    i = (i + 1).min(b.len());
                } else {
                    let vs = i;
                    while i < b.len() && b[i] != b',' && b[i] != b';' && b[i] != b':' {
                        i += 1;
                    }
                    val.push_str(&line[vs..i]);
                }
                if i < b.len() && b[i] == b',' {
                    val.push(',');
                    i += 1;
                } else {
                    break;
                }
            }
        }
        params.push((key, val));
        if i >= b.len() {
            return None;
        }
    }
    if b[i] != b':' {
        return None;
    }
    Some(Prop { name, params, value: line[i + 1..].to_string() })
}

pub fn parse(text: &str) -> Component {
    let mut stack = vec![Component::default()];
    for line in unfold(text) {
        let Some(p) = parse_line(&line) else { continue };
        match p.name.as_str() {
            "BEGIN" => stack.push(Component { name: p.value.to_ascii_uppercase(), ..Default::default() }),
            "END" => {
                if stack.len() > 1 {
                    let done = stack.pop().unwrap();
                    stack.last_mut().unwrap().children.push(done);
                }
            }
            _ => stack.last_mut().unwrap().props.push(p),
        }
    }
    while stack.len() > 1 {
        let done = stack.pop().unwrap();
        stack.last_mut().unwrap().children.push(done);
    }
    stack.pop().unwrap()
}
