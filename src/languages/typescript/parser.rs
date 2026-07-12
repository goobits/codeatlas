mod format;
mod module_info;
mod visitor;

pub(crate) use module_info::{ExportInfo, ImportInfo, TypeScriptModuleInfo};

use anyhow::Result;
use module_info::{collect_exports, collect_imports};
use std::collections::HashMap;
use std::path::Path;
use swc_core::common::{
    errors::{ColorConfig, Handler},
    sync::Lrc,
    FileName, SourceFile, SourceMap,
};
use swc_core::ecma::ast::Module;
use swc_core::ecma::parser::{lexer::Lexer, Parser, StringInput, Syntax, TsConfig};
use swc_core::ecma::visit::VisitWith;
use visitor::SymbolVisitor;

pub(crate) fn parse_file(file_path: &Path, root_dir: &Path) -> Result<Vec<crate::domain::Symbol>> {
    Ok(parse_module_info(file_path, root_dir)?.symbols)
}

pub(crate) fn parse_module_info(file_path: &Path, root_dir: &Path) -> Result<TypeScriptModuleInfo> {
    let (module, source_map) = parse_module(file_path)?;
    let relative_path = pathdiff::diff_paths(file_path, root_dir)
        .unwrap_or(file_path.to_path_buf())
        .to_string_lossy()
        .to_string();
    Ok(build_module_info(module, source_map, relative_path))
}

pub(crate) fn parse_source(source: &str, relative_path: &str) -> Result<TypeScriptModuleInfo> {
    let source_map: Lrc<SourceMap> = Default::default();
    let file = source_map.new_source_file(
        FileName::Custom(relative_path.to_string()),
        source.to_string(),
    );
    let module = parse_source_file(file, source_map.clone(), false)?;
    Ok(build_module_info(
        module,
        source_map,
        relative_path.to_string(),
    ))
}

fn build_module_info(
    module: Module,
    source_map: Lrc<SourceMap>,
    relative_path: String,
) -> TypeScriptModuleInfo {
    let mut visitor = SymbolVisitor {
        symbols: Vec::new(),
        relative_path,
        source_map,
    };

    module.visit_with(&mut visitor);
    let exports = collect_exports(&module);
    for symbol in &mut visitor.symbols {
        if exports.local_exports.contains(&symbol.name) {
            symbol.visibility = crate::domain::Visibility::Public;
        }
    }
    consolidate_overloads(&mut visitor.symbols);

    TypeScriptModuleInfo {
        symbols: visitor.symbols,
        exports,
        imports: collect_imports(&module),
    }
}

fn consolidate_overloads(symbols: &mut Vec<crate::domain::Symbol>) {
    let mut consolidated: Vec<crate::domain::Symbol> = Vec::with_capacity(symbols.len());
    let mut indices: HashMap<String, usize> = HashMap::new();
    for mut symbol in symbols.drain(..) {
        consolidate_overloads(&mut symbol.children);
        if let Some(index) = indices.get(&symbol.id).copied() {
            let existing = &mut consolidated[index];
            if !existing
                .signature
                .lines()
                .any(|line| line == symbol.signature.as_str())
            {
                existing.signature.push('\n');
                existing.signature.push_str(&symbol.signature);
            }
            existing.children.extend(symbol.children);
            consolidate_overloads(&mut existing.children);
        } else {
            indices.insert(symbol.id.clone(), consolidated.len());
            consolidated.push(symbol);
        }
    }
    *symbols = consolidated;
}

fn parse_module(file_path: &Path) -> Result<(Module, Lrc<SourceMap>)> {
    let source_map: Lrc<SourceMap> = Default::default();
    let file = source_map.load_file(file_path)?;
    let tsx = file_path
        .extension()
        .is_some_and(|extension| extension == "tsx");
    let module = parse_source_file(file, source_map.clone(), tsx)?;
    Ok((module, source_map))
}

fn parse_source_file(
    file: Lrc<SourceFile>,
    source_map: Lrc<SourceMap>,
    tsx: bool,
) -> Result<Module> {
    let handler =
        Handler::with_tty_emitter(ColorConfig::Auto, true, false, Some(source_map.clone()));
    let lexer = Lexer::new(
        Syntax::Typescript(TsConfig {
            tsx,
            decorators: true,
            ..Default::default()
        }),
        Default::default(),
        StringInput::from(&*file),
        None,
    );
    let mut parser = Parser::new_from(lexer);
    for error in parser.take_errors() {
        error.into_diagnostic(&handler).emit();
    }
    let module = parser
        .parse_module()
        .map_err(|error| anyhow::anyhow!("Parse failed: {:?}", error))?;
    Ok(module)
}
