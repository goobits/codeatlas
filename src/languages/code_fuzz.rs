use crate::config::ResolvedAnalysisProject;
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
        crate::domain::source_graph::SourceLanguage::Rust => unsupported("rust_adapter_pending"),
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

fn unsupported(reason: &str) -> Result<CodeFuzzHarnessCapability> {
    Ok(CodeFuzzHarnessCapability::Unsupported {
        reason: reason.to_string(),
    })
}
