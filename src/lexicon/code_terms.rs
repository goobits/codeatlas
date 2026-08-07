use super::subject_terms::{
    RepositoryLexiconSubject, RepositoryTermCompleteness, RepositoryTermConfidence,
    RepositoryTermRole, RepositoryTermSource, RepositoryTermSourceKind, SubjectTermCollection,
    SubjectTermSeed, SubjectTermSeedKind,
};
use anyhow::Result;
use codeatlas_domain::{ScanReport, Symbol};

pub(crate) fn collect_code_terms(scan: &ScanReport) -> Result<SubjectTermCollection> {
    let completeness =
        RepositoryTermCompleteness::from_reasons((!scan.skipped_files.is_empty()).then(|| {
            format!(
                "{} source files were skipped during code term extraction.",
                scan.skipped_files.len()
            )
        }));
    let mut collection =
        SubjectTermCollection::new(RepositoryLexiconSubject::Code, completeness.clone());
    collect_code_symbols(&scan.symbols, &completeness, &mut collection)?;
    Ok(collection)
}

fn collect_code_symbols(
    symbols: &[Symbol],
    completeness: &RepositoryTermCompleteness,
    collection: &mut SubjectTermCollection,
) -> Result<()> {
    for symbol in symbols {
        let owner = symbol
            .package
            .clone()
            .unwrap_or_else(|| "repository-root".to_string());
        let declaration = RepositoryTermSource::new(
            RepositoryTermSourceKind::Declaration,
            Some(symbol.file_path.clone()),
        )
        .at(
            symbol.span.as_ref().map(|span| span.start_line),
            symbol.span.as_ref().map(|span| span.start_col),
        );
        collection.push(SubjectTermSeed {
            value: symbol.name.clone(),
            kind: SubjectTermSeedKind::Identifier,
            role: RepositoryTermRole::CodeSymbol,
            owner: owner.clone(),
            target: symbol.id.clone(),
            source: declaration.clone(),
            confidence: RepositoryTermConfidence::High,
            completeness: completeness.clone(),
        })?;
        if let Some(callable) = &symbol.callable {
            for name in callable
                .signatures
                .iter()
                .flat_map(|signature| &signature.parameters)
                .filter_map(|parameter| parameter.name.as_ref())
            {
                collection.push(SubjectTermSeed {
                    value: name.clone(),
                    kind: SubjectTermSeedKind::Identifier,
                    role: RepositoryTermRole::CodeCallableParameter,
                    owner: owner.clone(),
                    target: symbol.id.clone(),
                    source: declaration.clone(),
                    confidence: RepositoryTermConfidence::High,
                    completeness: completeness.clone(),
                })?;
            }
        }
        if let Some(documentation) = &symbol.docs {
            for text in std::iter::once(&documentation.summary)
                .chain(documentation.remarks.iter())
                .chain(documentation.params.values())
                .chain(documentation.returns.iter())
                .chain(documentation.throws.iter())
            {
                if text.trim().is_empty() {
                    continue;
                }
                collection.push(SubjectTermSeed {
                    value: text.clone(),
                    kind: SubjectTermSeedKind::Text,
                    role: RepositoryTermRole::CodeDocumentation,
                    owner: owner.clone(),
                    target: symbol.id.clone(),
                    source: RepositoryTermSource::new(
                        RepositoryTermSourceKind::Documentation,
                        Some(symbol.file_path.clone()),
                    )
                    .at(
                        symbol.span.as_ref().map(|span| span.start_line),
                        symbol.span.as_ref().map(|span| span.start_col),
                    ),
                    confidence: RepositoryTermConfidence::High,
                    completeness: completeness.clone(),
                })?;
            }
        }
        collect_code_symbols(&symbol.children, completeness, collection)?;
    }
    Ok(())
}
