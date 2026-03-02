use std::collections::HashMap;
use std::sync::Arc;
use tower_lsp_server::ls_types::Uri;

pub struct AstCache<T> {
    entries: HashMap<Uri, Arc<T>>,
}

impl<T> AstCache<T> {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    pub fn insert(&mut self, uri: Uri, ast: T) {
        self.entries.insert(uri, Arc::new(ast));
    }
}