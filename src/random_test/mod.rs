pub(crate) mod generate;
pub(crate) mod parse;

mod cargo_reg;
mod cross_check;
mod runner;
mod task_spec;

pub(crate) use cargo_reg::ensure_cross_bin_registered;
pub(crate) use cross_check::{CrossCheckArgs, run_cross_check};
pub(crate) use parse::{
    annotate_blocks, apply_string_symbol_fallback, parse_constraints, parse_input_blocks,
    ConstraintParsed, InputBlock, TypedBranch,
};
pub(crate) use runner::{RandomTestArgs, run_random_tests};

/// Print a section separator banner to stderr and flush so it appears before any progress bar.
pub(crate) fn write_section_banner(out: &mut dyn termcolor::WriteColor, title: &str) -> anyhow::Result<()> {
    writeln!(out)?;
    writeln!(out, "══════════════════════════════════════════")?;
    writeln!(out, "{:^42}", title)?;
    writeln!(out, "══════════════════════════════════════════")?;
    out.flush()?;
    Ok(())
}
