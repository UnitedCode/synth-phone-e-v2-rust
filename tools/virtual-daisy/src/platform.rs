use std::{error::Error, fs, path::Path};

pub fn prepare(renode: &Path, directory: &Path, library: &Path) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(directory)?;
    let directory = fs::canonicalize(directory)?;
    let mut platform = fs::read_to_string(renode.join("platforms/cpus/stm32h7.repl"))?;
    // STM32H750/Daisy memory limits: do not inherit H743's extra flash or
    // the generic board's unbounded FMC windows. Bank2 only backs the unused
    // flash-controller object and is deliberately not on the system bus.
    platform = platform.replace(
        "flashBank1: Memory.MappedMemory @ sysbus 0x08000000\n    size: 0x100000",
        "flashBank1: Memory.MappedMemory @ sysbus 0x08000000\n    size: 0x20000",
    );
    platform = platform.replace(
        "flashBank2: Memory.MappedMemory @ sysbus 0x08100000\n    size: 0x100000",
        "flashBank2: Memory.MappedMemory @ none\n    size: 0x20000",
    );
    platform = platform.replace(
        "sdramBank1: Memory.MappedMemory @ sysbus 0xC0000000\n    size: 0x10000000",
        "sdramBank1: Memory.MappedMemory @ sysbus 0xC0000000\n    size: 0x4000000",
    );
    for (name, address) in [
        ("sdramBank2", "0xD0000000"),
        ("externalFlash", "0x80000000"),
        ("sramBank2", "0x70000000"),
        ("sramBank1", "0x60000000"),
    ] {
        platform = platform.replace(
            &format!("{name}: Memory.MappedMemory @ sysbus {address}\n    size: 0x10000000"),
            "",
        );
    }
    let start = platform
        .find("pwr: Python.PythonPeripheral")
        .ok_or("PWR model not found")?;
    let end = platform[start..]
        .find("usb1:")
        .ok_or("USB boundary not found")?
        + start;
    let native = format!("pwr: Miscellaneous.DaisyNative @ sysbus 0x58024800\n    libraryPath: \"{}\"\n    kind: 0\n    size: 0x400\n\nfmc: Miscellaneous.DaisyNative @ sysbus 0x52004000\n    libraryPath: \"{}\"\n    kind: 1\n    size: 0x1000\n\n", library.display(), library.display());
    platform.replace_range(start..end, &native);
    platform = platform.replace("GPIOPort.STM32_GPIOPort", "Miscellaneous.DaisyGpio");
    for (port, letter) in ('A'..='K').enumerate() {
        let marker = format!("rcc RegistrationPeripheralAlias.GPIO{letter}\n    }}");
        platform = platform.replace(
            &marker,
            &format!(
                "{marker}\n    libraryPath: \"{}\"\n    port: {port}",
                library.display()
            ),
        );
    }
    platform = platform
        .lines()
        .filter(|line| {
            !line.trim_start().starts_with("numberOfAFs:")
                && !line.trim_start().starts_with("invertedAFPins:")
        })
        .collect::<Vec<_>>()
        .join("\n");
    platform = platform.replace(
        "syscfg: Miscellaneous.STM32_SYSCFG @",
        "syscfg: Miscellaneous.DaisySyscfg @",
    );
    platform = platform.replace(
        "adcM1S2: Analog.STM32H7_ADC @",
        "adcM1S2: Miscellaneous.DaisyAdc @",
    );
    platform = platform.replace("dma1: DMA.STM32DMA @", "dma1: Miscellaneous.DaisyDma @");
    platform = platform.replace(
        "qspi: SPI.STM32H7_QuadSPI @",
        "qspi: Miscellaneous.DaisyQspi @",
    );
    platform = platform.replace(
        "    -> nvic@92",
        &format!("    libraryPath: \"{}\"", library.display()),
    );
    platform = platform.replace(
        "    [0-7] -> nvic@[11-17, 47]",
        &format!(
            "    libraryPath: \"{}\"\n    [0-7] -> nvic@[11-17, 47]",
            library.display()
        ),
    );
    platform = platform.replace(
        "    referenceVoltage: 3.3",
        &format!(
            "    referenceVoltage: 3.3\n    libraryPath: \"{}\"",
            library.display()
        ),
    );
    // ADC3 stays the upstream model and does not accept the bridge parameter.
    if let Some(index) = platform.find("adc3:") {
        let end = platform[index..].find("bdma:").unwrap() + index;
        let block = platform[index..end]
            .replace(&format!("    libraryPath: \"{}\"\n", library.display()), "");
        platform.replace_range(index..end, &block);
    }
    platform = platform.replace(
        "    [0-15] -> exti@[0-15]",
        &format!(
            "    libraryPath: \"{}\"\n    [0-15] -> exti@[0-15]",
            library.display()
        ),
    );
    // Direct API loading has no Monitor and no executable platform init hooks.
    let mut in_init = false;
    platform = platform
        .lines()
        .filter(|line| {
            if line.trim() == "init:" {
                in_init = true;
                return false;
            }
            if in_init && !line.is_empty() && !line.starts_with(' ') {
                in_init = false;
            }
            !in_init
        })
        .collect::<Vec<_>>()
        .join("\n");
    platform = platform
        .trim_end()
        .trim_end_matches("sysbus:")
        .trim_end()
        .to_owned();
    if platform.contains("Python.") || platform.contains("`py ") {
        return Err("Python platform code remains".into());
    }
    platform.push_str(&format!("\n\ncodec: Miscellaneous.DaisyI2c @ i2c2 0x1a\n    libraryPath: \"{}\"\n    kind: 4\n\noled: Miscellaneous.DaisyI2c @ i2c1 0x3c\n    libraryPath: \"{}\"\n    kind: 5\n", library.display(), library.display()));
    platform.push_str(&format!("\nsai1: Miscellaneous.DaisySai @ sysbus 0x40015800\n    libraryPath: \"{}\"\n    codec: codec\n    DmaA -> dmamux1@87\n    DmaB -> dmamux1@88\n", library.display()));
    fs::write(directory.join("daisy.repl"), platform)?;
    let obsolete = directory.join("boot.resc");
    if obsolete.exists() {
        fs::remove_file(obsolete)?;
    }
    Ok(())
}
