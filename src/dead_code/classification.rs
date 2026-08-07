use super::model::{DeadCodeFinding, DeadCodeFindingKind, DeadCodeRootContext};
use codeatlas_domain::source_graph::{
    ContextRole, FindingConfidence, NodeId, SourceEvidence, SourceLanguage,
};
use codeatlas_domain::{EvidenceClass, SourceDisposition};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::Path;

pub(super) struct FindingDetails {
    pub project: String,
    pub node_id: Option<NodeId>,
    pub path: String,
    pub symbol: Option<String>,
    pub language: Option<SourceLanguage>,
    pub contexts: Vec<String>,
    pub root_contexts: Vec<DeadCodeRootContext>,
    pub roles: BTreeSet<ContextRole>,
    pub confidence: FindingConfidence,
    pub evidence: SourceEvidence,
    pub message: String,
    pub identity_detail: Option<String>,
}

pub(super) fn build_finding(kind: DeadCodeFindingKind, details: FindingDetails) -> DeadCodeFinding {
    let id = stable_finding_id(kind, &details);
    DeadCodeFinding {
        id,
        kind,
        project: details.project,
        node_id: details.node_id,
        path: details.path.clone(),
        symbol: details.symbol,
        language: details.language,
        contexts: details.contexts,
        root_contexts: details.root_contexts,
        roles: details.roles,
        confidence: details.confidence,
        evidence_class: evidence_class(kind, details.confidence),
        source_disposition: source_disposition(&details.path),
        evidence: details.evidence,
        message: details.message,
        gates: kind.gates_at(details.confidence),
    }
}

fn evidence_class(kind: DeadCodeFindingKind, confidence: FindingConfidence) -> EvidenceClass {
    if confidence != FindingConfidence::High
        || matches!(
            kind,
            DeadCodeFindingKind::UnreferencedPublic | DeadCodeFindingKind::DynamicBoundary
        )
    {
        return EvidenceClass::BoundaryLimited;
    }
    if matches!(
        kind,
        DeadCodeFindingKind::UnreachableFile
            | DeadCodeFindingKind::UnusedPrivateSymbol
            | DeadCodeFindingKind::TestOnly
            | DeadCodeFindingKind::ToolingOnly
    ) {
        EvidenceClass::Inferred
    } else {
        EvidenceClass::Direct
    }
}

fn source_disposition(path: &str) -> SourceDisposition {
    let parts = path
        .split(['/', '\\'])
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>();
    if parts.iter().any(|part| {
        matches!(
            part.as_str(),
            "fixture" | "fixtures" | "__fixtures__" | "testdata" | "test-data" | "snapshots"
        )
    }) {
        return SourceDisposition::Fixture;
    }
    if parts.iter().any(|part| {
        matches!(
            part.as_str(),
            "generated" | "__generated__" | "dist" | "build" | "coverage" | "target"
        )
    }) {
        return SourceDisposition::Generated;
    }
    if crate::source_policy::is_conventional_test_source(Path::new(path)) {
        return SourceDisposition::Test;
    }
    if parts.iter().any(|part| {
        matches!(
            part.as_str(),
            "tool" | "tools" | "script" | "scripts" | "xtask" | "benches"
        )
    }) {
        return SourceDisposition::Tooling;
    }
    SourceDisposition::Maintained
}

fn stable_finding_id(kind: DeadCodeFindingKind, details: &FindingDetails) -> String {
    let mut digest = Sha256::new();
    digest.update(b"atlas.codeatlas.dev/dead-code-finding/v1\0");
    for value in [
        kind.as_str(),
        details.project.as_str(),
        details
            .node_id
            .as_ref()
            .map(|node| node.0.as_str())
            .unwrap_or(""),
        details.path.as_str(),
        details.symbol.as_deref().unwrap_or(""),
        details.evidence.extractor.as_str(),
        details.identity_detail.as_deref().unwrap_or(""),
    ] {
        digest.update((value.len() as u64).to_le_bytes());
        digest.update(value.as_bytes());
    }
    match &details.evidence.span {
        Some(span) => {
            digest.update([1]);
            for position in [span.start_line, span.start_col, span.end_line, span.end_col] {
                digest.update(position.to_le_bytes());
            }
        }
        None => digest.update([0]),
    }
    format!("dead-code/{}/{:x}", kind.as_str(), digest.finalize())
}

#[cfg(test)]
mod tests {
    use super::source_disposition;
    use codeatlas_domain::SourceDisposition;

    #[test]
    fn source_disposition_uses_structural_path_segments() {
        assert_eq!(
            source_disposition("tests/fixtures/account.py"),
            SourceDisposition::Fixture
        );
        assert_eq!(
            source_disposition("src/__generated__/schema.ts"),
            SourceDisposition::Generated
        );
        assert_eq!(
            source_disposition("src/account.test.ts"),
            SourceDisposition::Test
        );
        assert_eq!(
            source_disposition("scripts/release.rs"),
            SourceDisposition::Tooling
        );
        assert_eq!(
            source_disposition("src/account.ts"),
            SourceDisposition::Maintained
        );
    }
}
