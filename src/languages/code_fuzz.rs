use crate::config::ResolvedAnalysisProject;
use crate::domain::{
    CallableBody, CallableKind, CallableSignature, ParameterRequirement, ParameterRole,
    ReceiverRequirement,
};
use crate::fuzz::code::{
    CodeFuzzContract, CodeFuzzInputValue, CodeFuzzSignatureCorpus, CodeHarnessInput,
};
use crate::fuzz::FuzzLimits;
use anyhow::Result;

pub(crate) struct CodeFuzzHarnessRequest<'a> {
    pub target_id: &'a str,
    pub project: &'a ResolvedAnalysisProject,
    pub contract: &'a CodeFuzzContract,
    pub signature: &'a CodeFuzzSignatureCorpus,
    pub seed: &'a str,
    pub limits: &'a FuzzLimits,
    pub image: Option<&'a str>,
    pub replay_input: Option<&'a [CodeFuzzInputValue]>,
}

pub(crate) struct GeneratedCodeFuzzHarness {
    pub engine: crate::external_tool::ExternalToolFingerprint,
    pub adapter_version: &'static str,
    pub input: CodeHarnessInput,
}

pub(crate) enum CodeFuzzHarnessCapability {
    Available(Box<GeneratedCodeFuzzHarness>),
    Unsupported { reason: String },
}

pub(crate) fn generate_code_fuzz_harness(
    request: &CodeFuzzHarnessRequest<'_>,
) -> Result<CodeFuzzHarnessCapability> {
    match request.contract.language {
        crate::domain::source_graph::SourceLanguage::Python => {
            super::python::fuzz::generate_harness(request)
        }
        crate::domain::source_graph::SourceLanguage::Rust => {
            super::rust::fuzz::generate_harness(request)
        }
        crate::domain::source_graph::SourceLanguage::JavaScript => {
            unsupported("javascript_adapter_pending")
        }
        crate::domain::source_graph::SourceLanguage::TypeScript => {
            unsupported("typescript_adapter_pending")
        }
        crate::domain::source_graph::SourceLanguage::Svelte => {
            unsupported("svelte_is_not_a_callable_fuzz_language")
        }
    }
}

pub(in crate::languages) fn requires_concrete_free_function(signature: &CallableSignature) -> bool {
    signature.kind == CallableKind::Function
        && signature.body == CallableBody::Present
        && signature.receiver.requirement == ReceiverRequirement::None
        && signature.type_parameters.is_empty()
        && signature.parameters.iter().all(|parameter| {
            parameter.requirement == ParameterRequirement::Required
                && matches!(
                    parameter.role,
                    ParameterRole::Positional
                        | ParameterRole::PositionalOnly
                        | ParameterRole::PositionalOrNamed
                )
        })
}

pub(in crate::languages) fn has_one_dimension_per_parameter(
    request: &CodeFuzzHarnessRequest<'_>,
    signature: &CallableSignature,
) -> bool {
    let expected = signature
        .parameters
        .iter()
        .map(|parameter| format!("parameter:{}", parameter.position))
        .collect::<std::collections::BTreeSet<_>>();
    let actual = request
        .signature
        .dimensions
        .iter()
        .map(|dimension| dimension.path.clone())
        .collect::<std::collections::BTreeSet<_>>();
    expected == actual
}

pub(in crate::languages) fn project_mount(project: &ResolvedAnalysisProject) -> Result<String> {
    if project.report_root == "." {
        return Ok("/codeatlas/workspace".to_string());
    }
    if project.report_root.is_empty()
        || project.report_root.starts_with('/')
        || project
            .report_root
            .split('/')
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
    {
        anyhow::bail!("Code fuzz project root is not a safe workspace-relative path");
    }
    Ok(format!("/codeatlas/workspace/{}", project.report_root))
}

fn unsupported(reason: &str) -> Result<CodeFuzzHarnessCapability> {
    Ok(CodeFuzzHarnessCapability::Unsupported {
        reason: reason.to_string(),
    })
}
