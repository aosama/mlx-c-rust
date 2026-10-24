//! Pure parsing primitives for the upstream surface audit: the C-header
//! declaration scanner, the C++ public-signature scanner, and the markdown
//! section slicer. The audit command (`audit_upstream_surface`) owns the
//! walking and comparison policy; this module owns turning header and
//! markdown text into names. Keeping them apart means each parser is
//! testable on synthetic fixtures without any filesystem or repository
//! context.

use std::collections::BTreeSet;

/// Collect declared function names from C header source: every
/// `mlx_`-prefixed identifier directly followed by `(`, after stripping
/// comments. Declaration headers never call functions, so usage-shaped
/// false positives cannot arise; type names and function-pointer typedefs
/// are excluded by construction (no paren follows the bare name).
pub(crate) fn function_names_in_c_source(header_source: &str) -> Vec<String> {
    function_names_with_prefix(header_source, "mlx_")
}

/// The prefix-parameterized form of the C declaration scanner: the gap
/// shim declares its own `mlxrs_`-prefixed C functions, which the vendored
/// `mlx_` scan cannot see because `mlxrs_` does not contain the substring
/// `mlx_`.
pub(crate) fn function_names_with_prefix(
    header_source: &str,
    identifier_prefix: &str,
) -> Vec<String> {
    let comment_stripped = strip_c_comments(header_source);
    let mut function_names = Vec::new();
    for source_line in comment_stripped.lines() {
        // Preprocessor lines are skipped: mlx_error, for example, is a
        // variadic #define forwarding to _mlx_error, so no declaration
        // exists for either the header parser or bindgen (the exclusion
        // registry in COVERAGE_INVENTORY.md records this).
        if source_line.trim_start().starts_with('#') {
            continue;
        }
        let mut search_from = 0;
        while let Some(found_at) = source_line[search_from..].find(identifier_prefix) {
            let identifier_start_at = search_from + found_at;
            let identifier_end_offset = source_line[identifier_start_at..]
                .find(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
                .unwrap_or(source_line.len() - identifier_start_at);
            let identifier =
                &source_line[identifier_start_at..identifier_start_at + identifier_end_offset];
            let after_identifier = &source_line[identifier_start_at + identifier_end_offset..];
            let preceding_byte = if identifier_start_at == 0 {
                None
            } else {
                Some(source_line.as_bytes()[identifier_start_at - 1])
            };
            let identifier_start_is_boundary =
                preceding_byte.is_none_or(|byte| !byte.is_ascii_alphanumeric() && byte != b'_');
            if identifier != identifier_prefix
                && identifier_start_is_boundary
                && after_identifier.trim_start().starts_with('(')
            {
                function_names.push(identifier.to_owned());
            }
            search_from = identifier_start_at + identifier_prefix.len();
        }
    }
    function_names
}

/// Replace block and line comments with spaces so comment text cannot
/// contribute identifiers, while preserving line structure for the
/// preprocessor check.
fn strip_c_comments(source: &str) -> String {
    let mut stripped = String::with_capacity(source.len());
    let mut inside_block_comment = false;
    for line in source.lines() {
        let mut remaining_line = line;
        let mut stripped_line = String::with_capacity(line.len());
        loop {
            if inside_block_comment {
                match remaining_line.find("*/") {
                    Some(close_at) => {
                        stripped_line.push_str(&" ".repeat(close_at + 2));
                        remaining_line = &remaining_line[close_at + 2..];
                        inside_block_comment = false;
                    }
                    None => {
                        stripped_line.push_str(&" ".repeat(remaining_line.len()));
                        break;
                    }
                }
            } else {
                // The earliest of the two comment openers wins: `code; // /*`
                // is a line comment, not the start of a block comment.
                let comment_start = [remaining_line.find("/*"), remaining_line.find("//")]
                    .into_iter()
                    .flatten()
                    .min();
                match comment_start {
                    Some(comment_at) => {
                        let is_block_comment = remaining_line[comment_at..].starts_with("/*");
                        stripped_line.push_str(&remaining_line[..comment_at]);
                        if is_block_comment {
                            stripped_line.push(' ');
                            remaining_line = &remaining_line[comment_at + 2..];
                            inside_block_comment = true;
                        } else {
                            break;
                        }
                    }
                    None => {
                        stripped_line.push_str(remaining_line);
                        break;
                    }
                }
            }
        }
        stripped.push_str(&stripped_line);
        stripped.push('\n');
    }
    stripped
}

pub(crate) fn quoted_includes(header_source: &str) -> Vec<String> {
    let comment_stripped = strip_c_comments(header_source);
    let mut includes = Vec::new();
    for line in comment_stripped.lines() {
        let trimmed_line = line.trim_start();
        let Some(rest) = trimmed_line.strip_prefix("#include \"") else {
            continue;
        };
        if let Some(closing_quote_at) = rest.find('"') {
            includes.push(rest[..closing_quote_at].to_owned());
        }
    }
    includes
}

/// Scan C++ header source for public free functions: declarations whose
/// `MLX_API` marker sits at column zero (class members are indented in
/// MLX's style, so the column rule separates them) and whose signature
/// reaches a `(` before any `{` or `;`. Handles both shapes MLX uses:
/// `MLX_API array fftn(` and the name on its own line,
/// `MLX_API array\nfftn(`. Template declarations are skipped whole — the
/// C API cannot express them, so they are not coverage gaps — by ignoring
/// everything from the `template` line through its terminating `;`.
pub(crate) fn cpp_public_free_function_names(header_source: &str) -> BTreeSet<String> {
    let comment_stripped = strip_c_comments(header_source);
    let mut function_names = BTreeSet::new();
    let mut skipping_template = false;
    let mut pending_signature = String::new();
    let mut pending_active = false;

    for line in comment_stripped.lines() {
        let trimmed_line = line.trim_start();
        if skipping_template {
            if trimmed_line.ends_with(';') || trimmed_line.starts_with('}') {
                skipping_template = false;
            }
            continue;
        }
        if trimmed_line.starts_with("template") {
            skipping_template = true;
            continue;
        }
        if !pending_active {
            if !line.starts_with("MLX_API") {
                continue;
            }
            pending_signature.clear();
            pending_active = true;
        }
        // The activation line is evaluated immediately: a signature that
        // ends with the file would otherwise never reach the name check.
        pending_signature.push('\n');
        pending_signature.push_str(line);
        let signature_termination = pending_signature.find([';', '{', '}']);
        let signature_head =
            &pending_signature[..signature_termination.unwrap_or(pending_signature.len())];
        if let Some(parenthesis_at) = first_parenthesis_outside_angle_brackets(signature_head) {
            pending_active = false;
            if let Some(function_name) =
                trailing_identifier_outside_angle_brackets(&signature_head[..parenthesis_at])
            {
                let is_not_operator = !function_name.starts_with("operator");
                let is_lowercase_start =
                    function_name.chars().next().is_some_and(char::is_lowercase);
                if is_not_operator && is_lowercase_start {
                    function_names.insert(function_name);
                }
            }
        } else if signature_termination.is_some() {
            // A ; or } arrived before any ( — a class, enum, or variable
            // declaration, not a function signature.
            pending_active = false;
        }
    }
    function_names
}

/// The identifier the parameter list belongs to: the last identifier
/// outside angle brackets. Signatures returning `std::function<array(…)>`
/// carry a `(` inside the return type, so the plain first-parenthesis
/// reading would name `array` instead of the function (`vmap`).
fn first_parenthesis_outside_angle_brackets(signature_head: &str) -> Option<usize> {
    let mut angle_bracket_depth = 0usize;
    for (byte_offset, character) in signature_head.char_indices() {
        match character {
            '<' => angle_bracket_depth += 1,
            '>' => angle_bracket_depth = angle_bracket_depth.saturating_sub(1),
            '(' if angle_bracket_depth == 0 => return Some(byte_offset),
            _ => {}
        }
    }
    None
}

/// The last identifier in a return-type prefix, ignoring identifiers that
/// sit inside angle brackets (they name template arguments, not the
/// function): `std::pair<array, array> vjp` → `vjp`.
fn trailing_identifier_outside_angle_brackets(type_and_name: &str) -> Option<String> {
    let mut angle_bracket_depth = 0usize;
    let mut trailing_identifier = String::new();
    for character in type_and_name.chars() {
        match character {
            '<' | '>' => {
                angle_bracket_depth = if character == '<' {
                    angle_bracket_depth + 1
                } else {
                    angle_bracket_depth.saturating_sub(1)
                };
                trailing_identifier.clear();
            }
            _ if angle_bracket_depth > 0 => {}
            _ if character.is_ascii_alphanumeric() || character == '_' => {
                trailing_identifier.push(character);
            }
            _ => {
                trailing_identifier.clear();
            }
        }
    }
    (!trailing_identifier.is_empty()).then_some(trailing_identifier)
}

/// The lines of a `## <heading>` section in markdown, up to the next
/// `## ` heading line.
pub(crate) fn markdown_section(markdown: &str, section_heading: &str) -> Option<String> {
    let mut section_lines: Vec<&str> = Vec::new();
    let mut inside_section = false;
    for line in markdown.lines() {
        if line.starts_with("## ") {
            if inside_section {
                break;
            }
            inside_section = line.trim() == section_heading;
        } else if inside_section {
            section_lines.push(line);
        }
    }
    if inside_section || !section_lines.is_empty() {
        Some(section_lines.join("\n"))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_extract_declared_c_function_names_and_skip_types_and_comments() {
        let header_source = r#"
/* mlx_in_comment( is not a function */
int mlx_add(mlx_array* res, const mlx_array a);
// mlx_in_line_comment( is not a function
int mlx_linalg_norm(mlx_array* res,
                    const mlx_array a);
typedef struct mlx_array_ { void* ctx; } mlx_array;
static mlx_array mlx_array_empty;
typedef void (*mlx_error_handler)(int code);
int mlx_error(int code, const char* fmt, ...);
"#;
        let extracted = function_names_in_c_source(header_source);
        assert_eq!(
            extracted,
            vec![
                "mlx_add".to_owned(),
                "mlx_linalg_norm".to_owned(),
                "mlx_error".to_owned()
            ]
        );
    }

    #[test]
    fn should_not_treat_embedded_identifier_fragments_as_declarations() {
        // my_mlx_add( contains mlx_ but the identifier boundary rule must
        // reject it: the character before mlx_ is part of a longer name.
        let header_source = "int my_mlx_add(int left, int right);";
        let extracted = function_names_in_c_source(header_source);
        assert!(extracted.is_empty());
    }

    #[test]
    fn should_parse_single_line_multi_line_and_name_on_next_line_cpp_signatures() {
        let header_source = r#"
namespace mlx::core {

MLX_API array square(const array& x, StreamOrDevice s = {});

MLX_API std::vector<array>
split(const array& a, const std::vector<int>& indices, StreamOrDevice s = {});

MLX_API array
transpose(const array& a, std::vector<int> axes, StreamOrDevice s = {});
}
"#;
        let parsed = cpp_public_free_function_names(header_source);
        assert_eq!(
            parsed,
            ["square", "split", "transpose"]
                .into_iter()
                .map(String::from)
                .collect()
        );
    }

    #[test]
    fn should_skip_cpp_classes_operators_and_templates() {
        let header_source = r#"
MLX_API class Device {
public:
  MLX_API void encode() {}
};

MLX_API std::ostream& operator<<(std::ostream& os, const array& a);

template <typename... Args>
MLX_API array stack(std::vector<array> arrays, int axis, Args&&... args);

MLX_API array real_op(const array& a);
"#;
        let parsed = cpp_public_free_function_names(header_source);
        assert_eq!(parsed, ["real_op"].into_iter().map(String::from).collect());
    }

    #[test]
    fn should_not_extract_preprocessor_macro_definitions() {
        // mlx_error is a variadic #define in the vendored tree; the parser
        // must see no declaration for it, matching the exclusion registry.
        let header_source = "#define mlx_error(code, fmt, ...) _mlx_error(code, fmt, __VA_ARGS__)\nint mlx_add(mlx_array* res, const mlx_array a);\n";
        let extracted = function_names_in_c_source(header_source);
        assert_eq!(extracted, vec!["mlx_add".to_owned()]);
    }

    #[test]
    fn should_slice_markdown_sections_without_leaking_the_next_section() {
        let markdown =
            "## Functions\n| `mlx_one` | bridged |\n\n## Types\n| `mlx_two` | bridged |\n";
        let functions_section = markdown_section(markdown, "## Functions").unwrap();
        assert!(functions_section.contains("mlx_one"));
        assert!(!functions_section.contains("mlx_two"));
        assert!(markdown_section(markdown, "## Missing").is_none());
    }

    #[test]
    fn should_reject_empty_identifier_rather_than_invent_one() {
        assert_eq!(trailing_identifier_outside_angle_brackets("   "), None);
        assert_eq!(
            trailing_identifier_outside_angle_brackets("array fftn"),
            Some("fftn".to_owned())
        );
        assert_eq!(
            trailing_identifier_outside_angle_brackets("std::vector<array> split"),
            Some("split".to_owned())
        );
        // Identifiers inside angle brackets name template arguments, so a
        // head that never closes its brackets yields no function name.
        assert_eq!(
            trailing_identifier_outside_angle_brackets("std::function<array"),
            None
        );
    }

    #[test]
    fn should_name_functions_whose_return_type_contains_parentheses() {
        let header_source = r#"
MLX_API std::function<array(const array&)> vmap(
    const std::function<array(const array&)>& fun,
    const array& x);

MLX_API std::function<std::vector<array>(const std::vector<array>&)> checkpoint(
    const std::function<std::vector<array>(const std::vector<array>&)>& fun);
"#;
        let parsed = cpp_public_free_function_names(header_source);
        assert_eq!(
            parsed,
            ["checkpoint", "vmap"]
                .into_iter()
                .map(String::from)
                .collect()
        );
    }
}
