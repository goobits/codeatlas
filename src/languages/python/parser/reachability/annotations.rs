use super::qualified_expr_name;
use rustpython_ast::Visitor;
use rustpython_parser::{ast, Parse};
use std::collections::BTreeSet;

const MAX_FORWARD_ANNOTATION_BYTES: usize = 4 * 1024;
const MAX_FORWARD_ANNOTATION_DEPTH: usize = 32;

#[derive(Default)]
pub(super) struct AnnotationReferences {
    pub(super) names: BTreeSet<String>,
    pub(super) qualified_names: BTreeSet<String>,
    pub(super) unresolved: bool,
}

impl AnnotationReferences {
    pub(super) fn collect(expression: &ast::Expr) -> Self {
        let mut references = Self::default();
        references.visit_expr(expression.clone());
        scan_forward_annotations(expression, &mut references, 0);
        references
    }
}

impl Visitor for AnnotationReferences {
    fn visit_expr_name(&mut self, node: ast::ExprName) {
        if matches!(node.ctx, ast::ExprContext::Load) {
            self.names.insert(node.id.as_str().to_string());
        }
    }

    fn visit_expr_attribute(&mut self, node: ast::ExprAttribute) {
        if matches!(node.ctx, ast::ExprContext::Load) {
            if let Some(parent) = qualified_expr_name(&node.value) {
                self.qualified_names
                    .insert(format!("{parent}.{}", node.attr.as_str()));
            }
        }
        self.generic_visit_expr_attribute(node);
    }
}

fn scan_forward_annotations(
    expression: &ast::Expr,
    references: &mut AnnotationReferences,
    depth: usize,
) {
    if depth >= MAX_FORWARD_ANNOTATION_DEPTH {
        references.unresolved = true;
        return;
    }
    match expression {
        ast::Expr::Constant(constant) => {
            let ast::Constant::Str(value) = &constant.value else {
                return;
            };
            if value.is_empty() || value.len() > MAX_FORWARD_ANNOTATION_BYTES {
                references.unresolved = true;
                return;
            }
            match ast::Expr::parse(value, "<annotation>") {
                Ok(parsed) => {
                    references.visit_expr(parsed.clone());
                    scan_forward_annotations(&parsed, references, depth + 1);
                }
                Err(_) => references.unresolved = true,
            }
        }
        ast::Expr::Subscript(subscript) => {
            let form = qualified_expr_name(&subscript.value);
            if form.as_deref().is_some_and(is_literal_annotation) {
                return;
            }
            if form.as_deref().is_some_and(is_annotated_annotation) {
                let first = match subscript.slice.as_ref() {
                    ast::Expr::Tuple(tuple) => tuple.elts.first(),
                    expression => Some(expression),
                };
                if let Some(first) = first {
                    scan_forward_annotations(first, references, depth + 1);
                }
                return;
            }
            scan_forward_annotations(&subscript.slice, references, depth + 1);
        }
        ast::Expr::Call(call)
            if qualified_expr_name(&call.func)
                .as_deref()
                .is_some_and(is_forward_ref) =>
        {
            if let Some(argument) = call.args.first() {
                scan_forward_annotations(argument, references, depth + 1);
            } else {
                references.unresolved = true;
            }
        }
        ast::Expr::BinOp(binary) => {
            scan_forward_annotations(&binary.left, references, depth + 1);
            scan_forward_annotations(&binary.right, references, depth + 1);
        }
        ast::Expr::Tuple(tuple) => {
            for element in &tuple.elts {
                scan_forward_annotations(element, references, depth + 1);
            }
        }
        ast::Expr::List(list) => {
            for element in &list.elts {
                scan_forward_annotations(element, references, depth + 1);
            }
        }
        ast::Expr::Starred(starred) => {
            scan_forward_annotations(&starred.value, references, depth + 1);
        }
        _ => {}
    }
}

fn is_literal_annotation(name: &str) -> bool {
    name == "Literal" || name.ends_with(".Literal")
}

fn is_annotated_annotation(name: &str) -> bool {
    name == "Annotated" || name.ends_with(".Annotated")
}

fn is_forward_ref(name: &str) -> bool {
    name == "ForwardRef" || name.ends_with(".ForwardRef")
}

#[cfg(test)]
mod tests {
    use super::AnnotationReferences;
    use rustpython_parser::{ast, Parse};

    fn references(source: &str) -> AnnotationReferences {
        let expression = ast::Expr::parse(source, "<annotation-test>").expect("annotation");
        AnnotationReferences::collect(&expression)
    }

    #[test]
    fn forward_annotations_resolve_types_without_literal_or_metadata_strings() {
        let annotated = references(
            r#"Annotated[Union["CatModel", "DogModel"], Field(discriminator="pet_type")]"#,
        );
        assert!(annotated.names.contains("CatModel"));
        assert!(annotated.names.contains("DogModel"));
        assert!(!annotated.names.contains("pet_type"));
        assert!(!annotated.unresolved);

        let literal = references(r#"Literal["LiteralOnlyModel"]"#);
        assert!(!literal.names.contains("LiteralOnlyModel"));

        let forward_ref = references(r#"ForwardRef("ForwardModel")"#);
        assert!(forward_ref.names.contains("ForwardModel"));
    }

    #[test]
    fn invalid_forward_annotations_remain_an_explicit_boundary() {
        let references = references(r#"list["unterminated["]"#);
        assert!(references.unresolved);
    }
}
