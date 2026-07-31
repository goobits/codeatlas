use super::{exported_http_methods, line_at, push_pattern_operation};
use crate::http::model::{HttpConfidence, HttpSourceOperation};
use crate::http::openapi::normalize_path;
use std::path::Path;

pub(super) fn detect(
    path: &Path,
    repository_root: &Path,
    source: &str,
    output: &mut Vec<HttpSourceOperation>,
) {
    let Some(route) = route_path(path) else {
        return;
    };
    for method in exported_http_methods(source) {
        push_pattern_operation(
            output,
            method.as_str(),
            &normalize_path(&route),
            &route,
            "medusa_route",
            HttpConfidence::High,
            path,
            repository_root,
            line_at(source, method.start()),
        );
    }
}

pub(super) fn is_route(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some("route.ts" | "route.js")
    ) && route_path(path).is_some()
}

fn route_path(path: &Path) -> Option<String> {
    let normalized = path.to_string_lossy().replace('\\', "/");
    let (_, after) = normalized.rsplit_once("/src/api/")?;
    let route = after
        .strip_suffix("/route.ts")
        .or_else(|| after.strip_suffix("/route.js"))?;
    Some(if route.is_empty() {
        "/".to_string()
    } else {
        format!("/{route}")
    })
}

#[cfg(test)]
mod tests {
    use super::detect;
    use std::path::Path;

    #[test]
    fn discovers_static_and_parameterized_medusa_routes() {
        let mut operations = Vec::new();
        detect(
            Path::new("/repo/src/api/store/products/[productId]/route.ts"),
            Path::new("/repo"),
            "export async function GET() {}\nexport const POST = async () => {}\n",
            &mut operations,
        );

        let keys = operations
            .iter()
            .map(|operation| operation.key.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            keys,
            [
                "GET /store/products/{productId}",
                "POST /store/products/{productId}"
            ]
        );
        assert!(operations.iter().all(|operation| {
            operation.detector == "medusa_route"
                && operation.path_pattern.as_deref() == Some("/store/products/[productId]")
        }));
    }
}
