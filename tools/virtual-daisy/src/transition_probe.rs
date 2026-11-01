//! PM0253 frame reuse and priority ordering through executed ARM handlers.
use super::{grade, Result};
use std::{fs, path::Path};
fn words(prefix: &Path, suffix: &str) -> Result<Vec<u32>> {
    let data = fs::read(grade::artifact(prefix, suffix))?;
    if data.len() != 128 {
        return Err("Truncated ARM transition capture".into());
    }
    Ok(data
        .as_chunks::<4>()
        .0
        .iter()
        .map(|&b| u32::from_le_bytes(b))
        .collect())
}
fn equal(actual: u32, expected: u32, label: &str) -> Result<()> {
    if actual != expected {
        return Err(
            format!("ARM transition {label}: expected {expected:#x}, got {actual:#x}").into(),
        );
    }
    Ok(())
}
pub fn check(prefix: &Path, kind: &str) -> Result<()> {
    grade::execution_evidence(prefix)?;
    let report = grade::artifact(prefix, ".transition.txt");
    if report.exists() {
        fs::remove_file(&report)?;
    }
    let capture = words(prefix, ".transition.bin")?;
    let early = words(prefix, ".transition-early.bin")?;
    let nested = kind == "transition-nested";
    let floating_point = kind == "transition-fp";
    let low_first = nested || kind == "transition-equal";
    equal(early[0], 0x42, "early readiness")?;
    equal(early[1], 0, "handler before shared stack service")?;
    equal(capture[0], 0xab, "completion")?;
    equal(capture[1], 2, "handler count")?;
    equal(capture[2], 3, "both interrupts")?;
    equal(capture[4], capture[3], "restored thread SP")?;
    equal(capture[5], 0, "active interrupts after return")?;
    equal(capture[31], 0, "unexpected exception")?;
    if !(0x3001e000..=0x3001f000).contains(&capture[3])
        || (nested && !(0x3001e000..=0x3001f000).contains(&capture[7]))
    {
        return Err("ARM transition used an invalid shared-SRAM stack".into());
    }
    for (index, number) in if low_first { [16, 17] } else { [17, 16] }
        .into_iter()
        .enumerate()
    {
        let base = 8 + index * 8;
        equal(capture[base], number, "handler order")?;
        equal(capture[base + 3], number, "IPSR")?;
        let high_nested = nested && number == 17;
        equal(
            capture[base + 1],
            if high_nested {
                0xfffffff1
            } else if floating_point {
                0xffffffe9
            } else {
                0xfffffff9
            },
            "EXC_RETURN",
        )?;
        equal(
            capture[base + 2],
            if high_nested {
                capture[7] - 32
            } else {
                capture[3] - if floating_point { 104 } else { 32 }
            },
            "shared frame SP",
        )?;
        equal(
            (capture[base + 4] >> 11) & 1,
            u32::from(!high_nested),
            "RETTOBASE",
        )?;
        equal(
            capture[base + 5],
            if high_nested { 3 } else { 1 << (number - 16) },
            "active interrupt mask",
        )?;
        equal(
            capture[base + 7] & 0x1ff,
            if high_nested { 16 } else { 0 },
            "stacked IPSR",
        )?;
        equal((capture[base + 7] >> 24) & 1, 1, "stacked Thumb state")?;
    }
    if !nested {
        equal(capture[14], capture[22], "shared return PC")?;
    }
    if floating_point {
        equal(
            capture[24],
            0x3fc00000,
            "thread S0 restored after FP tail chain",
        )?;
        equal(
            capture[25],
            0x00400000,
            "thread FPSCR restored after FP tail chain",
        )?;
        equal(capture[26] & 1, 0, "lazy FP preservation completed")?;
    }
    if kind == "transition-return" {
        let returning = words(prefix, ".transition-return.bin")?;
        equal(returning[0], 0x42, "return still in progress")?;
        equal(returning[1], 1, "return handler count")?;
        equal(returning[2], 2, "return high handler completed")?;
        if grade::counter(
            &grade::artifact(prefix, ".transition-return.memory.txt"),
            "exception_return_wait_yields",
        )? == 0
            || grade::counter(
                &grade::artifact(prefix, ".memory.txt"),
                "abandoned_cpu_waits",
            )? == 0
        {
            return Err("ARM transition did not interrupt pending modeled unstack service".into());
        }
    }
    if grade::counter(
        &grade::artifact(prefix, ".memory.txt"),
        "exception_stack_words",
    )? != if nested { 16 } else { 8 }
    {
        return Err("ARM transition stacked extra or missing frames".into());
    }
    let early_stats = grade::artifact(prefix, ".transition-early.memory.txt");
    if grade::counter(&early_stats, "exception_stack_words")? != 8
        || grade::counter(&early_stats, "exception_entry_wait_yields")? == 0
    {
        return Err("ARM transition did not exercise a single pending shared-SRAM entry".into());
    }
    fs::write(report, format!("case={kind}\nhandler_order_frame_reuse_active_state_and_return=passed\npriority_masks_pipeline_and_cycle_latency=NOT_CERTIFIED\n"))?;
    Ok(())
}
