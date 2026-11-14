//! Vault・文書の Command・Query（`docs/spec/app-api.md` §5A）。

use std::path::PathBuf;
use std::sync::Arc;

use app_api::{
    ApiError, CompositionState, DocumentId, DocumentInfo, EditCommand, ErrorCode, Event, Movement,
    Query, QueryResponse, Selection, TextEdit, VaultFiles, ViewId, ViewState,
};
use document::{Change, DocumentError, DocumentSession};
use platform::PlatformServices;
use vault::{FileState, Vault, VaultError, VaultPath};

use crate::State;

/// 開いている文書。
pub(crate) struct OpenDocument {
    pub(crate) path: VaultPath,
    pub(crate) session: DocumentSession,
    pub(crate) file_state: FileState,
    /// API のビュー識別子と文書セッションのビュー（M2 では1つ）。
    view: (ViewId, document::ViewId),
}

fn err(code: ErrorCode, message: impl Into<String>) -> ApiError {
    ApiError::new(code, message)
}

/// Vault のエラーを API のエラーにする。メッセージは Vault 相対パスとエラーの種類のみ（文書の内容を含めない）。
fn vault_err(e: VaultError) -> ApiError {
    let code = match &e {
        VaultError::InvalidPath { .. } => ErrorCode::InvalidArgument,
        VaultError::NotFound { .. } => ErrorCode::NotFound,
        VaultError::NotAFile { .. } | VaultError::Symlink { .. } => ErrorCode::Unsupported,
        VaultError::NotUtf8 { .. } => ErrorCode::InvalidData,
        VaultError::Conflict { .. } => ErrorCode::Conflict,
        VaultError::InvalidRoot { kind } if *kind == std::io::ErrorKind::NotFound => {
            ErrorCode::NotFound
        }
        VaultError::InvalidRoot { .. } => ErrorCode::InvalidArgument,
        VaultError::Io { kind, .. } if *kind == std::io::ErrorKind::PermissionDenied => {
            ErrorCode::PermissionDenied
        }
        _ => ErrorCode::Io,
    };
    err(code, e.to_string())
}

fn doc_err(e: DocumentError) -> ApiError {
    match e {
        DocumentError::UnknownView(_) => err(ErrorCode::NotFound, e.to_string()),
        _ => err(ErrorCode::InvalidArgument, e.to_string()),
    }
}

fn no_document(document: DocumentId) -> ApiError {
    err(
        ErrorCode::NotFound,
        format!("document {} is not open", document.0),
    )
}

fn doc(state: &State, document: DocumentId) -> Result<&OpenDocument, ApiError> {
    state
        .documents
        .get(&document)
        .ok_or_else(|| no_document(document))
}

fn doc_mut(state: &mut State, document: DocumentId) -> Result<&mut OpenDocument, ApiError> {
    state
        .documents
        .get_mut(&document)
        .ok_or_else(|| no_document(document))
}

impl OpenDocument {
    fn session_view(&self, view: ViewId) -> Result<document::ViewId, ApiError> {
        if view == self.view.0 {
            Ok(self.view.1)
        } else {
            Err(err(
                ErrorCode::NotFound,
                format!("view {} is not open", view.0),
            ))
        }
    }

    fn view_state(&self) -> ViewState {
        let v = self.view.1;
        let s = &self.session;
        ViewState {
            selections: s
                .selections(v)
                .unwrap_or(&[])
                .iter()
                .map(|x| Selection {
                    anchor: x.anchor,
                    head: x.head,
                })
                .collect(),
            primary: s.primary_index(v).unwrap_or(0),
            composition: s.composition(v).ok().flatten().map(|c| CompositionState {
                range: c.range.clone(),
                text: c.text.clone(),
                selected: c.selected.clone(),
            }),
        }
    }
}

fn text_edits(change: &Change) -> Vec<TextEdit> {
    change
        .edits
        .iter()
        .map(|e| TextEdit {
            start: e.start,
            old_len: e.deleted.len(),
            new_len: e.inserted.len(),
        })
        .collect()
}

/// 文書の状態の変化に対応する Event（Spec §5A.3）。`before` は操作前の (ビューの状態, 未保存か)。
fn change_events(
    document: DocumentId,
    d: &OpenDocument,
    changes: &[Change],
    before: (ViewState, bool),
) -> Vec<Event> {
    let mut events: Vec<Event> = changes
        .iter()
        .map(|c| Event::DocumentChanged {
            document,
            version: c.version,
            edits: text_edits(c),
        })
        .collect();
    let state = d.view_state();
    if state != before.0 {
        events.push(Event::ViewStateChanged {
            document,
            view: d.view.0,
            state,
        });
    }
    let dirty = d.session.is_dirty();
    if dirty != before.1 {
        events.push(Event::DirtyChanged { document, dirty });
    }
    events
}

pub(crate) fn open_vault(
    state: &mut State,
    platform: &Arc<dyn PlatformServices>,
    path: &str,
) -> Result<Vec<Event>, ApiError> {
    let vault = Vault::open(PathBuf::from(path), platform.file_system()).map_err(vault_err)?;
    let mut events: Vec<Event> = std::mem::take(&mut state.documents)
        .into_keys()
        .map(|document| Event::DocumentClosed { document })
        .collect();
    state.vault = Some(vault);
    events.push(Event::VaultOpened {
        root: path.to_string(),
    });
    Ok(events)
}

fn vault_of(state: &State) -> Result<&Vault, ApiError> {
    state
        .vault
        .as_ref()
        .ok_or_else(|| err(ErrorCode::NotFound, "no vault is open"))
}

fn parse_path(path: &str) -> Result<VaultPath, ApiError> {
    VaultPath::new(path).map_err(|reason| {
        err(
            ErrorCode::InvalidArgument,
            format!("invalid vault path {path:?}: {reason}"),
        )
    })
}

pub(crate) fn open_document(state: &mut State, path: &str) -> Result<Vec<Event>, ApiError> {
    let vpath = parse_path(path)?;
    if let Some((id, d)) = state.documents.iter().find(|(_, d)| d.path == vpath) {
        return Ok(vec![Event::DocumentOpened {
            document: *id,
            view: d.view.0,
            path: path.to_string(),
        }]);
    }
    let loaded = vault_of(state)?.load(&vpath).map_err(vault_err)?;
    let mut session = DocumentSession::new(&loaded.text);
    let sv = session.open_view();
    let view = (ViewId(sv.get()), sv);
    let id = DocumentId(state.next_document);
    state.next_document += 1;
    state.documents.insert(
        id,
        OpenDocument {
            path: vpath,
            session,
            file_state: loaded.state,
            view,
        },
    );
    Ok(vec![Event::DocumentOpened {
        document: id,
        view: view.0,
        path: path.to_string(),
    }])
}

pub(crate) fn close_document(
    state: &mut State,
    document: DocumentId,
) -> Result<Vec<Event>, ApiError> {
    state
        .documents
        .remove(&document)
        .ok_or_else(|| no_document(document))?;
    Ok(vec![Event::DocumentClosed { document }])
}

fn movement(m: Movement) -> Result<document::Movement, ApiError> {
    Ok(match m {
        Movement::Left => document::Movement::Left,
        Movement::Right => document::Movement::Right,
        Movement::LineStart => document::Movement::LineStart,
        Movement::LineEnd => document::Movement::LineEnd,
        Movement::DocumentStart => document::Movement::DocumentStart,
        Movement::DocumentEnd => document::Movement::DocumentEnd,
        _ => return Err(err(ErrorCode::Unsupported, "unsupported movement")),
    })
}

pub(crate) fn edit(
    state: &mut State,
    document: DocumentId,
    view: ViewId,
    command: EditCommand,
) -> Result<Vec<Event>, ApiError> {
    let d = doc_mut(state, document)?;
    let v = d.session_view(view)?;
    let before = (d.view_state(), d.session.is_dirty());
    let s = &mut d.session;
    let changes: Vec<Change> = match command {
        EditCommand::InsertText { text } => s.insert_text(v, &text),
        EditCommand::InsertNewline => s.insert_newline(v),
        EditCommand::DeleteBackward => s.delete_backward(v),
        EditCommand::DeleteForward => s.delete_forward(v),
        EditCommand::DeleteSelection => s.delete_selection(v),
        EditCommand::MoveCursor {
            movement: m,
            extend,
        } => s.move_cursor(v, movement(m)?, extend),
        EditCommand::SelectAll => s.select_all(v),
        EditCommand::SetSelections {
            selections,
            primary,
        } => {
            let sels = selections
                .into_iter()
                .map(|x| document::Selection::new(x.anchor, x.head))
                .collect();
            s.set_selections(v, sels, primary)
        }
        EditCommand::Undo => s.undo(v),
        EditCommand::Redo => s.redo(v),
        EditCommand::SetComposition { text, selected } => {
            s.set_composition(v, &text, selected).map(|_| vec![])
        }
        EditCommand::CommitComposition { text } => s.commit_composition(v, &text),
        EditCommand::CancelComposition => s.cancel_composition(v).map(|_| vec![]),
        _ => return Err(err(ErrorCode::Unsupported, "unsupported edit command")),
    }
    .map_err(doc_err)?;
    Ok(change_events(document, d, &changes, before))
}

pub(crate) fn save(
    state: &mut State,
    document: DocumentId,
    overwrite: bool,
) -> Result<Vec<Event>, ApiError> {
    let vault = state
        .vault
        .as_ref()
        .ok_or_else(|| err(ErrorCode::NotFound, "no vault is open"))?;
    let d = state
        .documents
        .get_mut(&document)
        .ok_or_else(|| no_document(document))?;
    let was_dirty = d.session.is_dirty();
    let (snapshot, id) = d.session.save_point();
    let text = snapshot.to_string();
    let new_state = if overwrite {
        vault.save_overwrite(&d.path, &text, d.file_state.bom)
    } else {
        vault.save(&d.path, &text, &d.file_state)
    }
    .map_err(vault_err)?;
    d.file_state = new_state;
    d.session.mark_saved(id).map_err(doc_err)?;
    let mut events = vec![Event::DocumentSaved { document }];
    if was_dirty != d.session.is_dirty() {
        events.push(Event::DirtyChanged {
            document,
            dirty: d.session.is_dirty(),
        });
    }
    Ok(events)
}

pub(crate) fn reload(state: &mut State, document: DocumentId) -> Result<Vec<Event>, ApiError> {
    let vault = state
        .vault
        .as_ref()
        .ok_or_else(|| err(ErrorCode::NotFound, "no vault is open"))?;
    let d = state
        .documents
        .get_mut(&document)
        .ok_or_else(|| no_document(document))?;
    let loaded = vault.load(&d.path).map_err(vault_err)?;
    let before = (d.view_state(), d.session.is_dirty());
    let changes: Vec<Change> = d
        .session
        .reload(&loaded.text)
        .map_err(doc_err)?
        .into_iter()
        .collect();
    d.file_state = loaded.state;
    Ok(change_events(document, d, &changes, before))
}

pub(crate) fn query(state: &State, query: Query) -> Result<QueryResponse, ApiError> {
    match query {
        Query::VaultFiles => {
            let listing = vault_of(state)?.list().map_err(vault_err)?;
            Ok(QueryResponse::VaultFiles(VaultFiles {
                files: listing.files.iter().map(|p| p.to_string()).collect(),
                skipped: listing.skipped.len(),
            }))
        }
        Query::DocumentInfo { document } => {
            let d = doc(state, document)?;
            Ok(QueryResponse::DocumentInfo(DocumentInfo {
                path: d.path.to_string(),
                version: d.session.version(),
                dirty: d.session.is_dirty(),
            }))
        }
        Query::ViewState { document, view } => {
            let d = doc(state, document)?;
            d.session_view(view)?;
            Ok(QueryResponse::ViewState(d.view_state()))
        }
        Query::InputTextForRange {
            document,
            view,
            range,
        } => {
            let d = doc(state, document)?;
            Ok(QueryResponse::Text(
                d.session
                    .input_text_for_range(d.session_view(view)?, range)
                    .map_err(doc_err)?,
            ))
        }
        Query::InputSelectedRange { document, view } => {
            let d = doc(state, document)?;
            let (range, reversed) = d
                .session
                .input_selected_range(d.session_view(view)?)
                .map_err(doc_err)?;
            Ok(QueryResponse::SelectedRange { range, reversed })
        }
        Query::InputMarkedRange { document, view } => {
            let d = doc(state, document)?;
            Ok(QueryResponse::MarkedRange(
                d.session
                    .input_marked_range(d.session_view(view)?)
                    .map_err(doc_err)?,
            ))
        }
        Query::InputToBuffer {
            document,
            view,
            offset,
        } => {
            let d = doc(state, document)?;
            Ok(QueryResponse::Offset(
                d.session
                    .input_to_buffer(d.session_view(view)?, offset)
                    .map_err(doc_err)?,
            ))
        }
        Query::BufferToInput {
            document,
            view,
            offset,
        } => {
            let d = doc(state, document)?;
            Ok(QueryResponse::Offset(
                d.session
                    .buffer_to_input(d.session_view(view)?, offset)
                    .map_err(doc_err)?,
            ))
        }
        _ => Err(err(ErrorCode::Unsupported, "unsupported query")),
    }
}
