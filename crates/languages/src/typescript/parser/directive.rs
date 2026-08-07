use codeatlas_domain::FuzzPolicyEvidence;

pub(super) fn fuzz_policy(source: &str, start_line: u32) -> Option<FuzzPolicyEvidence> {
    let lines = source.lines().collect::<Vec<_>>();
    let mut index = start_line.saturating_sub(1) as usize;
    if index == 0 || index > lines.len() {
        return None;
    }
    index -= 1;
    while index > 0 && lines[index].trim_start().starts_with('@') {
        index -= 1;
    }
    if !lines.get(index)?.trim_end().ends_with("*/") {
        return None;
    }

    let end = index;
    while !lines[index].contains("/**") {
        if index == 0 {
            return None;
        }
        index -= 1;
    }
    let start = index;
    let mut documentation = Vec::new();
    for (offset, line) in lines[start..=end].iter().enumerate() {
        let mut line = line.trim();
        if offset == 0 {
            line = line.strip_prefix("/**").unwrap_or(line);
        }
        if start + offset == end {
            line = line.strip_suffix("*/").unwrap_or(line);
        }
        line = line.trim().strip_prefix('*').unwrap_or(line.trim()).trim();
        documentation.push(((start + offset + 1) as u32, line.to_string()));
    }
    codeatlas_domain::parse_fuzz_directive_lines(documentation)
}
