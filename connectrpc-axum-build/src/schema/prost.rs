#[cfg(any(test, feature = "tonic", feature = "tonic-client"))]
use super::TypePathMapping;
use super::{SchemaSet, TypeModel};
use convert_case::{Case, Casing};

/// Prost-compatible naming and type-path resolution over a normalized schema.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ProstSchemaResolver<'a> {
    schema: &'a SchemaSet,
}

impl<'a> ProstSchemaResolver<'a> {
    pub(crate) fn new(schema: &'a SchemaSet) -> Self {
        Self { schema }
    }

    #[cfg(any(test, feature = "tonic", feature = "tonic-client"))]
    pub(crate) fn type_path_mappings(&self) -> Vec<TypePathMapping> {
        self.schema
            .types
            .types
            .iter()
            .map(|ty| TypePathMapping {
                proto_path: ty.proto_fqn.clone(),
                rust_path: self.rust_type_path(ty),
            })
            .collect()
    }

    pub(crate) fn rust_method_name(&self, proto_name: &str) -> String {
        sanitize_identifier(&proto_name.to_case(Case::Snake))
    }

    pub(crate) fn rust_service_name(&self, proto_name: &str) -> String {
        sanitize_identifier(proto_name)
    }

    pub(crate) fn rust_type_relative(
        &self,
        proto_fqn: &str,
        current_package: &str,
        nesting: usize,
    ) -> Option<String> {
        let ty = self.schema.find_type(proto_fqn)?;
        let type_name = self.rust_type_path(ty);
        let target_parts: Vec<&str> = if ty.package.is_empty() {
            Vec::new()
        } else {
            ty.package.split('.').collect()
        };
        let current_parts: Vec<&str> = if current_package.is_empty() {
            Vec::new()
        } else {
            current_package.split('.').collect()
        };

        let common_len = current_parts
            .iter()
            .zip(&target_parts)
            .take_while(|(left, right)| left == right)
            .count();

        let up_count = (current_parts.len() - common_len) + nesting;
        let mut segments = Vec::with_capacity(up_count + target_parts.len() - common_len + 1);
        segments.extend(std::iter::repeat_n("super".to_string(), up_count));
        segments.extend(
            target_parts[common_len..]
                .iter()
                .map(|segment| (*segment).to_string()),
        );
        segments.push(type_name);

        Some(segments.join("::"))
    }

    fn rust_type_path(&self, ty: &TypeModel) -> String {
        ty.scoped_name.join("_")
    }
}

fn sanitize_identifier(ident: &str) -> String {
    match ident {
        "as" | "break" | "const" | "continue" | "else" | "enum" | "false" | "fn" | "for" | "if"
        | "impl" | "in" | "let" | "loop" | "match" | "mod" | "move" | "mut" | "pub" | "ref"
        | "return" | "static" | "struct" | "trait" | "true" | "type" | "unsafe" | "use"
        | "where" | "while" | "dyn" | "abstract" | "become" | "box" | "do" | "final" | "macro"
        | "override" | "priv" | "typeof" | "unsized" | "virtual" | "yield" | "async" | "await"
        | "try" | "gen" => {
            format!("r#{ident}")
        }
        "_" | "super" | "self" | "Self" | "extern" | "crate" => format!("{ident}_"),
        _ if ident.starts_with(|c: char| c.is_ascii_digit()) => format!("_{ident}"),
        _ => ident.to_string(),
    }
}
