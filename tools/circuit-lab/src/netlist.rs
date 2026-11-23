use crate::Result;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Graph {
    pub components: BTreeMap<String, String>,
    pub nets: BTreeMap<String, Vec<String>>,
}
fn unescape(s: &str) -> String {
    s.replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}
fn attr(line: &str, name: &str) -> Result<String> {
    let key = format!("{name}=\"");
    let s = line.split_once(&key).ok_or("missing XML attribute")?.1;
    Ok(unescape(
        s.split_once('"').ok_or("unterminated XML attribute")?.0,
    ))
}
impl Graph {
    /// Accept KiCad's line-oriented kicadxml exporter, not arbitrary user XML.
    pub fn xml(xml: &str) -> Result<Self> {
        let mut g = Self::default();
        let mut component = None;
        let mut net = None;
        for line in xml.lines().map(str::trim) {
            if line.starts_with("<comp ") {
                component = Some(attr(line, "ref")?);
            }
            if let Some(reference) = &component {
                if let Some(value) = line
                    .strip_prefix("<value>")
                    .and_then(|s| s.strip_suffix("</value>"))
                {
                    if g.components
                        .insert(reference.clone(), unescape(value))
                        .is_some()
                    {
                        return Err("duplicate component".into());
                    }
                }
            }
            if line == "</comp>" {
                component = None;
            }
            if line.starts_with("<net ") {
                let name = attr(line, "name")?;
                if g.nets.insert(name.clone(), Vec::new()).is_some() {
                    return Err("duplicate net".into());
                }
                net = Some(name);
            }
            if line.starts_with("<node ") {
                g.nets
                    .get_mut(net.as_ref().ok_or("node outside net")?)
                    .unwrap()
                    .push(format!("{}.{}", attr(line, "ref")?, attr(line, "pin")?));
            }
            if line == "</net>" {
                net = None;
            }
        }
        for nodes in g.nets.values_mut() {
            nodes.sort();
        }
        g.validate()?;
        Ok(g)
    }
    pub fn canonical(&self) -> String {
        let mut out = String::new();
        for (r, v) in &self.components {
            out.push_str(&format!("C\t{r}\t{v}\n"));
        }
        for (name, nodes) in &self.nets {
            let mut nodes = nodes.clone();
            nodes.sort();
            out.push_str(&format!("N\t{name}\t{}\n", nodes.join("\t")));
        }
        out
    }
    pub fn parse(text: &str) -> Result<Self> {
        let mut g = Self::default();
        for line in text
            .lines()
            .filter(|l| !l.starts_with('#') && !l.is_empty())
        {
            let f: Vec<_> = line.split('\t').collect();
            match f.as_slice() {
                ["C", r, v] => {
                    if g.components.insert((*r).into(), (*v).into()).is_some() {
                        return Err("duplicate component".into());
                    }
                }
                ["N", name, nodes @ ..] if !nodes.is_empty() => {
                    if g.nets
                        .insert(
                            (*name).into(),
                            nodes.iter().map(|s| s.to_string()).collect(),
                        )
                        .is_some()
                    {
                        return Err("duplicate net".into());
                    }
                }
                _ => return Err(format!("invalid baseline row: {line}").into()),
            }
        }
        for nodes in g.nets.values_mut() {
            nodes.sort();
        }
        g.validate()?;
        Ok(g)
    }
    fn validate(&self) -> Result<()> {
        if self.components.is_empty() || self.nets.is_empty() {
            return Err("empty circuit".into());
        }
        let mut seen = BTreeMap::new();
        for (net, nodes) in &self.nets {
            for node in nodes {
                let reference = node.rsplit_once('.').ok_or("invalid node")?.0;
                if !self.components.contains_key(reference) {
                    return Err(format!("unknown component {reference}").into());
                }
                if seen.insert(node, net).is_some() {
                    return Err(format!("pin {node} belongs to multiple nets").into());
                }
            }
        }
        Ok(())
    }
    pub fn net(&self, node: &str) -> Result<&str> {
        self.nets
            .iter()
            .find(|(_, n)| n.iter().any(|n| n == node))
            .map(|(name, _)| name.as_str())
            .ok_or_else(|| format!("missing pin {node}").into())
    }
    pub fn connected(&self, a: &str, b: &str) -> Result<()> {
        if self.net(a)? != self.net(b)? {
            return Err(format!("open/wrong route {a} -> {b}").into());
        }
        Ok(())
    }
    pub fn value(&self, r: &str) -> Result<f64> {
        let s = self.components.get(r).ok_or("missing resistor")?;
        let (v, m) = match s.chars().last() {
            Some('k') => (&s[..s.len() - 1], 1e3),
            Some('M') => (&s[..s.len() - 1], 1e6),
            Some('m') => (&s[..s.len() - 1], 1e-3),
            Some('u') => (&s[..s.len() - 1], 1e-6),
            Some('n') => (&s[..s.len() - 1], 1e-9),
            Some('p') => (&s[..s.len() - 1], 1e-12),
            _ => (s.as_str(), 1.),
        };
        let v = v.parse::<f64>()? * m;
        if !v.is_finite() || v <= 0. {
            return Err("invalid resistance".into());
        }
        Ok(v)
    }
    pub fn disconnect(&mut self, node: &str) {
        for nodes in self.nets.values_mut() {
            nodes.retain(|n| n != node);
        }
        self.nets.insert(format!("fault-{node}"), vec![node.into()]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_duplicate_pin_and_empty_import() {
        assert!(Graph::parse("C\tR1\t220\nN\ta\tR1.1\nN\tb\tR1.1\n").is_err());
        assert!(Graph::xml("<garbage/>").is_err());
    }
    #[test]
    fn canonical_round_trip_and_open_fault() {
        let mut g = Graph::parse("C\tR1\t220\nC\tA1\tSeed\nN\ta\tR1.1\tA1.15\n").unwrap();
        assert_eq!(g, Graph::parse(&g.canonical()).unwrap());
        g.connected("R1.1", "A1.15").unwrap();
        g.disconnect("A1.15");
        assert!(g.connected("R1.1", "A1.15").is_err());
    }
}
