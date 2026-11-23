//! KiCad schematic candidates only. No board routing or fabrication outputs.
use crate::{
    improvements::{DIODES, FILTER, FILTER470},
    netlist::Graph,
    Result,
};
use std::{fs, path::Path};
pub fn check_cli(args: &[String]) -> Result<()> {
    if args.len() != 5 {
        return Err("check-schematic NAME CANDIDATE KICAD_XML SCHEMATIC NEW_REPORT".into());
    }
    let candidate = Graph::parse(&fs::read_to_string(&args[1])?)?;
    let export = Graph::xml(&fs::read_to_string(&args[2])?)?;
    match args[0].as_str() {
        DIODES => check_harness_export(&candidate, &export)?,
        FILTER | FILTER470 | crate::qualification::RX | crate::qualification::HALL => {
            if candidate != export {
                return Err("schematic export differs from candidate graph".into());
            }
        }
        crate::qualification::OFFLOAD => {
            let expected: Vec<_> = candidate
                .components
                .keys()
                .filter(|r| r.starts_with('P'))
                .collect();
            if export.components.len() != expected.len() {
                return Err("power allocation component count differs".into());
            }
            for r in expected {
                if candidate.components.get(r) != export.components.get(r) {
                    return Err("power allocation component differs".into());
                }
                for (net, nodes) in &candidate.nets {
                    for pin in nodes.iter().filter(|p| p.starts_with(&format!("{r}."))) {
                        if export.net(pin)? != format!("/{net}") {
                            return Err(format!("power allocation label differs at {pin}").into());
                        }
                    }
                }
            }
        }
        _ => return Err("unknown schematic candidate".into()),
    }
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args[4])?;
    use std::io::Write;
    write!(f,"SCHEMATIC_CONNECTIVITY=PASS\ncandidate_sha256={}\nschematic_sha256={}\nexport_sha256={}\nPCB_FABRICATION=NOT_GENERATED\n",crate::sha256(Path::new(&args[1]))?,crate::sha256(Path::new(&args[3]))?,crate::sha256(Path::new(&args[2]))?)?;
    Ok(())
}
fn uuid(n: usize) -> String {
    format!("86205400-1234-4567-8900-{n:012}")
}
fn symbol(source: &str, name: &str) -> Result<String> {
    let start = source
        .find(&format!("(symbol \"{name}\""))
        .ok_or("library symbol missing")?;
    let mut depth = 0;
    let mut quoted = false;
    let mut escape = false;
    for (i, c) in source[start..].char_indices() {
        if escape {
            escape = false;
            continue;
        }
        if c == '\\' && quoted {
            escape = true;
            continue;
        }
        if c == '"' {
            quoted = !quoted;
        }
        if !quoted {
            if c == '(' {
                depth += 1;
            } else if c == ')' {
                depth -= 1;
                if depth == 0 {
                    return Ok(source[start..start + i + 1].into());
                }
            }
        }
    }
    Err("unbalanced library symbol".into())
}
pub fn cli(args: &[String]) -> Result<()> {
    if args.len() != 3 {
        return Err("schematic NAME SOURCE_SCHEMATIC_OR_KICAD_SYMBOL_DIR NEW_OUTPUT".into());
    }
    let out = Path::new(&args[2]);
    // Refuse overwrite, including a production source accidentally supplied as output.
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(out)?;
    let result = (|| -> Result<String> {
        match args[0].as_str() {
            FILTER | FILTER470 => {
                let text = fs::read_to_string(&args[1])?;
                let token = "(property \"Value\" \"4.7n\"";
                if text.matches(token).count() != 100 {
                    return Err("expected 100 independent 4.7n values".into());
                }
                Ok(text.replace(
                    token,
                    &format!(
                        "(property \"Value\" \"{}\"",
                        if args[0] == FILTER { "1n" } else { "470p" }
                    ),
                ))
            }
            DIODES => harness(Path::new(&args[1])),
            _ => Err("unknown schematic candidate".into()),
        }
    })();
    use std::io::Write;
    file.write_all(result?.as_bytes())?;
    Ok(())
}
fn harness(libs: &Path) -> Result<String> {
    let d = symbol(
        &fs::read_to_string(libs.join("Device.kicad_sym"))?,
        "D_Schottky",
    )?
    .replacen("\"D_Schottky\"", "\"Device:D_Schottky\"", 1);
    let s = symbol(
        &fs::read_to_string(libs.join("Switch.kicad_sym"))?,
        "SW_Push",
    )?
    .replacen("\"SW_Push\"", "\"Switch:SW_Push\"", 1);
    let root = uuid(0);
    let mut out=format!("(kicad_sch (version 20250114) (generator \"circuit-workbench\") (uuid \"{root}\") (paper \"A4\")\n(lib_symbols {d}\n{s})\n");
    let mut id = 100;
    for r in 0..4 {
        for c in 0..3 {
            let n = r * 3 + c + 1;
            let x = 50.8 + c as f64 * 76.2;
            let y = 45.72 + r as f64 * 35.56;
            for (reference, value, lib, pos, footprint) in [
                (
                    format!("KD{n}"),
                    "BAT54H",
                    "Device:D_Schottky",
                    x + 7.62,
                    "Diode_SMD:D_SOD-123F",
                ),
                (format!("KS{n}"), "SW_Push", "Switch:SW_Push", x - 7.62, ""),
            ] {
                let angle = if lib == "Switch:SW_Push" { 180 } else { 0 };
                out.push_str(&format!("(symbol (lib_id \"{lib}\") (at {pos} {y} {angle}) (unit 1) (in_bom yes) (on_board yes) (dnp no) (uuid \"{}\")\n(property \"Reference\" \"{reference}\" (at {pos} {} 0) (effects (font (size 1.27 1.27))))\n(property \"Value\" \"{value}\" (at {pos} {} 0) (effects (font (size 1.27 1.27))))\n(property \"Footprint\" \"{footprint}\" (at {pos} {y} 0) (effects (font (size 1.27 1.27)) (hide yes)))\n(instances (project \"keypad-harness\" (path \"/{root}\" (reference \"{reference}\") (unit 1)))))\n",uuid(id),y-5.,y+5.));
                id += 1;
            }
            // Standard symbols: diode K left at -3.81, A right +3.81; switch +/-5.08.
            out.push_str(&format!("(wire (pts (xy {} {y}) (xy {} {y})) (stroke (width 0) (type default)) (uuid \"{}\"))\n",x-2.54,x+3.81,uuid(id)));
            id += 1;
            for (name, pos, angle) in [
                (format!("col{}", c + 1), x - 12.7, 180),
                (format!("row{}", r + 1), x + 11.43, 0),
            ] {
                out.push_str(&format!("(label \"{name}\" (at {pos} {y} {angle}) (effects (font (size 1.27 1.27)) (justify left bottom)) (uuid \"{}\"))\n",uuid(id)));
                id += 1;
            }
        }
    }
    for (y,text) in [(18,"Synthphone-E candidate: one BAT54H per key; anode to row, cathode toward switch/column"),(25,"External keypad/harness replacement; unchanged main board and firmware. Do not put one diode per shared wire."),(182,"Mate to existing J6: pins 1=col3, 2=col2, 3=col1, 4=row4, 5=row3, 6=row2, 7=row1"),(189,"Schematic experiment only. Switch mechanics and PCB layout are for the builder; no fabrication files generated.")] {
        out.push_str(&format!("(text \"{text}\" (at 20 {y} 0) (effects (font (size 1.27 1.27)) (justify left)) (uuid \"{}\"))\n",uuid(id)));id+=1;
    }
    out.push_str("(sheet_instances (path \"/\" (page \"1\"))) (embedded_fonts no))\n");
    Ok(out)
}
/// Validate exported schematic connectivity against the exact candidate's added subcircuit.
pub fn check_harness_export(candidate: &Graph, export: &Graph) -> Result<()> {
    if export.components.len() != 24 {
        return Err("harness export must contain exactly twelve diodes and switches".into());
    }
    for (r, v) in &export.components {
        if candidate.components.get(r) != Some(v) {
            return Err("export component differs".into());
        }
    }
    for nodes in export.nets.values() {
        let first = nodes.first().ok_or("empty export net")?;
        for n in nodes {
            candidate.connected(first, n)?;
        }
    }
    for r in 0..4 {
        for c in 0..3 {
            let n = r * 3 + c + 1;
            if export.net(&format!("KD{n}.2"))? != format!("/row{}", r + 1)
                || export.net(&format!("KS{n}.2"))? != format!("/col{}", c + 1)
            {
                return Err("export row/column label mismatch".into());
            }
            export.connected(&format!("KD{n}.1"), &format!("KS{n}.1"))?;
        }
    }
    Ok(())
}

/// A separate full schematic with only the selected receiver pull-up changed.
pub fn rx_resistor(input: &Path, out: &Path) -> Result<()> {
    let text = fs::read_to_string(input)?;
    let pos = text
        .find("(property \"Reference\" \"R5\"")
        .ok_or("R5 reference missing")?;
    let start = text[..pos]
        .rfind("\n\t(symbol")
        .ok_or("R5 instance missing")?
        + 1;
    let tail = &text[start..];
    let mut depth = 0;
    let mut quoted = false;
    let mut escaped = false;
    let mut end = None;
    for (i, c) in tail.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if c == '\\' && quoted {
            escaped = true;
            continue;
        }
        if c == '"' {
            quoted = !quoted;
        }
        if !quoted {
            if c == '(' {
                depth += 1;
            } else if c == ')' {
                depth -= 1;
                if depth == 0 {
                    end = Some(start + i + 1);
                    break;
                }
            }
        }
    }
    let end = end.ok_or("unbalanced R5 instance")?;
    let block = &text[start..end];
    let old = "(property \"Value\" \"220\"";
    if block.matches(old).count() != 1 {
        return Err("R5 value differs from 220 ohm baseline".into());
    }
    let changed = format!(
        "{}{}{}",
        &text[..start],
        block.replace(old, "(property \"Value\" \"2.2k\""),
        &text[end..]
    );
    use std::io::Write;
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(out)?
        .write_all(changed.as_bytes())?;
    Ok(())
}

/// Functional allocation drawing of a new carrier rail and labeled boundaries.
/// Pins are modeled terminals, not a production library/footprint qualification.
pub fn allocation(base: &Graph, g: &Graph, out: &Path) -> Result<()> {
    let refs: Vec<_> = g
        .components
        .keys()
        .filter(|r| !base.components.contains_key(*r))
        .collect();
    let root = uuid(9000);
    let mut defs = String::new();
    let mut body = String::new();
    let mut id = 10000;
    for (index, r) in refs.iter().enumerate() {
        let mut pins: Vec<_> = g
            .nets
            .iter()
            .flat_map(|(net, nodes)| {
                nodes.iter().filter_map(move |n| {
                    n.strip_prefix(&format!("{r}."))
                        .map(|p| (p.to_string(), net.clone()))
                })
            })
            .collect();
        pins.sort_by_key(|(p, _)| p.parse::<usize>().unwrap_or(0));
        let name = format!("Allocation{index}");
        let lib = format!("CircuitStudy:{name}");
        let height = (pins.len() as f64 + 1.) * 2.54;
        defs.push_str(&format!("(symbol \"{lib}\" (pin_names (offset 0.5) (hide yes)) (property \"Reference\" \"U\" (at 0 5.08 0) (effects (font (size 1.27 1.27)))) (property \"Value\" \"{name}\" (at 0 2.54 0) (effects (font (size 1.27 1.27)))) (symbol \"{name}_0_1\" (rectangle (start -20.32 2.54) (end 20.32 -{height}) (stroke (width 0.254) (type default)) (fill (type background)))) (symbol \"{name}_1_1\""));
        let x = 101.6 + (index % 5) as f64 * 157.48;
        let y = 40.64 + (index / 5) as f64 * 76.2;
        for (i, (pin, net)) in pins.iter().enumerate() {
            let dy = i as f64 * 2.54;
            defs.push_str(&format!("(pin passive line (at -25.4 -{dy} 0) (length 5.08) (name \"{pin}\" (effects (font (size 1 1)))) (number \"{pin}\" (effects (font (size 1 1)))))"));
            body.push_str(&format!("(wire (pts (xy {} {}) (xy {} {})) (stroke (width 0) (type default)) (uuid \"{}\"))\n",x-30.48,y+dy,x-25.4,y+dy,uuid(id)));
            id += 1;
            body.push_str(&format!("(label \"{net}\" (at {} {} 0) (effects (font (size 1 1)) (justify right bottom)) (uuid \"{}\"))\n",x-30.48,y+dy,uuid(id)));
            id += 1;
        }
        defs.push_str("))\n");
        body.push_str(&format!("(symbol (lib_id \"{lib}\") (at {x} {y} 0) (unit 1) (in_bom yes) (on_board no) (dnp no) (uuid \"{}\") (property \"Reference\" \"{r}\" (at {x} {} 0) (effects (font (size 1.27 1.27)))) (property \"Value\" \"{}\" (at {x} {} 0) (effects (font (size 1 1)))) (instances (project \"power-allocation\" (path \"/{root}\" (reference \"{r}\") (unit 1)))))\n",uuid(id),y-7.62,g.components[*r],y-3.81));
        id += 1;
    }
    let text=format!("(kicad_sch (version 20250114) (generator \"circuit-workbench\") (uuid \"{root}\") (paper \"A1\") (lib_symbols {defs}) {body} (text \"Guarded carrier rail allocation: PG1 UNSELECTED. NOT accepted for promotion or fabrication. See study report.\" (at 25.4 15.24 0) (effects (font (size 2 2)) (justify left)) (uuid \"{}\")) (text \"New split-rail carrier required; a socket interposer alone cannot split the original power plane. Labels name Seed pads and carrier nets. External 5V never joins USB/VIN.\" (at 25.4 22.86 0) (effects (font (size 1.27 1.27)) (justify left)) (uuid \"{}\")) (sheet_instances (path \"/\" (page \"1\"))) (embedded_fonts no))",uuid(id),uuid(id+1));
    use std::io::Write;
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(out)?
        .write_all(text.as_bytes())?;
    Ok(())
}

pub fn hall_sensor(input: &Path, out: &Path) -> Result<()> {
    let text = fs::read_to_string(input)?;
    if text.matches("(lib_id \"HE60:MT9102ET\")").count() != 100
        || text.matches("(property \"Value\" \"MT9102ET\"").count() != 101
    {
        return Err("expected one original symbol definition and 100 Hall instances".into());
    }
    // DBZ pin functions match: VCC1, OUT2, GND3. Keep nets and geometry;
    // footprint remains a separate mechanical review, not claimed drop-in fit.
    let changed = text
        .replace("\"HE60:MT9102ET\"", "\"Study:DRV5053RA\"")
        .replace("\"MT9102ET_", "\"DRV5053RA_")
        .replace(
            "(property \"Value\" \"MT9102ET\"",
            "(property \"Value\" \"DRV5053RAQDBZR\"",
        );
    use std::io::Write;
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(out)?
        .write_all(changed.as_bytes())?;
    Ok(())
}
