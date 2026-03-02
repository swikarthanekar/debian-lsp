use std::collections::HashMap;
use tower_lsp_server::ls_types::{Location, Uri};

#[derive(Clone)]
pub struct PackageSymbol {
    pub definition: Location,
}

pub struct SymbolIndex {
    definitions: HashMap<String, PackageSymbol>,
    reverse_deps: HashMap<String, Vec<String>>, // package -> dependents
}

impl SymbolIndex {
    pub fn new() -> Self {
        Self {
            definitions: HashMap::new(),
            reverse_deps: HashMap::new(),
        }
    }

    pub fn insert_definition(&mut self, name: String, location: Location) {
        self.definitions.insert(
            name,
            PackageSymbol {
                definition: location,
            },
        );
    }

    pub fn add_reverse_dependency(&mut self, dependency: String, dependent: String) {
        self.reverse_deps
            .entry(dependency)
            .or_default()
            .push(dependent);
    }

    pub fn get_definition(&self, name: &str) -> Option<Location> {
        self.definitions.get(name).map(|p| p.definition.clone())
    }

    pub fn get_reverse_dependencies(&self, name: &str) -> Vec<String> {
        self.reverse_deps
            .get(name)
            .cloned()
            .unwrap_or_default()
    }

    pub fn clear_file(&mut self, uri: &Uri) {
        self.definitions
            .retain(|_, symbol| symbol.definition.uri != *uri);

        // Rebuild reverse deps safely
        self.reverse_deps.clear();
    }
}