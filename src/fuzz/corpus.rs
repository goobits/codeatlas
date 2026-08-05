use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub(crate) const MAX_CORPUS_DEPTH: usize = 8;
pub(crate) const MAX_CORPUS_DIMENSIONS: usize = 64;
pub(crate) const MAX_POINTS_PER_DIMENSION: usize = 32;

#[derive(
    schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize,
)]
#[serde(deny_unknown_fields)]
pub(crate) struct CorpusDimension {
    pub path: String,
    pub points: Vec<BoundaryPoint>,
}

impl CorpusDimension {
    pub(crate) fn new(
        path: impl Into<String>,
        points: impl IntoIterator<Item = BoundaryPoint>,
    ) -> Result<Self> {
        let path = path.into();
        if path.is_empty() {
            anyhow::bail!("Corpus dimension path may not be empty");
        }
        let mut points = points.into_iter().collect::<Vec<_>>();
        points.sort();
        points.dedup();
        if points.is_empty() {
            anyhow::bail!("Corpus dimension {path:?} needs at least one boundary point");
        }
        if points.len() > MAX_POINTS_PER_DIMENSION {
            anyhow::bail!(
                "Corpus dimension {path:?} has {} points; the limit is {MAX_POINTS_PER_DIMENSION}",
                points.len()
            );
        }
        Ok(Self { path, points })
    }
}

#[derive(
    schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize,
)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum BoundaryPoint {
    Unit,
    Null,
    Boolean {
        value: bool,
    },
    Integer {
        point: IntegerBoundary,
    },
    Float {
        point: FloatBoundary,
    },
    Text {
        encoding: TextEncoding,
        point: TextBoundary,
    },
    Bytes {
        point: LengthBoundary,
    },
    Literal {
        canonical: String,
    },
    Presence {
        present: bool,
    },
    Collection {
        point: CollectionBoundary,
    },
    Variant {
        index: usize,
    },
}

#[derive(
    schemars::JsonSchema, Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub(crate) enum IntegerBoundary {
    Minimum,
    AboveMinimum,
    NegativeOne,
    Zero,
    One,
    BelowMaximum,
    Maximum,
}

#[derive(
    schemars::JsonSchema, Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FloatBoundary {
    NegativeInfinity,
    NegativeFiniteExtreme,
    NegativeOne,
    NegativeZero,
    PositiveZero,
    One,
    PositiveFiniteExtreme,
    PositiveInfinity,
    Nan,
}

#[derive(
    schemars::JsonSchema, Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LengthBoundary {
    Empty,
    One,
    BelowMinimum,
    Minimum,
    AboveMinimum,
    BelowMaximum,
    Maximum,
    AboveMaximum,
}

#[derive(
    schemars::JsonSchema, Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TextBoundary {
    Length(LengthBoundary),
    Ascii,
    Unicode,
    Combining,
    EncodingEdge,
}

#[derive(
    schemars::JsonSchema, Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TextEncoding {
    Utf8,
    Utf16,
    Unicode,
    Binary,
    Unknown,
}

#[derive(
    schemars::JsonSchema, Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CollectionBoundary {
    Empty,
    Singleton,
    DeclaredLimit,
    Duplicate,
    Sorted,
    Unsorted,
    Nested,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PairwiseSelection {
    pub cases: Vec<Vec<usize>>,
    pub complete: bool,
}

pub(crate) fn select_pairwise(
    dimensions: &[CorpusDimension],
    max_cases: usize,
) -> Result<PairwiseSelection> {
    if max_cases == 0 {
        anyhow::bail!("Pairwise case limit must be positive");
    }
    if dimensions.len() > MAX_CORPUS_DIMENSIONS {
        anyhow::bail!(
            "Corpus has {} dimensions; the limit is {MAX_CORPUS_DIMENSIONS}",
            dimensions.len()
        );
    }
    if dimensions
        .windows(2)
        .any(|pair| pair[0].path >= pair[1].path)
    {
        anyhow::bail!("Corpus dimensions must have unique paths in canonical order");
    }
    for dimension in dimensions {
        if dimension.points.is_empty() || dimension.points.len() > MAX_POINTS_PER_DIMENSION {
            anyhow::bail!(
                "Corpus dimension {:?} has an invalid point count",
                dimension.path
            );
        }
    }

    let mut cases = BTreeSet::new();
    let baseline = vec![0; dimensions.len()];
    if !insert_case(&mut cases, baseline.clone(), max_cases) {
        unreachable!("positive case limit accepts the baseline")
    }
    for (dimension, values) in dimensions.iter().enumerate() {
        for value in 1..values.points.len() {
            let mut candidate = baseline.clone();
            candidate[dimension] = value;
            if !insert_case(&mut cases, candidate, max_cases) {
                return Ok(selection(cases, false));
            }
        }
    }
    for left in 0..dimensions.len() {
        for right in left + 1..dimensions.len() {
            for left_value in 0..dimensions[left].points.len() {
                for right_value in 0..dimensions[right].points.len() {
                    let mut candidate = baseline.clone();
                    candidate[left] = left_value;
                    candidate[right] = right_value;
                    if !insert_case(&mut cases, candidate, max_cases) {
                        return Ok(selection(cases, false));
                    }
                }
            }
        }
    }
    Ok(selection(cases, true))
}

fn insert_case(cases: &mut BTreeSet<Vec<usize>>, candidate: Vec<usize>, limit: usize) -> bool {
    if cases.contains(&candidate) {
        return true;
    }
    if cases.len() == limit {
        return false;
    }
    cases.insert(candidate);
    true
}

fn selection(cases: BTreeSet<Vec<usize>>, complete: bool) -> PairwiseSelection {
    PairwiseSelection {
        cases: cases.into_iter().collect(),
        complete,
    }
}

#[cfg(test)]
mod tests {
    use super::{select_pairwise, BoundaryPoint, CorpusDimension};

    fn booleans(path: &str) -> CorpusDimension {
        CorpusDimension::new(
            path,
            [
                BoundaryPoint::Boolean { value: true },
                BoundaryPoint::Boolean { value: false },
            ],
        )
        .expect("boolean dimension")
    }

    #[test]
    fn dimensions_and_pairwise_cases_are_canonical_and_bounded() {
        let dimensions = [booleans("a"), booleans("b"), booleans("c")];
        assert_eq!(
            dimensions[0].points,
            [
                BoundaryPoint::Boolean { value: false },
                BoundaryPoint::Boolean { value: true },
            ]
        );

        let selection = select_pairwise(&dimensions, 32).expect("pairwise selection");
        assert!(selection.complete);
        assert_eq!(selection.cases.len(), 7);
        for left in 0..dimensions.len() {
            for right in left + 1..dimensions.len() {
                for left_value in 0..2 {
                    for right_value in 0..2 {
                        assert!(selection.cases.iter().any(|case| {
                            case[left] == left_value && case[right] == right_value
                        }));
                    }
                }
            }
        }

        let bounded = select_pairwise(&dimensions, 3).expect("bounded selection");
        assert_eq!(bounded.cases.len(), 3);
        assert!(!bounded.complete);

        let error = select_pairwise(&[booleans("b"), booleans("a")], 4)
            .expect_err("unordered dimensions must fail");
        assert!(error.to_string().contains("canonical order"));
    }
}
