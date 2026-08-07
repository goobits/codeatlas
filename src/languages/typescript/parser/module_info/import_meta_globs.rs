use super::DynamicDependencyTarget;

pub(super) fn group_targets(targets: Vec<DynamicDependencyTarget>) -> Vec<DynamicDependencyTarget> {
    let mut includes = Vec::new();
    let mut excludes = Vec::new();
    let mut has_unknown = false;

    for target in targets {
        match target {
            DynamicDependencyTarget::GlobSet {
                includes: target_includes,
                excludes: target_excludes,
            } => {
                for pattern in target_includes {
                    push_pattern(pattern, &mut includes, &mut excludes);
                }
                excludes.extend(target_excludes);
            }
            DynamicDependencyTarget::Literal(pattern) => {
                push_pattern(pattern, &mut includes, &mut excludes);
            }
            _ => {
                has_unknown = true;
            }
        }
    }

    includes.sort();
    includes.dedup();
    excludes.sort();
    excludes.dedup();

    let mut grouped = Vec::new();
    if !includes.is_empty() || !excludes.is_empty() {
        grouped.push(DynamicDependencyTarget::GlobSet { includes, excludes });
    }
    if has_unknown || grouped.is_empty() {
        grouped.push(DynamicDependencyTarget::Unknown);
    }
    grouped
}

fn push_pattern(pattern: String, includes: &mut Vec<String>, excludes: &mut Vec<String>) {
    if let Some(excluded) = pattern.strip_prefix('!') {
        excludes.push(excluded.to_string());
    } else {
        includes.push(pattern);
    }
}
