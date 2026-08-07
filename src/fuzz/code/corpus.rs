use crate::fuzz::corpus::{
    BoundaryPoint, CollectionBoundary, CorpusDimension, FloatBoundary, IntegerBoundary,
    LengthBoundary, TextBoundary, TextEncoding, MAX_CORPUS_DEPTH, MAX_CORPUS_DIMENSIONS,
};
use codeatlas_domain::{
    CallableSignature, ParameterRequirement, SemanticLiteral, SemanticType, StringEncoding,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(
    schemars::JsonSchema, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize,
)]
#[serde(deny_unknown_fields)]
pub(crate) struct CorpusMappingIssue {
    pub path: String,
    pub kind: CorpusMappingIssueKind,
}

#[derive(
    schemars::JsonSchema, Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CorpusMappingIssueKind {
    DepthLimit,
    DimensionLimit,
    UnknownType,
    UnsupportedType,
    TypeParameter,
}

pub(crate) fn map_signature(
    signature: &CallableSignature,
) -> (Vec<CorpusDimension>, Vec<CorpusMappingIssue>) {
    let mut dimensions = BTreeMap::<String, BTreeSet<BoundaryPoint>>::new();
    let mut issues = Vec::new();
    for parameter in &signature.parameters {
        let path = format!("parameter:{}", parameter.position);
        if parameter.requirement != ParameterRequirement::Required {
            add_points(
                &mut dimensions,
                format!("{path}:argument"),
                [
                    BoundaryPoint::Presence { present: false },
                    BoundaryPoint::Presence { present: true },
                ],
            );
        }
        map_type(
            &parameter.semantic_type,
            &path,
            0,
            &mut dimensions,
            &mut issues,
        );
    }
    if dimensions.len() > MAX_CORPUS_DIMENSIONS {
        issues.push(CorpusMappingIssue {
            path: "callable".to_string(),
            kind: CorpusMappingIssueKind::DimensionLimit,
        });
        dimensions.clear();
    }
    let dimensions = dimensions
        .into_iter()
        .map(|(path, points)| {
            CorpusDimension::new(path, points).expect("bounded mapping creates valid dimensions")
        })
        .collect();
    issues.sort();
    issues.dedup();
    (dimensions, issues)
}

fn map_type(
    semantic_type: &SemanticType,
    path: &str,
    depth: usize,
    dimensions: &mut BTreeMap<String, BTreeSet<BoundaryPoint>>,
    issues: &mut Vec<CorpusMappingIssue>,
) {
    if depth > MAX_CORPUS_DEPTH {
        issues.push(CorpusMappingIssue {
            path: path.to_string(),
            kind: CorpusMappingIssueKind::DepthLimit,
        });
        return;
    }
    match semantic_type {
        SemanticType::Unknown { .. } => issues.push(CorpusMappingIssue {
            path: path.to_string(),
            kind: CorpusMappingIssueKind::UnknownType,
        }),
        SemanticType::Unit => add_points(dimensions, path, [BoundaryPoint::Unit]),
        SemanticType::Boolean => add_points(
            dimensions,
            path,
            [
                BoundaryPoint::Boolean { value: false },
                BoundaryPoint::Boolean { value: true },
            ],
        ),
        SemanticType::Integer { signed, bits } => {
            let mut points = vec![
                BoundaryPoint::Integer {
                    point: IntegerBoundary::Zero,
                },
                BoundaryPoint::Integer {
                    point: IntegerBoundary::One,
                },
            ];
            if signed != &Some(false) {
                points.push(BoundaryPoint::Integer {
                    point: IntegerBoundary::NegativeOne,
                });
            }
            if bits.is_some() {
                points.extend([
                    BoundaryPoint::Integer {
                        point: IntegerBoundary::Minimum,
                    },
                    BoundaryPoint::Integer {
                        point: IntegerBoundary::AboveMinimum,
                    },
                    BoundaryPoint::Integer {
                        point: IntegerBoundary::BelowMaximum,
                    },
                    BoundaryPoint::Integer {
                        point: IntegerBoundary::Maximum,
                    },
                ]);
            }
            add_points(dimensions, path, points);
        }
        SemanticType::Float { allows_special, .. } => {
            let mut points = vec![
                BoundaryPoint::Float {
                    point: FloatBoundary::NegativeFiniteExtreme,
                },
                BoundaryPoint::Float {
                    point: FloatBoundary::NegativeOne,
                },
                BoundaryPoint::Float {
                    point: FloatBoundary::NegativeZero,
                },
                BoundaryPoint::Float {
                    point: FloatBoundary::PositiveZero,
                },
                BoundaryPoint::Float {
                    point: FloatBoundary::One,
                },
                BoundaryPoint::Float {
                    point: FloatBoundary::PositiveFiniteExtreme,
                },
            ];
            if *allows_special {
                points.extend([
                    BoundaryPoint::Float {
                        point: FloatBoundary::NegativeInfinity,
                    },
                    BoundaryPoint::Float {
                        point: FloatBoundary::PositiveInfinity,
                    },
                    BoundaryPoint::Float {
                        point: FloatBoundary::Nan,
                    },
                ]);
            }
            add_points(dimensions, path, points);
        }
        SemanticType::String {
            encoding,
            max_length,
        } => {
            let encoding = text_encoding(*encoding);
            let mut points = vec![
                BoundaryPoint::Text {
                    encoding,
                    point: TextBoundary::Length(LengthBoundary::Empty),
                },
                BoundaryPoint::Text {
                    encoding,
                    point: TextBoundary::Length(LengthBoundary::One),
                },
                BoundaryPoint::Text {
                    encoding,
                    point: TextBoundary::Ascii,
                },
                BoundaryPoint::Text {
                    encoding,
                    point: TextBoundary::Unicode,
                },
                BoundaryPoint::Text {
                    encoding,
                    point: TextBoundary::Combining,
                },
            ];
            if max_length.is_some() {
                points.extend([
                    BoundaryPoint::Text {
                        encoding,
                        point: TextBoundary::Length(LengthBoundary::BelowMaximum),
                    },
                    BoundaryPoint::Text {
                        encoding,
                        point: TextBoundary::Length(LengthBoundary::Maximum),
                    },
                    BoundaryPoint::Text {
                        encoding,
                        point: TextBoundary::Length(LengthBoundary::AboveMaximum),
                    },
                ]);
            }
            if matches!(encoding, TextEncoding::Utf16 | TextEncoding::Unknown) {
                points.push(BoundaryPoint::Text {
                    encoding,
                    point: TextBoundary::EncodingEdge,
                });
            }
            add_points(dimensions, path, points);
        }
        SemanticType::Bytes { max_length } => {
            let mut points = vec![
                BoundaryPoint::Bytes {
                    point: LengthBoundary::Empty,
                },
                BoundaryPoint::Bytes {
                    point: LengthBoundary::One,
                },
            ];
            if max_length.is_some() {
                points.extend([
                    BoundaryPoint::Bytes {
                        point: LengthBoundary::BelowMaximum,
                    },
                    BoundaryPoint::Bytes {
                        point: LengthBoundary::Maximum,
                    },
                    BoundaryPoint::Bytes {
                        point: LengthBoundary::AboveMaximum,
                    },
                ]);
            }
            add_points(dimensions, path, points);
        }
        SemanticType::Null => add_points(dimensions, path, [BoundaryPoint::Null]),
        SemanticType::Literal { value } => add_points(
            dimensions,
            path,
            [BoundaryPoint::Literal {
                canonical: literal(value),
            }],
        ),
        SemanticType::Optional { value } => {
            add_points(
                dimensions,
                format!("{path}:presence"),
                [
                    BoundaryPoint::Presence { present: false },
                    BoundaryPoint::Presence { present: true },
                ],
            );
            map_type(
                value,
                &format!("{path}:value"),
                depth + 1,
                dimensions,
                issues,
            );
        }
        SemanticType::Union { variants } => {
            add_points(
                dimensions,
                format!("{path}:variant"),
                (0..variants.len()).map(|index| BoundaryPoint::Variant { index }),
            );
            for (index, variant) in variants.iter().enumerate() {
                map_type(
                    variant,
                    &format!("{path}:variant:{index}"),
                    depth + 1,
                    dimensions,
                    issues,
                );
            }
        }
        SemanticType::List {
            value, max_items, ..
        }
        | SemanticType::Set { value, max_items } => {
            map_collection(path, max_items.is_some(), dimensions);
            map_type(
                value,
                &format!("{path}:element"),
                depth + 1,
                dimensions,
                issues,
            );
        }
        SemanticType::Tuple { values } => {
            for (index, value) in values.iter().enumerate() {
                map_type(
                    value,
                    &format!("{path}:item:{index}"),
                    depth + 1,
                    dimensions,
                    issues,
                );
            }
        }
        SemanticType::Map {
            key,
            value,
            max_items,
        } => {
            map_collection(path, max_items.is_some(), dimensions);
            map_type(key, &format!("{path}:key"), depth + 1, dimensions, issues);
            map_type(
                value,
                &format!("{path}:value"),
                depth + 1,
                dimensions,
                issues,
            );
        }
        SemanticType::Record { fields } => {
            for field in fields {
                if !field.required {
                    add_points(
                        dimensions,
                        format!("{path}:field:{}:presence", field.name),
                        [
                            BoundaryPoint::Presence { present: false },
                            BoundaryPoint::Presence { present: true },
                        ],
                    );
                }
                map_type(
                    &field.semantic_type,
                    &format!("{path}:field:{}", field.name),
                    depth + 1,
                    dimensions,
                    issues,
                );
            }
        }
        SemanticType::Result { .. } | SemanticType::Named { .. } => {
            issues.push(CorpusMappingIssue {
                path: path.to_string(),
                kind: CorpusMappingIssueKind::UnsupportedType,
            });
        }
        SemanticType::TypeParameter { .. } => issues.push(CorpusMappingIssue {
            path: path.to_string(),
            kind: CorpusMappingIssueKind::TypeParameter,
        }),
    }
}

fn add_points(
    dimensions: &mut BTreeMap<String, BTreeSet<BoundaryPoint>>,
    path: impl Into<String>,
    points: impl IntoIterator<Item = BoundaryPoint>,
) {
    dimensions.entry(path.into()).or_default().extend(points);
}

fn map_collection(
    path: &str,
    has_limit: bool,
    dimensions: &mut BTreeMap<String, BTreeSet<BoundaryPoint>>,
) {
    let mut points = vec![
        BoundaryPoint::Collection {
            point: CollectionBoundary::Empty,
        },
        BoundaryPoint::Collection {
            point: CollectionBoundary::Singleton,
        },
        BoundaryPoint::Collection {
            point: CollectionBoundary::Duplicate,
        },
        BoundaryPoint::Collection {
            point: CollectionBoundary::Sorted,
        },
        BoundaryPoint::Collection {
            point: CollectionBoundary::Unsorted,
        },
        BoundaryPoint::Collection {
            point: CollectionBoundary::Nested,
        },
    ];
    if has_limit {
        points.push(BoundaryPoint::Collection {
            point: CollectionBoundary::DeclaredLimit,
        });
    }
    add_points(dimensions, format!("{path}:shape"), points);
}

fn text_encoding(encoding: StringEncoding) -> TextEncoding {
    match encoding {
        StringEncoding::Utf8 => TextEncoding::Utf8,
        StringEncoding::Utf16 => TextEncoding::Utf16,
        StringEncoding::Unicode => TextEncoding::Unicode,
        StringEncoding::Unknown => TextEncoding::Unknown,
    }
}

fn literal(value: &SemanticLiteral) -> String {
    serde_json::to_string(value).expect("semantic literal always serializes")
}
