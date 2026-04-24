use super::{
    parse,
    apply_string_symbol_fallback, parse_constraints, parse_input_blocks,
    ConstraintParsed, InputBlock, TypedBranch,
};
use crate::{
    shell::Shell,
    web::html_parse::{is_case_placeholder_line, is_query_placeholder_line, parse_task_sections},
};
use anyhow::Context as _;
use std::path::Path;

pub(super) fn load_task_spec(
    task_html_path: &Path,
    bin_letter: &str,
    shell: &mut Shell,
) -> anyhow::Result<Option<(ConstraintParsed, Vec<InputBlock>)>> {
    if !task_html_path.exists() {
        shell.warn(format!(
            "task.html not found at {}; skipping random test",
            task_html_path.display()
        ))?;
        return Ok(None);
    }

    let task_html = std::fs::read_to_string(task_html_path)
        .with_context(|| format!("failed to read {}", task_html_path.display()))?;

    let sections = parse_task_sections(&task_html);
    let section = match sections.into_iter().find(|s| s.letter.eq_ignore_ascii_case(bin_letter)) {
        Some(s) => s,
        None => {
            shell.warn(format!(
                "task section '{}' not found in task.html; skipping random test",
                bin_letter
            ))?;
            return Ok(None);
        }
    };

    let mut parsed = parse_constraints(&section.constraints_items);
    let blocks = build_input_blocks(section.input_blocks);
    // Step B: is_string_symbol fallback (requires blocks to be built first)
    apply_string_symbol_fallback(&blocks, &section.constraints_items, &mut parsed);

    Ok(Some((parsed, blocks)))
}

/// Build input blocks from the raw `<pre>` blocks of a task section.
///
/// Handles two outer-repeat patterns:
/// - Case/query placeholder → `OuterRepeat` (T test cases form)
/// - Query placeholder + every subsequent block starts with a literal integer → `TypedRepeat`
pub(super) fn build_input_blocks(input_blocks: Vec<Vec<String>>) -> Vec<InputBlock> {
    if input_blocks.len() < 2 {
        let input_lines: Vec<String> = input_blocks.into_iter().flatten().collect();
        return parse_input_blocks(&input_lines);
    }

    let first_has_placeholder = input_blocks[0]
        .iter()
        .any(|l| is_case_placeholder_line(l) || is_query_placeholder_line(l));

    if !first_has_placeholder {
        let input_lines: Vec<String> = input_blocks.into_iter().flatten().collect();
        return parse_input_blocks(&input_lines);
    }

    // Outer lines = lines in block 0 without placeholder / vdots
    let outer_lines: Vec<String> = input_blocks[0]
        .iter()
        .filter(|l| {
            !is_case_placeholder_line(l)
                && !is_query_placeholder_line(l)
                && !l.contains("\\vdots")
        })
        .cloned()
        .collect();
    let outer_blocks = parse_input_blocks(&outer_lines);
    let count = outer_blocks
        .iter()
        .find_map(|b| {
            if let InputBlock::Scalars(vars) = b {
                vars.last().map(|v| parse::SizeRef::Var(v.to_lowercase()))
            } else {
                None
            }
        })
        .unwrap_or(parse::SizeRef::Lit(1));

    // Check if the remaining blocks are typed query branches:
    // each branch's first line starts with a literal non-negative integer.
    let first_has_query = input_blocks[0].iter().any(|l| is_query_placeholder_line(l));
    let branches_are_typed = first_has_query && input_blocks[1..].iter().all(|block| {
        block.first()
            .and_then(|l| l.split_whitespace().next())
            .and_then(|t| t.parse::<usize>().ok())
            .is_some()
    });

    let mut blocks = outer_blocks;

    if branches_are_typed {
        // TypedRepeat: each subsequent pre-block is one branch
        // e.g. "1 x" → type_val="1", inner=[Scalars(["x"])]
        let branches: Vec<TypedBranch> = input_blocks[1..]
            .iter()
            .map(|block| {
                let first = block.first().map(|s| s.as_str()).unwrap_or("");
                let mut toks = first.split_whitespace();
                let type_val = toks.next().unwrap_or("1").to_string();
                // Remaining tokens on first line become the first inner line
                let rest: String = toks.collect::<Vec<_>>().join(" ");
                let mut inner_lines: Vec<String> = Vec::new();
                if !rest.is_empty() {
                    inner_lines.push(rest);
                }
                inner_lines.extend(block[1..].iter().cloned());
                let inner = parse_input_blocks(&inner_lines);
                TypedBranch { type_val, inner }
            })
            .collect();
        blocks.push(InputBlock::TypedRepeat { count, branches });
    } else {
        // OuterRepeat: remaining pre-blocks flattened as inner blocks
        let inner_lines: Vec<String> = input_blocks[1..].iter().flatten().cloned().collect();
        let inner = parse_input_blocks(&inner_lines);
        blocks.push(InputBlock::OuterRepeat { count, inner });
    }

    blocks
}
