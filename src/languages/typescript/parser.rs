mod format;
mod module_info;
mod visitor;

pub(crate) use module_info::{
    DynamicDependencyKind, DynamicDependencyTarget, ExportInfo, ImportInfo, TypeScriptModuleInfo,
};

use anyhow::Result;
use module_info::{collect_exports, collect_imports, collect_reachability_facts};
use std::collections::HashMap;
use std::path::Path;
use swc_core::common::{
    errors::{ColorConfig, Handler},
    sync::Lrc,
    FileName, SourceFile, SourceMap,
};
use swc_core::ecma::ast::Module;
use swc_core::ecma::parser::{lexer::Lexer, EsConfig, Parser, StringInput, Syntax, TsConfig};
use swc_core::ecma::visit::VisitWith;
use visitor::SymbolVisitor;

pub(crate) fn parse_file(file_path: &Path, root_dir: &Path) -> Result<Vec<crate::domain::Symbol>> {
    Ok(parse_module_info(file_path, root_dir)?.symbols)
}

pub(crate) fn parse_module_info(file_path: &Path, root_dir: &Path) -> Result<TypeScriptModuleInfo> {
    let (module, source_map, has_shebang) = parse_syntax_tree_with_metadata(file_path)?;
    let relative_path = pathdiff::diff_paths(file_path, root_dir)
        .unwrap_or(file_path.to_path_buf())
        .to_string_lossy()
        .to_string();
    Ok(build_module_info(
        module,
        source_map,
        relative_path,
        has_shebang,
    ))
}

pub(crate) fn parse_source(source: &str, relative_path: &str) -> Result<TypeScriptModuleInfo> {
    let source_map: Lrc<SourceMap> = Default::default();
    let file = source_map.new_source_file(
        FileName::Custom(relative_path.to_string()),
        source.to_string(),
    );
    let module = parse_source_file(
        file,
        source_map.clone(),
        syntax_for_path(Path::new(relative_path)),
    )?;
    Ok(build_module_info(
        module,
        source_map,
        relative_path.to_string(),
        source.starts_with("#!"),
    ))
}

fn build_module_info(
    module: Module,
    source_map: Lrc<SourceMap>,
    relative_path: String,
    has_shebang: bool,
) -> TypeScriptModuleInfo {
    let mut visitor = SymbolVisitor {
        symbols: Vec::new(),
        relative_path,
        source_map: source_map.clone(),
    };

    module.visit_with(&mut visitor);
    let exports = collect_exports(&module);
    for symbol in &mut visitor.symbols {
        if exports.local_exports.contains(&symbol.name) {
            symbol.visibility = crate::domain::Visibility::Public;
        }
    }
    consolidate_overloads(&mut visitor.symbols);

    let reachability = collect_reachability_facts(&module, source_map.clone());
    TypeScriptModuleInfo {
        symbols: visitor.symbols,
        exports,
        imports: collect_imports(&module),
        reachability,
        has_shebang,
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

pub(crate) fn parse_syntax_tree(file_path: &Path) -> Result<(Module, Lrc<SourceMap>)> {
    let (module, source_map, _) = parse_syntax_tree_with_metadata(file_path)?;
    Ok((module, source_map))
}

fn parse_syntax_tree_with_metadata(file_path: &Path) -> Result<(Module, Lrc<SourceMap>, bool)> {
    let source_map: Lrc<SourceMap> = Default::default();
    let file = source_map.load_file(file_path)?;
    let has_shebang = file.src.starts_with("#!");
    let module = parse_source_file(file, source_map.clone(), syntax_for_path(file_path))?;
    Ok((module, source_map, has_shebang))
}

fn parse_source_file(
    file: Lrc<SourceFile>,
    source_map: Lrc<SourceMap>,
    syntax: Syntax,
) -> Result<Module> {
    let handler =
        Handler::with_tty_emitter(ColorConfig::Auto, true, false, Some(source_map.clone()));
    let lexer = Lexer::new(syntax, Default::default(), StringInput::from(&*file), None);
    let mut parser = Parser::new_from(lexer);
    for error in parser.take_errors() {
        error.into_diagnostic(&handler).emit();
    }
    let module = parser
        .parse_module()
        .map_err(|error| anyhow::anyhow!("Parse failed: {:?}", error))?;
    Ok(module)
}

fn syntax_for_path(path: &Path) -> Syntax {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("js" | "mjs" | "cjs") => Syntax::Es(EsConfig {
            decorators: true,
            import_attributes: true,
            ..Default::default()
        }),
        Some("jsx") => Syntax::Es(EsConfig {
            jsx: true,
            decorators: true,
            import_attributes: true,
            ..Default::default()
        }),
        Some("tsx") => Syntax::Typescript(TsConfig {
            tsx: true,
            decorators: true,
            ..Default::default()
        }),
        _ => Syntax::Typescript(TsConfig {
            decorators: true,
            ..Default::default()
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_source, DynamicDependencyKind, DynamicDependencyTarget};

    #[test]
    fn records_file_level_shebangs_during_parsing() {
        let script = parse_source(
            "#!/usr/bin/env node\nexport const runnable = true\n",
            "bin/tool.ts",
        )
        .expect("script module info");
        let module = parse_source("export const library = true\n", "src/library.ts")
            .expect("library module info");

        assert!(script.has_shebang);
        assert!(!module.has_shebang);
    }

    #[test]
    fn http_calls_do_not_pollute_public_symbol_scans() {
        let info = parse_source(
            r#"
const store = new Map()
store.get('/ordinary')
app.get('/route', () => undefined)
"#,
            "src/routes.ts",
        )
        .expect("module info");
        let names = info
            .symbols
            .iter()
            .map(|symbol| symbol.name.as_str())
            .collect::<Vec<_>>();

        assert!(!names.contains(&"store.get"));
        assert!(!names.contains(&"app.get"));
    }

    #[test]
    fn exported_class_reachability_includes_private_top_level_dependencies() {
        let info = parse_source(
            r#"
interface Uniforms {
    intensity: number
}

const cache = new Map()

function createTexture() {
    return cache.get("texture")
}

export class Filter {
    #uniforms: Uniforms

    constructor() {
        this.#uniforms = { intensity: createTexture() ?? 0 }
    }
}
"#,
            "src/filter.ts",
        )
        .expect("module info");
        let references = &info.reachability.symbol_references["Filter"];

        assert!(references.contains("Uniforms"));
        assert!(references.contains("createTexture"));
        assert!(info.reachability.symbol_references["createTexture"].contains("cache"));
    }

    #[test]
    fn test_config_reachability_follows_static_alias_replacements() {
        let info = parse_source(
            r#"
import path from "node:path"

const sharedMock = path.resolve(__dirname, "./tests/mocks/shared.ts")
const installedRoot = path.dirname(require.resolve("installed-package"))
const installedAdapter = path.join(installedRoot, "adapters", "zod4.js")

export default {
    build: {
        lib: {
            entry: path.resolve(__dirname, "./src/runtime.ts")
        }
    },
    resolve: {
        alias: {
            "$app/environment": path.resolve(__dirname, "./tests/mocks/environment.ts"),
            "@zip.js/zip.js": resolveInstalledPackageEntry("@zip.js/zip.js"),
            "@installed-adapter": installedAdapter
        }
    },
    test: {
        aliases: [
            { find: "shared", replacement: sharedMock },
            {
                find: "@swc/helpers",
                replacement: resolveInstalledPackageFile("@swc/helpers", "esm/$1.js")
            }
        ]
    }
}
"#,
            "vitest.config.ts",
        )
        .expect("module info");

        assert_eq!(
            info.reachability.configured_test_entrypoints,
            [
                "./tests/mocks/environment.ts".to_string(),
                "./tests/mocks/shared.ts".to_string()
            ]
            .into_iter()
            .collect()
        );
        assert!(info.reachability.configures_tests);
        assert_eq!(
            info.reachability.configured_aliases["shared"],
            ["./tests/mocks/shared.ts".to_string()]
                .into_iter()
                .collect()
        );
        assert!(!info
            .reachability
            .configured_aliases
            .contains_key("@installed-adapter"));
        assert_eq!(
            info.reachability.configured_runtime_entrypoints,
            ["./src/runtime.ts".to_string()].into_iter().collect()
        );
    }

    #[test]
    fn production_vite_config_does_not_claim_test_configuration() {
        let info = parse_source(
            r#"
export default {
    build: {
        lib: {
            entry: "./src/index.ts"
        }
    }
}
"#,
            "vite.config.ts",
        )
        .expect("module info");

        assert!(!info.reachability.configures_tests);
    }

    #[test]
    fn config_reachability_extracts_static_runtime_aliases() {
        let info = parse_source(
            r#"
import path from "node:path"
import metadata from "./metadata.json" with { type: "json" }

const shared = path.resolve(__dirname, "./src/shared")

export default {
    kit: {
        alias: {
            "@domains": "src/domains",
            "@shared": shared
        }
    }
}
"#,
            "svelte.config.js",
        )
        .expect("module info");

        assert_eq!(
            info.reachability.configured_aliases["@domains"],
            ["src/domains".to_string()].into_iter().collect()
        );
        assert_eq!(
            info.reachability.configured_aliases["@shared"],
            ["./src/shared".to_string()].into_iter().collect()
        );
        assert_eq!(info.imports[1].source, "./metadata.json");
    }

    #[test]
    fn config_reachability_extracts_alias_resolver_maps_without_metadata_aliases() {
        let info = parse_source(
            r#"
export default {
    settings: {
        resolver: {
            alias: {
                map: [
                    ["@domains", "./src/domains"],
                    ["@shared", "./src/shared"]
                ],
                extensions: [".js", ".ts"]
            }
        }
    }
}
"#,
            "eslint.config.js",
        )
        .expect("module info");

        assert_eq!(
            info.reachability.configured_aliases["@domains"],
            ["./src/domains".to_string()].into_iter().collect()
        );
        assert_eq!(
            info.reachability.configured_aliases["@shared"],
            ["./src/shared".to_string()].into_iter().collect()
        );
        assert!(!info.reachability.configured_aliases.contains_key("map"));
        assert!(!info
            .reachability
            .configured_aliases
            .contains_key("extensions"));
    }

    #[test]
    fn package_alias_factories_track_static_replacement_sources() {
        let info = parse_source(
            r#"
export function createCanvasShimAlias(workspaceRoot) {
    return {
        find: "canvas",
        replacement: resolve(workspaceRoot, "packages/b/src/canvasBrowserShim.js")
    }
}
"#,
            "src/workspaceAliases.ts",
        )
        .expect("module info");

        assert_eq!(
            info.reachability.configured_aliases["canvas"],
            ["packages/b/src/canvasBrowserShim.js".to_string()]
                .into_iter()
                .collect()
        );
    }

    #[test]
    fn static_file_readers_track_source_dependencies() {
        let info = parse_source(
            r#"
const sourceDirectory = "packages/example/src"
const source = read(`${sourceDirectory}/runtime.js`)
const generated = read(outputPath)
"#,
            "scripts/build.mjs",
        )
        .expect("module info");

        assert!(info
            .reachability
            .dynamic_dependencies
            .iter()
            .any(|dependency| {
                dependency.kind == DynamicDependencyKind::RuntimeFile
                    && dependency.target
                        == DynamicDependencyTarget::Literal(
                            "packages/example/src/runtime.js".to_string(),
                        )
            }));
        assert!(!info
            .reachability
            .dynamic_dependencies
            .iter()
            .any(|dependency| {
                dependency.kind == DynamicDependencyKind::RuntimeFile
                    && matches!(dependency.target, DynamicDependencyTarget::Unknown)
            }));
    }

    #[test]
    fn static_child_process_launchers_track_source_dependencies() {
        let info = parse_source(
            r#"
childProcess.spawnSync(process.execPath, ["-r", "dotenv/config", "./scripts/check.js"])
fork("./workers/child.ts")
spawn("tsx", ["./scripts/task.ts"])
spawnSync("tsc", ["--outFile", "generated.js"])
const spawnSync = () => {}
spawnSync("node", ["./scripts/local-shadow.js"])
"#,
            "scripts/run.mjs",
        )
        .expect("module info");
        let targets = info
            .reachability
            .dynamic_dependencies
            .iter()
            .filter(|dependency| dependency.kind == DynamicDependencyKind::RuntimeProcess)
            .map(|dependency| dependency.target.clone())
            .collect::<Vec<_>>();

        assert_eq!(
            targets,
            [
                DynamicDependencyTarget::Literal("./scripts/check.js".to_string()),
                DynamicDependencyTarget::Literal("./workers/child.ts".to_string()),
                DynamicDependencyTarget::Literal("./scripts/task.ts".to_string()),
            ]
        );
    }

    #[test]
    fn static_url_bindings_track_dynamic_imports() {
        let info = parse_source(
            r#"
const workerUrl = new URL("./worker.ts", import.meta.url)
const worker = await import(workerUrl.href)
"#,
            "scripts/run.mjs",
        )
        .expect("module info");

        assert!(info
            .reachability
            .dynamic_dependencies
            .iter()
            .any(|dependency| {
                dependency.kind == DynamicDependencyKind::Import
                    && dependency.target
                        == DynamicDependencyTarget::Literal("./worker.ts".to_string())
            }));
        assert!(!info
            .reachability
            .dynamic_dependencies
            .iter()
            .any(|dependency| {
                dependency.kind == DynamicDependencyKind::Import
                    && matches!(dependency.target, DynamicDependencyTarget::Unknown)
            }));
    }

    #[test]
    fn local_reader_helpers_do_not_create_source_dependencies() {
        let info = parse_source(
            r#"
const routePath = "src/routes/example"
const read = (name) => readFile(`${routePath}/${name}`, "utf8")
const source = read("+page.svelte")
"#,
            "tests/page-boundary.test.ts",
        )
        .expect("module info");

        assert!(!info
            .reachability
            .dynamic_dependencies
            .iter()
            .any(|dependency| {
                dependency.kind == DynamicDependencyKind::RuntimeFile
                    && dependency.target
                        == DynamicDependencyTarget::Literal("+page.svelte".to_string())
            }));
    }

    #[test]
    fn existence_probes_do_not_create_source_dependencies() {
        let info = parse_source(
            r#"
existsSync(new URL("./legacy.ts", import.meta.url))
fs.existsSync(new URL("./also-legacy.ts", import.meta.url))
const runtimeUrl = new URL("./worker.ts", import.meta.url)
void runtimeUrl
"#,
            "src/boundary.test.ts",
        )
        .expect("module info");
        let runtime_urls = info
            .reachability
            .dynamic_dependencies
            .iter()
            .filter(|dependency| dependency.kind == DynamicDependencyKind::RuntimeUrl)
            .map(|dependency| dependency.target.clone())
            .collect::<Vec<_>>();

        assert_eq!(
            runtime_urls,
            [DynamicDependencyTarget::Literal("./worker.ts".to_string())]
        );
    }
}
