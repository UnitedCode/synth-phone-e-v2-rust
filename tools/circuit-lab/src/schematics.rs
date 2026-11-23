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
        FILTER | FILTER470 => {
            if candidate != export {
                return Err("schematic export differs from candidate graph".into());
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
