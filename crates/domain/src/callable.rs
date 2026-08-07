use super::{EvidenceClass, Span};
use serde::{Deserialize, Serialize};

#[derive(
    schemars::JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord,
)]
#[serde(deny_unknown_fields)]
pub struct CallableContract {
    pub signatures: Vec<CallableSignature>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<CallableEffect>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub block_reasons: Vec<CallableBlockReason>,
}

impl CallableContract {
    pub fn new(
        signatures: impl IntoIterator<Item = CallableSignature>,
        effects: impl IntoIterator<Item = CallableEffect>,
    ) -> Self {
        let mut contract = Self {
            signatures: signatures.into_iter().collect(),
            effects: effects.into_iter().collect(),
            block_reasons: Vec::new(),
        };
        contract.normalize();
        contract
    }

    pub fn merge(&mut self, other: Self) {
        self.signatures.extend(other.signatures);
        self.effects.extend(other.effects);
        self.normalize();
    }

    pub fn replace_effects(&mut self, effects: impl IntoIterator<Item = CallableEffect>) {
        self.effects = effects.into_iter().collect();
        self.normalize();
    }

    fn normalize(&mut self) {
        self.signatures.sort();
        self.signatures.dedup();
        self.effects.sort();
        self.effects.dedup();
        self.block_reasons = collect_block_reasons(&self.signatures, &self.effects);
    }
}

#[derive(
    schemars::JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord,
)]
#[serde(deny_unknown_fields)]
pub struct CallableSignature {
    pub kind: CallableKind,
    pub body: CallableBody,
    pub is_async: bool,
    pub receiver: ReceiverContract,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub type_parameters: Vec<TypeParameterContract>,
    pub parameters: Vec<CallableParameter>,
    pub result: SemanticType,
}

#[derive(
    schemars::JsonSchema, Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord,
)]
#[serde(rename_all = "snake_case")]
pub enum CallableKind {
    Function,
    Method,
    Constructor,
    Getter,
    Setter,
}

#[derive(
    schemars::JsonSchema, Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord,
)]
#[serde(rename_all = "snake_case")]
pub enum CallableBody {
    Present,
    DeclarationOnly,
}

#[derive(
    schemars::JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord,
)]
#[serde(deny_unknown_fields)]
pub struct ReceiverContract {
    pub requirement: ReceiverRequirement,
    pub constructibility: Constructibility,
}

impl ReceiverContract {
    pub const fn none() -> Self {
        Self {
            requirement: ReceiverRequirement::None,
            constructibility: Constructibility::Direct,
        }
    }
}

#[derive(
    schemars::JsonSchema, Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord,
)]
#[serde(rename_all = "snake_case")]
pub enum ReceiverRequirement {
    None,
    Instance,
    MutableInstance,
    Type,
    Unknown,
}

#[derive(
    schemars::JsonSchema, Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord,
)]
#[serde(rename_all = "snake_case")]
pub enum Constructibility {
    Direct,
    RequiresFactory,
    Unsupported,
    Unknown,
}

#[derive(
    schemars::JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord,
)]
#[serde(deny_unknown_fields)]
pub struct TypeParameterContract {
    pub name: String,
    pub kind: TypeParameterKind,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub constraints: Vec<SemanticType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<SemanticType>,
}

#[derive(
    schemars::JsonSchema, Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord,
)]
#[serde(rename_all = "snake_case")]
pub enum TypeParameterKind {
    Type,
    Const,
    Lifetime,
    ParameterSpec,
    Variadic,
}

#[derive(
    schemars::JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord,
)]
#[serde(deny_unknown_fields)]
pub struct CallableParameter {
    pub position: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub role: ParameterRole,
    pub requirement: ParameterRequirement,
    pub semantic_type: SemanticType,
    pub constructibility: Constructibility,
}

#[derive(
    schemars::JsonSchema, Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord,
)]
#[serde(rename_all = "snake_case")]
pub enum ParameterRole {
    Positional,
    PositionalOnly,
    PositionalOrNamed,
    NamedOnly,
    VariadicPositional,
    VariadicNamed,
}

#[derive(
    schemars::JsonSchema, Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord,
)]
#[serde(rename_all = "snake_case")]
pub enum ParameterRequirement {
    Required,
    Optional,
    Defaulted,
}

#[derive(
    schemars::JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord,
)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SemanticType {
    Unknown {
        reason: TypeUnknownReason,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        display: Option<String>,
    },
    Unit,
    Boolean,
    Integer {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        signed: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bits: Option<u16>,
    },
    Float {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bits: Option<u16>,
        allows_special: bool,
    },
    String {
        encoding: StringEncoding,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_length: Option<u64>,
    },
    Bytes {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_length: Option<u64>,
    },
    Null,
    Literal {
        value: SemanticLiteral,
    },
    Optional {
        value: Box<SemanticType>,
    },
    Union {
        variants: Vec<SemanticType>,
    },
    List {
        value: Box<SemanticType>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        min_items: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_items: Option<u64>,
    },
    Tuple {
        values: Vec<SemanticType>,
    },
    Set {
        value: Box<SemanticType>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_items: Option<u64>,
    },
    Map {
        key: Box<SemanticType>,
        value: Box<SemanticType>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_items: Option<u64>,
    },
    Record {
        fields: Vec<SemanticField>,
    },
    Result {
        ok: Box<SemanticType>,
        error: Box<SemanticType>,
    },
    Named {
        identity: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        arguments: Vec<SemanticType>,
    },
    TypeParameter {
        name: String,
    },
}

impl SemanticType {
    pub fn unknown(reason: TypeUnknownReason, display: impl Into<String>) -> Self {
        let display = display.into();
        Self::Unknown {
            reason,
            display: (!display.is_empty()).then_some(display),
        }
    }

    pub fn union(variants: impl IntoIterator<Item = Self>) -> Self {
        let mut variants = variants.into_iter().collect::<Vec<_>>();
        variants.sort();
        variants.dedup();
        if variants.len() == 1 {
            variants.pop().expect("one union variant")
        } else {
            Self::Union { variants }
        }
    }

    pub fn constructibility(&self) -> Constructibility {
        match self {
            Self::Unknown { .. } | Self::TypeParameter { .. } => Constructibility::Unknown,
            Self::Named { .. } => Constructibility::RequiresFactory,
            Self::Result { .. } => Constructibility::Unsupported,
            Self::Union { variants } => combine_constructibility(variants),
            Self::Optional { value } | Self::List { value, .. } | Self::Set { value, .. } => {
                value.constructibility()
            }
            Self::Tuple { values } => combine_constructibility(values),
            Self::Map { key, value, .. } => {
                combine_constructibility([key.as_ref(), value.as_ref()])
            }
            Self::Record { fields } => {
                combine_constructibility(fields.iter().map(|field| &field.semantic_type))
            }
            Self::Unit
            | Self::Boolean
            | Self::Integer { .. }
            | Self::Float { .. }
            | Self::String { .. }
            | Self::Bytes { .. }
            | Self::Null
            | Self::Literal { .. } => Constructibility::Direct,
        }
    }
}

fn combine_constructibility<'a>(
    values: impl IntoIterator<Item = &'a SemanticType>,
) -> Constructibility {
    values
        .into_iter()
        .map(SemanticType::constructibility)
        .max()
        .unwrap_or(Constructibility::Direct)
}

#[derive(
    schemars::JsonSchema, Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord,
)]
#[serde(rename_all = "snake_case")]
pub enum TypeUnknownReason {
    MissingAnnotation,
    Unresolved,
    Unsupported,
    UnboundedRecursive,
    UnsupportedPattern,
}

#[derive(
    schemars::JsonSchema, Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord,
)]
#[serde(rename_all = "snake_case")]
pub enum StringEncoding {
    Utf8,
    Utf16,
    Unicode,
    Unknown,
}

#[derive(
    schemars::JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord,
)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum SemanticLiteral {
    Boolean(bool),
    Integer(String),
    Float(String),
    String(String),
    Null,
}

#[derive(
    schemars::JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord,
)]
#[serde(deny_unknown_fields)]
pub struct SemanticField {
    pub name: String,
    pub required: bool,
    pub semantic_type: SemanticType,
}

#[derive(
    schemars::JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord,
)]
#[serde(deny_unknown_fields)]
pub struct CallableEffect {
    pub kind: EffectKind,
    pub provenance: EffectProvenance,
    pub evidence: EvidenceClass,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span: Option<Span>,
}

impl CallableEffect {
    pub fn new_direct(kind: EffectKind, evidence: EvidenceClass, span: Option<Span>) -> Self {
        Self {
            kind,
            provenance: EffectProvenance::Direct,
            evidence,
            span,
        }
    }
}

#[derive(
    schemars::JsonSchema, Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord,
)]
#[serde(rename_all = "snake_case")]
pub enum EffectKind {
    FilesystemRead,
    FilesystemWrite,
    Network,
    Database,
    Process,
    Environment,
    Time,
    Randomness,
    AmbientState,
    Unknown,
}

#[derive(
    schemars::JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord,
)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EffectProvenance {
    Direct,
    Propagated { source_target: String },
}

#[derive(
    schemars::JsonSchema, Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord,
)]
#[serde(deny_unknown_fields)]
pub struct CallableBlockReason {
    pub kind: CallableBlockKind,
    pub subject: String,
}

#[derive(
    schemars::JsonSchema, Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord,
)]
#[serde(rename_all = "snake_case")]
pub enum CallableBlockKind {
    DeclarationOnly,
    MissingType,
    UnresolvedType,
    UnsupportedType,
    UnboundedType,
    UnsupportedPattern,
    UnknownReceiver,
    RequiresFactory,
    UnsupportedConstruction,
    UnknownEffectBoundary,
}

fn collect_block_reasons(
    signatures: &[CallableSignature],
    effects: &[CallableEffect],
) -> Vec<CallableBlockReason> {
    let mut reasons = Vec::new();
    if !signatures.is_empty()
        && signatures
            .iter()
            .all(|signature| signature.body == CallableBody::DeclarationOnly)
    {
        reasons.push(block_reason(CallableBlockKind::DeclarationOnly, "callable"));
    }
    for (signature_index, signature) in signatures.iter().enumerate() {
        let prefix = format!("signature:{signature_index}");
        collect_receiver_block_reasons(&mut reasons, &prefix, &signature.receiver);
        for parameter in &signature.type_parameters {
            let subject = format!("{prefix}:type_parameter:{}", parameter.name);
            for constraint in &parameter.constraints {
                collect_type_block_reasons(&mut reasons, &subject, constraint);
            }
            if let Some(default) = &parameter.default {
                collect_type_block_reasons(&mut reasons, &subject, default);
            }
        }
        for parameter in &signature.parameters {
            let subject = format!("{prefix}:parameter:{}", parameter.position);
            collect_type_block_reasons(&mut reasons, &subject, &parameter.semantic_type);
            if !matches!(&parameter.semantic_type, SemanticType::Unknown { .. }) {
                collect_constructibility_block_reasons(
                    &mut reasons,
                    &subject,
                    parameter.constructibility,
                );
            }
        }
        collect_type_block_reasons(&mut reasons, &format!("{prefix}:result"), &signature.result);
    }
    for effect in effects {
        if effect.kind == EffectKind::Unknown {
            reasons.push(block_reason(
                CallableBlockKind::UnknownEffectBoundary,
                "effects",
            ));
        }
    }
    reasons.sort();
    reasons.dedup();
    reasons
}

fn collect_receiver_block_reasons(
    reasons: &mut Vec<CallableBlockReason>,
    prefix: &str,
    receiver: &ReceiverContract,
) {
    if receiver.requirement == ReceiverRequirement::Unknown {
        reasons.push(block_reason(
            CallableBlockKind::UnknownReceiver,
            &format!("{prefix}:receiver"),
        ));
    }
    collect_constructibility_block_reasons(
        reasons,
        &format!("{prefix}:receiver"),
        receiver.constructibility,
    );
}

fn collect_constructibility_block_reasons(
    reasons: &mut Vec<CallableBlockReason>,
    subject: &str,
    constructibility: Constructibility,
) {
    let kind = match constructibility {
        Constructibility::Direct => return,
        Constructibility::RequiresFactory => CallableBlockKind::RequiresFactory,
        Constructibility::Unsupported => CallableBlockKind::UnsupportedConstruction,
        Constructibility::Unknown => CallableBlockKind::UnresolvedType,
    };
    reasons.push(block_reason(kind, subject));
}

fn collect_type_block_reasons(
    reasons: &mut Vec<CallableBlockReason>,
    subject: &str,
    semantic_type: &SemanticType,
) {
    match semantic_type {
        SemanticType::Unknown { reason, .. } => {
            let kind = match reason {
                TypeUnknownReason::MissingAnnotation => CallableBlockKind::MissingType,
                TypeUnknownReason::Unresolved => CallableBlockKind::UnresolvedType,
                TypeUnknownReason::Unsupported => CallableBlockKind::UnsupportedType,
                TypeUnknownReason::UnboundedRecursive => CallableBlockKind::UnboundedType,
                TypeUnknownReason::UnsupportedPattern => CallableBlockKind::UnsupportedPattern,
            };
            reasons.push(block_reason(kind, subject));
        }
        SemanticType::Optional { value }
        | SemanticType::List { value, .. }
        | SemanticType::Set { value, .. } => {
            collect_type_block_reasons(reasons, subject, value);
        }
        SemanticType::Union { variants } | SemanticType::Tuple { values: variants } => {
            for value in variants {
                collect_type_block_reasons(reasons, subject, value);
            }
        }
        SemanticType::Map { key, value, .. } => {
            collect_type_block_reasons(reasons, subject, key);
            collect_type_block_reasons(reasons, subject, value);
        }
        SemanticType::Record { fields } => {
            for field in fields {
                collect_type_block_reasons(
                    reasons,
                    &format!("{subject}:field:{}", field.name),
                    &field.semantic_type,
                );
            }
        }
        SemanticType::Result { ok, error } => {
            collect_type_block_reasons(reasons, subject, ok);
            collect_type_block_reasons(reasons, subject, error);
        }
        SemanticType::Named { arguments, .. } => {
            for argument in arguments {
                collect_type_block_reasons(reasons, subject, argument);
            }
        }
        SemanticType::Unit
        | SemanticType::Boolean
        | SemanticType::Integer { .. }
        | SemanticType::Float { .. }
        | SemanticType::String { .. }
        | SemanticType::Bytes { .. }
        | SemanticType::Null
        | SemanticType::Literal { .. }
        | SemanticType::TypeParameter { .. } => {}
    }
}

fn block_reason(kind: CallableBlockKind, subject: &str) -> CallableBlockReason {
    CallableBlockReason {
        kind,
        subject: subject.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CallableBody, CallableContract, CallableKind, CallableParameter, CallableSignature,
        Constructibility, ParameterRequirement, ParameterRole, ReceiverContract, SemanticType,
        TypeUnknownReason,
    };

    #[test]
    fn contract_normalization_is_deterministic_and_derives_blocks() {
        let signature = CallableSignature {
            kind: CallableKind::Function,
            body: CallableBody::Present,
            is_async: false,
            receiver: ReceiverContract::none(),
            type_parameters: Vec::new(),
            parameters: vec![CallableParameter {
                position: 0,
                name: Some("value".to_string()),
                role: ParameterRole::Positional,
                requirement: ParameterRequirement::Required,
                semantic_type: SemanticType::unknown(TypeUnknownReason::MissingAnnotation, "value"),
                constructibility: Constructibility::Unknown,
            }],
            result: SemanticType::Unit,
        };
        let mut contract = CallableContract::new([signature.clone(), signature], []);
        let other = contract.clone();
        contract.merge(other);

        assert_eq!(contract.signatures.len(), 1);
        assert_eq!(contract.block_reasons.len(), 1);
    }
}
