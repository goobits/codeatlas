use codeatlas_domain::{
    CallableBlockKind, CallableContract, CallableKind, EffectKind, EffectProvenance, EvidenceClass,
    ReceiverRequirement, SemanticType, Symbol,
};
use std::path::Path;

#[test]
fn callable_contracts_share_one_cross_language_shape() {
    let cases = [
        (
            "rust",
            rust_contract(
                "pub fn transform(value: String, enabled: bool) -> bool { enabled }",
                "transform",
            ),
            true,
        ),
        (
            "python",
            python_contract(
                "def transform(value: str, enabled: bool) -> bool:\n    return enabled\n",
                "transform",
            ),
            true,
        ),
        (
            "typescript",
            ecmascript_contract(
                "export function transform(value: string, enabled: boolean): boolean { return enabled }",
                "src/case.ts",
                "transform",
            ),
            true,
        ),
        (
            "javascript",
            ecmascript_contract(
                "export function transform(value, enabled) { return enabled }",
                "src/case.js",
                "transform",
            ),
            false,
        ),
    ];

    for (language, contract, has_types) in cases {
        assert_eq!(contract.signatures.len(), 1, "{language}");
        let signature = &contract.signatures[0];
        assert_eq!(signature.kind, CallableKind::Function, "{language}");
        assert_eq!(
            signature.receiver.requirement,
            ReceiverRequirement::None,
            "{language}"
        );
        assert_eq!(signature.parameters.len(), 2, "{language}");
        assert_eq!(
            signature.parameters[0].name.as_deref(),
            Some("value"),
            "{language}"
        );
        assert_eq!(
            signature.parameters[1].name.as_deref(),
            Some("enabled"),
            "{language}"
        );
        if has_types {
            assert!(matches!(
                signature.parameters[0].semantic_type,
                SemanticType::String { .. }
            ));
            assert_eq!(signature.parameters[1].semantic_type, SemanticType::Boolean);
            assert_eq!(signature.result, SemanticType::Boolean);
            assert!(contract.block_reasons.is_empty(), "{language}");
        } else {
            assert_eq!(
                contract
                    .block_reasons
                    .iter()
                    .filter(|reason| reason.kind == CallableBlockKind::MissingType)
                    .count(),
                3,
                "{language}"
            );
        }

        let first = serde_json::to_vec(&contract).expect("serialize contract");
        let second = serde_json::to_vec(&contract).expect("serialize contract again");
        assert_eq!(first, second, "{language}");
    }
}

#[test]
fn receivers_are_structured_without_becoming_parameters() {
    let contracts = [
        rust_child_contract(
            "struct Service; impl Service { fn ping(&mut self, enabled: bool) -> bool { enabled } }",
            "Service",
            "ping",
        ),
        python_child_contract(
            "class Service:\n    def ping(self, enabled: bool) -> bool:\n        return enabled\n",
            "Service",
            "ping",
        ),
        ecmascript_child_contract(
            "export class Service { ping(enabled: boolean): boolean { return enabled } }",
            "src/service.ts",
            "Service",
            "ping",
        ),
    ];

    for contract in contracts {
        let signature = &contract.signatures[0];
        assert_eq!(signature.kind, CallableKind::Method);
        assert!(matches!(
            signature.receiver.requirement,
            ReceiverRequirement::Instance | ReceiverRequirement::MutableInstance
        ));
        assert_eq!(signature.parameters.len(), 1);
        assert_eq!(signature.parameters[0].name.as_deref(), Some("enabled"));
    }
}

#[test]
fn overloads_merge_structured_signatures_without_display_reparsing() {
    let info = codeatlas_languages::typescript::parser::parse_source(
        r#"
export function parse(value: string): boolean;
export function parse(value: boolean): boolean;
export function parse(value: string | boolean): boolean { return Boolean(value) }
"#,
        "src/overloads.ts",
    )
    .expect("TypeScript overloads");
    let symbol = find_symbol(&info.symbols, "parse");
    let contract = symbol.callable.as_ref().expect("callable contract");

    assert_eq!(contract.signatures.len(), 3);
    assert_eq!(
        contract
            .signatures
            .iter()
            .filter(|signature| signature.body == codeatlas_domain::CallableBody::DeclarationOnly)
            .count(),
        2
    );
    assert!(!contract
        .block_reasons
        .iter()
        .any(|reason| reason.kind == CallableBlockKind::DeclarationOnly));
}

#[test]
fn known_direct_effects_share_one_conservative_cross_language_vocabulary() {
    let contracts = [
        rust_contract(
            r#"pub fn inspect() {
                let _ = std::fs::read_to_string("x");
                let _ = std::fs::write("x", b"");
                let _ = std::net::TcpStream::connect("127.0.0.1:1");
                let _ = rusqlite::Connection::open("db");
                let _ = std::process::Command::spawn();
                let _ = std::env::var("HOME");
                let _ = std::time::Instant::now();
                let _ = rand::random();
                let _ = std::io::stdout();
            }"#,
            "inspect",
        ),
        python_contract(
            "def inspect() -> None:\n    pathlib.Path('x').read_text()\n    pathlib.Path('x').write_text('')\n    requests.get('http://example.invalid')\n    sqlite3.connect('db')\n    subprocess.run([])\n    os.getenv('HOME')\n    time.time()\n    random.random()\n    print()\n",
            "inspect",
        ),
        ecmascript_contract(
            "export function inspect(): void { fs.readFileSync('x'); fs.writeFileSync('x', ''); fetch('http://example.invalid'); pg.connect(); child_process.spawn('x'); process.env.HOME; Date.now(); Math.random(); console.log(); }",
            "src/effects.ts",
            "inspect",
        ),
        ecmascript_contract(
            "export function inspect() { fs.readFileSync('x'); fs.writeFileSync('x', ''); fetch('http://example.invalid'); pg.connect(); child_process.spawn('x'); process.env.HOME; Date.now(); Math.random(); console.log(); }",
            "src/effects.js",
            "inspect",
        ),
    ];

    for contract in contracts {
        assert_eq!(
            contract
                .effects
                .iter()
                .map(|effect| effect.kind)
                .collect::<Vec<_>>(),
            vec![
                EffectKind::FilesystemRead,
                EffectKind::FilesystemWrite,
                EffectKind::Network,
                EffectKind::Database,
                EffectKind::Process,
                EffectKind::Environment,
                EffectKind::Time,
                EffectKind::Randomness,
                EffectKind::AmbientState,
            ]
        );
        assert!(contract.effects.iter().all(|effect| {
            effect.provenance == EffectProvenance::Direct
                && effect.evidence == EvidenceClass::BoundaryLimited
        }));
    }
}

fn rust_contract(source: &str, symbol: &str) -> CallableContract {
    let info = codeatlas_languages::rust::parser::parse_module_info(
        Path::new("src/case.rs"),
        Path::new("."),
        source,
    )
    .expect("Rust contract source");
    find_symbol(&info.symbols, symbol)
        .callable
        .clone()
        .expect("Rust callable contract")
}

fn python_contract(source: &str, symbol: &str) -> CallableContract {
    let info = codeatlas_languages::python::parser::parse_module_info(
        Path::new("src/case.py"),
        Path::new("."),
        source,
    )
    .expect("Python contract source");
    find_symbol(&info.symbols, symbol)
        .callable
        .clone()
        .expect("Python callable contract")
}

fn ecmascript_contract(source: &str, path: &str, symbol: &str) -> CallableContract {
    let info = codeatlas_languages::typescript::parser::parse_source(source, path)
        .expect("ECMAScript contract source");
    find_symbol(&info.symbols, symbol)
        .callable
        .clone()
        .expect("ECMAScript callable contract")
}

fn rust_child_contract(source: &str, parent: &str, child: &str) -> CallableContract {
    let info = codeatlas_languages::rust::parser::parse_module_info(
        Path::new("src/service.rs"),
        Path::new("."),
        source,
    )
    .expect("Rust method source");
    child_contract(&info.symbols, parent, child)
}

fn python_child_contract(source: &str, parent: &str, child: &str) -> CallableContract {
    let info = codeatlas_languages::python::parser::parse_module_info(
        Path::new("src/service.py"),
        Path::new("."),
        source,
    )
    .expect("Python method source");
    child_contract(&info.symbols, parent, child)
}

fn ecmascript_child_contract(
    source: &str,
    path: &str,
    parent: &str,
    child: &str,
) -> CallableContract {
    let info = codeatlas_languages::typescript::parser::parse_source(source, path)
        .expect("ECMAScript method source");
    child_contract(&info.symbols, parent, child)
}

fn child_contract(symbols: &[Symbol], parent: &str, child: &str) -> CallableContract {
    find_symbol(&find_symbol(symbols, parent).children, child)
        .callable
        .clone()
        .expect("child callable contract")
}

fn find_symbol<'a>(symbols: &'a [Symbol], name: &str) -> &'a Symbol {
    symbols
        .iter()
        .find(|symbol| symbol.name == name)
        .unwrap_or_else(|| panic!("missing symbol {name}"))
}
