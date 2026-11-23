//! Project-owned circuit fixtures with frozen KiCad connectivity and explicit evidence levels.
pub mod carrier;
#[cfg(feature = "device")]
pub mod device;
pub mod electrical;
pub mod fixture;
pub mod netlist;
pub mod oled;
pub mod profiles;
#[cfg(feature = "spice")]
pub mod spice;
use std::{fs, path::Path, process::Command};
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub fn sha256(path: &Path) -> Result<String> {
    let out = Command::new("shasum")
        .arg("-a")
        .arg("256")
        .arg(path)
        .output()?;
    if !out.status.success() {
        return Err("shasum failed".into());
    }
    Ok(String::from_utf8(out.stdout)?
        .split_whitespace()
        .next()
        .ok_or("missing hash")?
        .into())
}

pub fn cli(args: Vec<String>) -> Result<()> {
    match args.first().map(String::as_str) {
        #[cfg(feature="device")]
        Some("device")=>return device::run(&args[1..]),
        Some("import") if args.len() == 4 => {
            let graph = netlist::Graph::xml(&fs::read_to_string(&args[1])?)?;
            // Source hash ties the deterministic export to a particular design, not an export date.
            let text = format!("# source_sha256={}\n{}", sha256(Path::new(&args[2]))?, graph.canonical());
            fs::write(&args[3], text)?;
            println!("Imported {} components, {} nets", graph.components.len(), graph.nets.len());
        }
        Some("verify") if args.len() == 5 => {
            let input = Path::new(&args[2]);
            let fixture_path = Path::new(&args[3]);
            let fixture = fixture::Fixture::parse(&fs::read_to_string(fixture_path)?)?;
            let output = Path::new(&args[4]);
            // Preserve failed runs too; refuse to clobber anything.
            fs::create_dir(output)?;
            let baseline = fs::read_to_string(input)?;
            let graph = netlist::Graph::parse(&baseline)?;
            fs::copy(input, output.join("baseline.net"))?;
            fs::copy(fixture_path, output.join("assumptions.fixture"))?;
            fs::write(output.join("runner-sources.sha256"),env!("CIRCUIT_WORKBENCH_SOURCES").replace(';',"\n"))?;
            let result = profiles::verify(&args[1], &graph, &fixture, output);
            let report = match &result {
                Ok(report) => format!("MODEL_REGRESSION=PASS\nprofile={}\nbaseline_sha256={}\nfixture_sha256={}\nrunner_version={}\n{}",args[1],sha256(input)?,sha256(fixture_path)?,env!("CARGO_PKG_VERSION"),report),
                Err(error) => format!("MODEL_REGRESSION=FAIL\nprofile={}\nerror={error}\n",args[1]),
            };
            fs::write(output.join("report.txt"), &report)?;
            println!("{report}");
            result?;
        }
        Some("check-source") if args.len() == 3 => {
            let baseline = fs::read_to_string(&args[1])?;
            let hash = sha256(Path::new(&args[2]))?;
            if baseline.lines().next() != Some(&format!("# source_sha256={hash}")) {
                return Err("design source differs from frozen baseline; create a new revision for comparison".into());
            }
            println!("SOURCE_MATCH=PASS");
        }
        _ => return Err("usage: circuit-workbench import KICAD_XML SCHEMATIC BASELINE | check-source BASELINE SCHEMATIC | verify PROFILE BASELINE FIXTURE FRESH_OUTPUT".into()),
    }
    Ok(())
}
