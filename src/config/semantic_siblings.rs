use super::validate_lexicon_identifier;
use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

pub(crate) const MAXIMUM_SEMANTIC_SIBLING_NOMINATIONS: u32 = 200;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct SemanticSiblingsConfig {
    pub comparison_sets: Vec<SemanticSiblingComparisonSetConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SemanticSiblingComparisonSetConfig {
    pub id: String,
    #[serde(default)]
    pub purpose: Option<String>,
    pub members: Vec<SemanticSiblingMemberConfig>,
    #[serde(default = "default_maximum_nominations")]
    pub maximum_nominations: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SemanticSiblingMemberConfig {
    pub id: String,
    pub paths: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum SemanticSiblingPathKind {
    File,
    Directory,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct ResolvedSemanticSiblingPath {
    pub relative: String,
    pub absolute: PathBuf,
    pub kind: SemanticSiblingPathKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ResolvedSemanticSiblingMember {
    pub id: String,
    pub paths: Vec<ResolvedSemanticSiblingPath>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ResolvedSemanticSiblingComparisonSet {
    pub id: String,
    pub purpose: Option<String>,
    pub members: Vec<ResolvedSemanticSiblingMember>,
    pub maximum_nominations: u32,
}

impl SemanticSiblingsConfig {
    pub(crate) fn validate_structure(&self) -> Result<()> {
        let mut set_ids = BTreeSet::new();
        for set in &self.comparison_sets {
            validate_lexicon_identifier(&set.id, "semantic sibling comparison-set")?;
            if !set_ids.insert(set.id.as_str()) {
                anyhow::bail!(
                    "Duplicate lexicon semantic sibling comparison-set ID {:?}",
                    set.id
                );
            }
            if let Some(purpose) = &set.purpose {
                validate_purpose(purpose, &set.id)?;
            }
            if set.maximum_nominations == 0
                || set.maximum_nominations > MAXIMUM_SEMANTIC_SIBLING_NOMINATIONS
            {
                anyhow::bail!(
                    "Lexicon semantic sibling comparison set {:?} maximum_nominations must be between 1 and {MAXIMUM_SEMANTIC_SIBLING_NOMINATIONS}",
                    set.id
                );
            }
            if set.members.len() < 2 {
                anyhow::bail!(
                    "Lexicon semantic sibling comparison set {:?} needs at least two members",
                    set.id
                );
            }
            let mut member_ids = BTreeSet::new();
            for member in &set.members {
                validate_lexicon_identifier(&member.id, "semantic sibling member")?;
                if !member_ids.insert(member.id.as_str()) {
                    anyhow::bail!(
                        "Duplicate lexicon semantic sibling member ID {:?} in comparison set {:?}",
                        member.id,
                        set.id
                    );
                }
                if member.paths.is_empty() {
                    anyhow::bail!(
                        "Lexicon semantic sibling member {:?} in comparison set {:?} needs at least one path",
                        member.id,
                        set.id
                    );
                }
                for path in &member.paths {
                    validate_relative_path(path, &set.id, &member.id)?;
                }
            }
        }
        Ok(())
    }

    pub(crate) fn resolve(
        &self,
        repository_root: &Path,
    ) -> Result<Vec<ResolvedSemanticSiblingComparisonSet>> {
        self.validate_structure()?;
        let repository_root = repository_root.canonicalize().with_context(|| {
            format!(
                "Could not resolve semantic sibling repository root {}",
                repository_root.display()
            )
        })?;
        let mut resolved = Vec::with_capacity(self.comparison_sets.len());
        for set in &self.comparison_sets {
            let mut members = Vec::with_capacity(set.members.len());
            let mut set_roots = Vec::<(&str, PathBuf)>::new();
            for member in &set.members {
                let mut paths = Vec::with_capacity(member.paths.len());
                for configured in &member.paths {
                    let candidate = repository_root.join(configured);
                    let absolute = candidate.canonicalize().with_context(|| {
                        format!(
                            "Lexicon semantic sibling path {:?} in {}/{} does not exist",
                            configured, set.id, member.id
                        )
                    })?;
                    if !absolute.starts_with(&repository_root) {
                        anyhow::bail!(
                            "Lexicon semantic sibling path {:?} in {}/{} resolves outside the repository",
                            configured,
                            set.id,
                            member.id
                        );
                    }
                    let metadata = std::fs::metadata(&absolute).with_context(|| {
                        format!(
                            "Could not inspect semantic sibling path {}",
                            absolute.display()
                        )
                    })?;
                    let kind = if metadata.is_file() {
                        SemanticSiblingPathKind::File
                    } else if metadata.is_dir() {
                        SemanticSiblingPathKind::Directory
                    } else {
                        anyhow::bail!(
                            "Lexicon semantic sibling path {:?} in {}/{} is not a file or directory",
                            configured,
                            set.id,
                            member.id
                        );
                    };
                    for (existing_member, existing) in &set_roots {
                        if paths_overlap(existing, &absolute) {
                            anyhow::bail!(
                                "Lexicon semantic sibling paths in comparison set {:?} overlap between members {:?} and {:?}: {} and {}",
                                set.id,
                                existing_member,
                                member.id,
                                existing.display(),
                                absolute.display()
                            );
                        }
                    }
                    let relative = absolute
                        .strip_prefix(&repository_root)
                        .expect("confined semantic sibling path")
                        .to_path_buf();
                    let relative = crate::paths::normalize_path(&relative);
                    set_roots.push((&member.id, absolute.clone()));
                    paths.push(ResolvedSemanticSiblingPath {
                        relative,
                        absolute,
                        kind,
                    });
                }
                paths.sort();
                members.push(ResolvedSemanticSiblingMember {
                    id: member.id.clone(),
                    paths,
                });
            }
            members.sort_by(|left, right| left.id.cmp(&right.id));
            resolved.push(ResolvedSemanticSiblingComparisonSet {
                id: set.id.clone(),
                purpose: set.purpose.clone(),
                members,
                maximum_nominations: set.maximum_nominations,
            });
        }
        resolved.sort_by(|left, right| left.id.cmp(&right.id));
        validate_cross_set_overlaps(&resolved)?;
        Ok(resolved)
    }
}

const fn default_maximum_nominations() -> u32 {
    MAXIMUM_SEMANTIC_SIBLING_NOMINATIONS
}

fn validate_purpose(value: &str, set_id: &str) -> Result<()> {
    if value.trim().is_empty()
        || value.trim() != value
        || value.len() > 500
        || value.chars().any(char::is_control)
    {
        anyhow::bail!(
            "Lexicon semantic sibling comparison set {set_id:?} purpose must be a canonical nonblank string of at most 500 bytes"
        );
    }
    Ok(())
}

fn validate_relative_path(value: &str, set_id: &str, member_id: &str) -> Result<()> {
    let path = Path::new(value);
    let invalid_text = value.is_empty()
        || value.trim() != value
        || value.starts_with('/')
        || value.ends_with('/')
        || value.contains(['\\', ':', '\0'])
        || value.contains("//")
        || value.chars().any(|character| {
            character.is_control() || matches!(character, '*' | '?' | '[' | ']' | '{' | '}')
        });
    let invalid_components = value.split('/').any(|part| matches!(part, "." | ".."))
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)));
    if invalid_text || invalid_components {
        anyhow::bail!(
            "Lexicon semantic sibling path {:?} in {set_id}/{member_id} must be an exact canonical repository-relative path without globs or traversal",
            value
        );
    }
    Ok(())
}

fn validate_cross_set_overlaps(sets: &[ResolvedSemanticSiblingComparisonSet]) -> Result<()> {
    for (index, left) in sets.iter().enumerate() {
        for right in &sets[index + 1..] {
            let overlaps = left.members.iter().any(|left_member| {
                right.members.iter().any(|right_member| {
                    left_member.paths.iter().any(|left_path| {
                        right_member.paths.iter().any(|right_path| {
                            paths_overlap(&left_path.absolute, &right_path.absolute)
                        })
                    })
                })
            });
            if !overlaps {
                continue;
            }
            let distinct_purposes = left
                .purpose
                .as_deref()
                .zip(right.purpose.as_deref())
                .is_some_and(|(left, right)| left != right);
            if !distinct_purposes {
                anyhow::bail!(
                    "Lexicon semantic sibling comparison sets {:?} and {:?} overlap without distinct explicit purposes",
                    left.id,
                    right.id
                );
            }
        }
    }
    Ok(())
}

fn paths_overlap(left: &Path, right: &Path) -> bool {
    left == right || left.starts_with(right) || right.starts_with(left)
}

#[cfg(test)]
mod tests {
    use super::{SemanticSiblingsConfig, MAXIMUM_SEMANTIC_SIBLING_NOMINATIONS};
    use serde_json::{json, Value};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    fn fixture_root() -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "codeatlas-semantic-sibling-config-{}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&root);
        for path in ["src/alpha", "src/beta", "src/gamma"] {
            std::fs::create_dir_all(root.join(path)).expect("semantic sibling fixture path");
        }
        root
    }

    fn parse(value: Value) -> SemanticSiblingsConfig {
        serde_json::from_value(value).expect("semantic sibling config")
    }

    fn comparison_set(id: &str, purpose: Option<&str>, members: [(&str, &str); 2]) -> Value {
        json!({
            "id": id,
            "purpose": purpose,
            "members": members.map(|(id, path)| json!({"id": id, "paths": [path]})),
            "maximum_nominations": 50
        })
    }

    #[test]
    fn configured_sets_resolve_once_into_canonical_deterministic_members() {
        let root = fixture_root();
        let first = parse(json!({"comparison_sets": [comparison_set(
            "adapters",
            None,
            [("beta", "src/beta"), ("alpha", "src/alpha")]
        )]}));
        let second = parse(json!({"comparison_sets": [comparison_set(
            "adapters",
            None,
            [("alpha", "src/alpha"), ("beta", "src/beta")]
        )]}));
        let first = first.resolve(&root).expect("first resolved config");
        let second = second.resolve(&root).expect("second resolved config");
        assert_eq!(first, second);
        assert_eq!(first[0].maximum_nominations, 50);
        assert_eq!(first[0].members[0].id, "alpha");
        assert_eq!(first[0].members[0].paths[0].relative, "src/alpha");
        std::fs::remove_dir_all(root).expect("remove semantic sibling fixture");
    }

    #[test]
    fn structure_rejects_ambiguous_ids_members_paths_and_bounds() {
        let valid = comparison_set(
            "adapters",
            None,
            [("alpha", "src/alpha"), ("beta", "src/beta")],
        );
        let mut cases = Vec::new();
        cases.push(json!({"comparison_sets": [valid.clone(), valid.clone()]}));
        let mut invalid_id = valid.clone();
        invalid_id["id"] = json!("Not Canonical");
        cases.push(json!({"comparison_sets": [invalid_id]}));
        let mut one_member = valid.clone();
        one_member["members"] = json!([{"id":"alpha", "paths":["src/alpha"]}]);
        cases.push(json!({"comparison_sets": [one_member]}));
        let mut duplicate_member = valid.clone();
        duplicate_member["members"][1]["id"] = json!("alpha");
        cases.push(json!({"comparison_sets": [duplicate_member]}));
        for maximum in [0, MAXIMUM_SEMANTIC_SIBLING_NOMINATIONS + 1] {
            let mut invalid_limit = valid.clone();
            invalid_limit["maximum_nominations"] = json!(maximum);
            cases.push(json!({"comparison_sets": [invalid_limit]}));
        }
        for invalid_path in [
            "",
            ".",
            "../outside",
            "/absolute",
            "src/*",
            "src\\alpha",
            "src/./alpha",
            "src/../alpha",
        ] {
            let mut invalid = valid.clone();
            invalid["members"][0]["paths"] = json!([invalid_path]);
            cases.push(json!({"comparison_sets": [invalid]}));
        }
        for case in cases {
            assert!(
                parse(case).validate_structure().is_err(),
                "accepted invalid semantic sibling config"
            );
        }

        assert!(serde_json::from_value::<SemanticSiblingsConfig>(json!({
            "comparison_sets": [],
            "unknown": true
        }))
        .is_err());
    }

    #[cfg(unix)]
    #[test]
    fn resolution_rejects_overlap_missing_paths_and_symlink_escape() {
        use std::os::unix::fs::symlink;

        let root = fixture_root();
        let outside = root.parent().expect("fixture parent").join(format!(
            "{}-outside",
            root.file_name().unwrap().to_string_lossy()
        ));
        let _ = std::fs::remove_dir_all(&outside);
        std::fs::create_dir_all(&outside).expect("outside fixture");
        symlink(&outside, root.join("escaped")).expect("escape symlink");

        for set in [
            comparison_set(
                "missing",
                None,
                [("alpha", "src/alpha"), ("missing", "src/missing")],
            ),
            comparison_set("overlap", None, [("source", "src"), ("alpha", "src/alpha")]),
            comparison_set(
                "escape",
                None,
                [("alpha", "src/alpha"), ("escaped", "escaped")],
            ),
        ] {
            assert!(
                parse(json!({"comparison_sets": [set]}))
                    .resolve(&root)
                    .is_err(),
                "accepted unresolved or unconfined sibling path"
            );
        }

        let first = comparison_set(
            "primary",
            None,
            [("alpha", "src/alpha"), ("beta", "src/beta")],
        );
        let second = comparison_set(
            "secondary",
            None,
            [("alpha", "src/alpha"), ("gamma", "src/gamma")],
        );
        assert!(
            parse(json!({"comparison_sets": [first.clone(), second.clone()]}))
                .resolve(&root)
                .is_err()
        );
        let mut first_distinct = first;
        first_distinct["purpose"] = json!("compare language adapters");
        let mut second_distinct = second;
        second_distinct["purpose"] = json!("compare transport adapters");
        assert_eq!(
            parse(json!({"comparison_sets": [first_distinct, second_distinct]}))
                .resolve(&root)
                .expect("explicitly distinct overlapping sets")
                .len(),
            2
        );

        std::fs::remove_dir_all(root).expect("remove semantic sibling fixture");
        std::fs::remove_dir_all(outside).expect("remove outside fixture");
    }
}
