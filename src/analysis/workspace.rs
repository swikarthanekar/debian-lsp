/// NOTE:
/// This semantic workspace currently performs lightweight text-based indexing.
/// The intended future direction is to integrate with the existing parsed
/// deb822 AST from the main workspace to avoid duplicate parsing and enable
/// richer semantic analysis (e.g., field-aware indexing, cross-paragraph resolution).


use crate::analysis::ast_cache::AstCache;
use crate::analysis::index::SymbolIndex;
use tower_lsp_server::ls_types::{Location, Position, Range, Uri};

pub struct ParsedControlFile;

pub struct Workspace {
    pub ast_cache: AstCache<ParsedControlFile>,
    pub index: SymbolIndex,
}

impl Workspace {
    pub fn new() -> Self {
        Self {
            ast_cache: AstCache::new(),
            index: SymbolIndex::new(),
        }
    }

    pub fn update_control_file(&mut self, uri: Uri, text: String) {
        self.index.clear_file(&uri);

        let parsed = ParsedControlFile;

        self.index_control_file(&uri, &text);

        self.ast_cache.insert(uri, parsed);
    }

    pub fn extract_dependency_names(line: &str) -> Vec<String> {
        if let Some(rest) = line.strip_prefix("Depends:") {
            rest.split(',')
                .map(|part| {
                    let part = part.trim();

                    // Remove version constraint (everything after space + '(')
                    let name = part
                        .split_whitespace()
                        .next()
                        .unwrap_or("")
                        .to_string();

                    // Remove architecture qualifiers like :any
                    name.split(':').next().unwrap_or("").to_string()
                })
                .filter(|name| !name.is_empty())
                .collect()
        } else {
            Vec::new()
        }
    }

    fn index_control_file(&mut self, uri: &Uri, text: &str) {
        let mut lines = text.lines().enumerate().peekable();
        let mut current_package: Option<String> = None;

        while let Some((line_number, line)) = lines.next() {
            let trimmed = line.trim_start();

            // Package definition
            if let Some(rest) = trimmed.strip_prefix("Package:") {
                let name = rest.trim().to_string();
                current_package = Some(name.clone());

                let location = Location {
                    uri: uri.clone(),
                    range: Range {
                        start: Position {
                            line: line_number as u32,
                            character: 0,
                        },
                        end: Position {
                            line: line_number as u32,
                            character: line.len() as u32,
                        },
                    },
                };

                self.index.insert_definition(name, location);
            }

            // Depends field
            if trimmed.starts_with("Depends:") {
                let mut full_dep_line = trimmed.to_string();

                while let Some((_, next_line)) = lines.peek() {
                    if next_line.starts_with(' ') {
                        full_dep_line.push_str(next_line);
                        lines.next();
                    } else {
                        break;
                    }
                }

                if let Some(pkg) = &current_package {
                    let deps = Self::extract_dependency_names(&full_dep_line);
                    for dep in deps {
                        self.index.add_reverse_dependency(dep, pkg.clone());
                    }
                }
            }
        }
    }

    pub fn goto_definition(&self, dependency_name: &str) -> Option<Location> {
        self.index.get_definition(dependency_name)
    }

    pub fn get_reverse_dependencies(&self, name: &str) -> Vec<String> {
        self.index.get_reverse_dependencies(name)
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp_server::ls_types::Uri;

    #[test]
    fn test_index_and_goto_definition() {
        let mut ws = Workspace::new();

        let uri: Uri = "file:///debian/control".parse().unwrap();

        let content = r#"
Package: libfoo

Package: app
Depends: libfoo
"#;

        ws.update_control_file(uri.clone(), content.to_string());

        let location = ws.goto_definition("libfoo");

        assert!(location.is_some());
        let loc = location.unwrap();
        assert_eq!(loc.uri, uri);
    }

    #[test]
    fn test_dependency_parsing_real_world() {
        let line = "Depends: libfoo (>= 1.2), libc6, python3:any";

        let deps = Workspace::extract_dependency_names(line);

        assert!(deps.contains(&"libfoo".to_string()));
        assert!(deps.contains(&"libc6".to_string()));
        assert!(deps.contains(&"python3".to_string()));
    }

    #[test]
    fn test_multiline_depends_parsing() {
        let content = r#"
    Package: libfoo

    Package: app
    Depends:
    libfoo (>= 1.2),
    libc6,
    python3:any
    "#;

        let mut ws = Workspace::new();
        let uri: Uri = "file:///debian/control".parse().unwrap();

        ws.update_control_file(uri.clone(), content.to_string());

        let location = ws.goto_definition("libfoo");

        assert!(location.is_some());
    }

    #[test]
    fn test_reverse_dependencies() {
        let content = r#"
    Package: libfoo

    Package: app
    Depends: libfoo

    Package: tool
    Depends: libfoo
    "#;

        let mut ws = Workspace::new();
        let uri: Uri = "file:///debian/control".parse().unwrap();

        ws.update_control_file(uri, content.to_string());

        let reverse = ws.get_reverse_dependencies("libfoo");

        assert_eq!(reverse.len(), 2);
        assert!(reverse.contains(&"app".to_string()));
        assert!(reverse.contains(&"tool".to_string()));
    }

}