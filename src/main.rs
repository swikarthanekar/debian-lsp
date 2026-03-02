//! Debian Language Server Protocol implementation.

#![deny(missing_docs)]
#![deny(unsafe_code)]

use std::sync::Arc;
use tokio::sync::Mutex;
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::NumberOrString;
use tower_lsp_server::ls_types::*;
use tower_lsp_server::{Client, LanguageServer, LspService, Server};

mod analysis;
mod changelog;
mod control;
mod copyright;
mod position;
mod source_format;
mod tests;
mod watch;
mod workspace;

use position::{lsp_range_to_text_range, text_range_to_lsp_range};
use std::collections::HashMap;
// Removed unused imports - TextRange and TextSize are no longer used in main.rs
use workspace::Workspace;
use analysis::workspace::Workspace as SemanticWorkspace;

/// Debian file type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileType {
    /// debian/control file
    Control,
    /// debian/copyright file
    Copyright,
    /// debian/watch file
    Watch,
    /// debian/tests/control file
    TestsControl,
    /// debian/changelog file
    Changelog,
    /// debian/source/format file
    SourceFormat,
}

impl FileType {
    /// Detect the file type from a URI
    fn detect(uri: &Uri) -> Option<Self> {
        if control::is_control_file(uri) {
            Some(Self::Control)
        } else if copyright::is_copyright_file(uri) {
            Some(Self::Copyright)
        } else if watch::is_watch_file(uri) {
            Some(Self::Watch)
        } else if tests::is_tests_control_file(uri) {
            Some(Self::TestsControl)
        } else if changelog::is_changelog_file(uri) {
            Some(Self::Changelog)
        } else if source_format::is_source_format_file(uri) {
            Some(Self::SourceFormat)
        } else {
            None
        }
    }
}

/// Information about an open file
struct FileInfo {
    /// The workspace's source file ID
    source_file: workspace::SourceFile,
    /// The detected file type
    file_type: FileType,
}

/// Check if two LSP ranges overlap
fn range_overlaps(a: &Range, b: &Range) -> bool {
    // Check if range a starts before b ends and b starts before a ends
    (a.start.line < b.end.line
        || (a.start.line == b.end.line && a.start.character <= b.end.character))
        && (b.start.line < a.end.line
            || (b.start.line == a.end.line && b.start.character <= a.end.character))
}
fn extract_word(line: &str, character: usize) -> String {
    let chars: Vec<char> = line.chars().collect();
    if character >= chars.len() {
        return String::new();
    }

    let mut start = character;
    let mut end = character;

    while start > 0 && chars[start - 1].is_alphanumeric() {
        start -= 1;
    }

    while end < chars.len() && chars[end].is_alphanumeric() {
        end += 1;
    }

    chars[start..end].iter().collect()
}
struct Backend {
    client: Client,
    workspace: Arc<Mutex<Workspace>>,          // existing parser workspace
    semantic: Arc<Mutex<SemanticWorkspace>>,   // NEW semantic layer
    files: Arc<Mutex<HashMap<Uri, FileInfo>>>,
}

impl LanguageServer for Backend {
    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let files = self.files.lock().await;
        let file_info = match files.get(&uri) {
            Some(info) => info,
            None => return Ok(None),
        };

        if file_info.file_type != FileType::Control {
            return Ok(None);
        }

        let workspace = self.workspace.lock().await;
        let text = workspace.source_text(file_info.source_file);
        drop(workspace);

        let lines: Vec<&str> = text.lines().collect();
        if position.line as usize >= lines.len() {
            return Ok(None);
        }

        let line = lines[position.line as usize];

        let dependencies = analysis::workspace::Workspace::extract_dependency_names(line);
        let word = extract_word(line, position.character as usize);
        if !dependencies.contains(&word) {
            return Ok(None);
        }

        let semantic = self.semantic.lock().await;

        if let Some(location) = semantic.goto_definition(&word) {
            return Ok(Some(GotoDefinitionResponse::Scalar(location)));
        }

        Ok(None)
    }
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                definition_provider: Some(OneOf::Left(true)),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                completion_provider: Some(CompletionOptions {
                    resolve_provider: Some(false),
                    trigger_characters: Some(vec![":".to_string(), " ".to_string()]),
                    work_done_progress_options: Default::default(),
                    all_commit_characters: None,
                    completion_item: None,
                }),
                code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let files = self.files.lock().await;
        let file_info = match files.get(&uri) {
            Some(info) => info,
            None => return Ok(None),
        };

        if file_info.file_type != FileType::Control {
            return Ok(None);
        }

        let workspace = self.workspace.lock().await;
        let text = workspace.source_text(file_info.source_file);
        drop(workspace);

        let lines: Vec<&str> = text.lines().collect();
        if position.line as usize >= lines.len() {
            return Ok(None);
        }

        let line = lines[position.line as usize];
        let word = extract_word(line, position.character as usize);

        if word.is_empty() {
            return Ok(None);
        }

        let semantic = self.semantic.lock().await;

        let definition = semantic.goto_definition(&word);
        let reverse = semantic.get_reverse_dependencies(&word);

        if definition.is_none() && reverse.is_empty() {
            return Ok(None);
        }

        let mut contents = format!("Package: {}\n", word);

        if !reverse.is_empty() {
            contents.push_str("\nUsed by:\n");
            for pkg in reverse {
                contents.push_str(&format!("- {}\n", pkg));
            }
        }

        Ok(Some(Hover {
            contents: HoverContents::Scalar(MarkedString::String(contents)),
            range: None,
        }))
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "Debian LSP initialized!")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        self.client
            .log_message(
                MessageType::INFO,
                format!("file opened: {:?}", params.text_document.uri),
            )
            .await;

        // Detect file type once
        let Some(file_type) = FileType::detect(&params.text_document.uri) else {
            return;
        };

        let mut workspace = self.workspace.lock().await;
        let source_file = workspace.update_file(
            params.text_document.uri.clone(),
            params.text_document.text.clone(),
        );
        // Update semantic workspace (only for control files)
        if file_type == FileType::Control {
            let mut semantic = self.semantic.lock().await;
            semantic.update_control_file(
                params.text_document.uri.clone(),
                params.text_document.text.clone(),
            );
        }

        let mut files = self.files.lock().await;
        files.insert(
            params.text_document.uri.clone(),
            FileInfo {
                source_file,
                file_type,
            },
        );

        // Publish diagnostics based on file type
        match file_type {
            FileType::Control => {
                let diagnostics = workspace.get_diagnostics(source_file);
                self.client
                    .publish_diagnostics(params.text_document.uri.clone(), diagnostics, None)
                    .await;
            }
            FileType::Copyright => {
                let diagnostics = workspace.get_copyright_diagnostics(source_file);
                self.client
                    .publish_diagnostics(params.text_document.uri.clone(), diagnostics, None)
                    .await;
            }
            FileType::Watch
            | FileType::TestsControl
            | FileType::Changelog
            | FileType::SourceFormat => {
                // No diagnostics for these file types yet
            }
        }
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        self.client
            .log_message(
                MessageType::INFO,
                format!("file changed: {:?}", params.text_document.uri),
            )
            .await;

        // Get or detect the file type
        let mut files = self.files.lock().await;
        let file_type = files
            .get(&params.text_document.uri)
            .map(|info| info.file_type)
            .or_else(|| FileType::detect(&params.text_document.uri));

        let Some(file_type) = file_type else {
            return;
        };

        // Apply the content changes
        let Some(changes) = params.content_changes.first() else {
            return;
        };

        let mut workspace = self.workspace.lock().await;
        let source_file =
            workspace.update_file(params.text_document.uri.clone(), changes.text.clone());
                // Update semantic workspace for control files
                if file_type == FileType::Control {
                    let mut semantic = self.semantic.lock().await;
                    semantic.update_control_file(
                        params.text_document.uri.clone(),
                        changes.text.clone(),
                    );
                }
        files.insert(
            params.text_document.uri.clone(),
            FileInfo {
                source_file,
                file_type,
            },
        );

        // Publish diagnostics based on file type
        match file_type {
            FileType::Control => {
                let diagnostics = workspace.get_diagnostics(source_file);
                self.client
                    .publish_diagnostics(params.text_document.uri.clone(), diagnostics, None)
                    .await;
            }
            FileType::Copyright => {
                let diagnostics = workspace.get_copyright_diagnostics(source_file);
                self.client
                    .publish_diagnostics(params.text_document.uri.clone(), diagnostics, None)
                    .await;
            }
            FileType::Watch
            | FileType::TestsControl
            | FileType::Changelog
            | FileType::SourceFormat => {
                // No diagnostics for these file types yet
            }
        }
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;

        // Look up the file type from our cache
        let files = self.files.lock().await;
        let file_type = files.get(&uri).map(|info| info.file_type);
        drop(files); // Release the lock

        let completions = match file_type {
            Some(FileType::Control) => control::get_completions(&uri, position),
            Some(FileType::Copyright) => copyright::get_completions(&uri, position),
            Some(FileType::Watch) => watch::get_completions(&uri, position),
            Some(FileType::TestsControl) => tests::get_completions(&uri, position),
            Some(FileType::Changelog) => changelog::get_completions(&uri, position),
            Some(FileType::SourceFormat) => source_format::get_completions(&uri, position),
            None => Vec::new(),
        };

        if completions.is_empty() {
            Ok(None)
        } else {
            Ok(Some(CompletionResponse::Array(completions)))
        }
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        let workspace = self.workspace.lock().await;
        let files = self.files.lock().await;

        let file_info = match files.get(&params.text_document.uri) {
            Some(info) => info,
            None => return Ok(None),
        };

        // Only control and copyright files support code actions for now
        match file_info.file_type {
            FileType::Control | FileType::Copyright => {}
            _ => return Ok(None),
        }

        let source_text = workspace.source_text(file_info.source_file);

        let mut actions = Vec::new();

        // Check for field casing issues - only process fields in the requested range
        let text_range = lsp_range_to_text_range(&source_text, &params.range);

        let issues = match file_info.file_type {
            FileType::Control => {
                workspace.find_field_casing_issues(file_info.source_file, Some(text_range))
            }
            FileType::Copyright => workspace
                .find_copyright_field_casing_issues(file_info.source_file, Some(text_range)),
            _ => unreachable!(),
        };

        for issue in issues {
            let lsp_range = text_range_to_lsp_range(&source_text, issue.field_range);

            // Double-check it's within the requested range (should always be true)
            if range_overlaps(&lsp_range, &params.range) {
                // Check if there's a matching diagnostic in the context
                let matching_diagnostics = params
                    .context
                    .diagnostics
                    .iter()
                    .filter(|d| {
                        d.range == lsp_range
                            && d.code == Some(NumberOrString::String("field-casing".to_string()))
                    })
                    .cloned()
                    .collect::<Vec<_>>();

                // Create a code action to fix the casing
                let edit = TextEdit {
                    range: lsp_range,
                    new_text: issue.standard_name.clone(),
                };

                let workspace_edit = WorkspaceEdit {
                    changes: Some(
                        vec![(params.text_document.uri.clone(), vec![edit])]
                            .into_iter()
                            .collect(),
                    ),
                    ..Default::default()
                };

                let action = CodeAction {
                    title: format!(
                        "Fix field casing: {} -> {}",
                        issue.field_name, issue.standard_name
                    ),
                    kind: Some(CodeActionKind::QUICKFIX),
                    edit: Some(workspace_edit),
                    diagnostics: if !matching_diagnostics.is_empty() {
                        Some(matching_diagnostics)
                    } else {
                        None
                    },
                    ..Default::default()
                };

                actions.push(CodeActionOrCommand::CodeAction(action));
            }
        }

        if actions.is_empty() {
            Ok(None)
        } else {
            Ok(Some(actions))
        }
    }
}

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(|client| Backend {
    client,
    workspace: Arc::new(Mutex::new(Workspace::new())),
    semantic: Arc::new(Mutex::new(SemanticWorkspace::new())),
    files: Arc::new(Mutex::new(HashMap::new())),
    });

    Server::new(stdin, stdout, socket).serve(service).await;
}

#[cfg(test)]
mod main_tests {
    use super::*;

    #[tokio::test]
    async fn test_completion_returns_control_completions() {
        // Test that the completion method properly uses the control module
        let uri = str::parse("file:///path/to/debian/control").unwrap();
        let params = CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position::new(0, 0),
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: None,
        };

        // We can't easily test the actual completion without a full LSP setup,
        // but we can verify the control module works
        let completions = control::get_completions(
            &params.text_document_position.text_document.uri,
            params.text_document_position.position,
        );
        assert!(!completions.is_empty());
    }

    #[tokio::test]
    async fn test_completion_returns_none_for_non_control_files() {
        let uri = str::parse("file:///path/to/other.txt").unwrap();
        let position = Position::new(0, 0);

        let completions = control::get_completions(&uri, position);
        assert!(completions.is_empty());
    }

    #[test]
    fn test_control_module_integration() {
        // Test that the control module is properly integrated
        let control_uri = str::parse("file:///path/to/debian/control").unwrap();
        let non_control_uri = str::parse("file:///path/to/other.txt").unwrap();
        let position = Position::new(0, 0);

        // Control file should return completions
        let completions = control::get_completions(&control_uri, position);
        assert!(!completions.is_empty());
        assert!(completions.iter().any(|c| c.label == "Source"));
        assert!(completions.iter().any(|c| c.label == "debhelper-compat"));

        // Non-control file should return no completions
        let completions = control::get_completions(&non_control_uri, position);
        assert!(completions.is_empty());
    }

    #[test]
    fn test_workspace_integration() {
        // Test that the workspace can parse control files
        let mut workspace = workspace::Workspace::new();
        let url = str::parse("file:///debian/control").unwrap();
        let content = "source: test-package\nMaintainer: Test <test@example.com>\n";

        let file = workspace.update_file(url, content.to_string());
        let parsed = workspace.get_parsed_control(file);

        // Should parse correctly
        assert!(parsed.errors().is_empty());

        if let Ok(control) = parsed.to_result() {
            let mut field_names = Vec::new();
            for paragraph in control.as_deb822().paragraphs() {
                for entry in paragraph.entries() {
                    if let Some(name) = entry.key() {
                        field_names.push(name);
                    }
                }
            }
            assert!(field_names.contains(&"source".to_string()));
            assert!(field_names.contains(&"Maintainer".to_string()));
        }
    }

    #[test]
    fn test_field_casing_detection() {
        // Test that we can detect incorrect field casing
        use control::get_standard_field_name;

        // Test correct casing - should return the same
        assert_eq!(get_standard_field_name("Source"), Some("Source"));
        assert_eq!(get_standard_field_name("Package"), Some("Package"));
        assert_eq!(get_standard_field_name("Maintainer"), Some("Maintainer"));

        // Test incorrect casing - should return the standard form
        assert_eq!(get_standard_field_name("source"), Some("Source"));
        assert_eq!(get_standard_field_name("package"), Some("Package"));
        assert_eq!(get_standard_field_name("maintainer"), Some("Maintainer"));
        assert_eq!(get_standard_field_name("MAINTAINER"), Some("Maintainer"));

        // Test unknown fields - should return None
        assert_eq!(get_standard_field_name("UnknownField"), None);
        assert_eq!(get_standard_field_name("random"), None);
    }
}
